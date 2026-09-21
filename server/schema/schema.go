// Package schema owns the ClickHouse schema: the migration files embedded in
// the binary, and the runner that applies them.
//
// The package sits next to its SQL because go:embed cannot reference files
// outside the package directory ("..", or any path escaping the package, is
// rejected at compile time). It is deliberately NOT named "clickhouse": that
// name is taken by the driver import in every caller, and a collision there
// would force aliases everywhere for no gain.
//
// ClickHouse has no migration concept of its own — no transactional DDL, no
// catalog of applied changes — so we keep our own ledger in a
// schema_migrations table. ReplacingMergeTree keyed on version means a
// re-applied version collapses into one row at merge time instead of
// accumulating; readers use FINAL for exactness, same as the hosts table.
package schema

import (
	"context"
	"embed"
	"fmt"
	"hash/fnv"
	"regexp"
	"sort"
	"strconv"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

//go:embed migrations/*.sql
var files embed.FS

// The ledger. applied_at doubles as the ReplacingMergeTree version, so if a
// version ever IS re-inserted, the newest application wins.
const createMigrationsTable = `
CREATE TABLE IF NOT EXISTS schema_migrations (
    version    UInt32,
    name       String,
    applied_at DateTime64(3, 'UTC') DEFAULT now64(3),
    checksum   UInt64
) ENGINE = ReplacingMergeTree(applied_at) ORDER BY version`

// Migration is one embedded .sql file, parsed and hashed.
type Migration struct {
	Version  uint32
	Name     string
	SQL      string
	Checksum uint64
}

// Filenames are NNNN_snake_name.sql: exactly four digits, an underscore, a
// snake_case name. The number is the version. Anything else in the migrations
// directory is a mistake and must FAIL the load — a silently skipped file is
// a migration that never runs anywhere, discovered months later.
var filenameRE = regexp.MustCompile(`^(\d{4})_([a-z0-9][a-z0-9_]*)\.sql$`)

func parseFilename(name string) (uint32, string, error) {
	m := filenameRE.FindStringSubmatch(name)
	if m == nil {
		return 0, "", fmt.Errorf("schema: migration filename %q does not match NNNN_snake_name.sql", name)
	}
	v, err := strconv.ParseUint(m[1], 10, 32)
	if err != nil {
		return 0, "", fmt.Errorf("schema: migration filename %q: %w", name, err)
	}
	if v == 0 {
		return 0, "", fmt.Errorf("schema: migration filename %q: version 0 is not a valid version", name)
	}
	return uint32(v), m[2], nil
}

// checksum is FNV-1a over the raw file bytes. The column it lands in is a
// plain UInt64: only this package ever writes or compares it, so it does not
// need to match ClickHouse's sipHash64 — it only needs to be stable and to
// change when the file changes, which is the whole job of the column: to
// catch an already-applied migration being edited after the fact.
func checksum(data []byte) uint64 {
	h := fnv.New64a()
	h.Write(data)
	return h.Sum64()
}

// Load reads the embedded migrations, sorted by version.
func Load() ([]Migration, error) {
	entries, err := files.ReadDir("migrations")
	if err != nil {
		return nil, fmt.Errorf("schema: read embedded migrations: %w", err)
	}
	migs := make([]Migration, 0, len(entries))
	for _, e := range entries {
		version, name, err := parseFilename(e.Name())
		if err != nil {
			return nil, err
		}
		data, err := files.ReadFile("migrations/" + e.Name())
		if err != nil {
			return nil, fmt.Errorf("schema: read %s: %w", e.Name(), err)
		}
		migs = append(migs, Migration{
			Version:  version,
			Name:     name,
			SQL:      string(data),
			Checksum: checksum(data),
		})
	}
	// ReadDir returns lexical order, which for zero-padded numbers is already
	// version order — but that is a property of the padding, not a guarantee
	// we want to lean on. Sort explicitly: 0002 must never run before 0001.
	sort.Slice(migs, func(i, j int) bool { return migs[i].Version < migs[j].Version })
	for i := 1; i < len(migs); i++ {
		if migs[i].Version == migs[i-1].Version {
			return nil, fmt.Errorf("schema: two migrations share version %04d (%s and %s)",
				migs[i].Version, migs[i-1].Name, migs[i].Name)
		}
	}
	return migs, nil
}

// plan decides what to run: every migration not yet in the ledger, in order.
//
// For a version that IS in the ledger, the stored checksum must match the
// embedded file. A mismatch means someone edited a migration that has already
// run — the one mistake this table exists to catch, because the edited SQL
// will never execute on this database and the schema silently forks from the
// files. That must fail loudly, not warn.
//
// Ledger versions with no matching file are tolerated on purpose: that is
// what a rolled-back binary sees against a newer schema, and refusing to
// start would turn every rollback into an outage.
func plan(migs []Migration, applied map[uint32]uint64) ([]Migration, error) {
	var pending []Migration
	for _, m := range migs {
		sum, ok := applied[m.Version]
		if !ok {
			pending = append(pending, m)
			continue
		}
		if sum != m.Checksum {
			return nil, fmt.Errorf(
				"schema: migration %04d_%s has already been applied but its file has changed "+
					"(ledger checksum %d, file checksum %d). Applied migrations are immutable: "+
					"the edited SQL will never run here and the schema would silently diverge "+
					"from the files. Revert the edit and add a new migration instead",
				m.Version, m.Name, sum, m.Checksum)
		}
	}
	return pending, nil
}

// Apply brings the connection's database up to date and reports how many
// migrations it ran. Safe to call on every startup: an up-to-date database
// costs one small SELECT.
func Apply(ctx context.Context, conn driver.Conn) (int, error) {
	migs, err := Load()
	if err != nil {
		return 0, err
	}

	if err := conn.Exec(ctx, createMigrationsTable); err != nil {
		return 0, fmt.Errorf("schema: create schema_migrations: %w", err)
	}

	// FINAL collapses ReplacingMergeTree duplicates that have not merged yet,
	// so a version re-inserted moments ago still reads as one row.
	rows, err := conn.Query(ctx, "SELECT version, checksum FROM schema_migrations FINAL")
	if err != nil {
		return 0, fmt.Errorf("schema: read schema_migrations: %w", err)
	}
	applied := make(map[uint32]uint64)
	for rows.Next() {
		var version uint32
		var sum uint64
		if err := rows.Scan(&version, &sum); err != nil {
			rows.Close()
			return 0, fmt.Errorf("schema: read schema_migrations: %w", err)
		}
		applied[version] = sum
	}
	if err := rows.Err(); err != nil {
		rows.Close()
		return 0, fmt.Errorf("schema: read schema_migrations: %w", err)
	}
	rows.Close()

	pending, err := plan(migs, applied)
	if err != nil {
		return 0, err
	}

	done := 0
	for _, m := range pending {
		// The driver executes exactly ONE statement per Exec, so the file is
		// split first. There is no transaction to lean on — ClickHouse DDL is
		// not transactional — so a failure mid-file leaves the earlier
		// statements in place and the version NOT recorded. That is the least
		// bad option: every statement is IF NOT EXISTS by convention, so the
		// re-run after a fix skips what already landed.
		for _, stmt := range SplitStatements(m.SQL) {
			if err := conn.Exec(ctx, stmt); err != nil {
				return done, fmt.Errorf("schema: migration %04d_%s: %w\nstatement:\n%s",
					m.Version, m.Name, err, stmt)
			}
		}
		if err := conn.Exec(ctx,
			"INSERT INTO schema_migrations (version, name, checksum) VALUES (?, ?, ?)",
			m.Version, m.Name, m.Checksum); err != nil {
			return done, fmt.Errorf("schema: record migration %04d_%s: %w", m.Version, m.Name, err)
		}
		done++
	}
	return done, nil
}
