package storage

import (
	"context"
	"fmt"
	"sync/atomic"
	"time"

	"ergo.services/ergo/act"
	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// WriterConfig describes one table's writer. Every writer runs the same code;
// only these values differ.
type WriterConfig struct {
	// Name is used in logs, e.g. "metrics".
	Name string

	// Insert is the full INSERT statement with its column list.
	Insert string

	// MaxRows flushes early once the buffer reaches this size. Tune per table:
	// logs arrive 10-100x more often than metrics, check runs far less often.
	MaxRows int

	// FlushInterval flushes on a timer even under light traffic.
	FlushInterval time.Duration

	// BufferLimit caps memory use when ClickHouse is unreachable. Beyond this
	// rows are dropped — losing data beats an out-of-memory kill.
	BufferLimit int

	// MaxInFlight bounds concurrent flushes, so a slow database does not
	// multiply goroutines and connections.
	MaxInFlight int32
}

func (c WriterConfig) withDefaults() WriterConfig {
	if c.MaxRows == 0 {
		c.MaxRows = 5000
	}
	if c.FlushInterval == 0 {
		c.FlushInterval = 2 * time.Second
	}
	if c.BufferLimit == 0 {
		c.BufferLimit = 200_000
	}
	if c.MaxInFlight == 0 {
		c.MaxInFlight = 4
	}
	return c
}

// BatchWriter buffers rows for ONE table and flushes them in batches.
//
// One writer per table, not one writer for everything. The reason is the
// mailbox: logs arrive 10-100x more often than metrics, and a burst of them
// would make metrics queue behind logs for no reason. Separate actors mean
// separate mailboxes, separate buffers and independent failure.
//
// A pool of writers for the SAME table would be wrong for the opposite reason:
//
//	1 writer  -> 1 buffer  -> large batches -> few parts in ClickHouse
//	N writers -> N buffers -> N× smaller batches -> N× more parts
//
// That reintroduces the "Too many parts" problem buffering exists to avoid.
// What a pool would legitimately fix — stalling during a flush — is solved
// instead by handing the full buffer to a goroutine and carrying on with a
// fresh one (see flushBuffer).
type BatchWriter struct {
	act.Actor

	cfg    WriterConfig
	conn   driver.Conn
	buffer []Row
	cancel gen.CancelFunc
	log    gen.Log

	inFlight atomic.Int32
	written  atomic.Uint64
	dropped  atomic.Uint64
	failed   atomic.Uint64
}

func (w *BatchWriter) Init(args ...any) error {
	if len(args) < 2 {
		return fmt.Errorf("writer: expected (WriterConfig, driver.Conn), got %d args", len(args))
	}
	cfg, ok := args[0].(WriterConfig)
	if !ok {
		return fmt.Errorf("writer: first arg must be WriterConfig, got %T", args[0])
	}
	conn, ok := args[1].(driver.Conn)
	if !ok {
		return fmt.Errorf("writer: second arg must be driver.Conn, got %T", args[1])
	}

	w.cfg = cfg.withDefaults()
	w.conn = conn
	w.buffer = make([]Row, 0, w.cfg.MaxRows)
	// Captured so background flush goroutines can log after Init returns.
	w.log = w.Log()

	stop, err := w.SendEvery(w.PID(), flush{}, w.cfg.FlushInterval)
	if err != nil {
		return fmt.Errorf("writer %s: timer: %w", w.cfg.Name, err)
	}
	w.cancel = stop

	w.log.Info("writer %s started, flush every %s or %d rows",
		w.cfg.Name, w.cfg.FlushInterval, w.cfg.MaxRows)
	return nil
}

func (w *BatchWriter) HandleMessage(from gen.PID, message any) error {
	switch m := message.(type) {
	case flush:
		w.flushBuffer()

	case rowsProvider:
		// Every Write* message satisfies this, so the writer stays unaware of
		// which table it serves.
		rows := m.rows()
		if len(w.buffer)+len(rows) > w.cfg.BufferLimit {
			total := w.dropped.Add(uint64(len(rows)))
			w.log.Warning("writer %s: buffer full (%d rows), dropped %d (total %d)",
				w.cfg.Name, len(w.buffer), len(rows), total)
			return nil
		}

		w.buffer = append(w.buffer, rows...)
		if len(w.buffer) >= w.cfg.MaxRows {
			w.flushBuffer()
		}

	default:
		w.log.Warning("writer %s: unexpected message %T", w.cfg.Name, message)
	}
	return nil
}

func (w *BatchWriter) HandleCall(from gen.PID, ref gen.Ref, request any) (any, error) {
	return map[string]any{
		"table":     w.cfg.Name,
		"buffered":  len(w.buffer),
		"written":   w.written.Load(),
		"dropped":   w.dropped.Load(),
		"failed":    w.failed.Load(),
		"in_flight": w.inFlight.Load(),
	}, nil
}

func (w *BatchWriter) Terminate(reason error) {
	if w.cancel != nil {
		w.cancel()
	}
	// Final flush, synchronous this time — we are shutting down and there is
	// no mailbox left to protect. The connection is owned by the supervisor,
	// so it is not closed here.
	if len(w.buffer) > 0 {
		w.send(w.buffer)
	}
}

// flushBuffer hands the full buffer to a background goroutine and immediately
// continues with a fresh one.
//
// This is the important bit: the actor must never block on I/O. A batch insert
// can take seconds, and while it runs the mailbox would pile up.
func (w *BatchWriter) flushBuffer() {
	if len(w.buffer) == 0 {
		return
	}

	// Backpressure: if too many flushes are already running, keep buffering
	// rather than piling up goroutines and connections.
	if w.inFlight.Load() >= w.cfg.MaxInFlight {
		w.log.Warning("writer %s: %d flushes in flight, deferring (buffered %d)",
			w.cfg.Name, w.inFlight.Load(), len(w.buffer))
		return
	}

	batch := w.buffer
	// Fresh buffer, so the goroutine below owns `batch` exclusively.
	// No locking needed: only this actor ever touches w.buffer.
	w.buffer = make([]Row, 0, w.cfg.MaxRows)

	w.inFlight.Add(1)
	go func() {
		defer w.inFlight.Add(-1)
		w.send(batch)
	}()
}

func (w *BatchWriter) send(batch []Row) {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	prepared, err := w.conn.PrepareBatch(ctx, w.cfg.Insert)
	if err != nil {
		w.failed.Add(uint64(len(batch)))
		w.log.Error("writer %s: prepare batch: %s", w.cfg.Name, err)
		return
	}

	for _, row := range batch {
		if err := row.AppendTo(prepared); err != nil {
			w.failed.Add(uint64(len(batch)))
			w.log.Error("writer %s: append row: %s", w.cfg.Name, err)
			return
		}
	}

	if err := prepared.Send(); err != nil {
		// The batch is lost. We deliberately do NOT put it back: the actor has
		// moved on and re-queueing from another goroutine would need locking,
		// which is exactly what this design avoids.
		w.failed.Add(uint64(len(batch)))
		w.log.Error("writer %s: send batch (%d rows): %s", w.cfg.Name, len(batch), err)
		return
	}

	total := w.written.Add(uint64(len(batch)))
	w.log.Debug("writer %s: wrote %d rows (total %d)", w.cfg.Name, len(batch), total)
}
