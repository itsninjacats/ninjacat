// Code generated from DataDog/rum-events-format@ab3e7c5a13f1a6fed3d63bb22f1e8a9ebaa25671. DO NOT EDIT.
//
// Regenerate with: RUM_EVENTS_FORMAT=<checkout> go generate ./rumevents/...

package replay

import (
	"github.com/itsninjacats/server/rumevents/internal/jsonx"
)

// Segment: Schema of a Session Replay data Segment.
//
// Implemented by: BrowserSegment, MobileSegment.
type Segment interface {
	// SchemaRoots lists the platform root schemas whose union contains the variant, i.e. which SDK
	// families can send it.
	SchemaRoots() []string
	isSegment()
}

// SchemaRoots reports the platform root schemas that list BrowserSegment.
func (*BrowserSegment) SchemaRoots() []string { return []string{"session-replay-browser-schema.json"} }
func (*BrowserSegment) isSegment()            {}

// SchemaRoots reports the platform root schemas that list MobileSegment.
func (*MobileSegment) SchemaRoots() []string { return []string{"session-replay-mobile-schema.json"} }
func (*MobileSegment) isSegment()            {}

// segmentVariants lists every concrete top-level type in schema order together with the
// discriminator constraints that select it. A constraint on an optional path matches when the path
// is absent; a constraint on a required path does not. The first matching variant wins.
var segmentVariants = []jsonx.Variant{
	{Name: "BrowserSegment", New: func() any { return new(BrowserSegment) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{"browser"}, Required: true}}},
	{Name: "MobileSegment", New: func() any { return new(MobileSegment) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{"android", "ios", "flutter", "react-native", "kotlin-multiplatform", "maui"}, Required: true}}},
}
