package replay

import (
	"fmt"

	"github.com/itsninjacats/server/rumevents/internal/jsonx"
)

// UnknownSegmentError is returned by Decode when the segment's `source`
// matches neither the browser nor the mobile segment schema. The caller
// still holds the raw bytes.
type UnknownSegmentError struct {
	// Source is the top-level "source" value, or "" when absent or not a string.
	Source string
}

func (e *UnknownSegmentError) Error() string {
	if e.Source == "" {
		return "replay: segment without a recognised \"source\" discriminator"
	}
	return fmt.Sprintf("replay: no segment variant matches source %q", e.Source)
}

// Decode decodes one decompressed Session Replay segment and returns the
// variant selected by its `source`. Unknown keys at any level are preserved
// in the AdditionalProperties map of the enclosing struct; records whose type
// is unknown are kept in the envelope's Raw field.
func Decode(data []byte) (Segment, error) {
	i, pr, err := jsonx.MatchVariant(data, segmentVariants)
	if err != nil {
		return nil, fmt.Errorf("replay: %w", err)
	}
	if i < 0 {
		src, _, _ := pr.Lookup([]string{"source"})
		s, _ := src.(string)
		return nil, &UnknownSegmentError{Source: s}
	}
	seg := segmentVariants[i].New().(Segment)
	if err := jsonx.UnmarshalNumber(data, seg); err != nil {
		return nil, fmt.Errorf("replay: %s: %w", segmentVariants[i].Name, err)
	}
	return seg, nil
}
