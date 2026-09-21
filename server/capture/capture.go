// Package capture dumps every incoming request to disk.
//
// This is a DEBUGGING TOOL, not part of the request path. It is how the whole
// Datadog protocol was worked out: point an agent at ninjacat, set
// DEBUG=true, and read what actually arrives — decompressed, with JSON pretty
// printed, protobuf walked field by field, and a hexdump as the last resort.
//
// It still earns its place: roughly forty Datadog endpoints remain unhandled,
// and this is the tool for each of them. It lives in its own package so that
// none of it is reachable from a handler.
package capture

import (
	"bytes"
	"compress/flate"
	"compress/gzip"
	"compress/zlib"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"math"
	"net/http/httputil"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/gin-gonic/gin"
	"github.com/klauspost/compress/zstd"
)

var (
	captureSeq  atomic.Uint64
	captureOnce sync.Once
	captureDir  string
	indexMu     sync.Mutex
)

// captureRoot returns the dump directory, creating it on first use.
// The path can be overridden with NINJACAT_CAPTURE_DIR.
func captureRoot() string {
	captureOnce.Do(func() {
		captureDir = os.Getenv("NINJACAT_CAPTURE_DIR")
		if captureDir == "" {
			captureDir = "captures"
		}
		if err := os.MkdirAll(captureDir, 0o755); err != nil {
			log.Printf("nie moge zalozyc %s: %v", captureDir, err)
		}
	})
	return captureDir
}

func RequestLogger() gin.HandlerFunc {
	return func(c *gin.Context) {
		t := time.Now()

		head, _ := httputil.DumpRequest(c.Request, false)

		raw, _ := io.ReadAll(c.Request.Body)
		c.Request.Body = io.NopCloser(bytes.NewBuffer(raw)) // oddajemy body handlerowi

		encoding := c.GetHeader("Content-Encoding")
		decoded, decErr := decodeBody(encoding, raw)

		c.Next()

		name, err := writeCapture(c, t, head, raw, decoded, decErr, encoding)
		if err != nil {
			log.Printf("zrzut nie zapisany: %v", err)
			return
		}
		log.Printf("%-6s %-24s %d  %8d B -> %-8d B  %-6s  %s",
			c.Request.Method, c.Request.URL.Path, c.Writer.Status(),
			len(raw), len(decoded), orDash(encoding), name)
	}
}

// writeCapture zapisuje jeden request do wlasnego pliku i dokleja linie do index.log.
func writeCapture(c *gin.Context, start time.Time, head, raw, decoded []byte, decErr error, encoding string) (string, error) {
	seq := captureSeq.Add(1)
	name := fmt.Sprintf("%s_%s_%s_%04d.txt",
		start.Format("20060102T150405.000"),
		c.Request.Method,
		slugify(c.Request.URL.Path),
		seq)
	full := filepath.Join(captureRoot(), name)

	var b strings.Builder
	fmt.Fprintf(&b, "# czas:      %s\n", start.Format(time.RFC3339Nano))
	fmt.Fprintf(&b, "# klient:    %s\n", c.ClientIP())
	fmt.Fprintf(&b, "# status:    %d\n", c.Writer.Status())
	fmt.Fprintf(&b, "# czas obsl: %s\n", time.Since(start))
	fmt.Fprintf(&b, "# body:      %d B surowe -> %d B po dekompresji (%s)\n",
		len(raw), len(decoded), orDash(encoding))
	if decErr != nil {
		fmt.Fprintf(&b, "# UWAGA:     dekompresja nie wyszla: %v\n", decErr)
	}
	b.WriteString("\n")
	b.Write(head)
	b.WriteString("\n---------------- BODY ----------------\n")
	b.WriteString(renderBody(c.GetHeader("Content-Type"), decoded))
	b.WriteString("\n")

	if err := os.WriteFile(full, []byte(b.String()), 0o644); err != nil {
		return "", err
	}
	appendIndex(c, start, raw, decoded, encoding, name)
	return full, nil
}

func appendIndex(c *gin.Context, start time.Time, raw, decoded []byte, encoding, name string) {
	indexMu.Lock()
	defer indexMu.Unlock()

	f, err := os.OpenFile(filepath.Join(captureRoot(), "index.log"),
		os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return
	}
	defer f.Close()

	fmt.Fprintf(f, "%s\t%s\t%s\t%d\t%d\t%d\t%s\t%s\n",
		start.Format(time.RFC3339), c.Request.Method, c.Request.URL.Path,
		c.Writer.Status(), len(raw), len(decoded), orDash(encoding), name)
}

// renderBody probuje po kolei: JSON, protobuf, czysty tekst, hex.
func renderBody(contentType string, b []byte) string {
	if len(b) == 0 {
		return "(puste body)"
	}
	var pretty bytes.Buffer
	if json.Indent(&pretty, b, "", "  ") == nil {
		return pretty.String()
	}
	if dump, ok := tryDumpProto(b); ok {
		return "(protobuf - zrzut generyczny, format: numer_pola: wartosc)\n\n" + dump
	}
	if isPrintable(b) {
		return string(b)
	}
	return hexDump(b)
}

// tryDumpProto parsuje wire format protobufa bez znajomosci schematu.
func tryDumpProto(b []byte) (string, bool) {
	var sb strings.Builder
	if !dumpProtoInto(&sb, b, 0) || sb.Len() == 0 {
		return "", false
	}
	return sb.String(), true
}

func dumpProtoInto(sb *strings.Builder, b []byte, depth int) bool {
	if len(b) == 0 || depth > 8 {
		return false
	}
	pad := strings.Repeat("  ", depth)

	for i := 0; i < len(b); {
		tag, n := binary.Uvarint(b[i:])
		if n <= 0 {
			return false
		}
		i += n

		field, wire := tag>>3, tag&7
		if field == 0 {
			return false
		}

		switch wire {
		case 0: // varint
			v, n := binary.Uvarint(b[i:])
			if n <= 0 {
				return false
			}
			i += n
			fmt.Fprintf(sb, "%s%d: %d\n", pad, field, v)

		case 1: // fixed64
			if i+8 > len(b) {
				return false
			}
			u := binary.LittleEndian.Uint64(b[i:])
			fmt.Fprintf(sb, "%s%d: %d (f64 %g)\n", pad, field, u, math.Float64frombits(u))
			i += 8

		case 5: // fixed32
			if i+4 > len(b) {
				return false
			}
			u := binary.LittleEndian.Uint32(b[i:])
			fmt.Fprintf(sb, "%s%d: %d (f32 %g)\n", pad, field, u, math.Float32frombits(u))
			i += 4

		case 2: // length-delimited
			ln, n := binary.Uvarint(b[i:])
			if n <= 0 {
				return false
			}
			i += n
			if ln > uint64(len(b)-i) {
				return false
			}
			val := b[i : i+int(ln)]
			i += int(ln)

			var nested strings.Builder
			switch {
			case len(val) == 0:
				fmt.Fprintf(sb, "%s%d: \"\"\n", pad, field)
			case isPrintable(val):
				fmt.Fprintf(sb, "%s%d: %q\n", pad, field, val)
			case dumpProtoInto(&nested, val, depth+1):
				fmt.Fprintf(sb, "%s%d: {\n%s%s}\n", pad, field, nested.String(), pad)
			default:
				fmt.Fprintf(sb, "%s%d: <%d B> %s\n", pad, field, len(val), hexPreview(val))
			}

		default:
			return false
		}
	}
	return true
}

func isPrintable(b []byte) bool {
	if !utf8.Valid(b) {
		return false
	}
	for _, r := range string(b) {
		if r == '\n' || r == '\r' || r == '\t' {
			continue
		}
		if !unicode.IsPrint(r) {
			return false
		}
	}
	return true
}

func hexPreview(b []byte) string {
	const max = 16
	if len(b) > max {
		return fmt.Sprintf("%x...", b[:max])
	}
	return fmt.Sprintf("%x", b)
}

func hexDump(b []byte) string {
	const limit = 4096
	truncated := false
	if len(b) > limit {
		b, truncated = b[:limit], true
	}

	var sb strings.Builder
	for off := 0; off < len(b); off += 16 {
		end := off + 16
		if end > len(b) {
			end = len(b)
		}
		row := b[off:end]

		fmt.Fprintf(&sb, "%08x  ", off)
		for i := 0; i < 16; i++ {
			if i < len(row) {
				fmt.Fprintf(&sb, "%02x ", row[i])
			} else {
				sb.WriteString("   ")
			}
		}
		sb.WriteString(" |")
		for _, ch := range row {
			if ch >= 0x20 && ch < 0x7f {
				sb.WriteByte(ch)
			} else {
				sb.WriteByte('.')
			}
		}
		sb.WriteString("|\n")
	}
	if truncated {
		fmt.Fprintf(&sb, "... (obciete do %d B)\n", limit)
	}
	return sb.String()
}

func slugify(path string) string {
	s := strings.Trim(path, "/")
	if s == "" {
		s = "root"
	}
	return strings.Map(func(r rune) rune {
		if r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' {
			return r
		}
		return '_'
	}, s)
}

func orDash(s string) string {
	if s == "" {
		return "-"
	}
	return s
}

// decodeBody mirrors intake's decompression. Duplicated on purpose: this
// package is a debugging tool and must not be something the request path
// depends on, so it does not import intake and intake does not import it.
func decodeBody(encoding string, body []byte) ([]byte, error) {
	switch encoding {
	case "zstd":
		r, err := zstd.NewReader(bytes.NewReader(body))
		if err != nil {
			return body, err
		}
		defer r.Close()
		out, err := io.ReadAll(r)
		if err != nil {
			return body, err
		}
		return out, nil

	case "gzip":
		r, err := gzip.NewReader(bytes.NewReader(body))
		if err != nil {
			return body, err
		}
		defer r.Close()
		out, err := io.ReadAll(r)
		if err != nil {
			return body, err
		}
		return out, nil

	case "deflate", "zlib":
		if r, err := zlib.NewReader(bytes.NewReader(body)); err == nil {
			defer r.Close()
			if out, err := io.ReadAll(r); err == nil {
				return out, nil
			}
		}
		r := flate.NewReader(bytes.NewReader(body))
		defer r.Close()
		out, err := io.ReadAll(r)
		if err != nil {
			return body, err
		}
		return out, nil

	default:
		return body, nil
	}
}
