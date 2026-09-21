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
// mirror application.go: metrics puts tags last of ten, hosts last of nine,
// k8s_resources at position 21 of 24, container_images (dd_tags) last of 21.
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
			argN: 10, tagsAt: 9,
		},
		{
			name: "hosts",
			row:  HostRow{TenantID: "t", Host: "h", SeenAt: now, Tags: tags},
			argN: 9, tagsAt: 8,
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
