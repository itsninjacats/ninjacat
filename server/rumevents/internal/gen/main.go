// Command gen turns the JSON Schemas published in DataDog/rum-events-format
// into the Go types of package rumevents (preset "rum": RUM, telemetry and
// timeseries events) and package rumevents/replay (preset "replay": Session
// Replay segments and records).
//
// # Why a bespoke generator instead of go-jsonschema or quicktype
//
// The RUM schemas are built almost entirely from `allOf` chains of shared
// fragments (`rum/_common-schema.json`, `rum/_view-properties-schema.json`,
// ...) that redeclare the same nested objects (`view`, `_dd`, `action`) with
// extra properties, plus a root `oneOf` discriminated by `type`. The Session
// Replay schemas add nested discriminated unions at every level (records by
// `type`, incremental data by `source`, DOM nodes by `type`, wireframes by
// `type`), integer discriminators, heterogeneous tuples and a recursive DOM
// tree. Every object is open unless stated otherwise, and the intake contract
// is "nothing may be lost". The off-the-shelf generators that were considered
// fail at least two of the hard requirements:
//
//   - go-jsonschema emits one type per `allOf` member and does not merge
//     redeclared nested objects, so `RumViewEvent.view` would lose half of its
//     fields; unknown keys are dropped; no discriminated unions.
//   - quicktype merges `allOf`, but decodes numbers through float64, drops
//     unknown keys, and has no notion of a discriminated union in Go.
//
// Both would need a post-processing step at least as large as this file, so
// the generator is written directly against the schemas. It is ~1900 lines,
// depends only on the standard library, and is deterministic: running it
// twice on the same checkout produces byte-identical output.
//
// # What it generates
//
//   - One struct per object with declared properties. `allOf` members are
//     merged; nested objects that are redeclared by several fragments are
//     merged recursively. Objects declared once, in a shared fragment, become
//     one shared Go type (e.g. RumCommonSession); objects that a leaf schema
//     extends become leaf-specific types (e.g. RumActionEventView).
//   - Every struct carries `AdditionalProperties map[string]any` and custom
//     UnmarshalJSON/MarshalJSON so keys outside the schema survive a round
//     trip, exactly like datadog-api-client-go does. This also holds for
//     objects the schema closes with `additionalProperties: false`: the
//     schema describes what Datadog declares today, the intake stores what
//     actually arrived.
//   - `integer` maps to int64 and `number` to json.Number; nothing goes
//     through float64. Optional fields use pointers with `omitzero`, so an
//     absent key, an empty array and a zero value stay distinguishable.
//   - Root-level `oneOf` alternatives become concrete top-level types behind
//     an interface (Event, Segment). Nested `oneOf`/`anyOf` of titled objects
//     with `const`/`enum` discriminators become union envelopes (preset
//     "replay"): a struct with one pointer per variant plus Raw for an
//     alternative that matches no variant. Nested unions without usable
//     discriminators, and every nested union in preset "rum", are flattened
//     into a single struct with the union of all properties. Non-object
//     `oneOf`/`anyOf` (string | string[], tuples) map to `any`/`[]any`.
//   - `allOf` over a union (SerializedNodeWithId = {id} & SerializedNode) is
//     distributed: every variant is merged with the fragment and renamed
//     (DocumentNodeWithId), which is also what closes the recursive DOM tree.
//   - A dispatch table of discriminator constraints (`const`/`enum` values on
//     paths that differ between variants, honouring `required`) that the
//     decoders use to pick a variant without guessing by shape.
//
// Usage:
//
//	go run ./internal/gen -preset rum    -schemas <rum-events-format>/schemas -out .
//	go run ./internal/gen -preset replay -schemas <rum-events-format>/schemas -out ./replay
package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"go/format"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"strings"
)

// preset describes one output package.
type preset struct {
	pkg           string
	importPath    string
	rootSchema    string   // synthetic or real root union, as JSON
	platformRoots []string // per-platform roots recorded by SchemaRoots
	typeMethod    string   // name of the method returning the top-level discriminator ("" for none)
	typePath      string   // JSON key that typeMethod returns
	nestedUnions  bool     // envelopes for nested discriminated unions (else flatten)
	nameByTitle   bool     // name shared types by schema title instead of file path
	group         func(g *gen, n *node, leaf *node) string
	files         map[string]string // group -> file name
	tableFile     string            // file for interfaces and dispatch tables
}

var presets = map[string]*preset{
	// rootSchema mirrors how rum-events-mobile-schema.json composes the intake
	// track: RUM events, telemetry events and timeseries events all travel on
	// /api/v2/rum. rum-events-schema.json is used instead of the mobile root
	// because it additionally lists `transition`, and the server accepts
	// traffic from every SDK.
	"rum": {
		pkg:        "rumevents",
		importPath: "github.com/itsninjacats/server/rumevents/internal/jsonx",
		rootSchema: `{
  "title": "Event",
  "description": "Any event accepted on the RUM intake track.",
  "oneOf": [
    { "$ref": "rum-events-schema.json" },
    { "$ref": "telemetry-events-schema.json" },
    {
      "title": "RumTimeseriesEvent",
      "description": "Schema of all properties of a timeseries event",
      "oneOf": [
        { "$ref": "rum/timeseries-memory-schema.json" },
        { "$ref": "rum/timeseries-cpu-schema.json" }
      ]
    }
  ]
}`,
		platformRoots: []string{
			"rum-events-schema.json",
			"rum-events-browser-schema.json",
			"rum-events-mobile-schema.json",
			"rum-events-electron-schema.json",
		},
		typeMethod:   "EventType",
		typePath:     "type",
		nestedUnions: false,
		nameByTitle:  false,
		group: func(g *gen, n *node, leaf *node) string {
			if strings.HasPrefix(leaf.file, "telemetry") {
				return "telemetry"
			}
			return "rum"
		},
		files:     map[string]string{"rum": "rum_gen.go", "telemetry": "telemetry_gen.go"},
		tableFile: "event_gen.go",
	},
	// session-replay/segment-schema.json is the published union of the two
	// segment shapes; session-replay-schema.json is only an allOf catalogue
	// for TypeScript and is not a union, so it is not used as a root.
	"replay": {
		pkg:        "replay",
		importPath: "github.com/itsninjacats/server/rumevents/internal/jsonx",
		rootSchema: `{ "$ref": "session-replay/segment-schema.json" }`,
		platformRoots: []string{
			"session-replay-browser-schema.json",
			"session-replay-mobile-schema.json",
		},
		typeMethod:   "",
		nestedUnions: true,
		nameByTitle:  true,
		group: func(g *gen, n *node, leaf *node) string {
			switch {
			case strings.HasPrefix(n.file, "session-replay/browser/"):
				return "browser"
			case strings.HasPrefix(n.file, "session-replay/mobile/"):
				return "mobile"
			case strings.HasPrefix(n.file, "session-replay/common/"):
				return "common"
			}
			return "segment"
		},
		files: map[string]string{
			"browser": "browser_gen.go", "mobile": "mobile_gen.go",
			"common": "common_gen.go", "segment": "segment_types_gen.go",
		},
		tableFile: "segment_gen.go",
	},
}

func main() {
	schemasDir := flag.String("schemas", "", "path to the rum-events-format/schemas directory")
	outDir := flag.String("out", ".", "output package directory")
	presetName := flag.String("preset", "rum", "which package to generate: rum or replay")
	commit := flag.String("commit", "", "rum-events-format commit (default: git rev-parse HEAD of the checkout)")
	flag.Parse()
	if *schemasDir == "" {
		fmt.Fprintln(os.Stderr, "gen: -schemas is required (set RUM_EVENTS_FORMAT for go generate)")
		os.Exit(2)
	}
	p, ok := presets[*presetName]
	if !ok {
		fmt.Fprintf(os.Stderr, "gen: unknown preset %q\n", *presetName)
		os.Exit(2)
	}
	if *commit == "" {
		out, err := exec.Command("git", "-C", *schemasDir, "rev-parse", "HEAD").Output()
		if err != nil {
			fmt.Fprintln(os.Stderr, "gen: cannot determine commit, pass -commit:", err)
			os.Exit(2)
		}
		*commit = strings.TrimSpace(string(out))
	}
	g := &gen{
		preset:      p,
		schemasDir:  *schemasDir,
		commit:      *commit,
		files:       map[string]*schema{},
		resolved:    map[string]*node{},
		named:       map[string]*node{},
		fingerprint: map[string]string{},
		group:       map[string]string{},
		visited:     map[*node]bool{},
	}
	if err := g.run(*outDir); err != nil {
		fmt.Fprintln(os.Stderr, "gen:", err)
		os.Exit(1)
	}
}

// ---------------------------------------------------------------------------
// Schema loading

// schema is the subset of JSON Schema draft-07 that rum-events-format uses.
// parseSchema rejects any structural keyword outside this subset so that a
// future schema change cannot be silently mistranslated.
type schema struct {
	ID          string
	Title       string
	Description string
	Type        string
	TypeAlts    []string // type: ["string", "null"]
	Ref         string
	AllOf       []*schema
	OneOf       []*schema
	AnyOf       []*schema
	Required    []string
	Props       []propSchema // ordered as in the file
	Items       *schema
	Tuple       []*schema // items: [...]
	TupleRest   *schema   // additionalItems: {schema}
	TupleClosed bool      // additionalItems: false
	AddlBool    *bool     // additionalProperties: true/false
	AddlSchema  *schema   // additionalProperties: {schema}
	Enum        []string  // JSON literals
	Const       *string   // JSON literal
	Deprecated  bool
}

type propSchema struct {
	Name   string
	Schema *schema
}

var ignoredKeywords = map[string]bool{
	"$schema": true, "$comment": true, "readOnly": true, "pattern": true,
	"minimum": true, "maximum": true, "exclusiveMinimum": true, "exclusiveMaximum": true,
	"format": true, "default": true, "examples": true, "minItems": true, "maxItems": true,
	"uniqueItems": true, "minLength": true, "maxLength": true, "multipleOf": true,
	"propertyNames": true,
}

var scalarTypes = map[string]bool{"string": true, "integer": true, "number": true, "boolean": true}

func parseSchema(raw json.RawMessage) (*schema, error) {
	var m map[string]json.RawMessage
	if err := json.Unmarshal(raw, &m); err != nil {
		return nil, err
	}
	s := &schema{}
	for k, v := range m {
		var err error
		switch k {
		case "$id":
			err = json.Unmarshal(v, &s.ID)
		case "title":
			err = json.Unmarshal(v, &s.Title)
		case "description":
			err = json.Unmarshal(v, &s.Description)
		case "type":
			if json.Unmarshal(v, &s.Type) != nil {
				err = json.Unmarshal(v, &s.TypeAlts)
			}
		case "$ref":
			err = json.Unmarshal(v, &s.Ref)
		case "allOf":
			s.AllOf, err = parseList(v)
		case "oneOf":
			s.OneOf, err = parseList(v)
		case "anyOf":
			s.AnyOf, err = parseList(v)
		case "required":
			err = json.Unmarshal(v, &s.Required)
		case "properties":
			s.Props, err = parseProps(v)
		case "items":
			if bytes.HasPrefix(bytes.TrimSpace(v), []byte("[")) {
				s.Tuple, err = parseList(v)
			} else {
				s.Items, err = parseSchema(v)
			}
		case "additionalItems":
			if bytes.Equal(v, []byte("false")) {
				s.TupleClosed = true
			} else if !bytes.Equal(v, []byte("true")) {
				s.TupleRest, err = parseSchema(v)
			}
		case "additionalProperties":
			if bytes.Equal(v, []byte("true")) || bytes.Equal(v, []byte("false")) {
				b := bytes.Equal(v, []byte("true"))
				s.AddlBool = &b
			} else {
				s.AddlSchema, err = parseSchema(v)
			}
		case "enum":
			var vals []json.RawMessage
			if err = json.Unmarshal(v, &vals); err == nil {
				for _, x := range vals {
					s.Enum = append(s.Enum, literal(x))
				}
			}
		case "const":
			lit := literal(v)
			s.Const = &lit
		case "deprecated":
			err = json.Unmarshal(v, &s.Deprecated)
		default:
			if !ignoredKeywords[k] {
				return nil, fmt.Errorf("unsupported JSON Schema keyword %q", k)
			}
		}
		if err != nil {
			return nil, fmt.Errorf("keyword %q: %w", k, err)
		}
	}
	if s.Type != "" && s.Type != "object" && s.Type != "array" && s.Type != "null" && !scalarTypes[s.Type] {
		return nil, fmt.Errorf("unsupported type %q", s.Type)
	}
	if len(s.OneOf) > 0 && len(s.AnyOf) > 0 {
		return nil, errors.New("oneOf and anyOf on the same schema are not supported")
	}
	return s, nil
}

// literal returns the compact JSON text of a value.
func literal(raw json.RawMessage) string {
	var b bytes.Buffer
	if json.Compact(&b, raw) != nil {
		return string(raw)
	}
	return b.String()
}

// literalKind classifies a JSON literal by the schema type it belongs to.
func literalKind(lit string) string {
	switch {
	case strings.HasPrefix(lit, `"`):
		return "string"
	case lit == "true" || lit == "false":
		return "boolean"
	case lit == "null":
		return "null"
	case strings.HasPrefix(lit, "{") || strings.HasPrefix(lit, "["):
		return "composite"
	case strings.ContainsAny(lit, ".eE"):
		return "number"
	default:
		return "integer"
	}
}

func parseList(raw json.RawMessage) ([]*schema, error) {
	var items []json.RawMessage
	if err := json.Unmarshal(raw, &items); err != nil {
		return nil, err
	}
	out := make([]*schema, 0, len(items))
	for i, it := range items {
		s, err := parseSchema(it)
		if err != nil {
			return nil, fmt.Errorf("[%d]: %w", i, err)
		}
		out = append(out, s)
	}
	return out, nil
}

// parseProps keeps the declaration order of properties, which encoding/json
// maps would lose; the order drives the order of struct fields.
func parseProps(raw json.RawMessage) ([]propSchema, error) {
	dec := json.NewDecoder(bytes.NewReader(raw))
	if tok, err := dec.Token(); err != nil || tok != json.Delim('{') {
		return nil, errors.New("properties is not an object")
	}
	var out []propSchema
	for dec.More() {
		tok, err := dec.Token()
		if err != nil {
			return nil, err
		}
		name := tok.(string)
		var v json.RawMessage
		if err := dec.Decode(&v); err != nil {
			return nil, err
		}
		s, err := parseSchema(v)
		if err != nil {
			return nil, fmt.Errorf("property %q: %w", name, err)
		}
		out = append(out, propSchema{Name: name, Schema: s})
	}
	return out, nil
}

func (g *gen) load(file string) (*schema, error) {
	if s, ok := g.files[file]; ok {
		return s, nil
	}
	raw, err := os.ReadFile(filepath.Join(g.schemasDir, filepath.FromSlash(file)))
	if err != nil {
		return nil, err
	}
	s, err := parseSchema(raw)
	if err != nil {
		return nil, fmt.Errorf("%s: %w", file, err)
	}
	if s.ID != file {
		return nil, fmt.Errorf("%s: $id is %q, expected the relative path", file, s.ID)
	}
	g.files[file] = s
	return s, nil
}

func resolveRef(from, ref string) (string, error) {
	if strings.Contains(ref, "#") {
		return "", fmt.Errorf("$ref with fragment is not supported: %q", ref)
	}
	if from == "" {
		return path.Clean(ref), nil
	}
	return path.Clean(path.Join(path.Dir(from), ref)), nil
}

// ---------------------------------------------------------------------------
// Resolved model

type origin struct{ file, ptr string }

type node struct {
	kind       string // object, array, string, integer, number, boolean, any, union
	desc       string // description at the point of use (field doc)
	typeDesc   string // description of the shared shape (type doc), when they differ
	descMixed  bool   // flattened alternatives disagree on the description: keep none
	title      string
	file       string
	deprecated bool
	pending    bool // placeholder for a $ref target still being resolved (cycle)

	props    []*prop
	required map[string]bool
	addl     *node // additionalProperties schema, nil when absent/true
	addlNone bool  // additionalProperties: false
	items    *node
	tuple    []*node
	tupleDoc string
	enum     []string // JSON literals
	constLit string   // JSON literal of a scalar const
	constDoc string   // textual form of a composite/null const
	origins  []origin

	variants  []*node  // kind == union
	altsDoc   string   // kind == any produced by a non-object oneOf/anyOf
	flattened []string // titles of object alternatives merged into this node
	disc      [][]constraint

	goName    string
	titleName bool     // name the node after its title: file roots and distributed variants
	roots     []string // platform root schemas listing this leaf variant
}

type prop struct {
	name string
	node *node
}

func (n *node) typeDescOrDesc() string {
	if n.typeDesc != "" {
		return n.typeDesc
	}
	return n.desc
}

func (n *node) prop(name string) (int, *prop) {
	for i, p := range n.props {
		if p.name == name {
			return i, p
		}
	}
	return -1, nil
}

func (n *node) isShapeless() bool {
	return n.kind == "any" && len(n.props) == 0 && n.items == nil && n.tuple == nil && n.altsDoc == "" &&
		n.constLit == "" && len(n.enum) == 0
}

type gen struct {
	preset     *preset
	schemasDir string
	commit     string
	files      map[string]*schema
	resolved   map[string]*node // whole-file $ref targets, keyed by file

	named       map[string]*node
	fingerprint map[string]string
	order       []string          // type names in emission order
	group       map[string]string // type name -> output group
	visited     map[*node]bool
}

func (g *gen) resolve(s *schema, file, ptr string) (*node, error) {
	if s.Ref != "" {
		target, err := resolveRef(file, s.Ref)
		if err != nil {
			return nil, err
		}
		if n, ok := g.resolved[target]; ok {
			return n, nil
		}
		ts, err := g.load(target)
		if err != nil {
			return nil, err
		}
		// A placeholder is registered before resolving so that a schema that
		// (transitively) references itself gets a stable pointer to fill.
		ph := &node{pending: true, file: target}
		g.resolved[target] = ph
		n, err := g.resolve(ts, target, "")
		if err != nil {
			return nil, fmt.Errorf("%s: %w", target, err)
		}
		*ph = *n
		return ph, nil
	}

	n := &node{kind: s.Type, desc: s.Description, title: s.Title, file: file, deprecated: s.Deprecated,
		required: map[string]bool{}, titleName: ptr == "" && s.Title != ""}
	for _, r := range s.Required {
		n.required[r] = true
	}
	if len(s.TypeAlts) > 0 {
		n.kind = "any"
		n.altsDoc = strings.Join(s.TypeAlts, " | ")
	}
	if s.Type == "null" {
		n.kind = "any"
		n.altsDoc = "null"
	}
	if s.Const != nil {
		switch k := literalKind(*s.Const); k {
		case "null", "composite":
			n.constDoc = *s.Const
			if n.kind == "" {
				n.kind = "any"
			}
		default:
			n.constLit = *s.Const
			if n.kind == "" {
				n.kind = k
			}
		}
	}
	if len(s.Enum) > 0 {
		n.enum = s.Enum
		if n.kind == "" {
			n.kind = literalKind(s.Enum[0])
		}
	}
	if len(s.Props) > 0 {
		if n.kind == "" {
			n.kind = "object"
		}
		n.origins = []origin{{file, ptr}}
		for _, p := range s.Props {
			child, err := g.resolve(p.Schema, file, ptr+"/properties/"+p.Name)
			if err != nil {
				return nil, fmt.Errorf("property %q: %w", p.Name, err)
			}
			n.props = append(n.props, &prop{name: p.Name, node: child})
		}
	}
	if s.Items != nil {
		if n.kind == "" {
			n.kind = "array"
		}
		child, err := g.resolve(s.Items, file, ptr+"/items")
		if err != nil {
			return nil, fmt.Errorf("items: %w", err)
		}
		n.items = child
	}
	if len(s.Tuple) > 0 {
		if n.kind == "" {
			n.kind = "array"
		}
		var docs []string
		for i, ts := range s.Tuple {
			child, err := g.resolve(ts, file, fmt.Sprintf("%s/items/%d", ptr, i))
			if err != nil {
				return nil, fmt.Errorf("items[%d]: %w", i, err)
			}
			n.tuple = append(n.tuple, child)
			docs = append(docs, describe(child))
		}
		if s.TupleRest != nil {
			rest, err := g.resolve(s.TupleRest, file, ptr+"/additionalItems")
			if err != nil {
				return nil, fmt.Errorf("additionalItems: %w", err)
			}
			docs = append(docs, "..."+describe(rest))
		} else if !s.TupleClosed {
			docs = append(docs, "...any")
		}
		n.tupleDoc = "[" + strings.Join(docs, ", ") + "]"
	}
	if s.AddlBool != nil && !*s.AddlBool {
		n.addlNone = true
	}
	if s.AddlSchema != nil {
		child, err := g.resolve(s.AddlSchema, file, ptr+"/additionalProperties")
		if err != nil {
			return nil, fmt.Errorf("additionalProperties: %w", err)
		}
		n.addl = child
	}
	if n.kind == "" {
		n.kind = "any"
	}

	for i, sub := range s.AllOf {
		m, err := g.resolve(sub, file, fmt.Sprintf("%s/allOf/%d", ptr, i))
		if err != nil {
			return nil, err
		}
		n, err = combine(n, m, s.Title)
		if err != nil {
			return nil, fmt.Errorf("allOf[%d]: %w", i, err)
		}
	}

	alts, kw := s.OneOf, "oneOf"
	if len(alts) == 0 {
		alts, kw = s.AnyOf, "anyOf"
	}
	if len(alts) > 0 {
		var resolved []*node
		allObjects, allLiterals := true, true
		litKind := ""
		for i, a := range alts {
			m, err := g.resolve(a, file, fmt.Sprintf("%s/%s/%d", ptr, kw, i))
			if err != nil {
				return nil, err
			}
			if m.kind != "object" && m.kind != "union" {
				allObjects = false
			}
			k := m.kind
			if m.constLit == "" && len(m.enum) == 0 {
				allLiterals = false
			} else if litKind == "" {
				litKind = k
			} else if litKind != k {
				allLiterals = false
			}
			resolved = append(resolved, m)
		}
		switch {
		case allObjects:
			if len(n.props) > 0 || n.kind == "union" {
				return nil, errors.New("a union schema with its own properties is not supported")
			}
			n.kind = "union"
			n.variants = resolved
		case allLiterals && n.isShapeless():
			// oneOf of consts of one scalar kind is an enum with named values.
			n.kind = litKind
			var docs []string
			for _, m := range resolved {
				n.enum = unionStrings(n.enum, values(m))
				if m.title != "" && m.constLit != "" {
					docs = append(docs, m.constLit+" = "+m.title)
				}
			}
			if len(docs) > 0 {
				n.altsDoc = strings.Join(docs, ", ")
			}
		default:
			if !n.isShapeless() {
				return nil, fmt.Errorf("%s of mixed shapes combined with an explicit type is not supported", kw)
			}
			var docs []string
			for _, m := range resolved {
				docs = append(docs, describe(m))
			}
			n.kind = "any"
			n.altsDoc = strings.Join(docs, " | ")
		}
	}
	return n, nil
}

func describe(n *node) string {
	if n.title != "" && n.constLit == "" {
		return n.title
	}
	switch n.kind {
	case "array":
		if n.tupleDoc != "" {
			return n.tupleDoc
		}
		if n.items != nil {
			return "array of " + describe(n.items)
		}
		return "array"
	case "union", "object":
		if n.title != "" {
			return n.title
		}
		return n.kind
	case "any":
		if n.constDoc != "" {
			return n.constDoc
		}
		if n.altsDoc != "" {
			return "(" + n.altsDoc + ")"
		}
		if n.title != "" {
			return n.title
		}
		return "any"
	default:
		if n.constLit != "" {
			return n.constLit
		}
		if n.title != "" {
			return n.title
		}
		return n.kind
	}
}

const (
	modeAllOf = iota // intersection semantics: the merged fragment refines
	modeOneOf        // union semantics: alternatives are folded into one shape
)

// combine merges an allOf member into the host, distributing over a union
// member: {id} & (A | B) becomes ({id} & A) | ({id} & B), with the variants
// renamed after the host (SerializedNode + WithId -> DocumentNodeWithId).
func combine(host, m *node, hostTitle string) (*node, error) {
	if host.pending || m.pending {
		return nil, errors.New("allOf over a schema that is still being resolved (recursive allOf)")
	}
	if host.kind == "union" && m.kind == "union" {
		return nil, errors.New("allOf of two unions is not supported")
	}
	if host.kind != "union" && m.kind != "union" {
		return merge(host, m, modeAllOf)
	}
	u, other := host, m
	if m.kind == "union" {
		u, other = m, host
	}
	if hostTitle == "" {
		hostTitle = host.title
	}
	suffix := ""
	if u.title != "" && hostTitle != "" && strings.HasPrefix(hostTitle, u.title) {
		suffix = strings.TrimPrefix(hostTitle, u.title)
	} else if hostTitle != "" {
		suffix = hostTitle
	}
	out := &node{kind: "union", title: hostTitle, desc: host.desc, file: host.file, required: map[string]bool{},
		titleName: host.titleName}
	if out.desc == "" {
		out.desc = u.desc
	}
	for _, v := range u.variants {
		var mv *node
		var err error
		if u == host {
			mv, err = merge(v, other, modeAllOf)
		} else {
			mv, err = merge(other, v, modeAllOf)
		}
		if err != nil {
			return nil, fmt.Errorf("variant %s: %w", v.title, err)
		}
		// An untitled alternative stays untitled: it cannot be a named variant
		// and leaves() folds it back into one object.
		mv.title, mv.titleName = "", false
		if v.title != "" {
			mv.title, mv.titleName = v.title+suffix, true
		}
		mv.file = host.file
		out.variants = append(out.variants, mv)
	}
	return out, nil
}

// merge returns a new node combining a and b; neither input is mutated, which
// matters because nodes of shared fragments are referenced by several leaves.
func merge(a, b *node, mode int) (*node, error) {
	if a.pending || b.pending {
		return nil, errors.New("merge over a schema that is still being resolved (recursive allOf)")
	}
	n := *a
	n.props = append([]*prop(nil), a.props...)
	n.required = map[string]bool{}
	for k := range a.required {
		n.required[k] = true
	}
	n.origins = append([]origin(nil), a.origins...)
	if mode == modeOneOf && a.desc != "" && b.desc != "" && a.desc != b.desc {
		n.desc, n.descMixed = "", true
	}
	if n.desc == "" && !n.descMixed {
		n.desc = b.desc
	}
	if n.title == "" {
		n.title = b.title
	}
	n.deprecated = a.deprecated || b.deprecated

	if a.isShapeless() {
		// a carries no shape of its own: adopt b.
		desc, title, deprecated, file := n.desc, n.title, n.deprecated, n.file
		n = *b
		n.props = append([]*prop(nil), b.props...)
		n.required = map[string]bool{}
		n.origins = append([]origin(nil), b.origins...)
		n.desc, n.title, n.deprecated, n.file = desc, title, deprecated, file
		if n.typeDesc == "" {
			n.typeDesc = b.desc
		}
		if mode == modeAllOf {
			for k := range b.required {
				n.required[k] = true
			}
		}
		return &n, nil
	}
	if b.isShapeless() {
		return &n, nil
	}
	if a.kind != b.kind {
		return nil, fmt.Errorf("cannot merge %s with %s", a.kind, b.kind)
	}

	switch a.kind {
	case "object":
		for _, bp := range b.props {
			if i, ap := n.prop(bp.name); ap != nil {
				m, err := merge(ap.node, bp.node, mode)
				if err != nil {
					return nil, fmt.Errorf("property %q: %w", bp.name, err)
				}
				n.props[i] = &prop{name: bp.name, node: m}
			} else {
				n.props = append(n.props, bp)
			}
		}
		if mode == modeAllOf {
			for k := range b.required {
				n.required[k] = true
			}
		}
		if n.addl == nil {
			n.addl = b.addl
		}
		n.addlNone = a.addlNone || b.addlNone
		for _, o := range b.origins {
			dup := false
			for _, e := range n.origins {
				if e == o {
					dup = true
				}
			}
			if !dup {
				n.origins = append(n.origins, o)
			}
		}
	case "array":
		if a.items != nil && b.items != nil {
			m, err := merge(a.items, b.items, mode)
			if err != nil {
				return nil, fmt.Errorf("items: %w", err)
			}
			n.items = m
		} else if n.items == nil {
			n.items = b.items
		}
		if n.tuple == nil {
			n.tuple, n.tupleDoc = b.tuple, b.tupleDoc
		}
	case "union":
		return nil, errors.New("merging unions is not supported")
	default:
		av, bv := values(a), values(b)
		switch mode {
		case modeAllOf:
			if a.constLit != "" && b.constLit != "" && a.constLit != b.constLit {
				return nil, fmt.Errorf("conflicting const %s vs %s", a.constLit, b.constLit)
			}
			if b.constLit != "" {
				n.constLit = b.constLit
			}
			if len(b.enum) > 0 {
				n.enum = b.enum
			}
			if b.constDoc != "" {
				n.constDoc = b.constDoc
			}
		case modeOneOf:
			n.constLit, n.enum = "", nil
			if len(av) > 0 && len(bv) > 0 {
				n.enum = unionStrings(av, bv)
				if len(n.enum) == 1 {
					n.constLit, n.enum = n.enum[0], nil
				}
			}
		}
	}
	return &n, nil
}

func values(n *node) []string {
	if n.constLit != "" {
		return []string{n.constLit}
	}
	return n.enum
}

func unionStrings(a, b []string) []string {
	out := append([]string(nil), a...)
	for _, x := range b {
		found := false
		for _, y := range out {
			if x == y {
				found = true
			}
		}
		if !found {
			out = append(out, x)
		}
	}
	return out
}

// ---------------------------------------------------------------------------
// Union lowering (after resolution, before naming)

// leaves flattens a union of unions into its object alternatives. A nested
// union that cannot be discriminated (untitled alternatives, no const/enum
// that tells them apart, e.g. MouseInteraction's two shapes) is folded into
// one object first and then contributes that object as a single alternative.
func (g *gen) leaves(u *node, out *[]*node) error {
	for _, v := range u.variants {
		switch v.kind {
		case "union":
			var sub []*node
			if err := g.leaves(v, &sub); err != nil {
				return err
			}
			if g.discriminable(sub) {
				*out = append(*out, sub...)
				continue
			}
			flat, err := flatten(v, sub)
			if err != nil {
				return fmt.Errorf("%s: %w", v.title, err)
			}
			*v = *flat
			*out = append(*out, v)
		case "object":
			*out = append(*out, v)
		default:
			return fmt.Errorf("union alternative of kind %s", v.kind)
		}
	}
	return nil
}

func (g *gen) discriminable(ls []*node) bool {
	if !g.preset.nestedUnions {
		return false
	}
	for _, l := range ls {
		if l.title == "" {
			return false
		}
	}
	_, err := discriminators(ls)
	return err == nil
}

// lower decides, for every union outside the root union tree, whether it
// becomes an envelope (discriminated) or is flattened into one object.
func (g *gen) lower(n *node, rootTree bool, visited map[*node]bool) error {
	if visited[n] {
		return nil
	}
	visited[n] = true
	switch n.kind {
	case "union":
		if rootTree {
			for _, v := range n.variants {
				if err := g.lower(v, true, visited); err != nil {
					return err
				}
			}
			return nil
		}
		var ls []*node
		if err := g.leaves(n, &ls); err != nil {
			return fmt.Errorf("%s: %w", n.title, err)
		}
		if g.discriminable(ls) {
			disc, _ := discriminators(ls)
			n.variants, n.disc = ls, disc
			for _, l := range ls {
				if err := g.lower(l, false, visited); err != nil {
					return err
				}
			}
			return nil
		}
		flat, err := flatten(n, ls)
		if err != nil {
			return fmt.Errorf("%s: %w", n.title, err)
		}
		*n = *flat
		delete(visited, n)
		return g.lower(n, false, visited)
	case "object":
		for _, p := range n.props {
			if err := g.lower(p.node, false, visited); err != nil {
				return err
			}
		}
		if n.addl != nil {
			return g.lower(n.addl, false, visited)
		}
	case "array":
		if n.items != nil {
			return g.lower(n.items, false, visited)
		}
		for _, t := range n.tuple {
			if err := g.lower(t, false, visited); err != nil {
				return err
			}
		}
	}
	return nil
}

// flatten folds object alternatives into one object: union of properties,
// intersection of required.
func flatten(u *node, ls []*node) (*node, error) {
	common := map[string]bool{}
	for k := range ls[0].required {
		common[k] = true
	}
	for _, l := range ls[1:] {
		for k := range common {
			if !l.required[k] {
				delete(common, k)
			}
		}
	}
	n := &node{kind: "any", desc: u.desc, title: u.title, file: u.file, deprecated: u.deprecated, required: map[string]bool{}}
	for _, l := range ls {
		var err error
		n, err = merge(n, l, modeOneOf)
		if err != nil {
			return nil, err
		}
		n.flattened = append(n.flattened, l.title)
	}
	n.title, n.file, n.titleName = u.title, u.file, u.titleName
	if u.desc != "" {
		n.desc = u.desc
	}
	n.required = map[string]bool{}
	for k := range common {
		n.required[k] = true
	}
	return n, nil
}

// ---------------------------------------------------------------------------
// Discriminators

type constraint struct {
	path     []string
	values   []string // JSON literals
	required bool
}

func collectConsts(n *node, jsonPath []string, required bool, out *[]constraint, depth int) {
	if depth > 4 || n.kind != "object" {
		return
	}
	for _, p := range n.props {
		req := required && n.required[p.name]
		if vs := values(p.node); len(vs) > 0 && scalarTypes[p.node.kind] {
			*out = append(*out, constraint{path: append(append([]string(nil), jsonPath...), p.name), values: vs, required: req})
		}
		if p.node.kind == "object" {
			collectConsts(p.node, append(jsonPath, p.name), req, out, depth+1)
		}
	}
}

// discriminators returns, per alternative, the constraints on paths whose
// allowed values differ between alternatives. It fails when an alternative
// has no such constraint or two alternatives are indistinguishable. Only
// `const` values are used when they suffice; `enum` values are added only
// when some alternative cannot be told apart otherwise (MousemoveData has
// source: [1, 6]), because an enum constraint rejects values the schema does
// not list yet.
func discriminators(alts []*node) ([][]constraint, error) {
	out, err := discriminatorsWith(alts, false)
	if err != nil {
		out, err = discriminatorsWith(alts, true)
	}
	return out, err
}

func discriminatorsWith(alts []*node, withEnums bool) ([][]constraint, error) {
	all := map[string]map[string]bool{}
	per := make([][]constraint, len(alts))
	// constPaths are the paths on which at least one alternative has a const;
	// an enum is only trusted as a discriminator on such a path.
	constPaths := map[string]bool{}
	for i, a := range alts {
		collectConsts(a, nil, true, &per[i], 0)
		for _, c := range per[i] {
			if len(c.values) == 1 {
				constPaths[strings.Join(c.path, ".")] = true
			}
		}
	}
	for i := range alts {
		var kept []constraint
		for _, c := range per[i] {
			if len(c.values) == 1 || (withEnums && constPaths[strings.Join(c.path, ".")]) {
				kept = append(kept, c)
			}
		}
		per[i] = kept
		for _, c := range per[i] {
			key := strings.Join(c.path, ".")
			if all[key] == nil {
				all[key] = map[string]bool{}
			}
			all[key][fmt.Sprint(c.values)] = true
		}
	}
	out := make([][]constraint, len(alts))
	seen := map[string]int{}
	for i, a := range alts {
		for _, c := range per[i] {
			if len(all[strings.Join(c.path, ".")]) < 2 {
				continue
			}
			out[i] = append(out[i], c)
		}
		if len(out[i]) == 0 {
			return nil, fmt.Errorf("%s has no discriminating constraint", a.title)
		}
		sig := fmt.Sprint(out[i])
		if j, dup := seen[sig]; dup {
			return nil, fmt.Errorf("%s and %s have identical discriminators", alts[j].title, a.title)
		}
		seen[sig] = i
	}
	return out, nil
}

// ---------------------------------------------------------------------------
// Naming

var initialisms = map[string]bool{
	"id": true, "url": true, "dd": true, "http": true, "os": true, "cpu": true, "dns": true,
	"ssl": true, "ip": true, "ui": true, "api": true, "sdk": true, "json": true, "html": true,
	"css": true, "csp": true, "ram": true, "uuid": true, "fps": true, "cls": true, "lcp": true,
	"fcp": true, "fid": true, "inp": true, "psr": true, "rtl": true, "ci": true,
}

func goIdent(name string) string {
	var b strings.Builder
	for _, word := range strings.FieldsFunc(name, func(r rune) bool { return r == '_' || r == '-' || r == '.' || r == '$' }) {
		for _, seg := range splitCamel(word) {
			if initialisms[strings.ToLower(seg)] {
				b.WriteString(strings.ToUpper(seg))
				continue
			}
			b.WriteString(strings.ToUpper(seg[:1]) + seg[1:])
		}
	}
	return b.String()
}

// splitCamel splits "slotId" into ["slot", "Id"] and keeps "SVG" together,
// so that initialisms inside camelCase names are recognised.
func splitCamel(s string) []string {
	var out []string
	start := 0
	for i := 1; i < len(s); i++ {
		lowerBefore := s[i-1] >= 'a' && s[i-1] <= 'z' || s[i-1] >= '0' && s[i-1] <= '9'
		upperNow := s[i] >= 'A' && s[i] <= 'Z'
		if lowerBefore && upperNow {
			out = append(out, s[start:i])
			start = i
		}
	}
	return append(out, s[start:])
}

func pascalPath(segs []string) string {
	var out []string
	for _, s := range segs {
		id := goIdent(s)
		if len(out) > 0 && strings.HasSuffix(out[len(out)-1], id) {
			// "_view-container" + "container" -> ViewContainer, not ViewContainerContainer.
			continue
		}
		out = append(out, id)
	}
	return strings.Join(out, "")
}

// fileSegments turns "rum/_view-container-schema.json" into
// ["rum", "view-container"]; pascalPath renders that as "RumViewContainer".
func fileSegments(file string) []string {
	dir, base := path.Split(file)
	base = strings.TrimSuffix(base, ".json")
	base = strings.TrimSuffix(base, "-schema")
	base = strings.TrimPrefix(base, "_")
	var segs []string
	for _, d := range strings.Split(strings.Trim(dir, "/"), "/") {
		if d != "" {
			segs = append(segs, d)
		}
	}
	return append(segs, base)
}

// ptrSegments extracts property names from a JSON pointer such as
// "/allOf/2/properties/_dd/properties/action".
func ptrSegments(ptr string) []string {
	parts := strings.Split(strings.TrimPrefix(ptr, "/"), "/")
	var out []string
	for i := 0; i < len(parts); i++ {
		if parts[i] == "properties" && i+1 < len(parts) {
			out = append(out, parts[i+1])
			i++
		}
	}
	return out
}

func (g *gen) chooseName(leaf, n *node, jsonPath []string) string {
	if g.preset.nameByTitle && n.titleName && n.title != "" {
		return n.title
	}
	if !g.preset.nameByTitle {
		for _, o := range n.origins {
			if o.file == leaf.file {
				return leaf.goName + pascalPath(jsonPath)
			}
		}
	}
	// Deepest JSON pointer wins: an inline fragment nested in an allOf is the
	// extension, a whole-file $ref target (pointer "") is the base it extends.
	best, bestDepth, tie := origin{}, -1, false
	for _, o := range n.origins {
		d := len(strings.Split(o.ptr, "/"))
		switch {
		case d > bestDepth:
			best, bestDepth, tie = o, d, false
		case d == bestDepth:
			tie = true
		}
	}
	if tie || bestDepth < 0 {
		return leaf.goName + pascalPath(jsonPath)
	}
	if g.preset.nameByTitle {
		return g.files[best.file].Title + pascalPath(ptrSegments(best.ptr))
	}
	return pascalPath(append(fileSegments(best.file), ptrSegments(best.ptr)...))
}

func (g *gen) register(name string, n *node, group string, where string) error {
	fp := fingerprint(n)
	if prev, ok := g.named[name]; ok {
		if g.fingerprint[name] != fp {
			return fmt.Errorf("type name %s is reused for a different shape at %s (first seen with %d props, title %q titleName %v origins %v; now %d props, title %q titleName %v origins %v)",
				name, where, len(prev.props), prev.title, prev.titleName, prev.origins, len(n.props), n.title, n.titleName, n.origins)
		}
		return nil
	}
	g.named[name] = n
	g.fingerprint[name] = fp
	g.order = append(g.order, name)
	g.group[name] = group
	return nil
}

func fingerprint(n *node) string {
	var b strings.Builder
	onStack := map[*node]int{}
	var walk func(n *node)
	walk = func(n *node) {
		if id, ok := onStack[n]; ok {
			fmt.Fprintf(&b, "@%d", id)
			return
		}
		onStack[n] = len(onStack)
		defer delete(onStack, n)
		b.WriteString(n.kind)
		b.WriteString("|")
		b.WriteString(n.constLit)
		b.WriteString(strings.Join(n.enum, ","))
		b.WriteString("|")
		b.WriteString(n.altsDoc)
		b.WriteString(n.tupleDoc)
		if n.deprecated {
			b.WriteString("|deprecated")
		}
		if n.addlNone {
			b.WriteString("|closed")
		}
		for _, p := range n.props {
			b.WriteString("{" + p.name)
			if n.required[p.name] {
				b.WriteString("!")
			}
			b.WriteString(":")
			walk(p.node)
			b.WriteString("}")
		}
		if n.items != nil {
			b.WriteString("[")
			walk(n.items)
			b.WriteString("]")
		}
		if n.addl != nil {
			b.WriteString("<")
			walk(n.addl)
			b.WriteString(">")
		}
		for _, v := range n.variants {
			b.WriteString("(")
			walk(v)
			b.WriteString(")")
		}
	}
	walk(n)
	return b.String()
}

func (g *gen) nameTree(leaf, n *node, jsonPath []string, group string) error {
	if g.visited[n] {
		return nil
	}
	g.visited[n] = true
	named := false
	switch {
	case n.kind == "union":
		if n.title == "" {
			return fmt.Errorf("union at %s.%s has no title", leaf.goName, strings.Join(jsonPath, "."))
		}
		n.goName = n.title
		named = true
	case n.kind == "object" && len(n.props) > 0 && n != leaf:
		n.goName = g.chooseName(leaf, n, jsonPath)
		named = true
	}
	if named {
		if err := g.register(n.goName, n, g.preset.group(g, n, leaf), leaf.goName+"."+strings.Join(jsonPath, ".")); err != nil {
			return err
		}
	}
	seen := map[string]bool{}
	for _, p := range n.props {
		if seen[goIdent(p.name)] {
			return fmt.Errorf("%s: properties %q collide after conversion to Go", n.goName, p.name)
		}
		seen[goIdent(p.name)] = true
		if err := g.nameTree(leaf, p.node, append(jsonPath, p.name), group); err != nil {
			return err
		}
	}
	if n.items != nil {
		if err := g.nameTree(leaf, n.items, jsonPath, group); err != nil {
			return err
		}
	}
	for _, t := range n.tuple {
		if err := g.nameTree(leaf, t, jsonPath, group); err != nil {
			return err
		}
	}
	if n.addl != nil {
		if err := g.nameTree(leaf, n.addl, jsonPath, group); err != nil {
			return err
		}
	}
	for _, v := range n.variants {
		if err := g.nameTree(leaf, v, jsonPath, group); err != nil {
			return err
		}
	}
	return nil
}

// ---------------------------------------------------------------------------
// Root union tree

type leafInfo struct {
	node        *node
	ancestors   []string // union titles from the root down
	constraints []constraint
	typeValue   string
}

func (g *gen) walkUnion(n *node, ancestors []string, out *[]leafInfo) error {
	if n.kind != "union" {
		if n.kind != "object" || n.title == "" {
			return fmt.Errorf("union leaf without a title at %s", n.file)
		}
		*out = append(*out, leafInfo{node: n, ancestors: ancestors})
		return nil
	}
	if n.title == "" {
		return errors.New("union without a title")
	}
	for _, v := range n.variants {
		if err := g.walkUnion(v, append(append([]string(nil), ancestors...), n.title), out); err != nil {
			return err
		}
	}
	return nil
}

// collectRoots resolves every platform root and records, per leaf, which
// roots list it. Every leaf of the combined root must appear in at least one.
func (g *gen) collectRoots(leaves []leafInfo) error {
	byTitle := map[string]*node{}
	for i := range leaves {
		byTitle[leaves[i].node.title] = leaves[i].node
	}
	for _, root := range g.preset.platformRoots {
		rs, err := g.load(root)
		if err != nil {
			return err
		}
		n, err := g.resolve(rs, root, "")
		if err != nil {
			return fmt.Errorf("%s: %w", root, err)
		}
		var found []leafInfo
		if err := g.walkUnion(n, nil, &found); err != nil {
			return fmt.Errorf("%s: %w", root, err)
		}
		for _, f := range found {
			leaf, ok := byTitle[f.node.title]
			if !ok {
				return fmt.Errorf("%s lists %s, which the combined root does not", root, f.node.title)
			}
			leaf.roots = append(leaf.roots, root)
		}
	}
	for i := range leaves {
		if len(leaves[i].node.roots) == 0 {
			return fmt.Errorf("%s is listed by no platform root", leaves[i].node.goName)
		}
	}
	return nil
}

// ---------------------------------------------------------------------------
// Emission

func (g *gen) run(outDir string) error {
	rs, err := parseSchema(json.RawMessage(g.preset.rootSchema))
	if err != nil {
		return err
	}
	root, err := g.resolve(rs, "", "")
	if err != nil {
		return err
	}
	if root.kind != "union" {
		return errors.New("the root schema is not a union")
	}
	if err := g.lower(root, true, map[*node]bool{}); err != nil {
		return err
	}
	var leafList []leafInfo
	if err := g.walkUnion(root, nil, &leafList); err != nil {
		return err
	}
	rootDisc, err := discriminators(leafNodes(leafList))
	if err != nil {
		return err
	}
	for i := range leafList {
		l := &leafList[i]
		l.constraints = rootDisc[i]
		for _, c := range l.constraints {
			if len(c.path) == 1 && c.path[0] == g.preset.typePath && len(c.values) == 1 {
				l.typeValue = c.values[0]
			}
		}
		l.node.goName = l.node.title
		if g.preset.typeMethod != "" && l.typeValue == "" {
			return fmt.Errorf("%s has no single const on %q for %s()", l.node.goName, g.preset.typePath, g.preset.typeMethod)
		}
		if err := g.register(l.node.goName, l.node, g.preset.group(g, l.node, l.node), l.node.file); err != nil {
			return err
		}
		if err := g.nameTree(l.node, l.node, nil, ""); err != nil {
			return fmt.Errorf("%s: %w", l.node.goName, err)
		}
	}
	if err := g.collectRoots(leafList); err != nil {
		return err
	}

	bufs := map[string]*bytes.Buffer{}
	for group := range g.preset.files {
		bufs[group] = &bytes.Buffer{}
	}
	for _, name := range g.order {
		n := g.named[name]
		w, ok := bufs[g.group[name]]
		if !ok {
			return fmt.Errorf("type %s has no output file for group %q", name, g.group[name])
		}
		if n.kind == "union" {
			g.emitEnvelope(w, n)
		} else {
			g.emitStruct(w, n)
		}
	}
	tables := &bytes.Buffer{}
	g.emitUnions(tables, root, leafList)

	for group, name := range g.preset.files {
		if bufs[group].Len() == 0 {
			continue
		}
		if err := g.writeFile(filepath.Join(outDir, name), bufs[group].Bytes()); err != nil {
			return err
		}
	}
	return g.writeFile(filepath.Join(outDir, g.preset.tableFile), tables.Bytes())
}

func leafNodes(ls []leafInfo) []*node {
	out := make([]*node, len(ls))
	for i, l := range ls {
		out[i] = l.node
	}
	return out
}

func (g *gen) writeFile(name string, body []byte) error {
	var b bytes.Buffer
	fmt.Fprintf(&b, "// Code generated from DataDog/rum-events-format@%s. DO NOT EDIT.\n", g.commit)
	fmt.Fprintf(&b, "//\n// Regenerate with: RUM_EVENTS_FORMAT=<checkout> go generate ./rumevents/...\n\n")
	fmt.Fprintf(&b, "package %s\n\n", g.preset.pkg)
	var imports []string
	if bytes.Contains(body, []byte("json.")) {
		imports = append(imports, `"encoding/json"`)
	}
	if bytes.Contains(body, []byte("jsonx.")) {
		imports = append(imports, fmt.Sprintf("%q", g.preset.importPath))
	}
	if len(imports) > 0 {
		fmt.Fprintf(&b, "import (\n\t%s\n)\n\n", strings.Join(imports, "\n\t"))
	}
	b.Write(body)
	src, err := format.Source(b.Bytes())
	if err != nil {
		_ = os.WriteFile(name+".broken", b.Bytes(), 0o644)
		return fmt.Errorf("%s: gofmt: %w (raw output kept as %s.broken)", name, err, name)
	}
	return os.WriteFile(name, src, 0o644)
}

func (g *gen) goType(n *node, required bool) string {
	ptr := ""
	if !required {
		ptr = "*"
	}
	switch n.kind {
	case "union":
		return "*" + n.goName
	case "object":
		if len(n.props) > 0 {
			return "*" + n.goName
		}
		if n.addl != nil {
			switch n.addl.kind {
			case "string", "integer", "boolean":
				return "map[string]" + g.goType(n.addl, true)
			}
		}
		return "map[string]any"
	case "array":
		if n.items == nil {
			return "[]any"
		}
		return "[]" + strings.TrimPrefix(g.goType(n.items, true), "*")
	case "string":
		return ptr + "string"
	case "integer":
		return ptr + "int64"
	case "number":
		return "json.Number"
	case "boolean":
		return ptr + "bool"
	default:
		return "any"
	}
}

func writeDoc(w *bytes.Buffer, indent string, lines ...string) {
	for _, l := range lines {
		if l == "" {
			fmt.Fprintf(w, "%s//\n", indent)
			continue
		}
		for _, wrapped := range wrap(l, 96-len(indent)) {
			fmt.Fprintf(w, "%s// %s\n", indent, wrapped)
		}
	}
}

func wrap(s string, width int) []string {
	words := strings.Fields(s)
	var lines []string
	cur := ""
	for _, w := range words {
		if cur != "" && len(cur)+1+len(w) > width {
			lines = append(lines, cur)
			cur = w
			continue
		}
		if cur == "" {
			cur = w
		} else {
			cur += " " + w
		}
	}
	if cur != "" {
		lines = append(lines, cur)
	}
	return lines
}

func fieldDoc(n *node) []string {
	var lines []string
	if n.desc != "" {
		lines = append(lines, strings.TrimRight(n.desc, ".")+".")
	}
	switch {
	case n.constLit != "":
		lines = append(lines, "Always "+n.constLit+".")
	case n.constDoc != "":
		lines = append(lines, "Always "+n.constDoc+".")
	case len(n.enum) > 0:
		lines = append(lines, "One of: "+strings.Join(n.enum, ", ")+".")
	}
	if n.altsDoc != "" {
		if n.kind == "any" {
			lines = append(lines, "Wire form: "+n.altsDoc+". Decoded as any (numbers as json.Number).")
		} else {
			lines = append(lines, "Values: "+n.altsDoc+".")
		}
	}
	if n.kind == "array" && n.tupleDoc != "" {
		lines = append(lines, "Tuple "+n.tupleDoc+". Decoded as []any (numbers as json.Number).")
	}
	if n.kind == "array" && n.items != nil && (n.items.altsDoc != "" || n.items.tupleDoc != "") {
		lines = append(lines, "Items: "+describe(n.items)+". Decoded as any (numbers as json.Number).")
	}
	if n.kind == "object" && len(n.props) == 0 && n.addl != nil {
		lines = append(lines, "Values: "+describe(n.addl)+".")
	}
	if n.deprecated {
		lines = append(lines, "", "Deprecated: marked deprecated in the schema.")
	}
	return lines
}

func lowerFirst(s string) string {
	return strings.ToLower(s[:1]) + s[1:]
}

func (g *gen) emitStruct(w *bytes.Buffer, n *node) {
	name := n.goName
	head := name
	if d := n.typeDescOrDesc(); d != "" {
		head += ": " + strings.TrimRight(d, ".") + "."
	}
	lines := []string{head}
	if len(n.roots) > 0 {
		lines = append(lines, "", "Listed by: "+strings.Join(n.roots, ", ")+".")
	}
	if len(n.flattened) > 0 {
		lines = append(lines, "", "Flattened oneOf of: "+strings.Join(n.flattened, ", ")+". All alternative-specific fields are optional.")
	}
	if n.addlNone {
		lines = append(lines, "", "The schema closes this object (additionalProperties: false); AdditionalProperties is kept regardless so that a newer SDK's fields survive.")
	}
	if n.deprecated {
		lines = append(lines, "", "Deprecated: marked deprecated in the schema.")
	}
	writeDoc(w, "", lines...)
	fmt.Fprintf(w, "type %s struct {\n", name)
	keys := make([]string, 0, len(n.props))
	for i, p := range n.props {
		if i > 0 {
			fmt.Fprintln(w)
		}
		writeDoc(w, "\t", fieldDoc(p.node)...)
		required := n.required[p.name]
		typ := g.goType(p.node, required)
		tag := p.name
		// A required scalar is a plain value and always written. A required
		// `any` (anyOf [integer, null], e.g. rrweb's nextId) is written even
		// when nil so that an explicit null survives the round trip.
		scalar := p.node.kind == "string" || p.node.kind == "integer" || p.node.kind == "boolean" || p.node.kind == "any"
		if !(required && scalar) {
			tag += ",omitzero"
		}
		fmt.Fprintf(w, "\t%s %s `json:\"%s\"`\n", goIdent(p.name), typ, tag)
		keys = append(keys, p.name)
	}
	fmt.Fprintln(w)
	writeDoc(w, "\t", "AdditionalProperties holds every key of the object that the schema does not declare. Nothing is dropped on decode and everything is written back on encode.")
	fmt.Fprintf(w, "\tAdditionalProperties map[string]any `json:\"-\"`\n}\n\n")

	keysVar := lowerFirst(name) + "Keys"
	fmt.Fprintf(w, "var %s = map[string]struct{}{", keysVar)
	for _, k := range keys {
		fmt.Fprintf(w, "%q: {}, ", k)
	}
	fmt.Fprintf(w, "}\n\n")
	fmt.Fprintf(w, "// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.\n")
	fmt.Fprintf(w, "func (o *%s) UnmarshalJSON(data []byte) error {\n", name)
	fmt.Fprintf(w, "\ttype plain %s\n\tvar p plain\n", name)
	fmt.Fprintf(w, "\textra, err := jsonx.UnmarshalObject(data, &p, %s)\n\tif err != nil {\n\t\treturn err\n\t}\n", keysVar)
	fmt.Fprintf(w, "\tp.AdditionalProperties = extra\n\t*o = %s(p)\n\treturn nil\n}\n\n", name)
	fmt.Fprintf(w, "// MarshalJSON encodes the declared fields together with AdditionalProperties.\n")
	fmt.Fprintf(w, "func (o %s) MarshalJSON() ([]byte, error) {\n", name)
	fmt.Fprintf(w, "\ttype plain %s\n\treturn jsonx.MarshalObject(plain(o), o.AdditionalProperties)\n}\n\n", name)
}

// emitEnvelope writes a nested discriminated union: one pointer per variant
// plus Raw for an alternative that matches none.
func (g *gen) emitEnvelope(w *bytes.Buffer, n *node) {
	name := n.goName
	head := name
	if n.desc != "" {
		head += ": " + strings.TrimRight(n.desc, ".") + "."
	}
	paths := map[string]bool{}
	var pathList []string
	for _, ds := range n.disc {
		for _, d := range ds {
			key := strings.Join(d.path, ".")
			if !paths[key] {
				paths[key] = true
				pathList = append(pathList, key)
			}
		}
	}
	writeDoc(w, "", head, "",
		"Discriminated union selected by "+strings.Join(pathList, ", ")+": exactly one variant field is set. "+
			"When no variant's discriminator matches (a record type newer than the schema), Raw keeps the original JSON so nothing is lost.")
	fmt.Fprintf(w, "type %s struct {\n", name)
	for _, v := range n.variants {
		fmt.Fprintf(w, "\t%s *%s\n", v.goName, v.goName)
	}
	fmt.Fprintf(w, "\n\t// Raw holds the JSON of an alternative that matched no variant.\n\tRaw json.RawMessage\n}\n\n")

	tableVar := lowerFirst(name) + "Variants"
	fmt.Fprintf(w, "var %s = []jsonx.Variant{\n", tableVar)
	for i, v := range n.variants {
		fmt.Fprintf(w, "\t{Name: %q, New: func() any { return new(%s) }, Match: %s},\n", v.goName, v.goName, goDiscriminators(n.disc[i]))
	}
	fmt.Fprintf(w, "}\n\n")

	fmt.Fprintf(w, "// UnmarshalJSON selects the variant by its discriminator; an unmatched alternative is kept in Raw.\n")
	fmt.Fprintf(w, "func (u *%s) UnmarshalJSON(data []byte) error {\n", name)
	fmt.Fprintf(w, "\t*u = %s{}\n\tif jsonx.IsNull(data) {\n\t\treturn nil\n\t}\n", name)
	fmt.Fprintf(w, "\ti, v, err := jsonx.DecodeVariant(data, %s)\n\tif err != nil {\n\t\treturn err\n\t}\n", tableVar)
	fmt.Fprintf(w, "\tswitch i {\n")
	for i, v := range n.variants {
		fmt.Fprintf(w, "\tcase %d:\n\t\tu.%s = v.(*%s)\n", i, v.goName, v.goName)
	}
	fmt.Fprintf(w, "\tdefault:\n\t\tu.Raw = append(json.RawMessage(nil), data...)\n\t}\n\treturn nil\n}\n\n")

	fmt.Fprintf(w, "// MarshalJSON encodes the set variant, or Raw, or null when neither is set.\n")
	fmt.Fprintf(w, "func (u %s) MarshalJSON() ([]byte, error) {\n", name)
	fmt.Fprintf(w, "\tif v := u.Variant(); v != nil {\n\t\treturn jsonx.Marshal(v)\n\t}\n")
	fmt.Fprintf(w, "\tif u.Raw != nil {\n\t\treturn u.Raw, nil\n\t}\n\treturn []byte(\"null\"), nil\n}\n\n")

	fmt.Fprintf(w, "// Variant returns the populated alternative as a pointer to its concrete type, or nil.\n")
	fmt.Fprintf(w, "func (u *%s) Variant() any {\n\tswitch {\n", name)
	for _, v := range n.variants {
		fmt.Fprintf(w, "\tcase u.%s != nil:\n\t\treturn u.%s\n", v.goName, v.goName)
	}
	fmt.Fprintf(w, "\t}\n\treturn nil\n}\n\n")
}

func goDiscriminators(cs []constraint) string {
	var parts []string
	for _, c := range cs {
		var vals []string
		for _, lit := range c.values {
			switch literalKind(lit) {
			case "string", "boolean":
				vals = append(vals, lit)
			default:
				vals = append(vals, fmt.Sprintf("json.Number(%q)", lit))
			}
		}
		parts = append(parts, fmt.Sprintf("{Path: %s, Values: []any{%s}, Required: %v}", goStringSlice(c.path), strings.Join(vals, ", "), c.required))
	}
	return "[]jsonx.Discriminator{" + strings.Join(parts, ", ") + "}"
}

func (g *gen) emitUnions(w *bytes.Buffer, root *node, leaves []leafInfo) {
	var unions []*node
	var collect func(n *node)
	collect = func(n *node) {
		if n.kind != "union" {
			return
		}
		unions = append(unions, n)
		for _, v := range n.variants {
			collect(v)
		}
	}
	collect(root)
	parent := map[string]string{}
	var link func(n *node)
	link = func(n *node) {
		for _, v := range n.variants {
			if v.kind == "union" {
				parent[v.title] = n.title
				link(v)
			}
		}
	}
	link(root)

	for _, u := range unions {
		var members []string
		for _, v := range u.variants {
			members = append(members, v.title)
		}
		desc := u.title
		if u.desc != "" {
			desc += ": " + strings.TrimRight(u.desc, ".") + "."
		}
		writeDoc(w, "", desc, "", "Implemented by: "+strings.Join(members, ", ")+".")
		fmt.Fprintf(w, "type %s interface {\n", u.title)
		if p, ok := parent[u.title]; ok {
			fmt.Fprintf(w, "\t%s\n", p)
		} else {
			if g.preset.typeMethod != "" {
				writeDoc(w, "\t", g.preset.typeMethod+" returns the value of the top-level \""+g.preset.typePath+"\" field that selects the variant.")
				fmt.Fprintf(w, "\t%s() string\n", g.preset.typeMethod)
			}
			writeDoc(w, "\t", "SchemaRoots lists the platform root schemas whose union contains the variant, i.e. which SDK families can send it.")
			fmt.Fprintf(w, "\tSchemaRoots() []string\n")
		}
		fmt.Fprintf(w, "\tis%s()\n}\n\n", u.title)
	}

	for _, l := range leaves {
		name := l.node.goName
		if g.preset.typeMethod != "" {
			fmt.Fprintf(w, "// %s reports the value of the %q discriminator for %s.\n", g.preset.typeMethod, g.preset.typePath, name)
			fmt.Fprintf(w, "func (*%s) %s() string { return %s }\n\n", name, g.preset.typeMethod, l.typeValue)
		}
		fmt.Fprintf(w, "// SchemaRoots reports the platform root schemas that list %s.\n", name)
		fmt.Fprintf(w, "func (*%s) SchemaRoots() []string { return %s }\n", name, goStringSlice(l.node.roots))
		for _, a := range l.ancestors {
			fmt.Fprintf(w, "func (*%s) is%s() {}\n", name, a)
		}
		fmt.Fprintln(w)
	}

	tableVar := lowerFirst(root.title) + "Variants"
	writeDoc(w, "",
		tableVar+" lists every concrete top-level type in schema order together with the discriminator constraints that select it. A constraint on an optional path matches when the path is absent; a constraint on a required path does not. The first matching variant wins.")
	fmt.Fprintf(w, "var %s = []jsonx.Variant{\n", tableVar)
	for _, l := range leaves {
		name := l.node.goName
		fmt.Fprintf(w, "\t{Name: %q, New: func() any { return new(%s) }, Match: %s},\n", name, name, goDiscriminators(l.constraints))
	}
	fmt.Fprintf(w, "}\n")
}

func goStringSlice(ss []string) string {
	quoted := make([]string, len(ss))
	for i, s := range ss {
		quoted[i] = fmt.Sprintf("%q", s)
	}
	return "[]string{" + strings.Join(quoted, ", ") + "}"
}
