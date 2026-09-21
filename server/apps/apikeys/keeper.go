package apikeys

import (
	"context"
	"fmt"
	"os"
	"time"

	"ergo.services/ergo/act"
	"ergo.services/ergo/gen"
	"github.com/jackc/pgx/v5/pgxpool"
)

// How often keys are re-read from the database.
const refreshInterval = 30 * time.Second

// Refresh tells the keeper to re-read the keys from the database now.
//
// The timer sends it every 30 seconds, but so does the panel right after a
// key is added or removed — which is what makes such a change take effect
// immediately instead of waiting for the next cycle.
//
// This is message passing used for what it is good at: a rare control event,
// not a read on the hot path. Reads go through the atomic.Pointer in
// store.go.
type Refresh struct{}

// Keeper is the sole owner of the API keys held in memory.
//
// It keeps a Postgres connection pool, re-reads the api_key table on a timer
// and publishes an immutable snapshot into Store. The HTTP layer reads that
// snapshot without locking — see the comment in store.go.
//
// If the actor dies, the supervisor restarts it and the first Init fetches
// the keys again. The old snapshot stays in Store meanwhile, so agents keep
// being accepted throughout the restart.
type Keeper struct {
	act.Actor

	pool  *pgxpool.Pool
	store *Store
	stop  gen.CancelFunc
}

func (k *Keeper) Init(args ...any) error {
	// Store comes from the application so the HTTP layer can hold the very
	// same pointer.
	if len(args) == 0 {
		return fmt.Errorf("keeper: missing Store in args")
	}
	store, ok := args[0].(*Store)
	if !ok {
		return fmt.Errorf("keeper: first arg must be *Store, got %T", args[0])
	}
	k.store = store

	url := os.Getenv("DATABASE_URL")
	if url == "" {
		return fmt.Errorf("keeper: DATABASE_URL not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	pool, err := pgxpool.New(ctx, url)
	if err != nil {
		return fmt.Errorf("keeper: connection pool: %w", err)
	}
	k.pool = pool

	// The first fetch is synchronous: we want to know straight away whether
	// the database answers. An error here aborts startup and escalates to
	// the supervisor.
	if err := k.fetch(); err != nil {
		pool.Close()
		return fmt.Errorf("keeper: initial fetch: %w", err)
	}

	// SendEvery wakes us on a cycle. The returned function cancels the timer
	// when the actor stops.
	stop, err := k.SendEvery(k.PID(), Refresh{}, refreshInterval)
	if err != nil {
		pool.Close()
		return fmt.Errorf("keeper: timer: %w", err)
	}
	k.stop = stop

	k.Log().Info("keeper started, %d keys, refreshing every %s",
		k.store.Current().Len(), refreshInterval)
	return nil
}

func (k *Keeper) HandleMessage(from gen.PID, message any) error {
	switch message.(type) {
	case Refresh:
		if err := k.fetch(); err != nil {
			// A failed refresh is NOT worth dying over: the old snapshot
			// still works and the database may come back. Log and carry on.
			k.Log().Warning("keeper: refresh failed: %s", err)
			return nil
		}
		k.Log().Debug("keeper: refreshed, %d keys", k.store.Current().Len())
	}
	return nil
}

// HandleCall lets something outside inspect the state, the observer included.
func (k *Keeper) HandleCall(from gen.PID, ref gen.Ref, request any) (any, error) {
	snap := k.store.Current()
	return map[string]any{
		"keys":      snap.Len(),
		"refreshed": snap.UpdatedAt,
	}, nil
}

func (k *Keeper) Terminate(reason error) {
	if k.stop != nil {
		k.stop()
	}
	if k.pool != nil {
		k.pool.Close()
	}
}

// fetch reads every key and publishes a new snapshot.
func (k *Keeper) fetch() error {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	rows, err := k.pool.Query(ctx, `SELECT id::text, name, tenant_id, key_hash FROM api_key`)
	if err != nil {
		return err
	}
	defer rows.Close()

	var keys []Key
	var hashes []string
	for rows.Next() {
		var id, name, tenant, hash string
		if err := rows.Scan(&id, &name, &tenant, &hash); err != nil {
			return err
		}
		keys = append(keys, Key{ID: id, Name: name, TenantID: tenant})
		hashes = append(hashes, hash)
	}
	if err := rows.Err(); err != nil {
		return err
	}

	k.store.Publish(keys, hashes)
	return nil
}
