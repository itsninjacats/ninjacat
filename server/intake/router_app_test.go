package intake

import (
	"encoding/binary"
	"errors"
	"net/http"
	"reflect"
	"testing"
	"time"

	"github.com/DataDog/agent-payload/v5/metrics/intake_v3"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
	"google.golang.org/protobuf/proto"
)

// The v3 columnar intake has no encoder outside the agent, so the tests bring
// their own. Everything below builds the columns the way the agent's
// serializer does — varint-length dictionaries, delta-encoded reference
// columns, length-prefixed tagsets — because a payload assembled by hand from
// literal deltas is unreadable, and a test nobody can read is a test nobody
// will fix when the format moves.

// v3Dict packs strings into a "varint length + bytes" dictionary. The empty
// string at index 0 is implicit and is never written, so the first string
// passed here is reference 1.
func v3Dict(strs ...string) []byte {
	var out []byte
	for _, s := range strs {
		out = binary.AppendUvarint(out, uint64(len(s)))
		out = append(out, s...)
	}
	return out
}

// v3Delta turns absolute references into the delta column the wire carries.
// The accumulator runs across the WHOLE array, which is what makes a column
// that is one entry short corrupt everything after it.
func v3Delta(refs ...int64) []int64 {
	out := make([]int64, len(refs))
	var acc int64
	for i, r := range refs {
		out[i] = r - acc
		acc = r
	}
	return out
}

// v3Tagsets packs tagset entries: each set is a length followed by its
// delta-encoded members, with the accumulator restarting per set. A NEGATIVE
// member is a back-reference to an earlier set rather than a tag index.
func v3Tagsets(sets ...[]int64) []int64 {
	var out []int64
	for _, set := range sets {
		out = append(out, int64(len(set)))
		var acc int64
		for _, idx := range set {
			out = append(out, idx-acc)
			acc = idx
		}
	}
	return out
}

// v3Resources packs resource sets given as {typeRef, nameRef} pairs. Type and
// Name are two parallel delta columns whose accumulators restart at each set.
func v3Resources(sets ...[][2]int64) (lens, typeCol, nameCol []int64) {
	for _, set := range sets {
		lens = append(lens, int64(len(set)))
		var accType, accName int64
		for _, pair := range set {
			typeCol = append(typeCol, pair[0]-accType)
			nameCol = append(nameCol, pair[1]-accName)
			accType, accName = pair[0], pair[1]
		}
	}
	return lens, typeCol, nameCol
}

// v3BinKeys delta-encodes one sketch point's bin keys. Unlike every other
// delta column this one restarts at each POINT.
func v3BinKeys(keys ...int32) []int32 {
	out := make([]int32, len(keys))
	var acc int32
	for i, k := range keys {
		out[i] = k - acc
		acc = k
	}
	return out
}

func v3Type(t intake_v3.MetricType, v intake_v3.ValueType, flags ...intake_v3.MetricFlags) uint64 {
	packed := uint64(t) | uint64(v)
	for _, f := range flags {
		packed |= uint64(f)
	}
	return packed
}

// v3Payload is the worked example the row assertions below are written
// against: three series off one agent, covering everything a real batch mixes
// in a single body.
//
//	series 0  system.cpu.user        Count  / Sint64   two points, host web-01
//	series 1  http.request.duration  Gauge  / Float64  one point, host web-02,
//	                                 a unit, a device resource, and a tagset
//	                                 that back-references series 0's tagset
//	series 2  queue.depth            Sketch / Float64  one point, three bins
//
// Payload-wide Metadata.Tags add env:staging to all three, which is what makes
// env multi-valued next to the series' own env:prod — the multiset case
// docs/decisions/0001-tags-are-a-multiset.md exists for.
func v3Payload() *intake_v3.Payload {
	lens, typeCol, nameCol := v3Resources(
		[][2]int64{{1, 2}},         // host=web-01
		[][2]int64{{1, 5}, {3, 4}}, // host=web-02, device=eth0
	)

	return &intake_v3.Payload{
		Metadata: &intake_v3.Metadata{
			Tags:      []string{"env:staging"},
			Resources: []string{"host", "fallback-host"},
		},
		MetricData: &intake_v3.MetricData{
			DictNameStr: v3Dict("system.cpu.user", "http.request.duration", "queue.depth"),
			DictTagStr:  v3Dict("env:prod", "kube_service:a", "kube_service:b", "role:web"),
			DictTagsets: v3Tagsets(
				[]int64{1, 2, 3}, // env:prod, kube_service:a, kube_service:b
				[]int64{-1, 4},   // that set again, plus role:web
			),
			DictResourceStr:    v3Dict("host", "web-01", "device", "eth0", "web-02"),
			DictResourceLen:    lens,
			DictResourceType:   typeCol,
			DictResourceName:   nameCol,
			DictSourceTypeName: v3Dict("system"),
			DictUnitStr:        v3Dict("millisecond"),
			DictOriginInfo:     []int32{10, 11, 42},

			Types: []uint64{
				v3Type(intake_v3.MetricType_Count, intake_v3.ValueType_Sint64),
				v3Type(intake_v3.MetricType_Gauge, intake_v3.ValueType_Float64, intake_v3.MetricFlags_flagHasUnit),
				v3Type(intake_v3.MetricType_Sketch, intake_v3.ValueType_Float64),
			},
			NameRefs:           v3Delta(1, 2, 3),
			TagsetRefs:         v3Delta(1, 2, 1),
			ResourcesRefs:      v3Delta(1, 2, 1),
			SourceTypeNameRefs: v3Delta(1, 0, 0),
			OriginInfoRefs:     v3Delta(1, 1, 1),
			UnitRefs:           v3Delta(1), // compact: one entry, for the one flagged series
			Intervals:          []uint64{15, 10, 0},
			NumPoints:          []uint64{2, 1, 1},

			Timestamps:  v3Delta(1700000000, 1700000015, 1700000020, 1700000030),
			ValsSint64:  []int64{7, 9, 6}, // series 0's two points, then the sketch count
			ValsFloat64: []float64{1.5, 12, 1, 9},
			// The sketch summary is sum, min, max in three consecutive
			// entries of the Float64 column; the count is always a sint64.
			SketchNumBins: []uint64{3},
			SketchBinKeys: v3BinKeys(4, 5, 9),
			SketchBinCnts: []uint32{1, 2, 3},
		},
	}
}

func v3Time(sec int64) time.Time { return time.Unix(sec, 0).UTC() }

// The whole point of this branch: a v3 batch must produce exactly the rows the
// v1/v2 handlers would have produced for the same metrics, because it goes
// into the same two tables and a dashboard cannot tell which intake a series
// arrived on.
func TestAppV3DecodeBuildsRows(t *testing.T) {
	res := appV3Decode("acme", v3Payload())

	if res.Err != nil {
		t.Fatalf("decode reported %v, want a clean walk", res.Err)
	}

	cpuTags := map[string][]string{
		"env":          {"prod", "staging"},
		"kube_service": {"a", "b"},
	}
	wantPoints := []storage.MetricPoint{
		{
			TenantID: "acme", Timestamp: v3Time(1700000000),
			Metric: "system.cpu.user", Host: "web-01",
			MetricType: "COUNT", SourceType: "system", Unit: "",
			Interval: 15, Value: 7, Tags: cpuTags,
		},
		{
			TenantID: "acme", Timestamp: v3Time(1700000015),
			Metric: "system.cpu.user", Host: "web-01",
			MetricType: "COUNT", SourceType: "system", Unit: "",
			Interval: 15, Value: 9, Tags: cpuTags,
		},
		{
			TenantID: "acme", Timestamp: v3Time(1700000020),
			Metric: "http.request.duration", Host: "web-02",
			MetricType: "GAUGE", SourceType: "", Unit: "millisecond",
			Interval: 10, Value: 1.5,
			Tags: map[string][]string{
				"env":          {"prod", "staging"},
				"kube_service": {"a", "b"},
				"role":         {"web"},
			},
		},
	}
	if !reflect.DeepEqual(res.Points, wantPoints) {
		t.Errorf("metric points:\n got %+v\nwant %+v", res.Points, wantPoints)
	}

	wantSketches := []storage.SketchRow{{
		TenantID: "acme", Timestamp: v3Time(1700000030),
		Metric: "queue.depth", Host: "web-01", Tags: cpuTags,
		Count: 6, Min: 1, Max: 9, Avg: 2, Sum: 12,
		BucketKeys: []int32{4, 5, 9}, BucketCounts: []uint32{1, 2, 3},
	}}
	if !reflect.DeepEqual(res.Sketches, wantSketches) {
		t.Errorf("sketch rows:\n got %+v\nwant %+v", res.Sketches, wantSketches)
	}

	// A resource that is not the host has nowhere to go but the tally.
	if got := res.Resources["device=eth0"]; got != 1 {
		t.Errorf("non-host resources: got %v, want device=eth0 once", res.Resources)
	}
	if got := res.Origins["10/11/42"]; got != 3 {
		t.Errorf("origins: got %v, want 10/11/42 three times", res.Origins)
	}
	if !reflect.DeepEqual(res.Hosts, []string{"web-01", "web-02"}) {
		t.Errorf("hosts: got %v, want [web-01 web-02] in first-seen order", res.Hosts)
	}
	if res.WideInts != 0 || res.NoIndex != 0 {
		t.Errorf("wide ints %d, no-index %d: want none for this payload", res.WideInts, res.NoIndex)
	}
}

// Reference 0 is the empty value in every dictionary, and a series that
// carries no name, no tags and no resources is not an error: it is what a
// metric submitted with nothing but a value looks like. The payload-wide
// Metadata then supplies the host and the tags, which is the only path by
// which a v3 row gets a host at all when the series does not name one.
func TestAppV3DecodeBaseOneEmptyRefs(t *testing.T) {
	payload := &intake_v3.Payload{
		Metadata: &intake_v3.Metadata{
			Tags:      []string{"env:prod"},
			Resources: []string{"host", "fallback-host", "device", "sda"},
		},
		MetricData: &intake_v3.MetricData{
			DictNameStr:   v3Dict("only.metric"),
			Types:         []uint64{v3Type(intake_v3.MetricType_Gauge, intake_v3.ValueType_Zero)},
			NameRefs:      v3Delta(0), // the implicit empty name
			TagsetRefs:    v3Delta(0),
			ResourcesRefs: v3Delta(0),
			NumPoints:     []uint64{1},
			Timestamps:    v3Delta(1700000000),
		},
	}

	res := appV3Decode("acme", payload)
	if res.Err != nil {
		t.Fatalf("decode reported %v, want a clean walk — empty references are legal", res.Err)
	}
	if len(res.Points) != 1 {
		t.Fatalf("points: got %d, want 1", len(res.Points))
	}
	got := res.Points[0]
	if got.Metric != "" {
		t.Errorf("metric: got %q, want empty — reference 0 is the implicit empty string", got.Metric)
	}
	if got.Host != "fallback-host" {
		t.Errorf("host: got %q, want fallback-host from Metadata.Resources", got.Host)
	}
	// ValueType_Zero puts nothing in any value column.
	if got.Value != 0 {
		t.Errorf("value: got %v, want 0 — ValueType_Zero is not on the wire", got.Value)
	}
	if !reflect.DeepEqual(got.Tags, map[string][]string{"env": {"prod"}}) {
		t.Errorf("tags: got %v, want the payload-wide env:prod", got.Tags)
	}
	if res.Resources["device=sda"] != 1 {
		t.Errorf("metadata device resource not tallied: %v", res.Resources)
	}
}

// A count of things does not fit a Float64 once it passes 2^53, and the value
// column is a Float64. The row is still written — a rounded value beats a
// hole in a graph — but the payload is kept so the exact integer is
// recoverable, which is what WideInts drives.
func TestAppV3DecodeCountsInt64PrecisionLoss(t *testing.T) {
	const beyond = int64(1)<<53 + 3

	payload := &intake_v3.Payload{
		MetricData: &intake_v3.MetricData{
			DictNameStr:   v3Dict("bytes.total"),
			Types:         []uint64{v3Type(intake_v3.MetricType_Count, intake_v3.ValueType_Sint64)},
			NameRefs:      v3Delta(1),
			TagsetRefs:    v3Delta(0),
			ResourcesRefs: v3Delta(0),
			NumPoints:     []uint64{2},
			Timestamps:    v3Delta(1700000000, 1700000015),
			ValsSint64:    []int64{beyond, 42},
		},
	}

	res := appV3Decode("acme", payload)
	if res.WideInts != 1 {
		t.Fatalf("wide ints: got %d, want 1 — only the first value is past 2^53", res.WideInts)
	}
	if len(res.Points) != 2 {
		t.Fatalf("points: got %d, want 2 — a rounded value is still stored", len(res.Points))
	}
	if res.Points[1].Value != 42 {
		t.Errorf("second point: got %v, want 42 — the small value must be exact", res.Points[1].Value)
	}
}

// A sketch summary's count is always a sint64 while sum/min/max follow the
// ValueType nibble, so the same summary encodes differently per value type.
// Every variant has to land on the same row, and Avg is reconstructed rather
// than read: the wire does not carry it.
func TestAppV3DecodeSketchSummaryPerValueType(t *testing.T) {
	base := func(vt intake_v3.ValueType) *intake_v3.MetricData {
		return &intake_v3.MetricData{
			DictNameStr:   v3Dict("latency"),
			Types:         []uint64{v3Type(intake_v3.MetricType_Sketch, vt)},
			NameRefs:      v3Delta(1),
			TagsetRefs:    v3Delta(0),
			ResourcesRefs: v3Delta(0),
			NumPoints:     []uint64{1},
			Timestamps:    v3Delta(1700000000),
			SketchNumBins: []uint64{2},
			SketchBinKeys: v3BinKeys(-3, 7),
			SketchBinCnts: []uint32{4, 6},
		}
	}

	cases := []struct {
		name string
		md   *intake_v3.MetricData
		want storage.SketchRow
	}{
		{
			name: "float64 summary, count alongside in the sint64 column",
			md: func() *intake_v3.MetricData {
				md := base(intake_v3.ValueType_Float64)
				md.ValsFloat64 = []float64{20, 1, 9}
				md.ValsSint64 = []int64{10}
				return md
			}(),
			want: storage.SketchRow{Count: 10, Sum: 20, Min: 1, Max: 9, Avg: 2},
		},
		{
			name: "float32 summary",
			md: func() *intake_v3.MetricData {
				md := base(intake_v3.ValueType_Float32)
				md.ValsFloat32 = []float32{20, 1, 9}
				md.ValsSint64 = []int64{10}
				return md
			}(),
			want: storage.SketchRow{Count: 10, Sum: 20, Min: 1, Max: 9, Avg: 2},
		},
		{
			name: "sint64 summary: four consecutive entries, sum min max count",
			md: func() *intake_v3.MetricData {
				md := base(intake_v3.ValueType_Sint64)
				md.ValsSint64 = []int64{20, 1, 9, 10}
				return md
			}(),
			want: storage.SketchRow{Count: 10, Sum: 20, Min: 1, Max: 9, Avg: 2},
		},
		{
			// An all-zero summary is not on the wire at all, but the count
			// still is — which is the one thing a naive "Zero means nothing
			// was sent" reading gets wrong, misaligning the sint64 column for
			// every series after it.
			name: "zero summary still spends one sint64 on the count",
			md: func() *intake_v3.MetricData {
				md := base(intake_v3.ValueType_Zero)
				md.ValsSint64 = []int64{5}
				return md
			}(),
			want: storage.SketchRow{Count: 5, Sum: 0, Min: 0, Max: 0, Avg: 0},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			res := appV3Decode("acme", &intake_v3.Payload{MetricData: tc.md})
			if res.Err != nil {
				t.Fatalf("decode reported %v", res.Err)
			}
			if len(res.Sketches) != 1 {
				t.Fatalf("sketch rows: got %d, want 1", len(res.Sketches))
			}
			got := res.Sketches[0]
			want := tc.want
			want.TenantID = "acme"
			want.Timestamp = v3Time(1700000000)
			want.Metric = "latency"
			want.BucketKeys = []int32{-3, 7}
			want.BucketCounts = []uint32{4, 6}
			if !reflect.DeepEqual(got, want) {
				t.Errorf("sketch row:\n got %+v\nwant %+v", got, want)
			}
		})
	}
}

// A short column means every cursor after it points at another series' data,
// so the walk stops there. What it already read is still rows — the payload
// goes to raw_payloads for the rest.
func TestAppV3DecodeStopsAtAShortColumn(t *testing.T) {
	payload := &intake_v3.Payload{
		MetricData: &intake_v3.MetricData{
			DictNameStr:   v3Dict("a", "b"),
			Types:         []uint64{v3Type(intake_v3.MetricType_Gauge, intake_v3.ValueType_Float64), v3Type(intake_v3.MetricType_Gauge, intake_v3.ValueType_Float64)},
			NameRefs:      v3Delta(1, 2),
			TagsetRefs:    v3Delta(0, 0),
			ResourcesRefs: v3Delta(0, 0),
			NumPoints:     []uint64{1, 1},
			Timestamps:    v3Delta(1700000000, 1700000001),
			ValsFloat64:   []float64{1.5}, // one value for two points
		},
	}

	res := appV3Decode("acme", payload)
	if res.Err == nil {
		t.Fatal("a short value column must be reported, otherwise the payload is dropped silently")
	}
	if !errors.Is(res.Err, errV3Truncated) {
		t.Errorf("error: got %v, want it to wrap errV3Truncated", res.Err)
	}
	if len(res.Points) != 1 || res.Points[0].Value != 1.5 {
		t.Errorf("points: got %+v, want the one row the walk did read", res.Points)
	}
}

// Nothing here may panic on a payload we did not write: a nil message, a nil
// MetricData and an empty one all arrive from senders in the wild.
func TestAppV3DecodeToleratesNils(t *testing.T) {
	cases := []struct {
		name string
		in   *intake_v3.Payload
	}{
		{"nil payload", nil},
		{"no metric data", &intake_v3.Payload{}},
		{"empty metric data", &intake_v3.Payload{MetricData: &intake_v3.MetricData{}}},
		{"metadata only", &intake_v3.Payload{Metadata: &intake_v3.Metadata{Tags: []string{"env:prod"}}}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			res := appV3Decode("acme", tc.in)
			if len(res.Points) != 0 || len(res.Sketches) != 0 {
				t.Errorf("got %d points and %d sketches, want none", len(res.Points), len(res.Sketches))
			}
		})
	}
}

// An odd number of Metadata.Resources elements cannot be [Type, Name] pairs.
// The pairs that ARE there are still read; the payload is kept raw.
func TestAppV3DecodeOddMetadataResources(t *testing.T) {
	res := appV3Decode("acme", &intake_v3.Payload{
		Metadata: &intake_v3.Metadata{Resources: []string{"host", "web-01", "device"}},
		MetricData: &intake_v3.MetricData{
			DictNameStr: v3Dict("m"), Types: []uint64{v3Type(intake_v3.MetricType_Gauge, intake_v3.ValueType_Zero)},
			NameRefs: v3Delta(1), TagsetRefs: v3Delta(0), ResourcesRefs: v3Delta(0),
			NumPoints: []uint64{1}, Timestamps: v3Delta(1700000000),
		},
	})
	if res.Err == nil {
		t.Fatal("an odd resources list must be reported")
	}
	if len(res.Points) != 1 || res.Points[0].Host != "web-01" {
		t.Errorf("points: got %+v, want the host from the complete pair", res.Points)
	}
}

// ---------------------------------------------------------------------------
// Column readers
// ---------------------------------------------------------------------------

func TestAppV3StrDict(t *testing.T) {
	cases := []struct {
		name    string
		in      []byte
		want    []string
		wantErr bool
	}{
		{"empty input is the implicit empty entry alone", nil, []string{""}, false},
		{"base-1", v3Dict("a", "bb"), []string{"", "a", "bb"}, false},
		{"an empty string is a legal entry", v3Dict("", "a"), []string{"", "", "a"}, false},
		{
			// A frame that was cut short must not silently shorten the
			// dictionary: every reference past the cut would then resolve to
			// the wrong string instead of being reported.
			name: "truncated value", in: append(v3Dict("ok"), 0x05, 'a', 'b'),
			want: []string{"", "ok"}, wantErr: true,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, err := appV3StrDict(tc.in)
			if (err != nil) != tc.wantErr {
				t.Fatalf("error: got %v, wantErr %v", err, tc.wantErr)
			}
			if !reflect.DeepEqual(got, tc.want) {
				t.Errorf("dict: got %q, want %q", got, tc.want)
			}
		})
	}
}

func TestAppV3Tagsets(t *testing.T) {
	tagStr := []string{"", "env:prod", "kube_service:a", "kube_service:b"}

	cases := []struct {
		name    string
		packed  []int64
		want    [][]string
		wantErr bool
	}{
		{"no sets", nil, [][]string{nil}, false},
		{
			name:   "two tags sharing a key stay two tags",
			packed: v3Tagsets([]int64{2, 3}),
			want:   [][]string{nil, {"kube_service:a", "kube_service:b"}},
		},
		{
			// This is why the format is dense at all: every series on a host
			// shares that host's tags, so the second set says "the first set,
			// plus one".
			name:   "a negative index includes an earlier set whole",
			packed: v3Tagsets([]int64{1, 2}, []int64{-1, 3}),
			want: [][]string{nil,
				{"env:prod", "kube_service:a"},
				{"env:prod", "kube_service:a", "kube_service:b"},
			},
		},
		{
			name:    "a length longer than what follows",
			packed:  []int64{4, 1},
			want:    [][]string{nil},
			wantErr: true,
		},
		{
			name:    "an index past the tag dictionary",
			packed:  v3Tagsets([]int64{9}),
			want:    [][]string{nil},
			wantErr: true,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, err := appV3Tagsets(tc.packed, tagStr)
			if (err != nil) != tc.wantErr {
				t.Fatalf("error: got %v, wantErr %v", err, tc.wantErr)
			}
			if !reflect.DeepEqual(got, tc.want) {
				t.Errorf("tagsets: got %v, want %v", got, tc.want)
			}
		})
	}
}

// The resource deltas restart at every set. Reading them as one run across the
// dictionary — the way every OTHER delta column in this format works — puts
// the wrong host name on every series after the first, which is the kind of
// wrong that looks like working software.
func TestAppV3ResourcesRestartsDeltasPerSet(t *testing.T) {
	strs := []string{"", "host", "web-01", "device", "eth0", "web-02"}
	lens, typeCol, nameCol := v3Resources(
		[][2]int64{{1, 2}},
		[][2]int64{{1, 5}, {3, 4}},
	)

	got, err := appV3Resources(lens, typeCol, nameCol, strs)
	if err != nil {
		t.Fatalf("unpack: %v", err)
	}
	want := [][]appV3Resource{
		nil,
		{{Type: "host", Name: "web-01"}},
		{{Type: "host", Name: "web-02"}, {Type: "device", Name: "eth0"}},
	}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("resource sets:\n got %v\nwant %v", got, want)
	}

	if _, err := appV3Resources([]int64{3}, []int64{1}, []int64{1}, strs); !errors.Is(err, errV3Truncated) {
		t.Errorf("a set longer than its columns: got %v, want errV3Truncated", err)
	}
	if _, err := appV3Resources([]int64{1}, []int64{99}, []int64{1}, strs); !errors.Is(err, errV3Ref) {
		t.Errorf("a reference past the dictionary: got %v, want errV3Ref", err)
	}
}

func TestAppV3Origins(t *testing.T) {
	got, err := appV3Origins([]int32{10, 11, 42, 1, 0, 7})
	if err != nil {
		t.Fatalf("unpack: %v", err)
	}
	want := []appV3Origin{{}, {10, 11, 42}, {1, 0, 7}}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("origins: got %v, want %v", got, want)
	}
	if _, err := appV3Origins([]int32{10, 11}); !errors.Is(err, errV3Truncated) {
		t.Errorf("a partial triple: got %v, want errV3Truncated", err)
	}
}

// The proto does not say whether UnitRefs has one entry per series or one per
// flagged series, and the two readings put different units on different
// metrics. The length is the only evidence a payload carries, so it decides —
// and a length that fits neither reading drops the units rather than guessing.
func TestAppV3UnitMode(t *testing.T) {
	flagged := v3Type(intake_v3.MetricType_Gauge, intake_v3.ValueType_Float64, intake_v3.MetricFlags_flagHasUnit)
	plain := v3Type(intake_v3.MetricType_Gauge, intake_v3.ValueType_Float64)

	cases := []struct {
		name    string
		types   []uint64
		refs    int
		want    appV3UnitLayout
		wantErr bool
	}{
		{"no column", []uint64{plain, flagged}, 0, appV3UnitNone, false},
		{"one entry per series", []uint64{plain, flagged}, 2, appV3UnitParallel, false},
		{"one entry per flagged series", []uint64{plain, flagged, plain}, 1, appV3UnitCompact, false},
		{"neither", []uint64{plain, flagged, plain}, 2, appV3UnitNone, true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, err := appV3UnitMode(tc.types, tc.refs)
			if (err != nil) != tc.wantErr {
				t.Fatalf("error: got %v, wantErr %v", err, tc.wantErr)
			}
			if got != tc.want {
				t.Errorf("layout: got %v, want %v", got, tc.want)
			}
		})
	}
}

// The compact and parallel layouts must put the same unit on the same metric.
func TestAppV3DecodeUnitLayouts(t *testing.T) {
	md := func(refs []int64) *intake_v3.MetricData {
		return &intake_v3.MetricData{
			DictNameStr: v3Dict("plain", "timed"),
			DictUnitStr: v3Dict("millisecond"),
			Types: []uint64{
				v3Type(intake_v3.MetricType_Gauge, intake_v3.ValueType_Zero),
				v3Type(intake_v3.MetricType_Gauge, intake_v3.ValueType_Zero, intake_v3.MetricFlags_flagHasUnit),
			},
			NameRefs: v3Delta(1, 2), TagsetRefs: v3Delta(0, 0), ResourcesRefs: v3Delta(0, 0),
			NumPoints: []uint64{1, 1}, Timestamps: v3Delta(1700000000, 1700000001),
			UnitRefs: refs,
		}
	}

	for _, tc := range []struct {
		name string
		refs []int64
	}{
		{"compact: only the flagged series has an entry", v3Delta(1)},
		{"parallel: every series has an entry", v3Delta(0, 1)},
	} {
		t.Run(tc.name, func(t *testing.T) {
			res := appV3Decode("acme", &intake_v3.Payload{MetricData: md(tc.refs)})
			if res.Err != nil {
				t.Fatalf("decode reported %v", res.Err)
			}
			if len(res.Points) != 2 {
				t.Fatalf("points: got %d, want 2", len(res.Points))
			}
			if res.Points[0].Unit != "" {
				t.Errorf("unflagged series unit: got %q, want empty", res.Points[0].Unit)
			}
			if res.Points[1].Unit != "millisecond" {
				t.Errorf("flagged series unit: got %q, want millisecond", res.Points[1].Unit)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// The handler
// ---------------------------------------------------------------------------

// Through the real engine: the key guard admits the request, the tenant off
// the key lands on every row, series go to the metrics writer and sketches to
// the sketches writer, and the agent still gets the 202 with an empty object
// it has always got.
func TestHandleAppV3StoresSeriesAndSketches(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeApp)

	body, err := proto.Marshal(v3Payload())
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	w := post(t, e, "/api/intake/metrics/v3/series", body)
	if w.Code != http.StatusAccepted {
		t.Fatalf("POST: got %d, want 202 (%s)", w.Code, w.Body.String())
	}
	if got := w.Body.String(); got != "{}" {
		t.Errorf("body: got %q, want {} — the agent's ack contract", got)
	}

	points := Rows[storage.MetricPoint](node)
	if len(points) != 3 {
		t.Fatalf("metric points: got %d, want 3", len(points))
	}
	sketches := Rows[storage.SketchRow](node)
	if len(sketches) != 1 {
		t.Fatalf("sketch rows: got %d, want 1", len(sketches))
	}
	for i, p := range points {
		if p.TenantID != testTenant {
			t.Errorf("point %d tenant: got %q, want %q", i, p.TenantID, testTenant)
		}
	}
	if sketches[0].TenantID != testTenant {
		t.Errorf("sketch tenant: got %q, want %q", sketches[0].TenantID, testTenant)
	}
	if n := len(node.SentTo(storage.MetricsWriter)); n != 1 {
		t.Errorf("messages to the metrics writer: got %d, want 1 — one batch per request", n)
	}
	if n := len(node.SentTo(storage.SketchesWriter)); n != 1 {
		t.Errorf("messages to the sketches writer: got %d, want 1", n)
	}
	// A payload that decoded cleanly must not also be mirrored into
	// raw_payloads: that table is a fallback, not a copy of the traffic.
	if n := len(Rows[storage.RawPayloadRow](node)); n != 0 {
		t.Errorf("raw payload rows: got %d, want 0 for a clean decode", n)
	}
}

// All three routes are the same handler, and the sketches route is the one an
// agent configured for v3 sketches actually posts to.
func TestHandleAppV3RoutesAllThreePaths(t *testing.T) {
	body, err := proto.Marshal(v3Payload())
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	for _, path := range []string{
		"/api/intake/metrics/v3/series",
		"/api/intake/metrics/v3/sketches",
		"/api/intake/metrics/v3beta/sketches",
	} {
		t.Run(path, func(t *testing.T) {
			a, node := newTestServer(t)
			e := newTestEngine(t, a, a.routeApp)
			if w := post(t, e, path, body); w.Code != http.StatusAccepted {
				t.Fatalf("got %d, want 202", w.Code)
			}
			if n := len(Rows[storage.MetricPoint](node)); n != 3 {
				t.Errorf("metric points: got %d, want 3", n)
			}
		})
	}
}

// proto.Unmarshal fails OPEN on this format: a body that is still compressed,
// or that belongs to another intake entirely, often parses into a structurally
// valid Payload with no columns at all. That used to be a log line and a
// dropped body; the bytes are the only evidence of what the sender meant, so
// they go to raw_payloads.
func TestHandleAppV3KeepsUndecodableBodies(t *testing.T) {
	cases := []struct {
		name       string
		body       []byte
		wantReason string
	}{
		{"not protobuf at all", []byte{0xff, 0xff, 0xff, 0xff}, "decode_error"},
		{"decodes to zero series", func() []byte {
			b, err := proto.Marshal(&intake_v3.Payload{
				Metadata: &intake_v3.Metadata{Tags: []string{"env:prod"}},
			})
			if err != nil {
				panic(err)
			}
			return b
		}(), "unexpected_shape"},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			a, node := newTestServer(t)
			e := newTestEngine(t, a, a.routeApp)

			if w := post(t, e, "/api/intake/metrics/v3/series", tc.body); w.Code != http.StatusAccepted {
				t.Fatalf("got %d, want 202 — an undecodable body is still acknowledged", w.Code)
			}

			raw := Rows[storage.RawPayloadRow](node)
			if len(raw) != 1 {
				t.Fatalf("raw payload rows: got %d, want 1", len(raw))
			}
			if raw[0].Reason != tc.wantReason {
				t.Errorf("reason: got %q, want %q", raw[0].Reason, tc.wantReason)
			}
			if raw[0].Intake != "v3series" {
				t.Errorf("intake: got %q, want the router's own label v3series", raw[0].Intake)
			}
			if raw[0].Body != string(tc.body) {
				t.Errorf("body: got %q, want the bytes the handler saw", raw[0].Body)
			}
			if n := len(Rows[storage.MetricPoint](node)); n != 0 {
				t.Errorf("metric points: got %d, want 0", n)
			}
		})
	}
}

// A value past 2^53 rounds in the Float64 column, so the payload is kept
// alongside the rows — the rounded number is usable, the exact one stays
// recoverable.
func TestHandleAppV3KeepsPayloadForWideInt64(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeApp)

	body, err := proto.Marshal(&intake_v3.Payload{
		MetricData: &intake_v3.MetricData{
			DictNameStr:   v3Dict("bytes.total"),
			Types:         []uint64{v3Type(intake_v3.MetricType_Count, intake_v3.ValueType_Sint64)},
			NameRefs:      v3Delta(1),
			TagsetRefs:    v3Delta(0),
			ResourcesRefs: v3Delta(0),
			NumPoints:     []uint64{1},
			Timestamps:    v3Delta(1700000000),
			ValsSint64:    []int64{int64(1)<<53 + 3},
		},
	})
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	if w := post(t, e, "/api/intake/metrics/v3/series", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}
	if n := len(Rows[storage.MetricPoint](node)); n != 1 {
		t.Errorf("metric points: got %d, want 1 — the rounded row is still stored", n)
	}
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 || raw[0].Reason != "int64_precision" {
		t.Fatalf("raw payload rows: got %+v, want one with reason int64_precision", raw)
	}
}

// Without a tenant there is no first ORDER BY column to write under, so the
// request is answered exactly as before and nothing is stored.
func TestHandleAppV3WithoutTenantStoresNothing(t *testing.T) {
	a, node := newTestServer(t)
	// No key guard: the handler runs with no key in the context.
	e := a.baseEngine()
	a.routeApp(e.Group(""))

	body, err := proto.Marshal(v3Payload())
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if w := post(t, e, "/api/intake/metrics/v3/series", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}
	if n := len(node.Sends()); n != 0 {
		t.Errorf("%d messages stored without a tenant, want 0", n)
	}
}

// ---------------------------------------------------------------------------
// ClickHouse
// ---------------------------------------------------------------------------

// The arity tests prove the rows match their INSERTs by count. Only ClickHouse
// proves the v3 walk produces values those columns accept — the bucket key
// arrays in particular, which arrive as Int32/UInt32 slices built here rather
// than copied straight off a proto message as the v2 path does.
func TestAppV3RowsRoundTripThroughClickHouse(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	res := appV3Decode("default", v3Payload())
	if res.Err != nil {
		t.Fatalf("decode reported %v", res.Err)
	}

	// The payload's timestamps are a fixed point in 2023, and both tables drop
	// a part older than their 30-day TTL the moment it lands — the insert
	// succeeds and counts as zero rows, with no error anywhere. So the rows go
	// in stamped now; what the delta decoding made of the wire timestamps is
	// pinned by the decoder tests above, which need no database.
	now := time.Now().UTC().Truncate(time.Second)

	points := make([]storage.Row, 0, len(res.Points))
	for _, p := range res.Points {
		p.Timestamp = now
		points = append(points, p)
	}
	storagetest.Insert(t, conn, storage.MetricsWriter, points)

	sketches := make([]storage.Row, 0, len(res.Sketches))
	for _, s := range res.Sketches {
		s.Timestamp = now
		sketches = append(sketches, s)
	}
	storagetest.Insert(t, conn, storage.SketchesWriter, sketches)

	if got := storagetest.Count(t, conn, "metrics"); got != 3 {
		t.Errorf("metrics: %d rows, want 3", got)
	}
	if got := storagetest.Count(t, conn, "sketches"); got != 1 {
		t.Errorf("sketches: %d rows, want 1", got)
	}

	row := storagetest.QueryRow(t, conn,
		"SELECT metric, host, metric_type, unit, interval, value, tags['kube_service'] "+
			"FROM metrics WHERE metric = 'http.request.duration'")
	if len(row) != 7 {
		t.Fatalf("SELECT returned %d columns, want 7", len(row))
	}
	if row[0] != "http.request.duration" || row[1] != "web-02" {
		t.Errorf("metric/host: got %v/%v", row[0], row[1])
	}
	if row[2] != "GAUGE" || row[3] != "millisecond" {
		t.Errorf("metric_type/unit: got %v/%v, want GAUGE/millisecond", row[2], row[3])
	}
	if row[4] != uint32(10) {
		t.Errorf("interval: got %v (%T), want 10", row[4], row[4])
	}
	if row[5] != 1.5 {
		t.Errorf("value: got %v, want 1.5", row[5])
	}
	// The multiset survives the trip: two tags sharing a key are two values.
	if !reflect.DeepEqual(row[6], []string{"a", "b"}) {
		t.Errorf("tags['kube_service']: got %v, want [a b]", row[6])
	}

	sketch := storagetest.QueryRow(t, conn,
		"SELECT metric, count, sum, min, max, avg, bucket_keys, bucket_counts FROM sketches")
	if len(sketch) != 8 {
		t.Fatalf("SELECT returned %d columns, want 8", len(sketch))
	}
	if sketch[0] != "queue.depth" || sketch[1] != uint64(6) {
		t.Errorf("metric/count: got %v/%v", sketch[0], sketch[1])
	}
	if !reflect.DeepEqual(sketch[6], []int32{4, 5, 9}) {
		t.Errorf("bucket_keys: got %v, want [4 5 9] — delta-decoded per point", sketch[6])
	}
	if !reflect.DeepEqual(sketch[7], []uint32{1, 2, 3}) {
		t.Errorf("bucket_counts: got %v, want [1 2 3]", sketch[7])
	}
}
