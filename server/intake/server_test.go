package intake

import (
	"reflect"
	"testing"
)

// A Datadog tag list is a multiset: two tags may share a key, and both values
// must survive. The old map[string]string here kept whichever came last —
// every log entry in a captured batch lost one of its two kube_service values
// that way. See docs/decisions/0001-tags-are-a-multiset.md.
func TestTagsToMultiMap(t *testing.T) {
	cases := []struct {
		name string
		in   []string
		want map[string][]string
	}{
		{
			// The entire point of the change: both values, in arrival order.
			name: "duplicate key keeps both values in order",
			in:   []string{"kube_service:a", "env:prod", "kube_service:b"},
			want: map[string][]string{"kube_service": {"a", "b"}, "env": {"prod"}},
		},
		{
			// Datadog allows bare tags; the key must stay visible.
			name: "bare tag keeps its key with an empty value",
			in:   []string{"standalone"},
			want: map[string][]string{"standalone": {""}},
		},
		{
			// Only the FIRST colon splits — values contain colons legally.
			name: "value with colons is not split further",
			in:   []string{"url:http://x"},
			want: map[string][]string{"url": {"http://x"}},
		},
		{
			name: "empty list produces no map",
			in:   []string{},
			want: nil,
		},
		{
			name: "nil list produces no map",
			in:   nil,
			want: nil,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := tagsToMultiMap(tc.in)
			if !reflect.DeepEqual(got, tc.want) {
				t.Errorf("tagsToMultiMap(%v) = %v, want %v", tc.in, got, tc.want)
			}
		})
	}
}

// kvToMap serves the maps that are NOT tags — Kubernetes labels and
// annotations, whose keys their own API guarantees unique. It must stay
// single-valued and share splitTag's first-colon rule.
func TestKvToMap(t *testing.T) {
	cases := []struct {
		name string
		in   []string
		want map[string]string
	}{
		{
			name: "pairs split at the first colon",
			in:   []string{"app:web", "note:a:b"},
			want: map[string]string{"app": "web", "note": "a:b"},
		},
		{
			name: "nil list produces no map",
			in:   nil,
			want: nil,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := kvToMap(tc.in)
			if !reflect.DeepEqual(got, tc.want) {
				t.Errorf("kvToMap(%v) = %v, want %v", tc.in, got, tc.want)
			}
		})
	}
}
