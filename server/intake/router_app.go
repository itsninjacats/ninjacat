package intake

import (
	"encoding/binary"
	"log"
	"net/http"
	"strings"

	"github.com/DataDog/agent-payload/v5/metrics/intake_v3"
	"github.com/gin-gonic/gin"
	"google.golang.org/protobuf/proto"
)

// app.<site> — columnar metrics, intake v3.
//
//	config: dd_url, plus use_v3_api.series.enabled: true. The default,
//	        "datadog_only", keeps a third-party dd_url on /api/v2/series
//	        (router_api.go). Sketches need the URL listed under
//	        serializer_experimental_use_v3_api.sketches.endpoints; the
//	        v3beta path is a shadow copy of v2 traffic, and its route is
//	        overridable via ...sketches.beta_route.
//
// Three routes, one message: intake_v3.Payload from agent-payload. Series
// and sketches differ only in the metricType nibble of each Types entry.
//
// The body is NOT one protobuf blob. The agent compresses every field header
// and every column as its own zstd/gzip frame and concatenates them, relying
// on the decompressor to splice consecutive frames — which klauspost zstd and
// Go's gzip both do, so after Decompress() the bytes are a plain Payload.
// Length prefixes inside describe the UNCOMPRESSED data, so decompress first
// and parse second, never the other way round. Details in docs/, section 5.2.
//
// Decoded but not stored yet.
func (a *Server) routeApp(g *gin.RouterGroup) {
	g.POST("/api/intake/metrics/v3/series", a.handleAppV3("v3series"))
	g.POST("/api/intake/metrics/v3/sketches", a.handleAppV3("v3sketches"))
	g.POST("/api/intake/metrics/v3beta/sketches", a.handleAppV3("v3beta")) // shadow of /api/beta/sketches
}

func (a *Server) handleAppV3(label string) gin.HandlerFunc {
	return func(c *gin.Context) {
		defer c.JSON(http.StatusAccepted, gin.H{})

		body, err := c.GetRawData()
		if err != nil {
			log.Printf("[%s] cannot read body: %v", label, err)
			return
		}
		// The raw payload outlives every path out of this handler: a decoder
		// that fails or does not exist yet must not make the bytes disappear.
		defer func() { _ = body }()

		// A pointer, not a value: proto messages carry a mutex-bearing
		// MessageState, so the hand-off below must not copy the struct.
		payload := new(intake_v3.Payload)
		if err := proto.Unmarshal(body, payload); err != nil {
			log.Printf("[%s] protobuf: %v (%d bytes)", label, err, len(body))
			return
		}

		// Everything below this line is a read-only view for the log. The
		// helpers unpack a few dictionaries into strings and count what the
		// Types nibbles say; they never modify payload and the result is not
		// what gets stored. The full columnar struct — every dictionary, every
		// per-series ref column, every per-point value column, and the
		// sketch bins — stays intact in payload until the hand-off below.
		md := payload.GetMetricData()
		if len(md.GetTypes()) == 0 {
			// proto.Unmarshal fails OPEN — see router_containers.go. Here an
			// undecompressed body is the other likely cause: a zstd frame
			// happens to parse as garbage-but-valid protobuf with no columns.
			// Logged but not dropped: Metadata may still be populated, and
			// deciding what an empty MetricData means is the owner's call.
			log.Printf("[%s] decoded to zero series (%d bytes) — wrong payload type or still compressed?", label, len(body))
		}

		names := appV3StrDict(md.GetDictNameStr())
		hosts := appV3Hosts(md, payload.GetMetadata())

		var points, sketches uint64
		for i, t := range md.GetTypes() {
			if i < len(md.GetNumPoints()) {
				points += md.GetNumPoints()[i]
			}
			if intake_v3.MetricType(t&0xF) == intake_v3.MetricType_Sketch {
				sketches++
			}
		}

		log.Printf("[%s] hosts=%v — %d series (%d sketches), %d points, %d names, %d B",
			label, hosts, len(md.GetTypes()), sketches, points, len(names), len(body))
		if tags := payload.GetMetadata().GetTags(); len(tags) > 0 {
			log.Printf("   metadata tags=%v", tags)
		}
		log.Printf("   %s", appV3Join(names, 20))

		// TODO(ninjacat): tables. Complete, unconverted, ready to take.
		//
		// payload is the intake_v3.Payload exactly as proto.Unmarshal filled
		// it: Metadata (Tags, Resources) and MetricData with all 29 columns.
		// The log above touched only DictNameStr, DictResourceStr/Len/Type/
		// Name, Types, NumPoints and the two Metadata slices. Untouched and
		// still here: DictTagStr, DictTagsets, DictSourceTypeName,
		// DictOriginInfo, DictUnitStr, NameRefs, TagsetRefs, ResourcesRefs,
		// Intervals, SourceTypeNameRefs, OriginInfoRefs, UnitRefs,
		// Timestamps, ValsSint64, ValsFloat32, ValsFloat64, SketchNumBins,
		// SketchBinKeys, SketchBinCnts.
		//
		// Reading rules for whoever takes it: dictionary indexes are base-1
		// (0 = empty), ref columns and Timestamps are delta-encoded across
		// the whole array, SketchBinKeys per series; the value column for a
		// series is picked by the ValueType nibble of its Types entry, and
		// ValsSint64 must stay int64 — never widen through float64. A zero
		// timestamp means "not given", not 1970. fakeintake's metricReaderV3
		// is the reference walk.
		_ = payload
	}
}

// appV3StrDict unpacks a "varint length + bytes" string dictionary.
//
// Index 0 is the implicit empty string: every reference column is base-1, so
// the slice is laid out to be indexed directly. Truncated input stops the
// walk rather than failing — this is a log line, not a validator.
func appV3StrDict(raw []byte) []string {
	dict := []string{""}
	for len(raw) > 0 {
		n, k := binary.Uvarint(raw)
		if k <= 0 || n > uint64(len(raw)-k) {
			break
		}
		dict = append(dict, string(raw[k:k+int(n)]))
		raw = raw[k+int(n):]
	}
	return dict
}

// appV3Hosts lists the distinct "host" resources in a payload.
//
// The v3 format has no host field. A serie's host is one [Type, Name] pair
// in its resource set — the dictionary walk below — or, for the whole
// payload, a pair in Metadata.Resources. Type and name columns are each
// delta-encoded across the entire dictionary, not per set.
func appV3Hosts(md *intake_v3.MetricData, meta *intake_v3.Metadata) []string {
	strs := appV3StrDict(md.GetDictResourceStr())
	types, names := md.GetDictResourceType(), md.GetDictResourceName()

	seen := map[string]bool{}
	var hosts []string
	add := func(typ, name string) {
		if typ == "host" && !seen[name] {
			seen[name] = true
			hosts = append(hosts, name)
		}
	}

	var typeRef, nameRef int64
	var pos int64
	for _, size := range md.GetDictResourceLen() {
		for i := int64(0); i < size && pos < int64(len(types)) && pos < int64(len(names)); i++ {
			typeRef += types[pos]
			nameRef += names[pos]
			pos++
			if typeRef < 0 || nameRef < 0 || typeRef >= int64(len(strs)) || nameRef >= int64(len(strs)) {
				continue
			}
			add(strs[typeRef], strs[nameRef])
		}
	}

	res := meta.GetResources()
	for i := 0; i+1 < len(res); i += 2 {
		add(res[i], res[i+1])
	}
	return hosts
}

// appV3Join renders a name dictionary for one log line, capped at limit.
func appV3Join(dict []string, limit int) string {
	names := dict[1:] // skip the implicit empty entry
	if len(names) <= limit {
		return strings.Join(names, " ")
	}
	return strings.Join(names[:limit], " ") + " ..."
}
