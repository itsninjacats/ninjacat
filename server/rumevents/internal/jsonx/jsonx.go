// Package jsonx holds the small runtime the generated event types share:
// number-preserving decoding, unknown-key preservation and discriminator
// based variant selection. It is internal to rumevents and its subpackages.
package jsonx

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strconv"
	"strings"
)

// UnmarshalNumber decodes JSON into v with numbers kept as json.Number so that
// 64-bit integers never pass through float64. Trailing data is an error.
func UnmarshalNumber(data []byte, v any) error {
	dec := json.NewDecoder(bytes.NewReader(data))
	dec.UseNumber()
	if err := dec.Decode(v); err != nil {
		return err
	}
	if _, err := dec.Token(); err != io.EOF {
		return errors.New("trailing data after JSON value")
	}
	return nil
}

// UnmarshalObject decodes an object into v (a struct without custom JSON
// methods) and returns every key that is not in known, decoded with numbers
// as json.Number. It returns nil when there are no extra keys.
func UnmarshalObject(data []byte, v any, known map[string]struct{}) (map[string]any, error) {
	if err := UnmarshalNumber(data, v); err != nil {
		return nil, err
	}
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(data, &raw); err != nil {
		return nil, err
	}
	var extra map[string]any
	for k, r := range raw {
		if _, declared := known[k]; declared {
			continue
		}
		var val any
		if err := UnmarshalNumber(r, &val); err != nil {
			return nil, fmt.Errorf("key %q: %w", k, err)
		}
		if extra == nil {
			extra = make(map[string]any, len(raw))
		}
		extra[k] = val
	}
	return extra, nil
}

// MarshalObject encodes v (a struct without custom JSON methods) and merges
// extra into the resulting object. A key present in both keeps the declared
// field's value.
func MarshalObject(v any, extra map[string]any) ([]byte, error) {
	body, err := Marshal(v)
	if err != nil {
		return nil, err
	}
	if len(extra) == 0 {
		return body, nil
	}
	var obj map[string]json.RawMessage
	if err := json.Unmarshal(body, &obj); err != nil {
		return nil, err
	}
	for k, val := range extra {
		if _, declared := obj[k]; declared {
			continue
		}
		enc, err := Marshal(val)
		if err != nil {
			return nil, fmt.Errorf("key %q: %w", k, err)
		}
		obj[k] = enc
	}
	return Marshal(obj)
}

// Marshal encodes v without HTML escaping and without a trailing newline.
func Marshal(v any) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimRight(buf.Bytes(), "\n"), nil
}

// Discriminator is a `const`/`enum` constraint at a JSON path. When the path
// is absent the constraint matches unless the schema marks it required; when
// present, the value must equal one of Values (strings, json.Number, bools).
type Discriminator struct {
	Path     []string
	Values   []any
	Required bool
}

// Variant is one alternative of a discriminated union.
type Variant struct {
	Name  string
	New   func() any
	Match []Discriminator
}

// Probe reads discriminator paths from a raw object without decoding the
// whole document. Nested objects are decoded shallowly and cached per path.
type Probe struct {
	root   map[string]json.RawMessage
	nested map[string]map[string]json.RawMessage
}

// NewProbe parses the top level of a JSON object. It fails when data is not
// an object (null included).
func NewProbe(data []byte) (*Probe, error) {
	var root map[string]json.RawMessage
	if err := json.Unmarshal(data, &root); err != nil {
		return nil, fmt.Errorf("not a JSON object: %w", err)
	}
	if root == nil {
		return nil, errors.New("not a JSON object: null")
	}
	return &Probe{root: root, nested: map[string]map[string]json.RawMessage{}}, nil
}

// Lookup returns the decoded scalar at path (string, json.Number, bool or
// nil for null) and whether the path exists. A present value that is an
// object or array is reported as present with ok=false.
func (p *Probe) Lookup(path []string) (value any, present bool, ok bool) {
	obj := p.root
	for i, seg := range path[:len(path)-1] {
		raw, found := obj[seg]
		if !found {
			return nil, false, false
		}
		key := strings.Join(path[:i+1], "\x00")
		sub, cached := p.nested[key]
		if !cached {
			if json.Unmarshal(raw, &sub) != nil || sub == nil {
				return nil, true, false
			}
			p.nested[key] = sub
		}
		obj = sub
	}
	raw, found := obj[path[len(path)-1]]
	if !found {
		return nil, false, false
	}
	if err := UnmarshalNumber(raw, &value); err != nil {
		return nil, true, false
	}
	switch value.(type) {
	case map[string]any, []any:
		return nil, true, false
	}
	return value, true, true
}

// Matches reports whether every discriminator holds for the probed document.
func (p *Probe) Matches(ds []Discriminator) bool {
	for _, d := range ds {
		value, present, ok := p.Lookup(d.Path)
		if !present {
			if d.Required {
				return false
			}
			continue
		}
		if !ok {
			return false
		}
		hit := false
		for _, want := range d.Values {
			if LiteralEqual(value, want) {
				hit = true
				break
			}
		}
		if !hit {
			return false
		}
	}
	return true
}

// LiteralEqual compares two decoded JSON scalars; numbers compare by value.
func LiteralEqual(a, b any) bool {
	an, aok := a.(json.Number)
	bn, bok := b.(json.Number)
	if aok && bok {
		if an == bn {
			return true
		}
		ai, aerr := strconv.ParseInt(string(an), 10, 64)
		bi, berr := strconv.ParseInt(string(bn), 10, 64)
		if aerr == nil && berr == nil {
			return ai == bi
		}
		af, aerr := strconv.ParseFloat(string(an), 64)
		bf, berr := strconv.ParseFloat(string(bn), 64)
		return aerr == nil && berr == nil && af == bf
	}
	return a == b
}

// MatchVariant returns the index of the first variant whose discriminators
// match data, or -1 when none does. The error is non-nil only when data is
// not a JSON object.
func MatchVariant(data []byte, variants []Variant) (int, *Probe, error) {
	pr, err := NewProbe(data)
	if err != nil {
		return -1, nil, err
	}
	for i := range variants {
		if pr.Matches(variants[i].Match) {
			return i, pr, nil
		}
	}
	return -1, pr, nil
}

// DecodeVariant selects and decodes the matching variant. It returns -1 and a
// nil value when no variant matches; decoding errors name the variant.
func DecodeVariant(data []byte, variants []Variant) (int, any, error) {
	i, _, err := MatchVariant(data, variants)
	if err != nil || i < 0 {
		return -1, nil, err
	}
	v := variants[i].New()
	if err := UnmarshalNumber(data, v); err != nil {
		return i, nil, fmt.Errorf("%s: %w", variants[i].Name, err)
	}
	return i, v, nil
}

// IsNull reports whether data is the JSON literal null (surrounding
// whitespace allowed).
func IsNull(data []byte) bool {
	return bytes.Equal(bytes.TrimSpace(data), []byte("null"))
}
