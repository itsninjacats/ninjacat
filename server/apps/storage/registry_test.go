package storage

import "testing"

// Writers() is what the supervisor builds its children from, so a writer
// missing here is a table that silently never starts — and since the only
// thing that puts one there is an init() in a rows_*.go file, this is also the
// test that a file carved out of the old shared lists still registers. It pins
// the original tables only; see the note at the end.
func TestWritersCoversEveryTable(t *testing.T) {
	// Table name per writer, as the INSERT targets it. Spelled out rather than
	// derived, because "whatever is registered" would make this test agree
	// with a file that stopped registering.
	expected := map[string]string{
		string(MetricsWriter):         "metrics",
		string(SketchesWriter):        "sketches",
		string(ChecksWriter):          "check_runs",
		string(LogsWriter):            "logs",
		string(HostsWriter):           "hosts",
		string(ProcessesWriter):       "processes",
		string(EventsWriter):          "events",
		string(K8sResourcesWriter):    "k8s_resources",
		string(K8sManifestsWriter):    "k8s_manifests",
		string(K8sClusterWriter):      "k8s_cluster",
		string(K8sActionsWriter):      "k8s_actions",
		string(ContainerEventsWriter): "container_events",
		string(ContainerImagesWriter): "container_images",
		string(RawPayloadsWriter):     "raw_payloads",
	}

	got := map[string]string{}
	for _, s := range Writers() {
		got[string(s.Name)] = s.Config.Name
	}

	for name, table := range expected {
		switch have, ok := got[name]; {
		case !ok:
			t.Errorf("%s is not in Writers() — its rows_*.go init() did not register it", name)
		case have != table:
			t.Errorf("%s serves table %q, want %q", name, have, table)
		}
	}
	// Deliberately a subset check, not an exact count: tables added later
	// register from their own rows_*.go and are covered by the arity test in
	// storagetest, which walks Writers(). An exact count here would make this
	// one file a merge conflict for every table added in parallel.
}

// Every spec must carry a zero row, because that is what the arity test walks
// instead of a list somebody has to maintain. registerWriter takes it as a
// parameter, so this only catches an explicit nil.
func TestEverySpecHasAZeroRow(t *testing.T) {
	for _, s := range Writers() {
		if s.Zero == nil {
			t.Errorf("writer %s (table %s) has no Zero row", s.Name, s.Config.Name)
		}
	}
}

// Two files claiming one process name is a programming error, and the only
// alternative to failing loudly is routing one table's rows into another
// table's writer. It panics at init, before a node exists, so the blast
// radius is a failed test run.
func TestRegisterWriterRejectsDuplicates(t *testing.T) {
	defer func() {
		if recover() == nil {
			t.Errorf("registering %s twice did not panic", MetricsWriter)
		}
	}()
	registerWriter(MetricsWriter, WriterConfig{Name: "duplicate"}, MetricPoint{})
}
