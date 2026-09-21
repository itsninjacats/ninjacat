package panelapi

import (
	"reflect"
	"testing"
	"time"

	"github.com/itsninjacats/server/apps/query"
)

func TestParseTagFilters(t *testing.T) {
	tests := []struct {
		name    string
		in      []string
		out     []query.TagFilter
		wantErr bool
	}{
		{name: "none", in: nil, out: nil},
		{
			name: "single tag",
			in:   []string{"env:prod"},
			out:  []query.TagFilter{{Key: "env", Values: []string{"prod"}}},
		},
		{
			// The Datadog scope rule: repeating a key widens, adding a key
			// narrows. Same key merges into one OR-ed filter.
			name: "same key ORs into one filter",
			in:   []string{"kube_service:a", "kube_service:b"},
			out: []query.TagFilter{
				{Key: "kube_service", Values: []string{"a", "b"}},
			},
		},
		{
			name: "distinct keys stay separate filters",
			in:   []string{"env:prod", "kube_service:a", "kube_service:b"},
			out: []query.TagFilter{
				{Key: "env", Values: []string{"prod"}},
				{Key: "kube_service", Values: []string{"a", "b"}},
			},
		},
		{
			// Only the first colon splits — an image tag or URL in the value
			// keeps its own colons.
			name: "value keeps its colons",
			in:   []string{"image:nginx:1.25"},
			out:  []query.TagFilter{{Key: "image", Values: []string{"nginx:1.25"}}},
		},
		{name: "missing colon", in: []string{"envprod"}, wantErr: true},
		{name: "empty key", in: []string{":prod"}, wantErr: true},
		{name: "empty value", in: []string{"env:"}, wantErr: true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			out, err := parseTagFilters(tt.in)
			if tt.wantErr {
				if err == nil {
					t.Fatalf("want error, got %#v", out)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %s", err)
			}
			if !reflect.DeepEqual(out, tt.out) {
				t.Errorf("out = %#v, want %#v", out, tt.out)
			}
		})
	}
}

func TestParseTimeRelative(t *testing.T) {
	now := time.Date(2026, 9, 21, 12, 0, 0, 0, time.UTC)
	fallback := now.Add(-time.Hour)

	tests := []struct {
		in      string
		want    time.Time
		wantErr bool
	}{
		{in: "", want: fallback},
		{in: "now", want: now},
		{in: "-6h", want: now.Add(-6 * time.Hour)},
		{in: "-15m", want: now.Add(-15 * time.Minute)},
		// Days and weeks are ours, not time.ParseDuration's.
		{in: "-7d", want: now.Add(-7 * 24 * time.Hour)},
		{in: "-2w", want: now.Add(-14 * 24 * time.Hour)},
		{in: "-1.5d", want: now.Add(-36 * time.Hour)},
		{in: "2026-09-21T09:30:00Z", want: time.Date(2026, 9, 21, 9, 30, 0, 0, time.UTC)},
		{in: "yesterday", wantErr: true},
		{in: "-3x", wantErr: true},
	}

	for _, tt := range tests {
		t.Run(tt.in, func(t *testing.T) {
			got, err := parseTime(tt.in, fallback, now)
			if tt.wantErr {
				if err == nil {
					t.Fatalf("want error, got %s", got)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %s", err)
			}
			if !got.Equal(tt.want) {
				t.Errorf("parseTime(%q) = %s, want %s", tt.in, got, tt.want)
			}
		})
	}
}

// TestResolveStepCeiling: the maxPoints ceiling widens the step instead of
// refusing the request — a month at one-second resolution comes back at
// whatever step yields at most maxPoints buckets.
func TestResolveStep(t *testing.T) {
	tests := []struct {
		name string
		raw  string
		span time.Duration
		want time.Duration
	}{
		{
			name: "explicit step honoured when it fits",
			raw:  "60s",
			span: 6 * time.Hour, // 360 points at 60s, under the ceiling
			want: time.Minute,
		},
		{
			name: "step widened, not refused, past the ceiling",
			raw:  "1s",
			span: 30 * 24 * time.Hour, // a month at 1s would be 2.6M points
			want: (30 * 24 * time.Hour / maxPoints).Round(time.Second),
		},
		{
			name: "derived step lands near 300 points",
			raw:  "",
			span: 5 * time.Hour,
			want: time.Minute,
		},
		{
			name: "never below one second",
			raw:  "",
			span: time.Minute,
			want: time.Second,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := resolveStep(tt.raw, tt.span)
			if got != tt.want {
				t.Errorf("resolveStep(%q, %s) = %s, want %s", tt.raw, tt.span, got, tt.want)
			}
			if tt.span/got > maxPoints {
				t.Errorf("step %s over span %s yields %d points, above the %d ceiling",
					got, tt.span, tt.span/got, maxPoints)
			}
		})
	}
}
