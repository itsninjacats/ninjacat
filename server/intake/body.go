package intake

import (
	"bytes"
	"compress/flate"
	"compress/gzip"
	"compress/zlib"
	"io"
	"log"
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/klauspost/compress/zstd"
)

// Decompress replaces the request body with a decompressed one, so handlers
// can read it with c.GetRawData() and never learn it was compressed.
//
// Nobody upstream does this for us, and that is by design rather than an
// oversight: Content-Encoding is END-TO-END. It belongs to the sender and the
// final recipient, and an intermediary must not touch it. Transfer-Encoding is
// the hop-by-hop one, which net/http does unwrap.
//
// So a reverse proxy in front of us — Traefik, nginx, whatever — forwards the
// body still compressed, with its Content-Encoding header intact. Correct
// behaviour, and the reason this middleware exists.
//
// Datadog uses three codecs, one per subsystem: zstd for metrics and /intake/,
// gzip for traces and APM stats, deflate for distribution points.
func Decompress() gin.HandlerFunc {
	return func(c *gin.Context) {
		encoding := c.GetHeader("Content-Encoding")
		if encoding == "" {
			c.Next()
			return
		}

		raw, err := io.ReadAll(c.Request.Body)
		if err != nil {
			c.AbortWithStatusJSON(http.StatusBadRequest,
				gin.H{"errors": []string{"cannot read body"}})
			return
		}

		decoded, err := decompress(encoding, raw)
		if err != nil {
			// Failing open on purpose: a mislabelled Content-Encoding should
			// not lose a payload the handler could still parse. decompress
			// returns the original bytes alongside the error.
			log.Printf("[body] %s decompression failed, passing through: %v", encoding, err)
		}

		c.Request.Body = io.NopCloser(bytes.NewReader(decoded))
		c.Request.ContentLength = int64(len(decoded))
		c.Next()
	}
}

// decompress returns the body decompressed, or the body unchanged when the
// encoding is unknown or decompression fails.
func decompress(encoding string, body []byte) ([]byte, error) {
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
		// The RFC says zlib, but some clients send bare deflate — try both.
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
