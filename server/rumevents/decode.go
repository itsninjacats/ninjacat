package rumevents

import (
	"fmt"

	"github.com/itsninjacats/server/rumevents/internal/jsonx"
)

// UnknownEventError is returned by Decode when no variant's discriminators
// match. The raw JSON is still valid input for a generic reader; the caller
// keeps it.
type UnknownEventError struct {
	// Type is the top-level "type" value, or "" when it is absent or not a string.
	Type string
}

func (e *UnknownEventError) Error() string {
	if e.Type == "" {
		return "rumevents: event without a recognised \"type\" discriminator"
	}
	return fmt.Sprintf("rumevents: no variant matches event type %q", e.Type)
}

// Decode decodes a single JSON object from the RUM intake track and returns
// the variant selected by its discriminator fields. Unknown keys at any level
// are preserved in the AdditionalProperties map of the enclosing struct.
func Decode(data []byte) (Event, error) {
	i, pr, err := jsonx.MatchVariant(data, eventVariants)
	if err != nil {
		return nil, fmt.Errorf("rumevents: %w", err)
	}
	if i < 0 {
		t, _, _ := pr.Lookup([]string{"type"})
		s, _ := t.(string)
		return nil, &UnknownEventError{Type: s}
	}
	ev := eventVariants[i].New().(Event)
	if err := jsonx.UnmarshalNumber(data, ev); err != nil {
		return nil, fmt.Errorf("rumevents: %s: %w", eventVariants[i].Name, err)
	}
	return ev, nil
}
