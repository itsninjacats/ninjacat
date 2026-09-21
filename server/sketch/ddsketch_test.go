package sketch

import (
	"math"
	"testing"
)

// Values checked against the agent's own constants in pkg/util/quantile.
// If any of these move, our buckets stop lining up with the agent's and
// merged percentiles go quietly wrong.
func TestKeyMatchesAgent(t *testing.T) {
	cases := []struct {
		value float64
		key   int32
	}{
		{11.9, 1498},
		{12.1, 1499},
		{47.3, 1587},
		{890.0, 1776},
		{1150.0, 1793},
	}
	for _, c := range cases {
		if got := Key(c.value); got != c.key {
			t.Errorf("Key(%g) = %d, want %d", c.value, got, c.key)
		}
	}
}

func TestConstants(t *testing.T) {
	if Gamma != 1.015625 {
		t.Errorf("Gamma = %v, want 1.015625", Gamma)
	}
	if bias != 1338 {
		t.Errorf("bias = %d, want 1338", bias)
	}
}

// Key rounds to NEAREST, so a value can sit below Value(k). The agent's own
// doc comment claims otherwise; this test pins the real behaviour.
func TestValueIsMiddleNotFloor(t *testing.T) {
	const v = 1150.0
	k := Key(v)
	if Value(k) <= v {
		t.Fatalf("Value(%d) = %g; expected it ABOVE %g (round-to-nearest)", k, Value(k), v)
	}
	lo, hi := Bounds(k)
	if v < lo || v >= hi {
		t.Errorf("%g outside Bounds(%d) = [%g, %g)", v, k, lo, hi)
	}
}

// The guarantee that makes the whole scheme worth using.
func TestRelativeErrorWithinEps(t *testing.T) {
	for v := 0.001; v < 1e6; v *= 1.3 {
		k := Key(v)
		if err := math.Abs(Value(k)-v) / v; err > Eps {
			t.Errorf("value %g: relative error %.5f exceeds eps %.5f", v, err, Eps)
		}
	}
}

// Every value must fall inside the bucket it was assigned.
func TestBoundsContainValue(t *testing.T) {
	for v := 0.5; v < 4e6; v *= 1.17 {
		k := Key(v)
		lo, hi := Bounds(k)
		if v < lo || v >= hi {
			t.Errorf("%g outside Bounds(%d) = [%g, %g)", v, k, lo, hi)
		}
	}
}

// Round-tripping a bucket's own representative must not move it.
func TestKeyValueRoundTrip(t *testing.T) {
	for k := int32(1200); k < 2000; k += 7 {
		if got := Key(Value(k)); got != k {
			t.Errorf("Key(Value(%d)) = %d", k, got)
		}
	}
}

func TestBuild(t *testing.T) {
	values := []float64{11.9, 12.1, 12.0, 12.2, 47.3, 47.0, 46.9, 48.1, 51.0, 890.0, 895.0, 1150.0}
	keys, counts, stats := Build(values)

	if len(keys) != len(counts) {
		t.Fatalf("keys/counts length mismatch: %d vs %d", len(keys), len(counts))
	}
	for i := 1; i < len(keys); i++ {
		if keys[i] <= keys[i-1] {
			t.Fatalf("keys not sorted ascending: %v", keys)
		}
	}
	var total uint32
	for _, n := range counts {
		total += n
	}
	if int64(total) != stats.Count || stats.Count != int64(len(values)) {
		t.Errorf("counts sum to %d, Count = %d, want %d", total, stats.Count, len(values))
	}
	if stats.Min != 11.9 || stats.Max != 1150.0 {
		t.Errorf("Min/Max = %g/%g, want 11.9/1150", stats.Min, stats.Max)
	}
	if math.Abs(stats.Sum-3223.5) > 1e-9 {
		t.Errorf("Sum = %g, want 3223.5", stats.Sum)
	}
	if math.Abs(stats.Avg-268.625) > 1e-9 {
		t.Errorf("Avg = %g, want 268.625", stats.Avg)
	}
}

func TestBuildEmpty(t *testing.T) {
	keys, counts, stats := Build(nil)
	if keys != nil || counts != nil || stats.Count != 0 {
		t.Errorf("Build(nil) should be empty, got %v %v %+v", keys, counts, stats)
	}
}
