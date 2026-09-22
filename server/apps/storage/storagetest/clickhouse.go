package storagetest

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/schema"
)

// dialTimeout bounds the "is ClickHouse there?" question. Short on purpose:
// on a laptop with no dev stack up, every integration test in the tree pays
// this once, and a developer waiting ten seconds to be told a test skipped is
// a developer who stops running the suite.
const dialTimeout = 2 * time.Second

// OpenScratchDB connects to ClickHouse, creates a throwaway database, applies
// the full schema to it and returns a connection already pointed at it. The
// database is dropped in t.Cleanup.
//
// WHY a database per test rather than the shared dev one:
//
//   - The migration ledger is immutable by design: schema.Apply refuses to run
//     if a version in schema_migrations has a different checksum than the
//     embedded file. A test that applied migrations to the dev database would
//     poison it for the next branch that edits an unshipped migration, and the
//     failure would surface as "the server will not start", far from here.
//   - Tests must be able to run in parallel, and with the suite twice over
//     (go test ./... on two branches at once). Counting rows in a table
//     somebody else is writing to is not a test, it is a coin toss.
//   - It exercises the real schema.Apply / SplitStatements path from empty,
//     which is what a fresh production ClickHouse does at first boot. That
//     path has no other test.
//
// Connection defaults match the dev compose stack, so no env is needed to run
// against it: CLICKHOUSE_ADDR 127.0.0.1:9000, CLICKHOUSE_USER/PASSWORD
// ninjacat/ninjacat.
//
// If ClickHouse is unreachable the test SKIPS — the unit tests around it must
// stay runnable with nothing installed. Set NINJACAT_TEST_REQUIRE_CLICKHOUSE=1
// (CI does) to turn that skip into a failure, so "all green" cannot quietly
// mean "nothing ran".
func OpenScratchDB(t testing.TB) driver.Conn {
	t.Helper()

	addr := envOr("CLICKHOUSE_ADDR", "127.0.0.1:9000")
	user := envOr("CLICKHOUSE_USER", "ninjacat")
	pass := envOr("CLICKHOUSE_PASSWORD", "ninjacat")

	// The admin connection goes to the database that certainly exists — the
	// one the dev stack provisions. CREATE DATABASE is a server-level
	// operation, so which database it is connected to does not matter.
	admin, err := open(addr, envOr("CLICKHOUSE_DB", "ninjacat"), user, pass)
	if err == nil {
		ctx, cancel := context.WithTimeout(context.Background(), dialTimeout)
		err = admin.Ping(ctx)
		cancel()
		if err != nil {
			admin.Close()
		}
	}
	if err != nil {
		msg := fmt.Sprintf("ClickHouse unreachable at %s as %q: %v\n"+
			"Start it with `docker compose up -d clickhouse` from the repo root.", addr, user, err)
		if os.Getenv("NINJACAT_TEST_REQUIRE_CLICKHOUSE") == "1" {
			t.Fatal(msg)
		}
		t.Skip(msg)
	}

	name := "nc_test_" + randomHex(8)

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	if err := admin.Exec(ctx, "CREATE DATABASE IF NOT EXISTS "+name); err != nil {
		admin.Close()
		t.Fatalf("create scratch database %s: %v", name, err)
	}

	conn, err := open(addr, name, user, pass)
	if err != nil {
		_ = admin.Exec(ctx, "DROP DATABASE IF EXISTS "+name)
		admin.Close()
		t.Fatalf("connect to scratch database %s: %v", name, err)
	}

	t.Cleanup(func() {
		conn.Close()
		// Its own context: the test's may well be why we are cleaning up.
		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancel()
		if err := admin.Exec(ctx, "DROP DATABASE IF EXISTS "+name); err != nil {
			t.Logf("could not drop scratch database %s: %v", name, err)
		}
		admin.Close()
	})

	// The migrations carry no database qualifier, which is exactly what lets
	// them apply to a scratch database — see the header of 0001_initial.sql.
	if _, err := schema.Apply(ctx, conn); err != nil {
		t.Fatalf("apply schema to %s: %v", name, err)
	}
	return conn
}

// Insert writes rows through the writer's REGISTERED INSERT statement, the
// same string the BatchWriter uses in production. Going through the registry
// rather than a literal is the point: a test that spelled its own INSERT would
// keep passing after the writer's column list changed.
func Insert(t testing.TB, conn driver.Conn, writer gen.Atom, rows []storage.Row) {
	t.Helper()

	spec, ok := writerSpec(writer)
	if !ok {
		t.Fatalf("writer %s is not registered", writer)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	batch, err := conn.PrepareBatch(ctx, spec.Config.Insert)
	if err != nil {
		t.Fatalf("%s: prepare batch: %v", writer, err)
	}
	defer batch.Close()

	for i, r := range rows {
		if err := r.AppendTo(batch); err != nil {
			t.Fatalf("%s: append row %d: %v", writer, i, err)
		}
	}
	if err := batch.Send(); err != nil {
		t.Fatalf("%s: send batch of %d: %v", writer, len(rows), err)
	}
}

// Count returns the number of rows in a table.
func Count(t testing.TB, conn driver.Conn, table string) uint64 {
	t.Helper()

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	var n uint64
	// The table name is a test constant, never user input, so interpolating
	// it is fine — ClickHouse has no placeholder for an identifier anyway.
	if err := conn.QueryRow(ctx, "SELECT count() FROM "+table).Scan(&n); err != nil {
		t.Fatalf("count %s: %v", table, err)
	}
	return n
}

// QueryRow runs a SELECT and returns the first row's columns as Go values,
// typed by what the driver says each column scans as.
//
// Deliberately minimal: it exists so a test can read a couple of columns back
// and prove the values survived the trip, not to grow into a query API. A test
// that needs more should use conn.Query directly with typed destinations.
func QueryRow(t testing.TB, conn driver.Conn, query string, args ...any) []any {
	t.Helper()

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	rows, err := conn.Query(ctx, query, args...)
	if err != nil {
		t.Fatalf("query %q: %v", query, err)
	}
	defer rows.Close()

	if !rows.Next() {
		if err := rows.Err(); err != nil {
			t.Fatalf("query %q: %v", query, err)
		}
		t.Fatalf("query %q returned no rows", query)
	}

	types := rows.ColumnTypes()
	dest := make([]any, len(types))
	for i, ct := range types {
		dest[i] = reflect.New(ct.ScanType()).Interface()
	}
	if err := rows.Scan(dest...); err != nil {
		t.Fatalf("scan %q: %v", query, err)
	}

	out := make([]any, len(dest))
	for i := range dest {
		out[i] = reflect.ValueOf(dest[i]).Elem().Interface()
	}
	return out
}

func open(addr, database, user, pass string) (driver.Conn, error) {
	return clickhouse.Open(&clickhouse.Options{
		Addr: strings.Split(addr, ","),
		Auth: clickhouse.Auth{
			Database: database,
			Username: user,
			Password: pass,
		},
		DialTimeout: dialTimeout,
	})
}

func randomHex(n int) string {
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		// crypto/rand failing is not a condition a test can work around, and
		// a fixed name would make two concurrent runs delete each other's
		// database.
		panic("storagetest: " + err.Error())
	}
	return hex.EncodeToString(b)
}

// envOr mirrors the storage app's own helper: these variables are read this
// way throughout the server, never with a bare os.Getenv.
func envOr(name, fallback string) string {
	if v := os.Getenv(name); v != "" {
		return v
	}
	return fallback
}
