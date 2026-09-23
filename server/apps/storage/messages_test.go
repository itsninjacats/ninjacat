package storage

import (
	"reflect"
	"testing"
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/column"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// captureBatch records what AppendTo hands the driver, so a test can check
// the argument ORDER — the one thing that must stay aligned with the INSERT
// column lists in application.go, and the one thing the compiler cannot see.
type captureBatch struct {
	args []any
}

func (c *captureBatch) Append(v ...any) error {
	c.args = v
	return nil
}

func (c *captureBatch) Abort() error                  { return nil }
func (c *captureBatch) AppendStruct(any) error        { return nil }
func (c *captureBatch) Column(int) driver.BatchColumn { return nil }
func (c *captureBatch) Flush() error                  { return nil }
func (c *captureBatch) Send() error                   { return nil }
func (c *captureBatch) IsSent() bool                  { return false }
func (c *captureBatch) Rows() int                     { return 0 }
func (c *captureBatch) Columns() []column.Interface   { return nil }
func (c *captureBatch) Close() error                  { return nil }

// A multi-valued tag must ride AppendTo intact and land in the argument slot
// its INSERT list declares for the tag column. The positions asserted here
// mirror the INSERT lists: metrics at 10 of 15, hosts at 9 of 27 (and
// host_tags at 24 of the same row), k8s_resources at position 21 of 24,
// container_images (dd_tags) last of 21, external_host_tags last of 5,
// action_connections at 5 of 8.
func TestAppendToCarriesMultiValuedTags(t *testing.T) {
	now := time.Unix(1700000000, 0).UTC()
	tags := map[string][]string{"kube_service": {"a", "b"}}

	cases := []struct {
		name   string
		row    Row
		argN   int // total arguments the INSERT list expects
		tagsAt int // index of the tag map among them
	}{
		{
			name: "metrics",
			row: MetricPoint{TenantID: "t", Timestamp: now, Metric: "m",
				Host: "h", Value: 1, Tags: tags},
			argN: 15, tagsAt: 9,
		},
		{
			name: "hosts",
			row:  HostRow{TenantID: "t", Host: "h", SeenAt: now, Tags: tags},
			argN: 27, tagsAt: 8,
		},
		{
			name: "k8s_resources",
			row:  K8sResourceRow{TenantID: "t", CollectedAt: now, Tags: tags},
			argN: 24, tagsAt: 20,
		},
		{
			name: "container_images",
			row:  ContainerImageRow{TenantID: "t", CollectedAt: now, DDTags: tags},
			argN: 21, tagsAt: 20,
		},
		{
			// The V5 collector's external host tags: one row per (host,
			// source), and the tag list is the whole point of the row.
			name: "external_host_tags",
			row:  ExternalHostTagsRow{TenantID: "t", ReceivedAt: now, Host: "h", Source: "vsphere", Tags: tags},
			argN: 5, tagsAt: 4,
		},
		{
			// A Private Action Runner connection carries the tags the runner
			// was registered with.
			name: "action_connections",
			row:  ActionConnectionRow{TenantID: "t", At: now, Name: "c", RunnerID: "r", Tags: tags},
			argN: 8, tagsAt: 4,
		},
		{
			// hosts has a SECOND tag-shaped column: every tag source the
			// agent reports, not only "system".
			name: "hosts (host_tags)",
			row:  HostRow{TenantID: "t", Host: "h", SeenAt: now, HostTags: tags},
			argN: 27, tagsAt: 23,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			b := &captureBatch{}
			if err := tc.row.AppendTo(b); err != nil {
				t.Fatalf("AppendTo: %v", err)
			}
			if len(b.args) != tc.argN {
				t.Fatalf("argument count: got %d, want %d — AppendTo drifted from the INSERT list", len(b.args), tc.argN)
			}
			got, ok := b.args[tc.tagsAt].(map[string][]string)
			if !ok {
				t.Fatalf("argument %d: got %T, want map[string][]string", tc.tagsAt, b.args[tc.tagsAt])
			}
			if !reflect.DeepEqual(got, tags) {
				t.Errorf("tags: got %v, want %v — a duplicate-key tag must survive the trip", got, tags)
			}
		})
	}
}

// The driver rejects nil maps, so a row built with no tags at all must still
// hand it an empty — never nil — map.
func TestAppendToGuardsNilTagMaps(t *testing.T) {
	rows := []struct {
		name   string
		row    Row
		tagsAt int
	}{
		{"metrics", MetricPoint{}, 9},
		{"hosts", HostRow{}, 8},
		{"k8s_resources", K8sResourceRow{}, 20},
		{"container_images", ContainerImageRow{}, 20},
		{"external_host_tags", ExternalHostTagsRow{}, 4},
		{"action_connections", ActionConnectionRow{}, 4},
		{"hosts (host_tags)", HostRow{}, 23},
	}
	for _, tc := range rows {
		t.Run(tc.name, func(t *testing.T) {
			b := &captureBatch{}
			if err := tc.row.AppendTo(b); err != nil {
				t.Fatalf("AppendTo: %v", err)
			}
			got, ok := b.args[tc.tagsAt].(map[string][]string)
			if !ok {
				t.Fatalf("argument %d: got %T, want map[string][]string", tc.tagsAt, b.args[tc.tagsAt])
			}
			if got == nil {
				t.Error("nil tag map reached the batch — the driver would reject it")
			}
		})
	}
}
