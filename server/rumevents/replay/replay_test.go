package replay

// The files under testdata/samples are copied verbatim from
// DataDog/rum-events-format (samples/session-replay/{browser,mobile}),
// Apache-2.0, at the commit named in the *_gen.go headers. Upstream ships
// single records for the browser (no segment wrapper) and one full mobile
// segment; the browser segment used below wraps the upstream records in a
// synthetic envelope, and records for variants without an upstream sample
// are synthetic too. Each test says which is which.

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/itsninjacats/server/rumevents/internal/jsonx"
)

func readSample(t *testing.T, name string) []byte {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "samples", filepath.FromSlash(name)))
	if err != nil {
		t.Fatal(err)
	}
	return data
}

func jsonValue(t *testing.T, data []byte) any {
	t.Helper()
	var v any
	if err := jsonx.UnmarshalNumber(data, &v); err != nil {
		t.Fatalf("invalid JSON: %v\n%s", err, data)
	}
	return v
}

// assertRoundTrip decodes input into v, re-encodes it and checks that not a
// single key or value changed, at any depth.
func assertRoundTrip(t *testing.T, input []byte, v any) {
	t.Helper()
	if err := jsonx.UnmarshalNumber(input, v); err != nil {
		t.Fatalf("decode: %v", err)
	}
	output, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	if want, got := jsonValue(t, input), jsonValue(t, output); !reflect.DeepEqual(want, got) {
		t.Fatalf("round trip changed the document\n input: %s\noutput: %s", input, output)
	}
}

func typeName(v any) string {
	return strings.TrimPrefix(fmt.Sprintf("%T", v), "*replay.")
}

// browserSegment wraps records in a synthetic browser segment envelope.
func browserSegment(records ...[]byte) []byte {
	var b bytes.Buffer
	b.WriteString(`{"application":{"id":"aaaaaaaa-0000-0000-0000-000000000000"},"session":{"id":"s"},"view":{"id":"v"},` +
		`"source":"browser","start":1657620232000,"end":1657620237000,"records_count":` + fmt.Sprint(len(records)) +
		`,"creation_reason":"init","has_full_snapshot":true,"index_in_view":0,"records":[`)
	for i, r := range records {
		if i > 0 {
			b.WriteByte(',')
		}
		b.Write(r)
	}
	b.WriteString("]}")
	return b.Bytes()
}

func TestMobileSegmentSample(t *testing.T) {
	doc := readSample(t, "mobile/segment/segment.json")
	seg, err := Decode(doc)
	if err != nil {
		t.Fatal(err)
	}
	ms, ok := seg.(*MobileSegment)
	if !ok {
		t.Fatalf("decoded as %T", seg)
	}
	if got := ms.SchemaRoots(); !reflect.DeepEqual(got, []string{"session-replay-mobile-schema.json"}) {
		t.Errorf("SchemaRoots = %v", got)
	}
	if ms.Source != "android" || len(ms.Records) != 2 {
		t.Fatalf("source=%q records=%d", ms.Source, len(ms.Records))
	}
	full := ms.Records[0].MobileFullSnapshotRecord
	if full == nil || ms.Records[0].Variant() != full {
		t.Fatalf("records[0] = %s, want MobileFullSnapshotRecord", typeName(ms.Records[0].Variant()))
	}
	if len(full.Data.Wireframes) != 3 || full.Data.Wireframes[0].ShapeWireframe == nil || full.Data.Wireframes[1].TextWireframe == nil {
		t.Errorf("wireframes not discriminated: %+v", full.Data.Wireframes)
	}
	// The sample carries "type": "sans-serif" inside textStyle and
	// "has_full_snapshot_record"/"creation_reason" on the segment, none of
	// which the mobile schema declares. They must survive.
	if got := full.Data.Wireframes[1].TextWireframe.TextStyle.AdditionalProperties["type"]; got != "sans-serif" {
		t.Errorf("undeclared textStyle.type = %#v", got)
	}
	if _, ok := ms.AdditionalProperties["has_full_snapshot_record"]; !ok {
		t.Errorf("undeclared segment key dropped: %v", ms.AdditionalProperties)
	}
	inc := ms.Records[1].MobileIncrementalSnapshotRecord
	if inc == nil || inc.Data.MobileMutationData == nil {
		t.Fatalf("records[1] = %s / data %s", typeName(ms.Records[1].Variant()), typeName(inc.Data.Variant()))
	}
	if len(inc.Data.MobileMutationData.Updates) != 2 || inc.Data.MobileMutationData.Updates[0].TextWireframeUpdate == nil {
		t.Errorf("wireframe updates not discriminated: %+v", inc.Data.MobileMutationData.Updates)
	}
	assertRoundTrip(t, doc, &MobileSegment{})
}

func TestMobileRecordSamples(t *testing.T) {
	cases := map[string]func(r *MobileRecord) any{
		"full-snapshot-record.json":                       func(r *MobileRecord) any { return r.MobileFullSnapshotRecord },
		"full-snapshot-record-with-composition-tree.json": func(r *MobileRecord) any { return r.MobileFullSnapshotRecord },
		"has-focus-record.json":                           func(r *MobileRecord) any { return r.FocusRecord },
		"incremental-snapshot-record.json":                func(r *MobileRecord) any { return r.MobileIncrementalSnapshotRecord },
		"incremental-snapshot-record-composition-tree-mutation.json": func(r *MobileRecord) any {
			return r.MobileIncrementalSnapshotRecord
		},
		"metadata-record.json": func(r *MobileRecord) any { return r.MetaRecord },
		"view-end-record.json": func(r *MobileRecord) any { return r.ViewEndRecord },
	}
	for name, field := range cases {
		t.Run(name, func(t *testing.T) {
			doc := readSample(t, "mobile/record/"+name)
			var rec MobileRecord
			assertRoundTrip(t, doc, &rec)
			if rec.Variant() == nil || rec.Raw != nil {
				t.Fatalf("no variant selected, raw=%s", rec.Raw)
			}
			if reflect.ValueOf(field(&rec)).IsNil() {
				t.Fatalf("selected %s instead of the expected variant", typeName(rec.Variant()))
			}
		})
	}

	var rec MobileRecord
	assertRoundTrip(t, readSample(t, "mobile/record/full-snapshot-record.json"), &rec)
	wf := rec.MobileFullSnapshotRecord.Data.Wireframes
	if wf[0].TextWireframe == nil || wf[1].ImageWireframe == nil || wf[4].ShapeWireframe == nil {
		t.Errorf("wireframe variants: %s %s %s", typeName(wf[0].Variant()), typeName(wf[1].Variant()), typeName(wf[4].Variant()))
	}
	if g := wf[4].ShapeWireframe.ShapeStyle.BackgroundGradient; g == nil || g.Type != "linear" || len(g.Stops) != 2 || g.StartPoint.Y.String() != "0.5" {
		t.Errorf("gradient: %+v", g)
	}

	assertRoundTrip(t, readSample(t, "mobile/record/incremental-snapshot-record-composition-tree-mutation.json"), &rec)
	ct := rec.MobileIncrementalSnapshotRecord.Data.CompositionTreeMutationData
	if ct == nil || ct.Root.ID != 1 || len(ct.Adds) != 1 || ct.Adds[0].Modifiers[0].CompositionLayerGaussianBlurModifier == nil {
		t.Errorf("composition tree mutation: %+v", ct)
	}
}

func TestBrowserRecordSamples(t *testing.T) {
	cases := map[string]func(r *BrowserRecord) any{
		"change-record.json":                    func(r *BrowserRecord) any { return r.BrowserChangeRecord },
		"full-snapshot-change-record.json":      func(r *BrowserRecord) any { return r.BrowserFullSnapshotChangeRecord },
		"full-snapshot-unspecified-record.json": func(r *BrowserRecord) any { return r.BrowserFullSnapshotV1Record },
		"full-snapshot-v1-record.json":          func(r *BrowserRecord) any { return r.BrowserFullSnapshotV1Record },
	}
	for name, field := range cases {
		t.Run(name, func(t *testing.T) {
			doc := readSample(t, "browser/record/"+name)
			var rec BrowserRecord
			assertRoundTrip(t, doc, &rec)
			if rec.Variant() == nil || rec.Raw != nil {
				t.Fatalf("no variant selected, raw=%s", rec.Raw)
			}
			if reflect.ValueOf(field(&rec)).IsNil() {
				t.Fatalf("selected %s instead of the expected variant", typeName(rec.Variant()))
			}
		})
	}

	// The V1 snapshot is a real DOM tree: walk it through the recursive type.
	var rec BrowserRecord
	assertRoundTrip(t, readSample(t, "browser/record/full-snapshot-v1-record.json"), &rec)
	root := rec.BrowserFullSnapshotV1Record.Data.Node
	if root.DocumentNodeWithId == nil {
		t.Fatalf("root is %s", typeName(root.Variant()))
	}
	var html *ElementNodeWithId
	for _, c := range root.DocumentNodeWithId.ChildNodes {
		if c.ElementNodeWithId != nil && c.ElementNodeWithId.TagName == "html" {
			html = c.ElementNodeWithId
		}
	}
	if html == nil || html.Attributes["lang"] != "en" {
		t.Fatalf("html element not found or attributes lost: %+v", html)
	}
	if depth := treeDepth(root); depth < 4 {
		t.Errorf("sample tree depth = %d, expected a nested document", depth)
	}
	if *rec.BrowserFullSnapshotV1Record.Format != 0 {
		t.Errorf("format = %v", *rec.BrowserFullSnapshotV1Record.Format)
	}

	// The change-format snapshot keeps its tuple payload verbatim as []any.
	assertRoundTrip(t, readSample(t, "browser/record/change-record.json"), &rec)
	data := rec.BrowserChangeRecord.Data
	first, ok := data[0].([]any)
	if !ok || first[0] != json.Number("0") || first[1] != "string" {
		t.Errorf("change tuple = %#v", data[0])
	}
}

func treeDepth(n *SerializedNodeWithId) int {
	var kids []SerializedNodeWithId
	switch {
	case n.DocumentNodeWithId != nil:
		kids = n.DocumentNodeWithId.ChildNodes
	case n.ElementNodeWithId != nil:
		kids = n.ElementNodeWithId.ChildNodes
	case n.DocumentFragmentNodeWithId != nil:
		kids = n.DocumentFragmentNodeWithId.ChildNodes
	}
	max := 0
	for i := range kids {
		if d := treeDepth(&kids[i]); d > max {
			max = d
		}
	}
	return max + 1
}

// TestBrowserSegmentAllVariants wraps the upstream browser records and
// synthetic records for every other record type and every incremental data
// source in a synthetic segment envelope, then checks that Decode selects
// BrowserSegment and every record lands in its variant.
func TestBrowserSegmentAllVariants(t *testing.T) {
	type expect struct {
		record string
		data   string // incremental data variant, when record is BrowserIncrementalSnapshotRecord
	}
	records := []struct {
		doc  string
		want expect
	}{
		{string(readSample(t, "browser/record/full-snapshot-v1-record.json")), expect{"BrowserFullSnapshotV1Record", ""}},
		{string(readSample(t, "browser/record/full-snapshot-change-record.json")), expect{"BrowserFullSnapshotChangeRecord", ""}},
		{string(readSample(t, "browser/record/change-record.json")), expect{"BrowserChangeRecord", ""}},
		{`{"timestamp":1,"type":3,"data":{"source":0,"adds":[{"parentId":1,"nextId":null,"node":{"id":9,"type":3,"textContent":"hi"}}],"removes":[{"id":2,"parentId":1}],"texts":[{"id":9,"value":null}],"attributes":[{"id":1,"attributes":{"class":"x","hidden":null}}]}}`,
			expect{"BrowserIncrementalSnapshotRecord", "BrowserMutationData"}},
		{`{"timestamp":1,"type":3,"data":{"source":1,"positions":[{"x":1.5,"y":2,"id":3,"timeOffset":0}]}}`, expect{"BrowserIncrementalSnapshotRecord", "MousemoveData"}},
		{`{"timestamp":1,"type":3,"data":{"source":6,"positions":[]}}`, expect{"BrowserIncrementalSnapshotRecord", "MousemoveData"}},
		{`{"timestamp":1,"type":3,"data":{"source":2,"type":1,"id":3,"x":1,"y":2}}`, expect{"BrowserIncrementalSnapshotRecord", "MouseInteractionData"}},
		{`{"timestamp":1,"type":3,"data":{"source":2,"type":5,"id":3}}`, expect{"BrowserIncrementalSnapshotRecord", "MouseInteractionData"}},
		{`{"timestamp":1,"type":3,"data":{"source":3,"id":3,"x":0,"y":100}}`, expect{"BrowserIncrementalSnapshotRecord", "ScrollData"}},
		{`{"timestamp":1,"type":3,"data":{"source":4,"width":800,"height":600}}`, expect{"BrowserIncrementalSnapshotRecord", "ViewportResizeData"}},
		{`{"timestamp":1,"type":3,"data":{"source":5,"id":3,"text":"abc"}}`, expect{"BrowserIncrementalSnapshotRecord", "InputData"}},
		{`{"timestamp":1,"type":3,"data":{"source":5,"id":3,"isChecked":true}}`, expect{"BrowserIncrementalSnapshotRecord", "InputData"}},
		{`{"timestamp":1,"type":3,"data":{"source":7,"id":3,"type":0}}`, expect{"BrowserIncrementalSnapshotRecord", "MediaInteractionData"}},
		{`{"timestamp":1,"type":3,"data":{"source":8,"id":3,"adds":[{"rule":"a{}","index":[0,1]}],"removes":[{"index":2}]}}`, expect{"BrowserIncrementalSnapshotRecord", "StyleSheetRuleData"}},
		{`{"timestamp":1,"type":3,"data":{"source":9,"pointerEventType":"down","pointerType":"touch","pointerId":1,"x":1,"y":2}}`, expect{"BrowserIncrementalSnapshotRecord", "PointerInteractionData"}},
		{`{"timestamp":1,"type":4,"data":{"width":1,"height":2,"href":"https://example.org/?a=1&b=<2>"}}`, expect{"MetaRecord", ""}},
		{`{"timestamp":1,"type":6,"data":{"has_focus":true}}`, expect{"FocusRecord", ""}},
		{`{"timestamp":1,"type":7,"slotId":"slot-1"}`, expect{"ViewEndRecord", ""}},
		{`{"timestamp":1,"type":8,"data":{"height":1,"offsetLeft":0,"offsetTop":0,"pageLeft":0,"pageTop":0,"scale":1.25,"width":2}}`, expect{"VisualViewportRecord", ""}},
		{`{"timestamp":1,"type":9,"data":{"frustrationTypes":["rage_click"],"recordIds":[1,2]}}`, expect{"FrustrationRecord", ""}},
	}
	var docs [][]byte
	for _, r := range records {
		docs = append(docs, []byte(r.doc))
	}
	doc := browserSegment(docs...)

	seg, err := Decode(doc)
	if err != nil {
		t.Fatal(err)
	}
	bs, ok := seg.(*BrowserSegment)
	if !ok {
		t.Fatalf("decoded as %T", seg)
	}
	if got := bs.SchemaRoots(); !reflect.DeepEqual(got, []string{"session-replay-browser-schema.json"}) {
		t.Errorf("SchemaRoots = %v", got)
	}
	if len(bs.Records) != len(records) {
		t.Fatalf("%d records decoded, want %d", len(bs.Records), len(records))
	}
	seenRecord, seenData := map[string]bool{}, map[string]bool{}
	for i, r := range records {
		rec := bs.Records[i]
		got := typeName(rec.Variant())
		if got != r.want.record {
			t.Errorf("records[%d]: %s, want %s", i, got, r.want.record)
			continue
		}
		seenRecord[got] = true
		if r.want.data != "" {
			if d := typeName(rec.BrowserIncrementalSnapshotRecord.Data.Variant()); d != r.want.data {
				t.Errorf("records[%d].data: %s, want %s", i, d, r.want.data)
			} else {
				seenData[d] = true
			}
		}
	}
	for _, v := range browserRecordVariants {
		if !seenRecord[v.Name] {
			t.Errorf("record variant %s not covered", v.Name)
		}
	}
	for _, v := range browserIncrementalDataVariants {
		if !seenData[v.Name] {
			t.Errorf("incremental data variant %s not covered", v.Name)
		}
	}
	// null-valued declared keys (nextId, value, hidden) survive as null.
	assertRoundTrip(t, doc, &BrowserSegment{})
}

func TestMobileVariantsCovered(t *testing.T) {
	// Every mobile record and incremental data variant is exercised by
	// upstream samples except the three below, which are synthetic.
	synthetic := []struct {
		doc  string
		want string
	}{
		{`{"timestamp":1,"type":8,"data":{"height":1,"offsetLeft":0,"offsetTop":0,"pageLeft":0,"pageTop":0,"scale":1,"width":2}}`, "VisualViewportRecord"},
		{`{"timestamp":1,"type":11,"data":{"source":2,"positions":[{"id":1,"x":10,"y":20,"timestamp":1}]}}`, "TouchData"},
		{`{"timestamp":1,"type":11,"data":{"source":4,"width":1,"height":2}}`, "ViewportResizeData"},
		{`{"timestamp":1,"type":11,"data":{"source":9,"pointerEventType":"up","pointerType":"pen","pointerId":1,"x":1,"y":2}}`, "PointerInteractionData"},
	}
	for _, s := range synthetic {
		var rec MobileRecord
		assertRoundTrip(t, []byte(s.doc), &rec)
		got := typeName(rec.Variant())
		if rec.MobileIncrementalSnapshotRecord != nil {
			got = typeName(rec.MobileIncrementalSnapshotRecord.Data.Variant())
		}
		if got != s.want {
			t.Errorf("%s: got %s", s.doc, got)
		}
	}
}

func TestRecursiveTreeRoundTrip(t *testing.T) {
	// A synthetic 12-level DOM chain: document > html > div > ... > text,
	// with an undeclared key and an unknown node type half way down.
	const depth = 12
	leaf := `{"id":99,"type":3,"textContent":"deep"}`
	unknown := `{"id":98,"type":42,"future":{"n":9007199254740993}}`
	inner := leaf
	for i := depth; i >= 1; i-- {
		extra := ""
		if i == 6 {
			extra = `,"zz_new":"kept"`
			inner = inner + "," + unknown
		}
		inner = fmt.Sprintf(`{"id":%d,"type":2,"tagName":"div","attributes":{"data-level":%d},"childNodes":[%s]%s}`, i, i, inner, extra)
	}
	doc := []byte(`{"timestamp":9007199254740993,"type":2,"format":0,"data":{"node":{"id":0,"type":0,"childNodes":[` + inner + `]},"initialOffset":{"top":0,"left":0.5}}}`)

	var rec BrowserRecord
	assertRoundTrip(t, doc, &rec)
	v1 := rec.BrowserFullSnapshotV1Record
	if v1 == nil {
		t.Fatalf("variant %s", typeName(rec.Variant()))
	}
	if v1.Timestamp != 9007199254740993 {
		t.Errorf("timestamp = %d", v1.Timestamp)
	}
	if v1.Data.InitialOffset.Left.String() != "0.5" {
		t.Errorf("initialOffset.left = %s", v1.Data.InitialOffset.Left)
	}
	if d := treeDepth(v1.Data.Node); d != depth+2 {
		t.Errorf("tree depth = %d, want %d", d, depth+2)
	}
	n := v1.Data.Node.DocumentNodeWithId.ChildNodes[0].ElementNodeWithId
	for level := 1; level <= depth; level++ {
		if n.ID != int64(level) || n.Attributes["data-level"] != json.Number(fmt.Sprint(level)) {
			t.Fatalf("level %d: id=%d attributes=%v", level, n.ID, n.Attributes)
		}
		if level == 6 {
			if n.AdditionalProperties["zz_new"] != "kept" {
				t.Errorf("undeclared key at depth 6 lost: %v", n.AdditionalProperties)
			}
			if len(n.ChildNodes) != 2 || n.ChildNodes[1].Variant() != nil || n.ChildNodes[1].Raw == nil {
				t.Fatalf("unknown node type not kept raw: %+v", n.ChildNodes)
			}
			if !bytes.Contains(n.ChildNodes[1].Raw, []byte("9007199254740993")) {
				t.Errorf("raw unknown node lost its big number: %s", n.ChildNodes[1].Raw)
			}
		}
		if level == depth {
			if txt := n.ChildNodes[0].TextNodeWithId; txt == nil || txt.TextContent != "deep" {
				t.Fatalf("leaf = %s", typeName(n.ChildNodes[0].Variant()))
			}
			break
		}
		n = n.ChildNodes[0].ElementNodeWithId
		if n == nil {
			t.Fatalf("chain broken at level %d", level+1)
		}
	}
}

func TestUnknownRecordKeptRaw(t *testing.T) {
	doc := browserSegment(
		[]byte(`{"timestamp":1,"type":7}`),
		[]byte(`{"timestamp":2,"type":99,"data":{"future":true,"big":9223372036854775807}}`),
	)
	seg, err := Decode(doc)
	if err != nil {
		t.Fatal(err)
	}
	bs := seg.(*BrowserSegment)
	if bs.Records[1].Variant() != nil || bs.Records[1].Raw == nil {
		t.Fatalf("unknown record: variant=%s raw=%s", typeName(bs.Records[1].Variant()), bs.Records[1].Raw)
	}
	assertRoundTrip(t, doc, &BrowserSegment{})
	out, _ := json.Marshal(bs)
	if !bytes.Contains(out, []byte(`9223372036854775807`)) {
		t.Errorf("raw record lost its big number: %s", out)
	}
}

func TestUnknownFieldsAndNumbers(t *testing.T) {
	doc := []byte(`{"application":{"id":"a","zz_app":1},"session":{"id":"s"},"view":{"id":"v"},"source":"browser",` +
		`"start":9007199254740993,"end":2,"records_count":1,"creation_reason":"init","zz_segment":[1,2],` +
		`"records":[{"timestamp":1,"type":4,"data":{"width":1,"height":2,"zz_data":"x"},"zz_record":9223372036854775807}]}`)
	seg, err := Decode(doc)
	if err != nil {
		t.Fatal(err)
	}
	bs := seg.(*BrowserSegment)
	if bs.Start != 9007199254740993 {
		t.Errorf("start = %d", bs.Start)
	}
	if bs.Application.AdditionalProperties["zz_app"] != json.Number("1") {
		t.Errorf("application extra = %v", bs.Application.AdditionalProperties)
	}
	if _, ok := bs.AdditionalProperties["zz_segment"]; !ok {
		t.Errorf("segment extra = %v", bs.AdditionalProperties)
	}
	meta := bs.Records[0].MetaRecord
	if meta.AdditionalProperties["zz_record"] != json.Number("9223372036854775807") || meta.Data.AdditionalProperties["zz_data"] != "x" {
		t.Errorf("record extras = %v / %v", meta.AdditionalProperties, meta.Data.AdditionalProperties)
	}
	assertRoundTrip(t, doc, &BrowserSegment{})
}

func TestDecodeRejectsUnknownAndInvalid(t *testing.T) {
	var unknown *UnknownSegmentError
	_, err := Decode([]byte(`{"source":"tvos","records":[]}`))
	if !errors.As(err, &unknown) || unknown.Source != "tvos" {
		t.Errorf("unknown source: %v", err)
	}
	_, err = Decode([]byte(`{"records":[]}`))
	if !errors.As(err, &unknown) || unknown.Source != "" {
		t.Errorf("missing source: %v", err)
	}
	for _, bad := range []string{`[]`, `null`, `"browser"`, `{"source":"browser"} x`, `{"source":"browser","start":"soon"}`} {
		if _, err := Decode([]byte(bad)); err == nil {
			t.Errorf("%s: expected an error", bad)
		}
	}
}

func TestNoFloat64InGeneratedCode(t *testing.T) {
	for _, dir := range []string{".", ".."} {
		matches, _ := filepath.Glob(filepath.Join(dir, "*_gen.go"))
		if len(matches) == 0 {
			t.Fatalf("no generated files in %s", dir)
		}
		for _, m := range matches {
			src, err := os.ReadFile(m)
			if err != nil {
				t.Fatal(err)
			}
			if bytes.Contains(src, []byte("float64")) {
				t.Errorf("%s mentions float64", m)
			}
		}
	}
}

// TestGeneratedCodeIsUpToDate regenerates the package into a temporary
// directory and compares it with the checked-in files. It needs a checkout of
// DataDog/rum-events-format in RUM_EVENTS_FORMAT and is skipped otherwise.
func TestGeneratedCodeIsUpToDate(t *testing.T) {
	checkout := os.Getenv("RUM_EVENTS_FORMAT")
	if checkout == "" {
		t.Skip("RUM_EVENTS_FORMAT not set")
	}
	tmp := t.TempDir()
	cmd := exec.Command("go", "run", "../internal/gen", "-preset", "replay", "-schemas", filepath.Join(checkout, "schemas"), "-out", tmp)
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("generator failed: %v\n%s", err, out)
	}
	checked, _ := filepath.Glob("*_gen.go")
	fresh, _ := filepath.Glob(filepath.Join(tmp, "*_gen.go"))
	if len(checked) != len(fresh) {
		t.Fatalf("%d generated files checked in, generator produced %d", len(checked), len(fresh))
	}
	for _, name := range checked {
		want, err := os.ReadFile(filepath.Join(tmp, name))
		if err != nil {
			t.Fatal(err)
		}
		got, _ := os.ReadFile(name)
		if !bytes.Equal(want, got) {
			t.Errorf("%s is stale: run `RUM_EVENTS_FORMAT=%s go generate ./rumevents/...`", name, checkout)
		}
	}
}
