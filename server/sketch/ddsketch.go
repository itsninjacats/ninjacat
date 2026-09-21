// Package sketch reproduces the Datadog agent's DDSketch bucketing.
//
// It exists so sketches we build ourselves land in the same buckets as the
// ones the agent sends. That is not a nicety: bucket counts are merged by key
// across hosts, so a key that means 47.3 ms here and 12 ms there produces
// percentiles that look plausible and are wrong, with nothing to signal it.
//
// Ported from DataDog/datadog-agent, pkg/util/quantile/config.go.
//
// Deliberately NOT github.com/DataDog/sketches-go: that library derives gamma
// from the DDSketch paper's (1+a)/(1-a) and has no index offset, so the same
// value lands on a key around 200 instead of around 1600. Its sketches cannot
// be merged with the agent's.
package sketch

import "math"

// Agent defaults. Not configurable per metric — the payload carries no
// accuracy field at all, so both sides must already agree on these.
const (
	// Eps is the guaranteed relative error: 1/128, about 0.78%.
	Eps = 1.0 / 128.0

	// Min is the smallest value that gets its own key. Anything smaller
	// collapses to key 0.
	Min = 1e-9

	// BinLimit is how many buckets the agent keeps before collapsing the
	// lowest ones together.
	BinLimit = 4096
)

var (
	// Gamma is the ratio between consecutive bucket boundaries: each bucket
	// is Gamma times wider than the one below it, which is what makes the
	// relative error constant across the whole range.
	Gamma = 1 + 2*Eps

	gammaLn = math.Log1p(2 * Eps)

	// bias shifts keys so the smallest supported value gets key 1 rather
	// than a large negative number.
	bias = -int(math.Floor(math.Log(Min)/gammaLn)) + 1
)

// Bias exposes the index offset for callers that need to convert keys back to
// values in SQL rather than in Go.
func Bias() int { return bias }

// Key returns the bucket a value belongs to.
//
// Note this rounds to NEAREST, not down. The agent's own comment claims
// "γ^k <= v < γ^(k+1)", which describes floor semantics and does not match
// its code: key(1150) is 1793, while γ^(1793-bias) is 1157.95 — above the
// value. Treat γ^(k-bias) as the middle of the bucket, not its floor.
func Key(v float64) int32 {
	switch {
	case v < 0:
		return -Key(-v)
	case v == 0 || v < Min:
		return 0
	}
	return int32(int(math.RoundToEven(math.Log(v)/gammaLn)) + bias)
}

// Value returns the representative value of a bucket — its middle on the
// logarithmic scale. This is what to report as a percentile: the error against
// any value actually in the bucket is then at most Eps.
func Value(k int32) float64 {
	if k == 0 {
		return 0
	}
	if k < 0 {
		return -Value(-k)
	}
	return math.Pow(Gamma, float64(int(k)-bias))
}

// Bounds returns the range of values that map to this bucket.
//
// Half a step either side of Value(k), because Key rounds to nearest.
func Bounds(k int32) (lo, hi float64) {
	if k == 0 {
		return 0, Min
	}
	e := float64(int(k) - bias)
	return math.Pow(Gamma, e-0.5), math.Pow(Gamma, e+0.5)
}

// Stats are the summary figures that travel alongside the buckets. The agent
// sends them too, so a caller asking only for an average never has to touch
// the bucket arrays.
type Stats struct {
	Count int64
	Min   float64
	Max   float64
	Avg   float64
	Sum   float64
}

// Build turns raw measurements into agent-compatible buckets.
//
// Keys come back sorted, with counts in matching order — the same shape the
// agent puts in K and N, and the same shape our sketches table stores.
func Build(values []float64) (keys []int32, counts []uint32, stats Stats) {
	if len(values) == 0 {
		return nil, nil, Stats{}
	}

	tally := make(map[int32]uint32, len(values))
	stats.Min, stats.Max = values[0], values[0]
	for _, v := range values {
		tally[Key(v)]++
		stats.Sum += v
		stats.Min = math.Min(stats.Min, v)
		stats.Max = math.Max(stats.Max, v)
	}
	stats.Count = int64(len(values))
	stats.Avg = stats.Sum / float64(stats.Count)

	keys = make([]int32, 0, len(tally))
	for k := range tally {
		keys = append(keys, k)
	}
	// Sorted because the agent sends them sorted and because reading a
	// percentile means walking the counts in ascending order.
	sortKeys(keys)

	counts = make([]uint32, len(keys))
	for i, k := range keys {
		counts[i] = tally[k]
	}
	return keys, counts, stats
}

// sortKeys is an insertion sort: key counts per payload are in the low
// hundreds, where this beats the overhead of sort.Slice's reflection.
func sortKeys(k []int32) {
	for i := 1; i < len(k); i++ {
		v := k[i]
		j := i - 1
		for j >= 0 && k[j] > v {
			k[j+1] = k[j]
			j--
		}
		k[j+1] = v
	}
}
