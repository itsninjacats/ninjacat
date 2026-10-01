package intake

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"math"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"ergo.services/ergo/gen"
	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// Golden fixtures for the F# port of this package.
//
// With NINJACAT_FIXTURE_DIR set, every request any test sends through an
// engine is written out as one JSON file: the request as it arrived, the
// response, and every batch the handler sent to storage — each row both as
// the struct and as the arguments its AppendTo hands the ClickHouse driver.
// The F# test suite replays the request and must produce the same three.
//
//	NINJACAT_FIXTURE_DIR=/tmp/fixtures go test ./intake

func init() {
	dir := os.Getenv("NINJACAT_FIXTURE_DIR")
	if dir == "" {
		return
	}
	rec := &fixtureRecorder{dir: dir, seq: map[string]int{}, inserts: map[string]string{}}
	for _, w := range storage.Writers() {
		rec.inserts[string(w.Name)] = w.Config.Insert
	}
	fixtureHook = rec.middleware
}

type fixtureRecorder struct {
	dir     string
	mu      sync.Mutex
	seq     map[string]int
	inserts map[string]string
}

type fixtureMessage struct {
	Method  string              `json:"method,omitempty"`
	Host    string              `json:"host,omitempty"`
	URL     string              `json:"url,omitempty"`
	Status  int                 `json:"status,omitempty"`
	Headers map[string][]string `json:"headers"`
	Body    string              `json:"body_b64"`
}

type fixtureSend struct {
	To     string `json:"to"`
	Type   string `json:"type"`
	Insert string `json:"insert"`
	Rows   []any  `json:"rows"`
	// Args[i] is what Rows[i].AppendTo gave the driver, in INSERT column order.
	Args     [][]any    `json:"args"`
	ArgTypes [][]string `json:"arg_types"`
}

type fixture struct {
	Test     string         `json:"test"`
	Seq      int            `json:"seq"`
	Routes   []string       `json:"routes"`
	Recorded string         `json:"recorded_at"`
	Request  fixtureMessage `json:"request"`
	Response fixtureMessage `json:"response"`
	// Sends is null when the test ran without a capturing node: only the
	// response can be compared then.
	Sends []fixtureSend `json:"sends"`
}

type bodyRecorder struct {
	gin.ResponseWriter
	body bytes.Buffer
}

func (w *bodyRecorder) Write(b []byte) (int, error) {
	w.body.Write(b)
	return w.ResponseWriter.Write(b)
}

func (w *bodyRecorder) WriteString(s string) (int, error) {
	w.body.WriteString(s)
	return w.ResponseWriter.WriteString(s)
}

func (rec *fixtureRecorder) middleware(a *Server, routes []func(*gin.RouterGroup)) gin.HandlerFunc {
	names := make([]string, 0, len(routes))
	for _, r := range routes {
		full := runtime.FuncForPC(reflect.ValueOf(r).Pointer()).Name()
		name := full[strings.LastIndex(full, ".")+1:]
		names = append(names, strings.TrimSuffix(name, "-fm"))
	}

	return func(c *gin.Context) {
		node, _ := a.Node.(*captureNode)

		body, _ := io.ReadAll(c.Request.Body)
		c.Request.Body = io.NopCloser(bytes.NewReader(body))

		before := 0
		if node != nil {
			before = len(node.Sends())
		}
		w := &bodyRecorder{ResponseWriter: c.Writer}
		c.Writer = w
		reqHeaders := c.Request.Header.Clone()
		url := c.Request.URL.RequestURI()

		c.Next()

		f := fixture{
			Test:     "anonymous",
			Routes:   names,
			Recorded: time.Now().UTC().Format(time.RFC3339Nano),
			Request: fixtureMessage{
				Method: c.Request.Method, Host: c.Request.Host, URL: url,
				Headers: reqHeaders, Body: base64.StdEncoding.EncodeToString(body),
			},
			Response: fixtureMessage{
				Status: c.Writer.Status(), Headers: c.Writer.Header().Clone(),
				Body: base64.StdEncoding.EncodeToString(w.body.Bytes()),
			},
		}
		if node != nil {
			f.Test = node.test
			f.Sends = []fixtureSend{}
			for _, s := range node.Sends()[before:] {
				f.Sends = append(f.Sends, rec.send(s))
			}
		}
		rec.write(f)
	}
}

func (rec *fixtureRecorder) send(s capturedSend) fixtureSend {
	to := fmt.Sprint(s.To)
	if atom, ok := s.To.(gen.Atom); ok {
		to = string(atom)
	}
	out := fixtureSend{To: to, Type: reflect.TypeOf(s.Msg).String(), Insert: rec.inserts[to], Rows: []any{}}

	v := reflect.ValueOf(s.Msg)
	if v.Kind() != reflect.Struct {
		return out
	}
	for i := 0; i < v.NumField(); i++ {
		f := v.Type().Field(i)
		if !f.IsExported() || f.Type.Kind() != reflect.Slice {
			continue
		}
		slice := v.Field(i)
		for j := 0; j < slice.Len(); j++ {
			row := slice.Index(j).Interface()
			out.Rows = append(out.Rows, encodeValue(reflect.ValueOf(row)))

			if r, ok := row.(storage.Row); ok {
				batch := &storagetest.CaptureBatch{}
				if err := r.AppendTo(batch); err == nil {
					args := make([]any, len(batch.Args))
					types := make([]string, len(batch.Args))
					for k, arg := range batch.Args {
						args[k] = encodeValue(reflect.ValueOf(arg))
						types[k] = fmt.Sprintf("%T", arg)
					}
					out.Args = append(out.Args, args)
					out.ArgTypes = append(out.ArgTypes, types)
				}
			}
		}
		break
	}
	return out
}

func (rec *fixtureRecorder) write(f fixture) {
	rec.mu.Lock()
	rec.seq[f.Test]++
	f.Seq = rec.seq[f.Test]
	rec.mu.Unlock()

	dir := filepath.Join(rec.dir, strings.ReplaceAll(f.Test, "/", "__"))
	if err := os.MkdirAll(dir, 0o755); err != nil {
		panic(err)
	}
	data, err := json.MarshalIndent(f, "", " ")
	if err != nil {
		panic(fmt.Sprintf("fixture %s #%d: %v", f.Test, f.Seq, err))
	}
	if err := os.WriteFile(filepath.Join(dir, fmt.Sprintf("%03d.json", f.Seq)), data, 0o644); err != nil {
		panic(err)
	}
}

var timeType = reflect.TypeOf(time.Time{})

// encodeBytes writes text as text and anything else as {"$b64": …}. A Go
// string and a []byte are the same thing to the row that carries them.
func encodeBytes(b []byte) any {
	if !utf8.Valid(b) {
		return map[string]any{"$b64": base64.StdEncoding.EncodeToString(b)}
	}
	return string(b)
}

// encodeValue turns a Go value into something encoding/json writes without
// loss: bytes that are not UTF-8 as {"$b64": …}, a [16]byte as a UUID, times
// as RFC 3339 with nanoseconds, NaN and infinities as strings, map keys as strings.
func encodeValue(v reflect.Value) any {
	if !v.IsValid() {
		return nil
	}
	if v.Type() == timeType {
		return v.Interface().(time.Time).UTC().Format(time.RFC3339Nano)
	}

	switch v.Kind() {
	case reflect.Pointer, reflect.Interface:
		if v.IsNil() {
			return nil
		}
		return encodeValue(v.Elem())
	case reflect.String:
		return encodeBytes([]byte(v.String()))
	case reflect.Bool:
		return v.Bool()
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		return v.Int()
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		return v.Uint()
	case reflect.Float32, reflect.Float64:
		f := v.Float()
		switch {
		case math.IsNaN(f):
			return "NaN"
		case math.IsInf(f, 1):
			return "+Inf"
		case math.IsInf(f, -1):
			return "-Inf"
		}
		return f
	case reflect.Slice, reflect.Array:
		if v.Type().Elem().Kind() == reflect.Uint8 {
			b := make([]byte, v.Len())
			reflect.Copy(reflect.ValueOf(b), v)
			if v.Kind() == reflect.Array && len(b) == 16 {
				return uuid.UUID(b).String()
			}
			return encodeBytes(b)
		}
		out := make([]any, v.Len())
		for i := range out {
			out[i] = encodeValue(v.Index(i))
		}
		return out
	case reflect.Map:
		out := map[string]any{}
		keys := v.MapKeys()
		sort.Slice(keys, func(i, j int) bool { return fmt.Sprint(keys[i]) < fmt.Sprint(keys[j]) })
		for _, k := range keys {
			out[fmt.Sprint(k.Interface())] = encodeValue(v.MapIndex(k))
		}
		return out
	case reflect.Struct:
		out := map[string]any{}
		for i := 0; i < v.NumField(); i++ {
			if f := v.Type().Field(i); f.IsExported() {
				out[f.Name] = encodeValue(v.Field(i))
			}
		}
		return out
	}
	return fmt.Sprintf("<%s>", v.Kind())
}
