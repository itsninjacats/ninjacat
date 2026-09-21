package query

import (
	"reflect"
	"strings"
	"testing"
)

// Tests here exercise the SQL builders as pure functions — no database. What
// matters is the shape of the fragment and, above all, that everything the
// caller controls ends up in the args and never in the SQL text.

func TestTagFilterSQL(t *testing.T) {
	tests := []struct {
		name    string
		filters []TagFilter
		hot     bool
		sql     string
		args    []any
		wantErr bool
	}{
		{
			name: "single cold key is membership, not equality",
			filters: []TagFilter{
				{Key: "kube_service", Values: []string{"web"}},
			},
			hot:  true,
			sql:  " AND has(tags[?], ?)",
			args: []any{"kube_service", "web"},
		},
		{
			name: "same key with several values ORs via hasAny",
			filters: []TagFilter{
				{Key: "kube_service", Values: []string{"a", "b"}},
			},
			hot:  true,
			sql:  " AND hasAny(tags[?], ?)",
			args: []any{"kube_service", []string{"a", "b"}},
		},
		{
			name: "distinct keys AND together",
			filters: []TagFilter{
				{Key: "team", Values: []string{"core"}},
				{Key: "zone", Values: []string{"a", "b"}},
			},
			hot: true,
			sql: " AND has(tags[?], ?) AND hasAny(tags[?], ?)",
			args: []any{
				"team", "core",
				"zone", []string{"a", "b"},
			},
		},
		{
			name: "hot key uses the materialised column",
			filters: []TagFilter{
				{Key: "env", Values: []string{"prod"}},
			},
			hot:  true,
			sql:  " AND env = ?",
			args: []any{"prod"},
		},
		{
			name: "hot key with several values uses IN",
			filters: []TagFilter{
				{Key: "service", Values: []string{"web", "api"}},
			},
			hot:  true,
			sql:  " AND service IN (?)",
			args: []any{[]string{"web", "api"}},
		},
		{
			name: "hot key falls back to the map when the table has no columns",
			filters: []TagFilter{
				{Key: "env", Values: []string{"prod"}},
			},
			hot:  false,
			sql:  " AND has(tags[?], ?)",
			args: []any{"env", "prod"},
		},
		{
			name:    "empty key is refused",
			filters: []TagFilter{{Key: "", Values: []string{"x"}}},
			hot:     true,
			wantErr: true,
		},
		{
			name:    "empty values are refused",
			filters: []TagFilter{{Key: "env", Values: nil}},
			hot:     true,
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			sql, args, err := tagFilterSQL(tt.filters, tt.hot)
			if tt.wantErr {
				if err == nil {
					t.Fatalf("want error, got sql %q", sql)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %s", err)
			}
			if sql != tt.sql {
				t.Errorf("sql = %q, want %q", sql, tt.sql)
			}
			if !reflect.DeepEqual(args, tt.args) {
				t.Errorf("args = %#v, want %#v", args, tt.args)
			}
		})
	}
}

// TestTagFilterSQLQuotedKeyIsBound is the injection check: a key carrying a
// quote must travel as a bound argument, never as SQL text. If the key ever
// shows up in the fragment, the string literal it would terminate is the
// whole query.
func TestTagFilterSQLQuotedKeyIsBound(t *testing.T) {
	evil := `env'] , 1); DROP TABLE metrics; --`

	sql, args, err := tagFilterSQL([]TagFilter{{Key: evil, Values: []string{"x'y"}}}, true)
	if err != nil {
		t.Fatalf("unexpected error: %s", err)
	}
	if strings.Contains(sql, "'") || strings.Contains(sql, "DROP") {
		t.Fatalf("caller-controlled text leaked into SQL: %q", sql)
	}
	if sql != " AND has(tags[?], ?)" {
		t.Fatalf("sql = %q, want placeholders only", sql)
	}
	if !reflect.DeepEqual(args, []any{evil, "x'y"}) {
		t.Fatalf("args = %#v, want the raw key and value", args)
	}
}

func TestNormalizeGroupBy(t *testing.T) {
	tests := []struct {
		name    string
		in      []string
		out     []string
		wantErr string
	}{
		{name: "nil stays nil", in: nil, out: nil},
		{
			name: "sorted for stable series names",
			in:   []string{"service", "env"},
			out:  []string{"env", "service"},
		},
		{
			name: "duplicates collapse",
			in:   []string{"env", "env", "service"},
			out:  []string{"env", "service"},
		},
		{
			name: "four keys pass",
			in:   []string{"a", "b", "c", "d"},
			out:  []string{"a", "b", "c", "d"},
		},
		{
			name:    "five keys are refused, not truncated",
			in:      []string{"a", "b", "c", "d", "pod_name"},
			wantErr: "too many group-by keys",
		},
		{
			name:    "empty key is refused",
			in:      []string{"env", ""},
			wantErr: "must not be empty",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			out, err := normalizeGroupBy(tt.in)
			if tt.wantErr != "" {
				if err == nil || !strings.Contains(err.Error(), tt.wantErr) {
					t.Fatalf("err = %v, want it to mention %q", err, tt.wantErr)
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

// TestGroupColumnsSQLBindsKeys: grouping keys are identifiers-in-data and get
// the same treatment as filter keys — placeholders, not splicing.
func TestGroupColumnsSQL(t *testing.T) {
	evil := `env']); DROP TABLE metrics; --`

	sql, args := groupColumnsSQL([]string{"env", evil})
	if sql != "arrayJoin(tags[?]) AS g0, arrayJoin(tags[?]) AS g1" {
		t.Fatalf("sql = %q, want placeholders only", sql)
	}
	if strings.Contains(sql, "DROP") {
		t.Fatalf("caller-controlled text leaked into SQL: %q", sql)
	}
	if !reflect.DeepEqual(args, []any{"env", evil}) {
		t.Fatalf("args = %#v, want the raw keys", args)
	}
}

func TestMessageTokens(t *testing.T) {
	tests := []struct {
		in  string
		out []string
	}{
		// hasToken throws on separators, so splitting must strip them all.
		{"connection refused", []string{"connection", "refused"}},
		{"GET /api/v2/logs?x=1", []string{"GET", "api", "v2", "logs", "x", "1"}},
		{"...", nil},
		{"", nil},
		{"err_code=500", []string{"err", "code", "500"}},
	}
	for _, tt := range tests {
		got := messageTokens(tt.in)
		if len(got) == 0 && len(tt.out) == 0 {
			continue
		}
		if !reflect.DeepEqual(got, tt.out) {
			t.Errorf("messageTokens(%q) = %#v, want %#v", tt.in, got, tt.out)
		}
	}
}

// TestLogsWhereSQLPhrase checks the two-stage message match: one hasToken per
// token so the tokenbf_v1 index can skip granules, then a single position()
// over the whole phrase, and everything bound.
func TestLogsWhereSQL(t *testing.T) {
	req := SearchLogs{
		TenantID: "default",
		Query:    "connection refused",
		Service:  "nginx",
		Tags:     []TagFilter{{Key: `k"ey'`, Values: []string{"v"}}},
	}
	sql, args, err := logsWhereSQL(req)
	if err != nil {
		t.Fatalf("unexpected error: %s", err)
	}

	if !strings.HasPrefix(sql, " WHERE tenant_id = ?") {
		t.Errorf("tenant scope must come first, got %q", sql)
	}
	if strings.Count(sql, "hasToken(message, ?)") != 2 {
		t.Errorf("want one hasToken per token, got %q", sql)
	}
	if tok := strings.Index(sql, "hasToken"); tok == -1 || tok > strings.Index(sql, "position(message") {
		t.Errorf("hasToken must precede position, got %q", sql)
	}
	if strings.ContainsAny(sql, `'"`) {
		t.Errorf("caller-controlled text leaked into SQL: %q", sql)
	}

	want := []any{
		"default", req.From, req.To,
		"nginx",
		"connection", "refused",
		"connection refused",
		`k"ey'`, "v",
	}
	if !reflect.DeepEqual(args, want) {
		t.Errorf("args = %#v, want %#v", args, want)
	}
}
