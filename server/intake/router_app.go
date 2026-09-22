package intake

import (
	"encoding/binary"
	"errors"
	"fmt"
	"log"
	"math"
	"net/http"
	"strings"

	"github.com/DataDog/agent-payload/v5/metrics/intake_v3"
	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
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
// STORED. The columnar walk below (appV3Decode) turns the payload into the
// same two tables the v1/v2 handlers feed, because it is the same signal in a
// denser encoding:
//
//	Count / Rate / Gauge series -> storage.MetricsWriter  -> metrics
//	Sketch series               -> storage.SketchesWriter -> sketches
//
// A body that does not decode, a payload that decodes to no series, and a
// column walk that runs off the end of a column all go to raw_payloads
// (intake/raw.go). docs/tables/metrics_v3.md has the column mapping and what
// v3 carries that neither table has a home for.
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

		// A pointer, not a value: proto messages carry a mutex-bearing
		// MessageState, so the hand-off below must not copy the struct.
		payload := new(intake_v3.Payload)
		if err := proto.Unmarshal(body, payload); err != nil {
			log.Printf("[%s] protobuf: %v (%d bytes)", label, err, len(body))
			a.storeRaw(c, label, "decode_error", err.Error(), body)
			return
		}

		md := payload.GetMetricData()
		if len(md.GetTypes()) == 0 {
			// proto.Unmarshal fails OPEN — see router_containers.go. Here an
			// undecompressed body is the other likely cause: a zstd frame
			// happens to parse as garbage-but-valid protobuf with no columns.
			// Nothing can be converted, so the bytes go to raw_payloads
			// instead of into a log line and a bin.
			log.Printf("[%s] decoded to zero series (%d bytes) — wrong payload type or still compressed?", label, len(body))
			a.storeRaw(c, label, "unexpected_shape",
				fmt.Sprintf("protobuf decoded but MetricData carries no series; metadata tags=%d resources=%d",
					len(payload.GetMetadata().GetTags()), len(payload.GetMetadata().GetResources())), body)
			return
		}

		tenant := TenantFromContext(c)
		res := appV3Decode(tenant, payload)

		log.Printf("[%s] hosts=%v — %d series (%d sketch series), %d points, %d sketches, %d names, %d B",
			label, res.Hosts, res.Series, res.SketchSeries,
			len(res.Points), len(res.Sketches), len(res.Names)-1, len(body))
		if tags := payload.GetMetadata().GetTags(); len(tags) > 0 {
			log.Printf("   metadata tags (applied to every series) = %v", tags)
		}
		log.Printf("   %s", appV3Join(res.Names, 20))
		// Origin and the non-host resources have no column on either table,
		// so the tally is the only record they leave; both are listed under
		// dropped by decision in docs/tables/metrics_v3.md.
		log.Printf("   origins (product/category/service, no column yet): %s", apiTally(res.Origins))
		if len(res.Resources) > 0 {
			log.Printf("   non-host resources (no column yet): %s", apiTally(res.Resources))
		}
		if res.NoIndex > 0 {
			log.Printf("   %d series flagged do-not-index by the agent (flagNoIndex)", res.NoIndex)
		}

		if tenant == "" {
			return
		}
		a.store(storage.MetricsWriter, storage.WriteMetrics{Points: res.Points}, len(res.Points))
		a.store(storage.SketchesWriter, storage.WriteSketches{Sketches: res.Sketches}, len(res.Sketches))

		// The rows above are what the walk could read; the two calls below
		// keep what it could not. Both are partial-failure paths — a payload
		// that decodes cleanly is never mirrored into raw_payloads.
		if res.Err != nil {
			log.Printf("[%s] columnar walk: %v — payload kept raw", label, res.Err)
			a.storeRaw(c, label, "unexpected_shape", res.Err.Error(), body)
		}
		if res.WideInts > 0 {
			// Float64 keeps 53 bits of mantissa and the value column is a
			// Float64, so a sint64 past 2^53 arrives rounded. The row is
			// still written (a rounded value beats no value) and the payload
			// is kept, so the exact integer stays recoverable.
			log.Printf("[%s] %d sint64 values beyond 2^53 — stored rounded, payload kept raw", label, res.WideInts)
			a.storeRaw(c, label, "int64_precision",
				fmt.Sprintf("%d sint64 values beyond 2^53 widened to Float64", res.WideInts), body)
		}
	}
}

// ---------------------------------------------------------------------------
// The columnar walk
// ---------------------------------------------------------------------------

// appV3Result is one payload turned into rows, plus everything the walk saw
// that no column takes.
type appV3Result struct {
	Points   []storage.MetricPoint
	Sketches []storage.SketchRow

	Series       int      // entries in Types
	SketchSeries int      // of which sketches
	Names        []string // the name dictionary, base-1, for the log line
	Hosts        []string // distinct host resources, in first-seen order

	// Origins counts "product/category/service"; Resources counts
	// "type=name" for every resource that is not the host. Neither has a
	// column — see docs/tables/metrics_v3.md.
	Origins   map[string]int
	Resources map[string]int

	NoIndex  int // series carrying flagNoIndex
	WideInts int // sint64 values that do not survive a Float64 exactly

	// Err is the FIRST inconsistency the walk met. It never stops the rows
	// already decoded from being stored — it means the payload ALSO goes to
	// raw_payloads, so the part we could not read is not gone.
	Err error
}

// appV3Resource is one [Type, Name] pair out of a resource set.
type appV3Resource struct{ Type, Name string }

// appV3Origin is one (product, category, service) triple out of
// DictOriginInfo. The numbers are Datadog's private origin.proto enums, the
// same ones apiOriginName renders for v2.
type appV3Origin struct{ Product, Category, Service uint32 }

var (
	errV3Truncated = errors.New("column ends mid-element")
	errV3Ref       = errors.New("reference outside its dictionary")
)

// maxExactInt is the largest integer a Float64 represents exactly. The value
// column is a Float64, so a sint64 past this is rounded on the way in — which
// is a thing to count and keep the payload for, never a reason to fail.
const maxExactInt = int64(1) << 53

// appV3Decode walks the columnar payload and builds the rows.
//
// This encoding has no published reader outside the agent, so the walk follows
// the agent's own MetricDataReader (comp/dogstatsd/http/impl/internal/reader,
// over the identical dogstatsdhttp proto) rule for rule, because these rules
// are not guessable from the .proto alone:
//
//   - every dictionary index is base-1, with the empty value implicit at 0;
//   - a tagset is a length followed by delta-encoded indexes into DictTagStr,
//     and a NEGATIVE index is not a tag but an earlier tagset included whole;
//   - resource Type and Name deltas restart at each resource SET, unlike every
//     other delta column, which accumulates across the whole array;
//   - the value column is chosen by the ValueType nibble, and ValueType_Zero
//     consumes nothing at all;
//   - a sketch point spends three consecutive entries of its value column on
//     sum, min, max and always one entry of ValsSint64 on the count — so a
//     Sint64 sketch spends four consecutive sint64s while a Float64 sketch
//     spends three float64s plus one sint64. Avg is not on the wire; the
//     intake reconstructs it as sum/count;
//   - SketchBinKeys is delta-encoded per POINT — not per series, and not
//     across the array like every other delta column.
//
// The walk is deliberately more forgiving than the agent's reader: where the
// reader returns an error and the caller drops the payload, this records the
// error in Result.Err — which sends the bytes to raw_payloads — and keeps the
// rows it already has. Losing forty good series because the forty-first column
// is short is the failure this project exists to avoid.
func appV3Decode(tenant string, payload *intake_v3.Payload) appV3Result {
	res := appV3Result{Origins: map[string]int{}, Resources: map[string]int{}}
	md := payload.GetMetricData()

	fail := func(err error) {
		if res.Err == nil {
			res.Err = err
		}
	}

	// --- dictionaries ------------------------------------------------------
	//
	// A truncated dictionary is recorded and then used as far as it goes: the
	// series referencing the part we did read are still good rows, and the
	// payload is kept raw either way.
	names, err := appV3StrDict(md.GetDictNameStr())
	if err != nil {
		fail(fmt.Errorf("dictNameStr: %w", err))
	}
	res.Names = names

	tagStr, err := appV3StrDict(md.GetDictTagStr())
	if err != nil {
		fail(fmt.Errorf("dictTagStr: %w", err))
	}
	tagsets, err := appV3Tagsets(md.GetDictTagsets(), tagStr)
	if err != nil {
		fail(fmt.Errorf("dictTagsets: %w", err))
	}
	resourceStr, err := appV3StrDict(md.GetDictResourceStr())
	if err != nil {
		fail(fmt.Errorf("dictResourceStr: %w", err))
	}
	resourceSets, err := appV3Resources(md.GetDictResourceLen(),
		md.GetDictResourceType(), md.GetDictResourceName(), resourceStr)
	if err != nil {
		fail(fmt.Errorf("dictResource: %w", err))
	}
	sourceTypes, err := appV3StrDict(md.GetDictSourceTypeName())
	if err != nil {
		fail(fmt.Errorf("dictSourceTypeName: %w", err))
	}
	units, err := appV3StrDict(md.GetDictUnitStr())
	if err != nil {
		fail(fmt.Errorf("dictUnitStr: %w", err))
	}
	origins, err := appV3Origins(md.GetDictOriginInfo())
	if err != nil {
		fail(fmt.Errorf("dictOriginInfo: %w", err))
	}

	// --- payload-wide metadata --------------------------------------------
	//
	// Metadata.Tags apply to EVERY series, and Metadata.Resources carries the
	// host of a payload whose series do not name one themselves.
	metaTags := payload.GetMetadata().GetTags()
	metaHost := ""
	metaRes := payload.GetMetadata().GetResources()
	for i := 0; i+1 < len(metaRes); i += 2 {
		if metaRes[i] == "host" {
			if metaHost == "" {
				metaHost = metaRes[i+1]
			}
			continue
		}
		res.Resources[metaRes[i]+"="+metaRes[i+1]]++
	}
	if len(metaRes)%2 != 0 {
		fail(fmt.Errorf("metadata.resources has %d elements, want [Type, Name] pairs", len(metaRes)))
	}

	// --- per-series columns ------------------------------------------------
	types := md.GetTypes()
	res.Series = len(types)
	nameRefs, tagsetRefs := md.GetNameRefs(), md.GetTagsetRefs()
	resourceRefs, numPoints := md.GetResourcesRefs(), md.GetNumPoints()
	sourceRefs, originRefs := md.GetSourceTypeNameRefs(), md.GetOriginInfoRefs()
	unitRefs, intervals := md.GetUnitRefs(), md.GetIntervals()

	// A column present but SHORTER than Types is a broken payload. A column
	// that is absent altogether is a sender that never writes it — older
	// encoders have no unit and no origin column at all — and absent reads as
	// "empty" for every series, which cannot misalign anything.
	for _, col := range []struct {
		name string
		n    int
	}{
		{"nameRefs", len(nameRefs)}, {"tagsetRefs", len(tagsetRefs)},
		{"resourcesRefs", len(resourceRefs)}, {"numPoints", len(numPoints)},
		{"sourceTypeNameRefs", len(sourceRefs)}, {"originInfoRefs", len(originRefs)},
		{"intervals", len(intervals)},
	} {
		if col.n != 0 && col.n < len(types) {
			fail(fmt.Errorf("%s has %d entries for %d series", col.name, col.n, len(types)))
		}
	}

	unitMode, unitErr := appV3UnitMode(types, len(unitRefs))
	if unitErr != nil {
		fail(unitErr)
	}

	// --- the walk ----------------------------------------------------------
	timestamps := md.GetTimestamps()
	valsSint64, valsFloat32, valsFloat64 := md.GetValsSint64(), md.GetValsFloat32(), md.GetValsFloat64()
	numBinsCol, binKeys, binCnts := md.GetSketchNumBins(), md.GetSketchBinKeys(), md.GetSketchBinCnts()

	// Accumulators for the columns delta-encoded across the WHOLE array, and
	// cursors into the per-point value columns. All of it is shared by every
	// series in the payload, which is why a short column poisons the rest of
	// the walk rather than one series: the walk stops at the first one.
	var nameRef, tagsetRef, resourceRef, sourceRef, originRef, unitRef, timestamp int64
	var iSint, iF32, iF64, iNumBins, iBins, iPoint, iUnit int

	seenHost := map[string]bool{}

	// sint64 returns the next sint64 value, counting the ones a Float64
	// rounds. The count is what WideInts is for: the row is still written.
	sint64 := func() (float64, bool) {
		if iSint >= len(valsSint64) {
			return 0, false
		}
		v := valsSint64[iSint]
		iSint++
		if v > maxExactInt || v < -maxExactInt {
			res.WideInts++
		}
		return float64(v), true
	}

series:
	for i, packed := range types {
		metricType := intake_v3.MetricType(packed & 0xF)
		valueType := intake_v3.ValueType(packed & 0xF0)

		nameRef += appV3At(nameRefs, i)
		tagsetRef += appV3At(tagsetRefs, i)
		resourceRef += appV3At(resourceRefs, i)
		sourceRef += appV3At(sourceRefs, i)
		originRef += appV3At(originRefs, i)

		if packed&uint64(intake_v3.MetricFlags_flagNoIndex) != 0 {
			res.NoIndex++
		}

		metric, ok := appV3Str(names, nameRef)
		if !ok {
			fail(fmt.Errorf("series %d nameRef %d: %w", i, nameRef, errV3Ref))
		}

		var tags []string
		if tagsetRef < 0 || tagsetRef >= int64(len(tagsets)) {
			fail(fmt.Errorf("series %d tagsetRef %d: %w", i, tagsetRef, errV3Ref))
		} else {
			tags = tagsets[tagsetRef]
		}
		// Metadata tags belong to every series, and a key may legally appear
		// in both lists — tagsToMultiMap keeps both values, which is the whole
		// point of the multiset shape. Built once per series and shared by its
		// points: a row is read-only once it has been sent.
		tagMap := tagsToMultiMap(appV3Concat(tags, metaTags))

		host := metaHost
		if resourceRef < 0 || resourceRef >= int64(len(resourceSets)) {
			fail(fmt.Errorf("series %d resourcesRef %d: %w", i, resourceRef, errV3Ref))
		} else {
			for _, r := range resourceSets[resourceRef] {
				if r.Type == "host" {
					host = r.Name
					continue
				}
				res.Resources[r.Type+"="+r.Name]++
			}
		}
		if host != "" && !seenHost[host] {
			seenHost[host] = true
			res.Hosts = append(res.Hosts, host)
		}

		sourceType, ok := appV3Str(sourceTypes, sourceRef)
		if !ok {
			fail(fmt.Errorf("series %d sourceTypeNameRef %d: %w", i, sourceRef, errV3Ref))
		}

		if originRef < 0 || originRef >= int64(len(origins)) {
			fail(fmt.Errorf("series %d originInfoRef %d: %w", i, originRef, errV3Ref))
		} else {
			o := origins[originRef]
			res.Origins[fmt.Sprintf("%d/%d/%d", o.Product, o.Category, o.Service)]++
		}

		unit := ""
		switch unitMode {
		case appV3UnitParallel:
			unitRef += unitRefs[i]
			if packed&uint64(intake_v3.MetricFlags_flagHasUnit) != 0 {
				unit, _ = appV3Str(units, unitRef)
			}
		case appV3UnitCompact:
			if packed&uint64(intake_v3.MetricFlags_flagHasUnit) != 0 && iUnit < len(unitRefs) {
				unitRef += unitRefs[iUnit]
				iUnit++
				unit, _ = appV3Str(units, unitRef)
			}
		}

		interval := uint32(0)
		if i < len(intervals) {
			interval = uint32(intervals[i])
		}
		// The v3 metricType numbers are the v2 ones for Count, Rate and Gauge,
		// so the row says the same word whichever intake a series arrived on.
		typeName := metricTypeName(int32(metricType))
		if metricType == intake_v3.MetricType_Sketch {
			res.SketchSeries++
		}

		points := uint64(0)
		if i < len(numPoints) {
			points = numPoints[i]
		}
		for p := uint64(0); p < points; p++ {
			if iPoint >= len(timestamps) {
				fail(fmt.Errorf("timestamps ran out at series %d point %d: %w", i, p, errV3Truncated))
				break series
			}
			timestamp += timestamps[iPoint]
			iPoint++
			// A zero timestamp means "not given", not 1970 — wireTime is the
			// one place in this codebase that turns that into arrival time,
			// and every other intake goes through it too.
			ts := wireTime(timestamp)

			if metricType != intake_v3.MetricType_Sketch {
				var value float64
				switch valueType {
				case intake_v3.ValueType_Zero:
					// The value is zero and is not on the wire at all.
				case intake_v3.ValueType_Sint64:
					if value, ok = sint64(); !ok {
						fail(fmt.Errorf("valsSint64 ran out at series %d point %d: %w", i, p, errV3Truncated))
						break series
					}
				case intake_v3.ValueType_Float32:
					if iF32 >= len(valsFloat32) {
						fail(fmt.Errorf("valsFloat32 ran out at series %d point %d: %w", i, p, errV3Truncated))
						break series
					}
					value = float64(valsFloat32[iF32])
					iF32++
				case intake_v3.ValueType_Float64:
					if iF64 >= len(valsFloat64) {
						fail(fmt.Errorf("valsFloat64 ran out at series %d point %d: %w", i, p, errV3Truncated))
						break series
					}
					value = valsFloat64[iF64]
					iF64++
				default:
					fail(fmt.Errorf("series %d: unknown valueType %#x", i, uint64(valueType)))
				}

				res.Points = append(res.Points, storage.MetricPoint{
					TenantID: tenant, Timestamp: ts,
					Metric: metric, Host: host,
					MetricType: typeName, SourceType: sourceType, Unit: unit,
					Interval: interval, Value: value, Tags: tagMap,
				})
				continue
			}

			// --- one sketch point -------------------------------------------
			if iNumBins >= len(numBinsCol) {
				fail(fmt.Errorf("sketchNumBins ran out at series %d point %d: %w", i, p, errV3Truncated))
				break series
			}
			bins := int(numBinsCol[iNumBins])
			iNumBins++
			if bins < 0 || iBins+bins > len(binKeys) || iBins+bins > len(binCnts) {
				fail(fmt.Errorf("sketch bins ran out at series %d point %d: %w", i, p, errV3Truncated))
				break series
			}
			keys := appV3BinKeys(binKeys[iBins : iBins+bins])
			counts := append([]uint32(nil), binCnts[iBins:iBins+bins]...)
			iBins += bins

			var sum, min, max float64
			switch valueType {
			case intake_v3.ValueType_Zero:
				// sum, min and max are all zero and none of them is on the
				// wire. The count still is.
			case intake_v3.ValueType_Sint64:
				var vals [3]float64
				for k := range vals {
					if vals[k], ok = sint64(); !ok {
						fail(fmt.Errorf("valsSint64 ran out in a sketch summary at series %d point %d: %w", i, p, errV3Truncated))
						break series
					}
				}
				sum, min, max = vals[0], vals[1], vals[2]
			case intake_v3.ValueType_Float32:
				if iF32+3 > len(valsFloat32) {
					fail(fmt.Errorf("valsFloat32 ran out in a sketch summary at series %d point %d: %w", i, p, errV3Truncated))
					break series
				}
				sum = float64(valsFloat32[iF32])
				min = float64(valsFloat32[iF32+1])
				max = float64(valsFloat32[iF32+2])
				iF32 += 3
			case intake_v3.ValueType_Float64:
				if iF64+3 > len(valsFloat64) {
					fail(fmt.Errorf("valsFloat64 ran out in a sketch summary at series %d point %d: %w", i, p, errV3Truncated))
					break series
				}
				sum, min, max = valsFloat64[iF64], valsFloat64[iF64+1], valsFloat64[iF64+2]
				iF64 += 3
			default:
				fail(fmt.Errorf("series %d: unknown valueType %#x", i, uint64(valueType)))
			}

			// The count is ALWAYS a sint64, whatever the summary's value type.
			// That asymmetry is the one part of the sketch layout a reader
			// cannot infer from the ValueType nibble.
			cnt, haveCount := sint64()
			if !haveCount {
				fail(fmt.Errorf("valsSint64 ran out reading a sketch count at series %d point %d: %w", i, p, errV3Truncated))
				break series
			}
			count := uint64(0)
			if cnt > 0 {
				count = uint64(cnt)
			}

			avg := 0.0
			if count > 0 {
				// Avg is not on the wire. Datadog's intake reconstructs it as
				// sum/count and so do we, rather than leaving the column at
				// zero for every v3 sketch.
				avg = sum / float64(count)
			}

			res.Sketches = append(res.Sketches, storage.SketchRow{
				TenantID: tenant, Timestamp: ts,
				Metric: metric, Host: host, Tags: tagMap,
				Count: count, Min: min, Max: max, Avg: avg, Sum: sum,
				BucketKeys: keys, BucketCounts: counts,
			})
		}
	}

	return res
}

// ---------------------------------------------------------------------------
// Dictionary and column readers
// ---------------------------------------------------------------------------

// appV3UnitLayout says how the UnitRefs column is laid out.
//
// THE ONE AMBIGUITY IN THIS FORMAT. The proto says only "value present if
// flagHasUnit is set, entire array is delta encoded", which reads either way:
// one entry per series, like every other ref column, or one entry per FLAGGED
// series, like sketchNumBins, which only has entries for sketch points. The
// agent's reference reader predates the unit column and settles nothing, and
// choosing wrong shifts every unit in the payload onto the wrong metric.
//
// So the length decides, per payload, and a length that fits neither reading
// means no units at all plus a trip to raw_payloads: a wrong unit is worse
// than a missing one, and the bytes are kept either way. When every series is
// flagged the two readings coincide, which is the common case.
type appV3UnitLayout int

const (
	appV3UnitNone appV3UnitLayout = iota
	appV3UnitParallel
	appV3UnitCompact
)

func appV3UnitMode(types []uint64, refs int) (appV3UnitLayout, error) {
	if refs == 0 {
		return appV3UnitNone, nil
	}
	flagged := 0
	for _, t := range types {
		if t&uint64(intake_v3.MetricFlags_flagHasUnit) != 0 {
			flagged++
		}
	}
	switch {
	case refs == len(types):
		return appV3UnitParallel, nil
	case refs == flagged:
		return appV3UnitCompact, nil
	default:
		return appV3UnitNone, fmt.Errorf(
			"unitRefs has %d entries: neither one per series (%d) nor one per flagHasUnit series (%d), units dropped",
			refs, len(types), flagged)
	}
}

// appV3StrDict unpacks a "varint length + bytes" string dictionary.
//
// Index 0 is the implicit empty string: every reference column is base-1, so
// the slice is laid out to be indexed directly. Truncated input returns what
// was read AND an error — the caller keeps the good entries and sends the
// payload to raw_payloads.
func appV3StrDict(raw []byte) ([]string, error) {
	dict := []string{""}
	for len(raw) > 0 {
		n, k := binary.Uvarint(raw)
		if k <= 0 || n > uint64(len(raw)-k) {
			return dict, errV3Truncated
		}
		dict = append(dict, string(raw[k:k+int(n)]))
		raw = raw[k+int(n):]
	}
	return dict, nil
}

// appV3Tagsets unpacks the tagset dictionary: a flat sequence of "length, then
// that many delta-encoded indexes".
//
// The delta accumulator restarts at every set. A NEGATIVE accumulated index is
// not a tag — it is an earlier tagset, included whole, which is how the agent
// avoids repeating the tags every series of a host shares. Entry 0 is the
// implicit empty set, so back-references are base-1 as well.
func appV3Tagsets(packed []int64, tagStr []string) ([][]string, error) {
	sets := [][]string{nil}
	for len(packed) > 0 {
		size := packed[0]
		packed = packed[1:]
		if size < 0 || size > int64(len(packed)) {
			return sets, errV3Truncated
		}
		tags := make([]string, 0, size)
		idx := int64(0)
		for i := int64(0); i < size; i++ {
			idx += packed[i]
			switch {
			case idx < 0:
				if idx == math.MinInt64 || -idx >= int64(len(sets)) {
					return sets, errV3Ref
				}
				tags = append(tags, sets[-idx]...)
			case idx >= int64(len(tagStr)):
				return sets, errV3Ref
			default:
				tags = append(tags, tagStr[idx])
			}
		}
		packed = packed[size:]
		sets = append(sets, tags)
	}
	return sets, nil
}

// appV3Resources unpacks the resource-set dictionary.
//
// DictResourceLen gives the size of each set; Type and Name are two parallel
// delta-encoded columns covering all the sets end to end. The accumulators
// restart at each SET — the one delta column in this format that does not run
// across the whole array, and reading it the other way renames every host
// after the first.
func appV3Resources(lens, typeCol, nameCol []int64, strs []string) ([][]appV3Resource, error) {
	sets := make([][]appV3Resource, 1, len(lens)+1)
	start := int64(0)
	for _, size := range lens {
		if size < 0 || start+size > int64(len(typeCol)) || start+size > int64(len(nameCol)) {
			return sets, errV3Truncated
		}
		set := make([]appV3Resource, 0, size)
		var typeRef, nameRef int64
		for i := int64(0); i < size; i++ {
			typeRef += typeCol[start+i]
			nameRef += nameCol[start+i]
			typ, okType := appV3Str(strs, typeRef)
			name, okName := appV3Str(strs, nameRef)
			if !okType || !okName {
				return sets, errV3Ref
			}
			set = append(set, appV3Resource{Type: typ, Name: name})
		}
		sets = append(sets, set)
		start += size
	}
	return sets, nil
}

// appV3Origins unpacks DictOriginInfo: flattened (product, category, service)
// triples, base-1 like every other dictionary.
func appV3Origins(raw []int32) ([]appV3Origin, error) {
	out := make([]appV3Origin, 1, len(raw)/3+1)
	for i := 0; i+2 < len(raw); i += 3 {
		out = append(out, appV3Origin{
			Product:  uint32(raw[i]),
			Category: uint32(raw[i+1]),
			Service:  uint32(raw[i+2]),
		})
	}
	if len(raw)%3 != 0 {
		return out, errV3Truncated
	}
	return out, nil
}

// appV3BinKeys delta-decodes one point's sketch bin keys into a fresh slice.
//
// A copy, not a decode in place: the column belongs to the payload, and the
// next point's keys start their own sequence from zero.
func appV3BinKeys(packed []int32) []int32 {
	keys := make([]int32, len(packed))
	var acc int32
	for i, d := range packed {
		acc += d
		keys[i] = acc
	}
	return keys
}

// appV3Str reads a base-1 dictionary, reporting an out-of-range reference
// rather than panicking on a payload we did not write.
func appV3Str(dict []string, ref int64) (string, bool) {
	if ref < 0 || ref >= int64(len(dict)) {
		return "", false
	}
	return dict[ref], true
}

// appV3At reads a per-series column the sender may have omitted whole. An
// absent column reads as a zero delta for every series, which is exactly what
// "this sender never writes that field" means.
func appV3At(col []int64, i int) int64 {
	if i < len(col) {
		return col[i]
	}
	return 0
}

// appV3Concat joins a series' own tags with the payload-wide ones without
// touching either slice — a tagset is shared by every series that references
// it, so appending in place would leak one series' extra tags into the next.
func appV3Concat(a, b []string) []string {
	if len(b) == 0 {
		return a
	}
	if len(a) == 0 {
		return b
	}
	out := make([]string, 0, len(a)+len(b))
	out = append(out, a...)
	return append(out, b...)
}

// appV3Join renders a name dictionary for one log line, capped at limit.
func appV3Join(dict []string, limit int) string {
	names := dict[1:] // skip the implicit empty entry
	if len(names) <= limit {
		return strings.Join(names, " ")
	}
	return strings.Join(names[:limit], " ") + " ..."
}
