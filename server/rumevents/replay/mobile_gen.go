// Code generated from DataDog/rum-events-format@ab3e7c5a13f1a6fed3d63bb22f1e8a9ebaa25671. DO NOT EDIT.
//
// Regenerate with: RUM_EVENTS_FORMAT=<checkout> go generate ./rumevents/...

package replay

import (
	"encoding/json"
	"github.com/itsninjacats/server/rumevents/internal/jsonx"
)

// MobileSegment: Mobile-specific. Schema of a Session Replay data Segment.
//
// Listed by: session-replay-mobile-schema.json.
type MobileSegment struct {
	// Application properties.
	Application *SegmentContextApplication `json:"application,omitzero"`

	// Session properties.
	Session *SegmentContextSession `json:"session,omitzero"`

	// View properties.
	View *SegmentContextView `json:"view,omitzero"`

	// The start UTC timestamp in milliseconds corresponding to the first record in the Segment data.
	// Each timestamp is computed as the UTC interval since 00:00:00.000 01.01.1970.
	Start int64 `json:"start"`

	// The end UTC timestamp in milliseconds corresponding to the last record in the Segment data.
	// Each timestamp is computed as the UTC interval since 00:00:00.000 01.01.1970.
	End int64 `json:"end"`

	// The number of records in this Segment.
	RecordsCount int64 `json:"records_count"`

	// The index of this Segment in the segments list that was recorded for this view ID. Starts from
	// 0.
	IndexInView *int64 `json:"index_in_view,omitzero"`

	// Whether this Segment contains a full snapshot record or not.
	HasFullSnapshot *bool `json:"has_full_snapshot,omitzero"`

	// The source of this record.
	// One of: "android", "ios", "flutter", "react-native", "kotlin-multiplatform", "maui".
	Source string `json:"source"`

	// The records contained by this Segment.
	Records []MobileRecord `json:"records,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var mobileSegmentKeys = map[string]struct{}{"application": {}, "session": {}, "view": {}, "start": {}, "end": {}, "records_count": {}, "index_in_view": {}, "has_full_snapshot": {}, "source": {}, "records": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *MobileSegment) UnmarshalJSON(data []byte) error {
	type plain MobileSegment
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, mobileSegmentKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = MobileSegment(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o MobileSegment) MarshalJSON() ([]byte, error) {
	type plain MobileSegment
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// MobileRecord: Mobile-specific. Schema of a Session Replay Record.
//
// Discriminated union selected by type: exactly one variant field is set. When no variant's
// discriminator matches (a record type newer than the schema), Raw keeps the original JSON so
// nothing is lost.
type MobileRecord struct {
	MobileFullSnapshotRecord        *MobileFullSnapshotRecord
	MobileIncrementalSnapshotRecord *MobileIncrementalSnapshotRecord
	MetaRecord                      *MetaRecord
	FocusRecord                     *FocusRecord
	ViewEndRecord                   *ViewEndRecord
	VisualViewportRecord            *VisualViewportRecord

	// Raw holds the JSON of an alternative that matched no variant.
	Raw json.RawMessage
}

var mobileRecordVariants = []jsonx.Variant{
	{Name: "MobileFullSnapshotRecord", New: func() any { return new(MobileFullSnapshotRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("10")}, Required: true}}},
	{Name: "MobileIncrementalSnapshotRecord", New: func() any { return new(MobileIncrementalSnapshotRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("11")}, Required: true}}},
	{Name: "MetaRecord", New: func() any { return new(MetaRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("4")}, Required: true}}},
	{Name: "FocusRecord", New: func() any { return new(FocusRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("6")}, Required: true}}},
	{Name: "ViewEndRecord", New: func() any { return new(ViewEndRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("7")}, Required: true}}},
	{Name: "VisualViewportRecord", New: func() any { return new(VisualViewportRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("8")}, Required: true}}},
}

// UnmarshalJSON selects the variant by its discriminator; an unmatched alternative is kept in Raw.
func (u *MobileRecord) UnmarshalJSON(data []byte) error {
	*u = MobileRecord{}
	if jsonx.IsNull(data) {
		return nil
	}
	i, v, err := jsonx.DecodeVariant(data, mobileRecordVariants)
	if err != nil {
		return err
	}
	switch i {
	case 0:
		u.MobileFullSnapshotRecord = v.(*MobileFullSnapshotRecord)
	case 1:
		u.MobileIncrementalSnapshotRecord = v.(*MobileIncrementalSnapshotRecord)
	case 2:
		u.MetaRecord = v.(*MetaRecord)
	case 3:
		u.FocusRecord = v.(*FocusRecord)
	case 4:
		u.ViewEndRecord = v.(*ViewEndRecord)
	case 5:
		u.VisualViewportRecord = v.(*VisualViewportRecord)
	default:
		u.Raw = append(json.RawMessage(nil), data...)
	}
	return nil
}

// MarshalJSON encodes the set variant, or Raw, or null when neither is set.
func (u MobileRecord) MarshalJSON() ([]byte, error) {
	if v := u.Variant(); v != nil {
		return jsonx.Marshal(v)
	}
	if u.Raw != nil {
		return u.Raw, nil
	}
	return []byte("null"), nil
}

// Variant returns the populated alternative as a pointer to its concrete type, or nil.
func (u *MobileRecord) Variant() any {
	switch {
	case u.MobileFullSnapshotRecord != nil:
		return u.MobileFullSnapshotRecord
	case u.MobileIncrementalSnapshotRecord != nil:
		return u.MobileIncrementalSnapshotRecord
	case u.MetaRecord != nil:
		return u.MetaRecord
	case u.FocusRecord != nil:
		return u.FocusRecord
	case u.ViewEndRecord != nil:
		return u.ViewEndRecord
	case u.VisualViewportRecord != nil:
		return u.VisualViewportRecord
	}
	return nil
}

// MobileFullSnapshotRecord: Mobile-specific. Schema of a Record type which contains the full
// snapshot of a screen.
type MobileFullSnapshotRecord struct {
	// Defines the UTC time in milliseconds when this Record was performed.
	Timestamp int64 `json:"timestamp"`

	// The type of this Record.
	// Always 10.
	Type int64 `json:"type"`

	Data *MobileFullSnapshotRecordData `json:"data,omitzero"`

	// Unique ID of the slot that generated this record.
	SlotID *string `json:"slotId,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var mobileFullSnapshotRecordKeys = map[string]struct{}{"timestamp": {}, "type": {}, "data": {}, "slotId": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *MobileFullSnapshotRecord) UnmarshalJSON(data []byte) error {
	type plain MobileFullSnapshotRecord
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, mobileFullSnapshotRecordKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = MobileFullSnapshotRecord(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o MobileFullSnapshotRecord) MarshalJSON() ([]byte, error) {
	type plain MobileFullSnapshotRecord
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// MobileFullSnapshotRecordData
type MobileFullSnapshotRecordData struct {
	// The Wireframes contained by this Record.
	Wireframes []Wireframe `json:"wireframes,omitzero"`

	// Optional composition tree describing the rendering hierarchy for a full snapshot. When present,
	// the player uses this tree for rendering order and group operations instead of rendering the
	// wireframes as a flat array.
	CompositionTree *CompositionTree `json:"compositionTree,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var mobileFullSnapshotRecordDataKeys = map[string]struct{}{"wireframes": {}, "compositionTree": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *MobileFullSnapshotRecordData) UnmarshalJSON(data []byte) error {
	type plain MobileFullSnapshotRecordData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, mobileFullSnapshotRecordDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = MobileFullSnapshotRecordData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o MobileFullSnapshotRecordData) MarshalJSON() ([]byte, error) {
	type plain MobileFullSnapshotRecordData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// Wireframe: Schema of a Wireframe type.
//
// Discriminated union selected by type: exactly one variant field is set. When no variant's
// discriminator matches (a record type newer than the schema), Raw keeps the original JSON so
// nothing is lost.
type Wireframe struct {
	ShapeWireframe           *ShapeWireframe
	TextWireframe            *TextWireframe
	ImageWireframe           *ImageWireframe
	PlaceholderWireframe     *PlaceholderWireframe
	WebviewWireframe         *WebviewWireframe
	EmbeddedContentWireframe *EmbeddedContentWireframe

	// Raw holds the JSON of an alternative that matched no variant.
	Raw json.RawMessage
}

var wireframeVariants = []jsonx.Variant{
	{Name: "ShapeWireframe", New: func() any { return new(ShapeWireframe) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"shape"}, Required: true}}},
	{Name: "TextWireframe", New: func() any { return new(TextWireframe) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"text"}, Required: true}}},
	{Name: "ImageWireframe", New: func() any { return new(ImageWireframe) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"image"}, Required: true}}},
	{Name: "PlaceholderWireframe", New: func() any { return new(PlaceholderWireframe) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"placeholder"}, Required: true}}},
	{Name: "WebviewWireframe", New: func() any { return new(WebviewWireframe) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"webview"}, Required: true}}},
	{Name: "EmbeddedContentWireframe", New: func() any { return new(EmbeddedContentWireframe) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"embedded_content"}, Required: true}}},
}

// UnmarshalJSON selects the variant by its discriminator; an unmatched alternative is kept in Raw.
func (u *Wireframe) UnmarshalJSON(data []byte) error {
	*u = Wireframe{}
	if jsonx.IsNull(data) {
		return nil
	}
	i, v, err := jsonx.DecodeVariant(data, wireframeVariants)
	if err != nil {
		return err
	}
	switch i {
	case 0:
		u.ShapeWireframe = v.(*ShapeWireframe)
	case 1:
		u.TextWireframe = v.(*TextWireframe)
	case 2:
		u.ImageWireframe = v.(*ImageWireframe)
	case 3:
		u.PlaceholderWireframe = v.(*PlaceholderWireframe)
	case 4:
		u.WebviewWireframe = v.(*WebviewWireframe)
	case 5:
		u.EmbeddedContentWireframe = v.(*EmbeddedContentWireframe)
	default:
		u.Raw = append(json.RawMessage(nil), data...)
	}
	return nil
}

// MarshalJSON encodes the set variant, or Raw, or null when neither is set.
func (u Wireframe) MarshalJSON() ([]byte, error) {
	if v := u.Variant(); v != nil {
		return jsonx.Marshal(v)
	}
	if u.Raw != nil {
		return u.Raw, nil
	}
	return []byte("null"), nil
}

// Variant returns the populated alternative as a pointer to its concrete type, or nil.
func (u *Wireframe) Variant() any {
	switch {
	case u.ShapeWireframe != nil:
		return u.ShapeWireframe
	case u.TextWireframe != nil:
		return u.TextWireframe
	case u.ImageWireframe != nil:
		return u.ImageWireframe
	case u.PlaceholderWireframe != nil:
		return u.PlaceholderWireframe
	case u.WebviewWireframe != nil:
		return u.WebviewWireframe
	case u.EmbeddedContentWireframe != nil:
		return u.EmbeddedContentWireframe
	}
	return nil
}

// ShapeWireframe: Schema of all properties of a ShapeWireframe.
type ShapeWireframe struct {
	// Defines the unique ID of the wireframe. This is persistent throughout the view lifetime.
	ID int64 `json:"id"`

	// The position in pixels on X axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	X int64 `json:"x"`

	// The position in pixels on Y axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	Y int64 `json:"y"`

	// The width in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the width of all UI elements is divided by 2 to get
	// a normalized width.
	Width int64 `json:"width"`

	// The height in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the height of all UI elements is divided by 2 to get
	// a normalized height.
	Height int64 `json:"height"`

	// Schema of clipping information for a Wireframe.
	Clip *WireframeClip `json:"clip,omitzero"`

	// The style of this wireframe.
	ShapeStyle *ShapeStyle `json:"shapeStyle,omitzero"`

	// The border properties of this wireframe. The default value is null (no-border).
	Border *ShapeBorder `json:"border,omitzero"`

	// The type of the wireframe.
	// Always "shape".
	Type string `json:"type"`

	// A globally unique and stable identifier for this UI element, computed as the hash of the
	// element's path. Used to correlate wireframes with RUM action events.
	PermanentID *string `json:"permanentId,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var shapeWireframeKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "clip": {}, "shapeStyle": {}, "border": {}, "type": {}, "permanentId": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *ShapeWireframe) UnmarshalJSON(data []byte) error {
	type plain ShapeWireframe
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, shapeWireframeKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = ShapeWireframe(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o ShapeWireframe) MarshalJSON() ([]byte, error) {
	type plain ShapeWireframe
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// WireframeClip: Schema of clipping information for a Wireframe.
type WireframeClip struct {
	// The amount of space in pixels that needs to be clipped (masked) at the top of the wireframe.
	Top *int64 `json:"top,omitzero"`

	// The amount of space in pixels that needs to be clipped (masked) at the bottom of the wireframe.
	Bottom *int64 `json:"bottom,omitzero"`

	// The amount of space in pixels that needs to be clipped (masked) at the left of the wireframe.
	Left *int64 `json:"left,omitzero"`

	// The amount of space in pixels that needs to be clipped (masked) at the right of the wireframe.
	Right *int64 `json:"right,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var wireframeClipKeys = map[string]struct{}{"top": {}, "bottom": {}, "left": {}, "right": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *WireframeClip) UnmarshalJSON(data []byte) error {
	type plain WireframeClip
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, wireframeClipKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = WireframeClip(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o WireframeClip) MarshalJSON() ([]byte, error) {
	type plain WireframeClip
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// ShapeStyle: The style of this wireframe.
type ShapeStyle struct {
	// The background color for this wireframe as a String hexadecimal. Follows the #RRGGBBAA color
	// format with the alpha value as optional. The default value is #FFFFFF00.
	BackgroundColor *string `json:"backgroundColor,omitzero"`

	// The background gradient for this wireframe.
	BackgroundGradient *ShapeLinearGradient `json:"backgroundGradient,omitzero"`

	// The opacity of this wireframe. Takes values from 0 to 1, default value is 1.
	Opacity json.Number `json:"opacity,omitzero"`

	// The corner(border) radius of this wireframe in pixels. The default value is 0.
	CornerRadius json.Number `json:"cornerRadius,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var shapeStyleKeys = map[string]struct{}{"backgroundColor": {}, "backgroundGradient": {}, "opacity": {}, "cornerRadius": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *ShapeStyle) UnmarshalJSON(data []byte) error {
	type plain ShapeStyle
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, shapeStyleKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = ShapeStyle(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o ShapeStyle) MarshalJSON() ([]byte, error) {
	type plain ShapeStyle
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// ShapeLinearGradient: A linear background gradient for a shape wireframe. Colors before the first
// stop and after the last stop are clamped to the nearest stop color.
//
// Flattened oneOf of: ShapeLinearGradient. All alternative-specific fields are optional.
type ShapeLinearGradient struct {
	// The type of the gradient.
	// Always "linear".
	Type string `json:"type"`

	// Ordered gradient color stops. Positions must be non-decreasing.
	Stops []ShapeGradientStop `json:"stops,omitzero"`

	// The point where position 0 of the gradient is placed.
	StartPoint *ShapeGradientPoint `json:"startPoint,omitzero"`

	// The point where position 1 of the gradient is placed.
	EndPoint *ShapeGradientPoint `json:"endPoint,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var shapeLinearGradientKeys = map[string]struct{}{"type": {}, "stops": {}, "startPoint": {}, "endPoint": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *ShapeLinearGradient) UnmarshalJSON(data []byte) error {
	type plain ShapeLinearGradient
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, shapeLinearGradientKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = ShapeLinearGradient(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o ShapeLinearGradient) MarshalJSON() ([]byte, error) {
	type plain ShapeLinearGradient
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// ShapeGradientStop: A color and its relative position in a shape gradient.
type ShapeGradientStop struct {
	// The stop color as a hexadecimal string in #RRGGBB or #RRGGBBAA format.
	Color string `json:"color"`

	// Relative stop position between 0 and 1. Stops must be ordered by non-decreasing position.
	Position json.Number `json:"position,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var shapeGradientStopKeys = map[string]struct{}{"color": {}, "position": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *ShapeGradientStop) UnmarshalJSON(data []byte) error {
	type plain ShapeGradientStop
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, shapeGradientStopKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = ShapeGradientStop(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o ShapeGradientStop) MarshalJSON() ([]byte, error) {
	type plain ShapeGradientStop
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// ShapeGradientPoint: A point in the wireframe's normalized coordinate space. The top-left corner
// is (0, 0), the bottom-right corner is (1, 1), and values outside this range place the point
// outside the wireframe bounds.
type ShapeGradientPoint struct {
	// Horizontal position, where 0 is the left edge and 1 is the right edge.
	X json.Number `json:"x,omitzero"`

	// Vertical position, where 0 is the top edge and 1 is the bottom edge.
	Y json.Number `json:"y,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var shapeGradientPointKeys = map[string]struct{}{"x": {}, "y": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *ShapeGradientPoint) UnmarshalJSON(data []byte) error {
	type plain ShapeGradientPoint
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, shapeGradientPointKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = ShapeGradientPoint(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o ShapeGradientPoint) MarshalJSON() ([]byte, error) {
	type plain ShapeGradientPoint
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// ShapeBorder: The border properties of this wireframe. The default value is null (no-border).
type ShapeBorder struct {
	// The border color as a String hexadecimal. Follows the #RRGGBBAA color format with the alpha
	// value as optional.
	Color string `json:"color"`

	// The width of the border in pixels.
	Width int64 `json:"width"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var shapeBorderKeys = map[string]struct{}{"color": {}, "width": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *ShapeBorder) UnmarshalJSON(data []byte) error {
	type plain ShapeBorder
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, shapeBorderKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = ShapeBorder(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o ShapeBorder) MarshalJSON() ([]byte, error) {
	type plain ShapeBorder
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TextWireframe: Schema of all properties of a TextWireframe.
type TextWireframe struct {
	// Defines the unique ID of the wireframe. This is persistent throughout the view lifetime.
	ID int64 `json:"id"`

	// The position in pixels on X axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	X int64 `json:"x"`

	// The position in pixels on Y axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	Y int64 `json:"y"`

	// The width in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the width of all UI elements is divided by 2 to get
	// a normalized width.
	Width int64 `json:"width"`

	// The height in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the height of all UI elements is divided by 2 to get
	// a normalized height.
	Height int64 `json:"height"`

	// Schema of clipping information for a Wireframe.
	Clip *WireframeClip `json:"clip,omitzero"`

	// The style of this wireframe.
	ShapeStyle *ShapeStyle `json:"shapeStyle,omitzero"`

	// The border properties of this wireframe. The default value is null (no-border).
	Border *ShapeBorder `json:"border,omitzero"`

	// The type of the wireframe.
	// Always "text".
	Type string `json:"type"`

	// The text value of the wireframe.
	Text string `json:"text"`

	// Schema of all properties of a TextStyle.
	TextStyle *TextStyle `json:"textStyle,omitzero"`

	// Schema of all properties of a TextPosition.
	TextPosition *TextPosition `json:"textPosition,omitzero"`

	// A globally unique and stable identifier for this UI element, computed as the hash of the
	// element's path. Used to correlate wireframes with RUM action events.
	PermanentID *string `json:"permanentId,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var textWireframeKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "clip": {}, "shapeStyle": {}, "border": {}, "type": {}, "text": {}, "textStyle": {}, "textPosition": {}, "permanentId": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TextWireframe) UnmarshalJSON(data []byte) error {
	type plain TextWireframe
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, textWireframeKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TextWireframe(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TextWireframe) MarshalJSON() ([]byte, error) {
	type plain TextWireframe
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TextStyle: Schema of all properties of a TextStyle.
type TextStyle struct {
	// The preferred font family collection, ordered by preference and formatted as a String list:
	// e.g. Century Gothic, Verdana, sans-serif.
	Family string `json:"family"`

	// The font size in pixels.
	Size int64 `json:"size"`

	// The font color as a string hexadecimal. Follows the #RRGGBBAA color format with the alpha value
	// as optional.
	Color string `json:"color"`

	// Defines how text should be truncated when it exceeds the wireframe bounds. If omitted, text
	// wraps naturally.
	// One of: "clip", "head", "tail", "middle".
	TruncationMode *string `json:"truncationMode,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var textStyleKeys = map[string]struct{}{"family": {}, "size": {}, "color": {}, "truncationMode": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TextStyle) UnmarshalJSON(data []byte) error {
	type plain TextStyle
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, textStyleKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TextStyle(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TextStyle) MarshalJSON() ([]byte, error) {
	type plain TextStyle
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TextPosition: Schema of all properties of a TextPosition.
type TextPosition struct {
	Padding *TextPositionPadding `json:"padding,omitzero"`

	Alignment *TextPositionAlignment `json:"alignment,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var textPositionKeys = map[string]struct{}{"padding": {}, "alignment": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TextPosition) UnmarshalJSON(data []byte) error {
	type plain TextPosition
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, textPositionKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TextPosition(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TextPosition) MarshalJSON() ([]byte, error) {
	type plain TextPosition
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TextPositionPadding
type TextPositionPadding struct {
	// The top padding in pixels. The default value is 0.
	Top *int64 `json:"top,omitzero"`

	// The bottom padding in pixels. The default value is 0.
	Bottom *int64 `json:"bottom,omitzero"`

	// The left padding in pixels. The default value is 0.
	Left *int64 `json:"left,omitzero"`

	// The right padding in pixels. The default value is 0.
	Right *int64 `json:"right,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var textPositionPaddingKeys = map[string]struct{}{"top": {}, "bottom": {}, "left": {}, "right": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TextPositionPadding) UnmarshalJSON(data []byte) error {
	type plain TextPositionPadding
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, textPositionPaddingKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TextPositionPadding(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TextPositionPadding) MarshalJSON() ([]byte, error) {
	type plain TextPositionPadding
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TextPositionAlignment
type TextPositionAlignment struct {
	// The horizontal text alignment. The default value is `left`.
	// One of: "left", "right", "center".
	Horizontal *string `json:"horizontal,omitzero"`

	// The vertical text alignment. The default value is `top`.
	// One of: "top", "bottom", "center".
	Vertical *string `json:"vertical,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var textPositionAlignmentKeys = map[string]struct{}{"horizontal": {}, "vertical": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TextPositionAlignment) UnmarshalJSON(data []byte) error {
	type plain TextPositionAlignment
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, textPositionAlignmentKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TextPositionAlignment(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TextPositionAlignment) MarshalJSON() ([]byte, error) {
	type plain TextPositionAlignment
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// ImageWireframe: Schema of all properties of a ImageWireframe.
type ImageWireframe struct {
	// Defines the unique ID of the wireframe. This is persistent throughout the view lifetime.
	ID int64 `json:"id"`

	// The position in pixels on X axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	X int64 `json:"x"`

	// The position in pixels on Y axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	Y int64 `json:"y"`

	// The width in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the width of all UI elements is divided by 2 to get
	// a normalized width.
	Width int64 `json:"width"`

	// The height in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the height of all UI elements is divided by 2 to get
	// a normalized height.
	Height int64 `json:"height"`

	// Schema of clipping information for a Wireframe.
	Clip *WireframeClip `json:"clip,omitzero"`

	// The style of this wireframe.
	ShapeStyle *ShapeStyle `json:"shapeStyle,omitzero"`

	// The border properties of this wireframe. The default value is null (no-border).
	Border *ShapeBorder `json:"border,omitzero"`

	// The type of the wireframe.
	// Always "image".
	Type string `json:"type"`

	// base64 representation of the image. Not required as the ImageWireframe can be initialised
	// without any base64.
	Base64 *string `json:"base64,omitzero"`

	// Unique identifier of the image resource.
	ResourceID *string `json:"resourceId,omitzero"`

	// MIME type of the image file.
	MimeType *string `json:"mimeType,omitzero"`

	// Flag describing an image wireframe that should render an empty state placeholder.
	IsEmpty *bool `json:"isEmpty,omitzero"`

	// A globally unique and stable identifier for this UI element, computed as the hash of the
	// element's path. Used to correlate wireframes with RUM action events.
	PermanentID *string `json:"permanentId,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var imageWireframeKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "clip": {}, "shapeStyle": {}, "border": {}, "type": {}, "base64": {}, "resourceId": {}, "mimeType": {}, "isEmpty": {}, "permanentId": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *ImageWireframe) UnmarshalJSON(data []byte) error {
	type plain ImageWireframe
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, imageWireframeKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = ImageWireframe(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o ImageWireframe) MarshalJSON() ([]byte, error) {
	type plain ImageWireframe
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// PlaceholderWireframe: Schema of all properties of a PlaceholderWireframe.
type PlaceholderWireframe struct {
	// Defines the unique ID of the wireframe. This is persistent throughout the view lifetime.
	ID int64 `json:"id"`

	// The position in pixels on X axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	X int64 `json:"x"`

	// The position in pixels on Y axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	Y int64 `json:"y"`

	// The width in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the width of all UI elements is divided by 2 to get
	// a normalized width.
	Width int64 `json:"width"`

	// The height in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the height of all UI elements is divided by 2 to get
	// a normalized height.
	Height int64 `json:"height"`

	// Schema of clipping information for a Wireframe.
	Clip *WireframeClip `json:"clip,omitzero"`

	// The type of the wireframe.
	// Always "placeholder".
	Type string `json:"type"`

	// Label of the placeholder.
	Label *string `json:"label,omitzero"`

	// A globally unique and stable identifier for this UI element, computed as the hash of the
	// element's path. Used to correlate wireframes with RUM action events.
	PermanentID *string `json:"permanentId,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var placeholderWireframeKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "clip": {}, "type": {}, "label": {}, "permanentId": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *PlaceholderWireframe) UnmarshalJSON(data []byte) error {
	type plain PlaceholderWireframe
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, placeholderWireframeKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = PlaceholderWireframe(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o PlaceholderWireframe) MarshalJSON() ([]byte, error) {
	type plain PlaceholderWireframe
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// WebviewWireframe: Schema of all properties of a WebviewWireframe.
type WebviewWireframe struct {
	// Defines the unique ID of the wireframe. This is persistent throughout the view lifetime.
	ID int64 `json:"id"`

	// The position in pixels on X axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	X int64 `json:"x"`

	// The position in pixels on Y axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	Y int64 `json:"y"`

	// The width in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the width of all UI elements is divided by 2 to get
	// a normalized width.
	Width int64 `json:"width"`

	// The height in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the height of all UI elements is divided by 2 to get
	// a normalized height.
	Height int64 `json:"height"`

	// Schema of clipping information for a Wireframe.
	Clip *WireframeClip `json:"clip,omitzero"`

	// The style of this wireframe.
	ShapeStyle *ShapeStyle `json:"shapeStyle,omitzero"`

	// The border properties of this wireframe. The default value is null (no-border).
	Border *ShapeBorder `json:"border,omitzero"`

	// The type of the wireframe.
	// Always "webview".
	Type string `json:"type"`

	// Unique Id of the slot containing this webview.
	SlotID string `json:"slotId"`

	// Whether this webview is visible or not.
	IsVisible *bool `json:"isVisible,omitzero"`

	// A globally unique and stable identifier for this UI element, computed as the hash of the
	// element's path. Used to correlate wireframes with RUM action events.
	PermanentID *string `json:"permanentId,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var webviewWireframeKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "clip": {}, "shapeStyle": {}, "border": {}, "type": {}, "slotId": {}, "isVisible": {}, "permanentId": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *WebviewWireframe) UnmarshalJSON(data []byte) error {
	type plain WebviewWireframe
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, webviewWireframeKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = WebviewWireframe(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o WebviewWireframe) MarshalJSON() ([]byte, error) {
	type plain WebviewWireframe
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// EmbeddedContentWireframe: Schema of all properties of an EmbeddedContentWireframe.
type EmbeddedContentWireframe struct {
	// Defines the unique ID of the wireframe. This is persistent throughout the view lifetime.
	ID int64 `json:"id"`

	// The position in pixels on X axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	X int64 `json:"x"`

	// The position in pixels on Y axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	Y int64 `json:"y"`

	// The width in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the width of all UI elements is divided by 2 to get
	// a normalized width.
	Width int64 `json:"width"`

	// The height in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the height of all UI elements is divided by 2 to get
	// a normalized height.
	Height int64 `json:"height"`

	// Schema of clipping information for a Wireframe.
	Clip *WireframeClip `json:"clip,omitzero"`

	// The style of this wireframe.
	ShapeStyle *ShapeStyle `json:"shapeStyle,omitzero"`

	// The border properties of this wireframe. The default value is null (no-border).
	Border *ShapeBorder `json:"border,omitzero"`

	// The type of the wireframe.
	// Always "embedded_content".
	Type string `json:"type"`

	// Unique Id of the slot containing this embedded content.
	SlotID string `json:"slotId"`

	// Whether this embedded content is visible or not.
	IsVisible *bool `json:"isVisible,omitzero"`

	// A globally unique and stable identifier for this UI element, computed as the hash of the
	// element's path. Used to correlate wireframes with RUM action events.
	PermanentID *string `json:"permanentId,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var embeddedContentWireframeKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "clip": {}, "shapeStyle": {}, "border": {}, "type": {}, "slotId": {}, "isVisible": {}, "permanentId": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *EmbeddedContentWireframe) UnmarshalJSON(data []byte) error {
	type plain EmbeddedContentWireframe
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, embeddedContentWireframeKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = EmbeddedContentWireframe(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o EmbeddedContentWireframe) MarshalJSON() ([]byte, error) {
	type plain EmbeddedContentWireframe
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CompositionTree: Optional composition tree describing the rendering hierarchy for a full
// snapshot. When present, the player uses this tree for rendering order and group operations
// instead of rendering the wireframes as a flat array.
type CompositionTree struct {
	// A rendering group that groups child wireframes and child layers. Does not draw pixels itself.
	// Ordered rendering modifiers and compositing are applied to its composed output.
	Root *CompositionLayer `json:"root,omitzero"`

	// Non-root composition layers referenced by the tree.
	Layers []CompositionLayer `json:"layers,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var compositionTreeKeys = map[string]struct{}{"root": {}, "layers": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CompositionTree) UnmarshalJSON(data []byte) error {
	type plain CompositionTree
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, compositionTreeKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CompositionTree(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CompositionTree) MarshalJSON() ([]byte, error) {
	type plain CompositionTree
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CompositionLayer: A rendering group that groups child wireframes and child layers. Does not draw
// pixels itself. Ordered rendering modifiers and compositing are applied to its composed output.
type CompositionLayer struct {
	// Stable layer identifier, persistent throughout the view lifetime.
	ID int64 `json:"id"`

	// The position in pixels on the X axis of the layer in absolute coordinates. Uses the same
	// coordinate space as mobile wireframes.
	X int64 `json:"x"`

	// The position in pixels on the Y axis of the layer in absolute coordinates. Uses the same
	// coordinate space as mobile wireframes.
	Y int64 `json:"y"`

	// The width in pixels of the layer. Uses the same coordinate space as mobile wireframes.
	Width int64 `json:"width"`

	// The height in pixels of the layer. Uses the same coordinate space as mobile wireframes.
	Height int64 `json:"height"`

	// Ordered back-to-front references to child wireframes or child layers.
	Children []CompositionLayerChild `json:"children,omitzero"`

	// Ordered list of rendering modifiers applied to the composed layer output in array order.
	Modifiers []CompositionLayerModifier `json:"modifiers,omitzero"`

	// Operation used when compositing the rendered group into its parent.
	// One of: "sourceOver", "destinationIn", "destinationOut", "plusDarker".
	CompositeOperation *string `json:"compositeOperation,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var compositionLayerKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "children": {}, "modifiers": {}, "compositeOperation": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CompositionLayer) UnmarshalJSON(data []byte) error {
	type plain CompositionLayer
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, compositionLayerKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CompositionLayer(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CompositionLayer) MarshalJSON() ([]byte, error) {
	type plain CompositionLayer
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CompositionLayerChild: A reference to a child wireframe or child layer in a composition layer.
type CompositionLayerChild struct {
	// The type of the child reference.
	// One of: "wireframe", "layer".
	Type string `json:"type"`

	// The id of the referenced wireframe or layer.
	ID int64 `json:"id"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var compositionLayerChildKeys = map[string]struct{}{"type": {}, "id": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CompositionLayerChild) UnmarshalJSON(data []byte) error {
	type plain CompositionLayerChild
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, compositionLayerChildKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CompositionLayerChild(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CompositionLayerChild) MarshalJSON() ([]byte, error) {
	type plain CompositionLayerChild
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CompositionLayerModifier: A rendering modifier applied to the composed layer output.
//
// Discriminated union selected by type: exactly one variant field is set. When no variant's
// discriminator matches (a record type newer than the schema), Raw keeps the original JSON so
// nothing is lost.
type CompositionLayerModifier struct {
	CompositionLayerClipModifier           *CompositionLayerClipModifier
	CompositionLayerOpacityModifier        *CompositionLayerOpacityModifier
	CompositionLayerColorMatrixModifier    *CompositionLayerColorMatrixModifier
	CompositionLayerGaussianBlurModifier   *CompositionLayerGaussianBlurModifier
	CompositionLayerShadowModifier         *CompositionLayerShadowModifier
	CompositionLayerBrightnessBiasModifier *CompositionLayerBrightnessBiasModifier
	CompositionLayerSaturateModifier       *CompositionLayerSaturateModifier
	CompositionLayerMaskImageModifier      *CompositionLayerMaskImageModifier

	// Raw holds the JSON of an alternative that matched no variant.
	Raw json.RawMessage
}

var compositionLayerModifierVariants = []jsonx.Variant{
	{Name: "CompositionLayerClipModifier", New: func() any { return new(CompositionLayerClipModifier) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"clip"}, Required: true}}},
	{Name: "CompositionLayerOpacityModifier", New: func() any { return new(CompositionLayerOpacityModifier) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"opacity"}, Required: true}}},
	{Name: "CompositionLayerColorMatrixModifier", New: func() any { return new(CompositionLayerColorMatrixModifier) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"colorMatrix"}, Required: true}}},
	{Name: "CompositionLayerGaussianBlurModifier", New: func() any { return new(CompositionLayerGaussianBlurModifier) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"gaussianBlur"}, Required: true}}},
	{Name: "CompositionLayerShadowModifier", New: func() any { return new(CompositionLayerShadowModifier) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"shadow"}, Required: true}}},
	{Name: "CompositionLayerBrightnessBiasModifier", New: func() any { return new(CompositionLayerBrightnessBiasModifier) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"brightnessBias"}, Required: true}}},
	{Name: "CompositionLayerSaturateModifier", New: func() any { return new(CompositionLayerSaturateModifier) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"saturate"}, Required: true}}},
	{Name: "CompositionLayerMaskImageModifier", New: func() any { return new(CompositionLayerMaskImageModifier) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"maskImage"}, Required: true}}},
}

// UnmarshalJSON selects the variant by its discriminator; an unmatched alternative is kept in Raw.
func (u *CompositionLayerModifier) UnmarshalJSON(data []byte) error {
	*u = CompositionLayerModifier{}
	if jsonx.IsNull(data) {
		return nil
	}
	i, v, err := jsonx.DecodeVariant(data, compositionLayerModifierVariants)
	if err != nil {
		return err
	}
	switch i {
	case 0:
		u.CompositionLayerClipModifier = v.(*CompositionLayerClipModifier)
	case 1:
		u.CompositionLayerOpacityModifier = v.(*CompositionLayerOpacityModifier)
	case 2:
		u.CompositionLayerColorMatrixModifier = v.(*CompositionLayerColorMatrixModifier)
	case 3:
		u.CompositionLayerGaussianBlurModifier = v.(*CompositionLayerGaussianBlurModifier)
	case 4:
		u.CompositionLayerShadowModifier = v.(*CompositionLayerShadowModifier)
	case 5:
		u.CompositionLayerBrightnessBiasModifier = v.(*CompositionLayerBrightnessBiasModifier)
	case 6:
		u.CompositionLayerSaturateModifier = v.(*CompositionLayerSaturateModifier)
	case 7:
		u.CompositionLayerMaskImageModifier = v.(*CompositionLayerMaskImageModifier)
	default:
		u.Raw = append(json.RawMessage(nil), data...)
	}
	return nil
}

// MarshalJSON encodes the set variant, or Raw, or null when neither is set.
func (u CompositionLayerModifier) MarshalJSON() ([]byte, error) {
	if v := u.Variant(); v != nil {
		return jsonx.Marshal(v)
	}
	if u.Raw != nil {
		return u.Raw, nil
	}
	return []byte("null"), nil
}

// Variant returns the populated alternative as a pointer to its concrete type, or nil.
func (u *CompositionLayerModifier) Variant() any {
	switch {
	case u.CompositionLayerClipModifier != nil:
		return u.CompositionLayerClipModifier
	case u.CompositionLayerOpacityModifier != nil:
		return u.CompositionLayerOpacityModifier
	case u.CompositionLayerColorMatrixModifier != nil:
		return u.CompositionLayerColorMatrixModifier
	case u.CompositionLayerGaussianBlurModifier != nil:
		return u.CompositionLayerGaussianBlurModifier
	case u.CompositionLayerShadowModifier != nil:
		return u.CompositionLayerShadowModifier
	case u.CompositionLayerBrightnessBiasModifier != nil:
		return u.CompositionLayerBrightnessBiasModifier
	case u.CompositionLayerSaturateModifier != nil:
		return u.CompositionLayerSaturateModifier
	case u.CompositionLayerMaskImageModifier != nil:
		return u.CompositionLayerMaskImageModifier
	}
	return nil
}

// CompositionLayerClipModifier: Geometric clipping applied to the composed layer output, in
// coordinates local to the layer rectangle.
type CompositionLayerClipModifier struct {
	// The type of the modifier.
	// Always "clip".
	Type string `json:"type"`

	// SVG path string defining the clip region, in coordinates local to the layer rectangle.
	Path string `json:"path"`

	// Path fill rule. Defaults to 'nonzero'.
	// One of: "nonzero", "evenodd".
	FillRule *string `json:"fillRule,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var compositionLayerClipModifierKeys = map[string]struct{}{"type": {}, "path": {}, "fillRule": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CompositionLayerClipModifier) UnmarshalJSON(data []byte) error {
	type plain CompositionLayerClipModifier
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, compositionLayerClipModifierKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CompositionLayerClipModifier(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CompositionLayerClipModifier) MarshalJSON() ([]byte, error) {
	type plain CompositionLayerClipModifier
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CompositionLayerOpacityModifier: Opacity applied to the composed layer output at this point in
// the modifier order.
type CompositionLayerOpacityModifier struct {
	// The type of the modifier.
	// Always "opacity".
	Type string `json:"type"`

	// Opacity value from 0 to 1.
	Value json.Number `json:"value,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var compositionLayerOpacityModifierKeys = map[string]struct{}{"type": {}, "value": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CompositionLayerOpacityModifier) UnmarshalJSON(data []byte) error {
	type plain CompositionLayerOpacityModifier
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, compositionLayerOpacityModifierKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CompositionLayerOpacityModifier(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CompositionLayerOpacityModifier) MarshalJSON() ([]byte, error) {
	type plain CompositionLayerOpacityModifier
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CompositionLayerColorMatrixModifier: Color transformation using a 4x5 matrix applied to the
// composed layer output.
type CompositionLayerColorMatrixModifier struct {
	// The type of the modifier.
	// Always "colorMatrix".
	Type string `json:"type"`

	// 4x5 color matrix encoded as 20 numbers in row-major order. Input and output color channels are
	// normalized to [0, 1]. The transform for each output channel is: R' = m[0]*R + m[1]*G + m[2]*B +
	// m[3]*A + m[4], G' = m[5]*R + m[6]*G + m[7]*B + m[8]*A + m[9], B' = m[10]*R + m[11]*G + m[12]*B
	// + m[13]*A + m[14], A' = m[15]*R + m[16]*G + m[17]*B + m[18]*A + m[19]. Each output channel is
	// clamped to [0, 1] after evaluation.
	Matrix []json.Number `json:"matrix,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var compositionLayerColorMatrixModifierKeys = map[string]struct{}{"type": {}, "matrix": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CompositionLayerColorMatrixModifier) UnmarshalJSON(data []byte) error {
	type plain CompositionLayerColorMatrixModifier
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, compositionLayerColorMatrixModifierKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CompositionLayerColorMatrixModifier(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CompositionLayerColorMatrixModifier) MarshalJSON() ([]byte, error) {
	type plain CompositionLayerColorMatrixModifier
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CompositionLayerGaussianBlurModifier: Gaussian blur applied to the composed layer output.
type CompositionLayerGaussianBlurModifier struct {
	// The type of the modifier.
	// Always "gaussianBlur".
	Type string `json:"type"`

	// Gaussian blur radius.
	Radius json.Number `json:"radius,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var compositionLayerGaussianBlurModifierKeys = map[string]struct{}{"type": {}, "radius": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CompositionLayerGaussianBlurModifier) UnmarshalJSON(data []byte) error {
	type plain CompositionLayerGaussianBlurModifier
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, compositionLayerGaussianBlurModifierKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CompositionLayerGaussianBlurModifier(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CompositionLayerGaussianBlurModifier) MarshalJSON() ([]byte, error) {
	type plain CompositionLayerGaussianBlurModifier
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CompositionLayerShadowModifier: Drop shadow drawn behind the composed layer output.
type CompositionLayerShadowModifier struct {
	// The type of the modifier.
	// Always "shadow".
	Type string `json:"type"`

	// The shadow color as a String hexadecimal. Follows the #RRGGBBAA color format with the alpha
	// value as optional. SDKs should encode the effective shadow alpha in this color and omit the
	// shadow modifier when the effective alpha is 0.
	Color string `json:"color"`

	// Horizontal shadow offset in pixels.
	OffsetX json.Number `json:"offsetX,omitzero"`

	// Vertical shadow offset in pixels.
	OffsetY json.Number `json:"offsetY,omitzero"`

	// Blur radius used to create the shadow.
	Radius json.Number `json:"radius,omitzero"`

	// Optional SVG path string defining the shadow outline, in coordinates local to the layer
	// rectangle. When present, the path is interpreted using the non-zero winding rule. When omitted,
	// the shadow follows the composed layer alpha.
	Path *string `json:"path,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var compositionLayerShadowModifierKeys = map[string]struct{}{"type": {}, "color": {}, "offsetX": {}, "offsetY": {}, "radius": {}, "path": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CompositionLayerShadowModifier) UnmarshalJSON(data []byte) error {
	type plain CompositionLayerShadowModifier
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, compositionLayerShadowModifierKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CompositionLayerShadowModifier(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CompositionLayerShadowModifier) MarshalJSON() ([]byte, error) {
	type plain CompositionLayerShadowModifier
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CompositionLayerBrightnessBiasModifier: Adds a signed brightness bias to the rendered layer
// contents.
type CompositionLayerBrightnessBiasModifier struct {
	// The type of the modifier.
	// Always "brightnessBias".
	Type string `json:"type"`

	// Brightness bias from -1 to 1 added to each normalized RGB channel (alpha is unchanged). 0
	// leaves content unchanged. Positive values brighten; negative values darken. Each channel is
	// clamped to [0, 1] after the bias is applied.
	Value json.Number `json:"value,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var compositionLayerBrightnessBiasModifierKeys = map[string]struct{}{"type": {}, "value": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CompositionLayerBrightnessBiasModifier) UnmarshalJSON(data []byte) error {
	type plain CompositionLayerBrightnessBiasModifier
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, compositionLayerBrightnessBiasModifierKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CompositionLayerBrightnessBiasModifier(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CompositionLayerBrightnessBiasModifier) MarshalJSON() ([]byte, error) {
	type plain CompositionLayerBrightnessBiasModifier
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CompositionLayerSaturateModifier: Applies a saturation adjustment to the rendered layer
// contents.
type CompositionLayerSaturateModifier struct {
	// The type of the modifier.
	// Always "saturate".
	Type string `json:"type"`

	// Saturation multiplier. 1 leaves content unchanged. 0 removes saturation.
	Value json.Number `json:"value,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var compositionLayerSaturateModifierKeys = map[string]struct{}{"type": {}, "value": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CompositionLayerSaturateModifier) UnmarshalJSON(data []byte) error {
	type plain CompositionLayerSaturateModifier
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, compositionLayerSaturateModifierKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CompositionLayerSaturateModifier(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CompositionLayerSaturateModifier) MarshalJSON() ([]byte, error) {
	type plain CompositionLayerSaturateModifier
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CompositionLayerMaskImageModifier: Image mask applied to the composed layer output at this point
// in the modifier order. The referenced image is mapped to the layer bounds and interpreted as an
// alpha mask: transparent pixels hide content, opaque pixels keep content, and partial alpha
// multiplies content alpha. RGB channels are ignored.
type CompositionLayerMaskImageModifier struct {
	// The type of the modifier.
	// Always "maskImage".
	Type string `json:"type"`

	// Unique identifier of the image resource used as a bounds-aligned alpha mask.
	ResourceID string `json:"resourceId"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var compositionLayerMaskImageModifierKeys = map[string]struct{}{"type": {}, "resourceId": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CompositionLayerMaskImageModifier) UnmarshalJSON(data []byte) error {
	type plain CompositionLayerMaskImageModifier
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, compositionLayerMaskImageModifierKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CompositionLayerMaskImageModifier(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CompositionLayerMaskImageModifier) MarshalJSON() ([]byte, error) {
	type plain CompositionLayerMaskImageModifier
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// MobileIncrementalSnapshotRecord: Mobile-specific. Schema of a Record type which contains
// mutations of a screen.
type MobileIncrementalSnapshotRecord struct {
	// Defines the UTC time in milliseconds when this Record was performed.
	Timestamp int64 `json:"timestamp"`

	// The type of this Record.
	// Always 11.
	Type int64 `json:"type"`

	// Mobile-specific. Schema of a Session Replay IncrementalData type.
	Data *MobileIncrementalData `json:"data,omitzero"`

	// Unique ID of the slot that generated this record.
	SlotID *string `json:"slotId,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var mobileIncrementalSnapshotRecordKeys = map[string]struct{}{"timestamp": {}, "type": {}, "data": {}, "slotId": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *MobileIncrementalSnapshotRecord) UnmarshalJSON(data []byte) error {
	type plain MobileIncrementalSnapshotRecord
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, mobileIncrementalSnapshotRecordKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = MobileIncrementalSnapshotRecord(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o MobileIncrementalSnapshotRecord) MarshalJSON() ([]byte, error) {
	type plain MobileIncrementalSnapshotRecord
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// MobileIncrementalData: Mobile-specific. Schema of a Session Replay IncrementalData type.
//
// Discriminated union selected by source: exactly one variant field is set. When no variant's
// discriminator matches (a record type newer than the schema), Raw keeps the original JSON so
// nothing is lost.
type MobileIncrementalData struct {
	MobileMutationData          *MobileMutationData
	TouchData                   *TouchData
	ViewportResizeData          *ViewportResizeData
	PointerInteractionData      *PointerInteractionData
	CompositionTreeMutationData *CompositionTreeMutationData

	// Raw holds the JSON of an alternative that matched no variant.
	Raw json.RawMessage
}

var mobileIncrementalDataVariants = []jsonx.Variant{
	{Name: "MobileMutationData", New: func() any { return new(MobileMutationData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("0")}, Required: true}}},
	{Name: "TouchData", New: func() any { return new(TouchData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("2")}, Required: true}}},
	{Name: "ViewportResizeData", New: func() any { return new(ViewportResizeData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("4")}, Required: true}}},
	{Name: "PointerInteractionData", New: func() any { return new(PointerInteractionData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("9")}, Required: true}}},
	{Name: "CompositionTreeMutationData", New: func() any { return new(CompositionTreeMutationData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("10")}, Required: true}}},
}

// UnmarshalJSON selects the variant by its discriminator; an unmatched alternative is kept in Raw.
func (u *MobileIncrementalData) UnmarshalJSON(data []byte) error {
	*u = MobileIncrementalData{}
	if jsonx.IsNull(data) {
		return nil
	}
	i, v, err := jsonx.DecodeVariant(data, mobileIncrementalDataVariants)
	if err != nil {
		return err
	}
	switch i {
	case 0:
		u.MobileMutationData = v.(*MobileMutationData)
	case 1:
		u.TouchData = v.(*TouchData)
	case 2:
		u.ViewportResizeData = v.(*ViewportResizeData)
	case 3:
		u.PointerInteractionData = v.(*PointerInteractionData)
	case 4:
		u.CompositionTreeMutationData = v.(*CompositionTreeMutationData)
	default:
		u.Raw = append(json.RawMessage(nil), data...)
	}
	return nil
}

// MarshalJSON encodes the set variant, or Raw, or null when neither is set.
func (u MobileIncrementalData) MarshalJSON() ([]byte, error) {
	if v := u.Variant(); v != nil {
		return jsonx.Marshal(v)
	}
	if u.Raw != nil {
		return u.Raw, nil
	}
	return []byte("null"), nil
}

// Variant returns the populated alternative as a pointer to its concrete type, or nil.
func (u *MobileIncrementalData) Variant() any {
	switch {
	case u.MobileMutationData != nil:
		return u.MobileMutationData
	case u.TouchData != nil:
		return u.TouchData
	case u.ViewportResizeData != nil:
		return u.ViewportResizeData
	case u.PointerInteractionData != nil:
		return u.PointerInteractionData
	case u.CompositionTreeMutationData != nil:
		return u.CompositionTreeMutationData
	}
	return nil
}

// MobileMutationData: Mobile-specific. Schema of a MutationData.
type MobileMutationData struct {
	// The source of this type of incremental data.
	// Always 0.
	Source int64 `json:"source"`

	// Contains the newly added wireframes.
	Adds []MobileMutationPayloadAdds `json:"adds,omitzero"`

	// Contains the removed wireframes as an array of ids.
	Removes []MobileMutationPayloadRemoves `json:"removes,omitzero"`

	// Contains the updated wireframes mutations.
	Updates []WireframeUpdateMutation `json:"updates,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var mobileMutationDataKeys = map[string]struct{}{"source": {}, "adds": {}, "removes": {}, "updates": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *MobileMutationData) UnmarshalJSON(data []byte) error {
	type plain MobileMutationData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, mobileMutationDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = MobileMutationData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o MobileMutationData) MarshalJSON() ([]byte, error) {
	type plain MobileMutationData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// MobileMutationPayloadAdds
type MobileMutationPayloadAdds struct {
	// The previous wireframe id next or after which this new wireframe is drawn or attached to,
	// respectively.
	PreviousID *int64 `json:"previousId,omitzero"`

	// Schema of a Wireframe type.
	Wireframe *Wireframe `json:"wireframe,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var mobileMutationPayloadAddsKeys = map[string]struct{}{"previousId": {}, "wireframe": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *MobileMutationPayloadAdds) UnmarshalJSON(data []byte) error {
	type plain MobileMutationPayloadAdds
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, mobileMutationPayloadAddsKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = MobileMutationPayloadAdds(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o MobileMutationPayloadAdds) MarshalJSON() ([]byte, error) {
	type plain MobileMutationPayloadAdds
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// MobileMutationPayloadRemoves
type MobileMutationPayloadRemoves struct {
	// The id of the wireframe that needs to be removed.
	ID int64 `json:"id"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var mobileMutationPayloadRemovesKeys = map[string]struct{}{"id": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *MobileMutationPayloadRemoves) UnmarshalJSON(data []byte) error {
	type plain MobileMutationPayloadRemoves
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, mobileMutationPayloadRemovesKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = MobileMutationPayloadRemoves(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o MobileMutationPayloadRemoves) MarshalJSON() ([]byte, error) {
	type plain MobileMutationPayloadRemoves
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// WireframeUpdateMutation: Schema of a WireframeUpdateMutation type.
//
// Discriminated union selected by type: exactly one variant field is set. When no variant's
// discriminator matches (a record type newer than the schema), Raw keeps the original JSON so
// nothing is lost.
type WireframeUpdateMutation struct {
	TextWireframeUpdate            *TextWireframeUpdate
	ShapeWireframeUpdate           *ShapeWireframeUpdate
	ImageWireframeUpdate           *ImageWireframeUpdate
	PlaceholderWireframeUpdate     *PlaceholderWireframeUpdate
	WebviewWireframeUpdate         *WebviewWireframeUpdate
	EmbeddedContentWireframeUpdate *EmbeddedContentWireframeUpdate

	// Raw holds the JSON of an alternative that matched no variant.
	Raw json.RawMessage
}

var wireframeUpdateMutationVariants = []jsonx.Variant{
	{Name: "TextWireframeUpdate", New: func() any { return new(TextWireframeUpdate) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"text"}, Required: true}}},
	{Name: "ShapeWireframeUpdate", New: func() any { return new(ShapeWireframeUpdate) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"shape"}, Required: true}}},
	{Name: "ImageWireframeUpdate", New: func() any { return new(ImageWireframeUpdate) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"image"}, Required: true}}},
	{Name: "PlaceholderWireframeUpdate", New: func() any { return new(PlaceholderWireframeUpdate) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"placeholder"}, Required: true}}},
	{Name: "WebviewWireframeUpdate", New: func() any { return new(WebviewWireframeUpdate) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"webview"}, Required: true}}},
	{Name: "EmbeddedContentWireframeUpdate", New: func() any { return new(EmbeddedContentWireframeUpdate) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"embedded_content"}, Required: true}}},
}

// UnmarshalJSON selects the variant by its discriminator; an unmatched alternative is kept in Raw.
func (u *WireframeUpdateMutation) UnmarshalJSON(data []byte) error {
	*u = WireframeUpdateMutation{}
	if jsonx.IsNull(data) {
		return nil
	}
	i, v, err := jsonx.DecodeVariant(data, wireframeUpdateMutationVariants)
	if err != nil {
		return err
	}
	switch i {
	case 0:
		u.TextWireframeUpdate = v.(*TextWireframeUpdate)
	case 1:
		u.ShapeWireframeUpdate = v.(*ShapeWireframeUpdate)
	case 2:
		u.ImageWireframeUpdate = v.(*ImageWireframeUpdate)
	case 3:
		u.PlaceholderWireframeUpdate = v.(*PlaceholderWireframeUpdate)
	case 4:
		u.WebviewWireframeUpdate = v.(*WebviewWireframeUpdate)
	case 5:
		u.EmbeddedContentWireframeUpdate = v.(*EmbeddedContentWireframeUpdate)
	default:
		u.Raw = append(json.RawMessage(nil), data...)
	}
	return nil
}

// MarshalJSON encodes the set variant, or Raw, or null when neither is set.
func (u WireframeUpdateMutation) MarshalJSON() ([]byte, error) {
	if v := u.Variant(); v != nil {
		return jsonx.Marshal(v)
	}
	if u.Raw != nil {
		return u.Raw, nil
	}
	return []byte("null"), nil
}

// Variant returns the populated alternative as a pointer to its concrete type, or nil.
func (u *WireframeUpdateMutation) Variant() any {
	switch {
	case u.TextWireframeUpdate != nil:
		return u.TextWireframeUpdate
	case u.ShapeWireframeUpdate != nil:
		return u.ShapeWireframeUpdate
	case u.ImageWireframeUpdate != nil:
		return u.ImageWireframeUpdate
	case u.PlaceholderWireframeUpdate != nil:
		return u.PlaceholderWireframeUpdate
	case u.WebviewWireframeUpdate != nil:
		return u.WebviewWireframeUpdate
	case u.EmbeddedContentWireframeUpdate != nil:
		return u.EmbeddedContentWireframeUpdate
	}
	return nil
}

// TextWireframeUpdate: Schema of all properties of a TextWireframeUpdate.
type TextWireframeUpdate struct {
	// Defines the unique ID of the wireframe. This is persistent throughout the view lifetime.
	ID int64 `json:"id"`

	// The position in pixels on X axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	X *int64 `json:"x,omitzero"`

	// The position in pixels on Y axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	Y *int64 `json:"y,omitzero"`

	// The width in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the width of all UI elements is divided by 2 to get
	// a normalized width.
	Width *int64 `json:"width,omitzero"`

	// The height in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the height of all UI elements is divided by 2 to get
	// a normalized height.
	Height *int64 `json:"height,omitzero"`

	// Schema of clipping information for a Wireframe.
	Clip *WireframeClip `json:"clip,omitzero"`

	// The style of this wireframe.
	ShapeStyle *ShapeStyle `json:"shapeStyle,omitzero"`

	// The border properties of this wireframe. The default value is null (no-border).
	Border *ShapeBorder `json:"border,omitzero"`

	// The type of the wireframe.
	// Always "text".
	Type string `json:"type"`

	// The text value of the wireframe.
	Text *string `json:"text,omitzero"`

	// Schema of all properties of a TextStyle.
	TextStyle *TextStyle `json:"textStyle,omitzero"`

	// Schema of all properties of a TextPosition.
	TextPosition *TextPosition `json:"textPosition,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var textWireframeUpdateKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "clip": {}, "shapeStyle": {}, "border": {}, "type": {}, "text": {}, "textStyle": {}, "textPosition": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TextWireframeUpdate) UnmarshalJSON(data []byte) error {
	type plain TextWireframeUpdate
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, textWireframeUpdateKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TextWireframeUpdate(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TextWireframeUpdate) MarshalJSON() ([]byte, error) {
	type plain TextWireframeUpdate
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// ShapeWireframeUpdate: Schema of a ShapeWireframeUpdate.
type ShapeWireframeUpdate struct {
	// Defines the unique ID of the wireframe. This is persistent throughout the view lifetime.
	ID int64 `json:"id"`

	// The position in pixels on X axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	X *int64 `json:"x,omitzero"`

	// The position in pixels on Y axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	Y *int64 `json:"y,omitzero"`

	// The width in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the width of all UI elements is divided by 2 to get
	// a normalized width.
	Width *int64 `json:"width,omitzero"`

	// The height in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the height of all UI elements is divided by 2 to get
	// a normalized height.
	Height *int64 `json:"height,omitzero"`

	// Schema of clipping information for a Wireframe.
	Clip *WireframeClip `json:"clip,omitzero"`

	// The style of this wireframe.
	ShapeStyle *ShapeStyle `json:"shapeStyle,omitzero"`

	// The border properties of this wireframe. The default value is null (no-border).
	Border *ShapeBorder `json:"border,omitzero"`

	// The type of the wireframe.
	// Always "shape".
	Type string `json:"type"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var shapeWireframeUpdateKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "clip": {}, "shapeStyle": {}, "border": {}, "type": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *ShapeWireframeUpdate) UnmarshalJSON(data []byte) error {
	type plain ShapeWireframeUpdate
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, shapeWireframeUpdateKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = ShapeWireframeUpdate(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o ShapeWireframeUpdate) MarshalJSON() ([]byte, error) {
	type plain ShapeWireframeUpdate
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// ImageWireframeUpdate: Schema of all properties of a ImageWireframeUpdate.
type ImageWireframeUpdate struct {
	// Defines the unique ID of the wireframe. This is persistent throughout the view lifetime.
	ID int64 `json:"id"`

	// The position in pixels on X axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	X *int64 `json:"x,omitzero"`

	// The position in pixels on Y axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	Y *int64 `json:"y,omitzero"`

	// The width in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the width of all UI elements is divided by 2 to get
	// a normalized width.
	Width *int64 `json:"width,omitzero"`

	// The height in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the height of all UI elements is divided by 2 to get
	// a normalized height.
	Height *int64 `json:"height,omitzero"`

	// Schema of clipping information for a Wireframe.
	Clip *WireframeClip `json:"clip,omitzero"`

	// The style of this wireframe.
	ShapeStyle *ShapeStyle `json:"shapeStyle,omitzero"`

	// The border properties of this wireframe. The default value is null (no-border).
	Border *ShapeBorder `json:"border,omitzero"`

	// The type of the wireframe.
	// Always "image".
	Type string `json:"type"`

	// base64 representation of the image. Not required as the ImageWireframe can be initialised
	// without any base64.
	Base64 *string `json:"base64,omitzero"`

	// Unique identifier of the image resource.
	ResourceID *string `json:"resourceId,omitzero"`

	// MIME type of the image file.
	MimeType *string `json:"mimeType,omitzero"`

	// Flag describing an image wireframe that should render an empty state placeholder.
	IsEmpty *bool `json:"isEmpty,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var imageWireframeUpdateKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "clip": {}, "shapeStyle": {}, "border": {}, "type": {}, "base64": {}, "resourceId": {}, "mimeType": {}, "isEmpty": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *ImageWireframeUpdate) UnmarshalJSON(data []byte) error {
	type plain ImageWireframeUpdate
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, imageWireframeUpdateKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = ImageWireframeUpdate(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o ImageWireframeUpdate) MarshalJSON() ([]byte, error) {
	type plain ImageWireframeUpdate
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// PlaceholderWireframeUpdate: Schema of all properties of a PlaceholderWireframe.
type PlaceholderWireframeUpdate struct {
	// Defines the unique ID of the wireframe. This is persistent throughout the view lifetime.
	ID int64 `json:"id"`

	// The position in pixels on X axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	X *int64 `json:"x,omitzero"`

	// The position in pixels on Y axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	Y *int64 `json:"y,omitzero"`

	// The width in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the width of all UI elements is divided by 2 to get
	// a normalized width.
	Width *int64 `json:"width,omitzero"`

	// The height in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the height of all UI elements is divided by 2 to get
	// a normalized height.
	Height *int64 `json:"height,omitzero"`

	// Schema of clipping information for a Wireframe.
	Clip *WireframeClip `json:"clip,omitzero"`

	// The type of the wireframe.
	// Always "placeholder".
	Type string `json:"type"`

	// Label of the placeholder.
	Label *string `json:"label,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var placeholderWireframeUpdateKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "clip": {}, "type": {}, "label": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *PlaceholderWireframeUpdate) UnmarshalJSON(data []byte) error {
	type plain PlaceholderWireframeUpdate
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, placeholderWireframeUpdateKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = PlaceholderWireframeUpdate(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o PlaceholderWireframeUpdate) MarshalJSON() ([]byte, error) {
	type plain PlaceholderWireframeUpdate
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// WebviewWireframeUpdate: Schema of all properties of a WebviewWireframeUpdate.
type WebviewWireframeUpdate struct {
	// Defines the unique ID of the wireframe. This is persistent throughout the view lifetime.
	ID int64 `json:"id"`

	// The position in pixels on X axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	X *int64 `json:"x,omitzero"`

	// The position in pixels on Y axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	Y *int64 `json:"y,omitzero"`

	// The width in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the width of all UI elements is divided by 2 to get
	// a normalized width.
	Width *int64 `json:"width,omitzero"`

	// The height in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the height of all UI elements is divided by 2 to get
	// a normalized height.
	Height *int64 `json:"height,omitzero"`

	// Schema of clipping information for a Wireframe.
	Clip *WireframeClip `json:"clip,omitzero"`

	// The style of this wireframe.
	ShapeStyle *ShapeStyle `json:"shapeStyle,omitzero"`

	// The border properties of this wireframe. The default value is null (no-border).
	Border *ShapeBorder `json:"border,omitzero"`

	// The type of the wireframe.
	// Always "webview".
	Type string `json:"type"`

	// Unique Id of the slot containing this webview.
	SlotID string `json:"slotId"`

	// Whether this webview is visible or not.
	IsVisible *bool `json:"isVisible,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var webviewWireframeUpdateKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "clip": {}, "shapeStyle": {}, "border": {}, "type": {}, "slotId": {}, "isVisible": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *WebviewWireframeUpdate) UnmarshalJSON(data []byte) error {
	type plain WebviewWireframeUpdate
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, webviewWireframeUpdateKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = WebviewWireframeUpdate(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o WebviewWireframeUpdate) MarshalJSON() ([]byte, error) {
	type plain WebviewWireframeUpdate
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// EmbeddedContentWireframeUpdate: Schema of all properties of an EmbeddedContentWireframeUpdate.
type EmbeddedContentWireframeUpdate struct {
	// Defines the unique ID of the wireframe. This is persistent throughout the view lifetime.
	ID int64 `json:"id"`

	// The position in pixels on X axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	X *int64 `json:"x,omitzero"`

	// The position in pixels on Y axis of the UI element in absolute coordinates. The anchor point is
	// always the top-left corner of the wireframe.
	Y *int64 `json:"y,omitzero"`

	// The width in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the width of all UI elements is divided by 2 to get
	// a normalized width.
	Width *int64 `json:"width,omitzero"`

	// The height in pixels of the UI element, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the height of all UI elements is divided by 2 to get
	// a normalized height.
	Height *int64 `json:"height,omitzero"`

	// Schema of clipping information for a Wireframe.
	Clip *WireframeClip `json:"clip,omitzero"`

	// The style of this wireframe.
	ShapeStyle *ShapeStyle `json:"shapeStyle,omitzero"`

	// The border properties of this wireframe. The default value is null (no-border).
	Border *ShapeBorder `json:"border,omitzero"`

	// The type of the wireframe.
	// Always "embedded_content".
	Type string `json:"type"`

	// Unique Id of the slot containing this embedded content.
	SlotID string `json:"slotId"`

	// Whether this embedded content is visible or not.
	IsVisible *bool `json:"isVisible,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var embeddedContentWireframeUpdateKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "clip": {}, "shapeStyle": {}, "border": {}, "type": {}, "slotId": {}, "isVisible": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *EmbeddedContentWireframeUpdate) UnmarshalJSON(data []byte) error {
	type plain EmbeddedContentWireframeUpdate
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, embeddedContentWireframeUpdateKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = EmbeddedContentWireframeUpdate(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o EmbeddedContentWireframeUpdate) MarshalJSON() ([]byte, error) {
	type plain EmbeddedContentWireframeUpdate
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TouchData: Schema of a TouchData.
type TouchData struct {
	// The source of this type of incremental data.
	// Always 2.
	Source int64 `json:"source"`

	// Contains the positions of the finger on the screen during the touchDown/touchUp event
	// lifecycle.
	Positions []TouchDataPositions `json:"positions,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var touchDataKeys = map[string]struct{}{"source": {}, "positions": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TouchData) UnmarshalJSON(data []byte) error {
	type plain TouchData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, touchDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TouchData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TouchData) MarshalJSON() ([]byte, error) {
	type plain TouchData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TouchDataPositions
type TouchDataPositions struct {
	// The touch id of the touch event this position corresponds to. In mobile it is possible to have
	// multiple touch events (fingers touching the screen) happening at the same time.
	ID int64 `json:"id"`

	// The x coordinate value of the position.
	X int64 `json:"x"`

	// The y coordinate value of the position.
	Y int64 `json:"y"`

	// The UTC timestamp in milliseconds corresponding to the moment the position change was recorded.
	// Each timestamp is computed as the UTC interval since 00:00:00.000 01.01.1970.
	Timestamp int64 `json:"timestamp"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var touchDataPositionsKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "timestamp": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TouchDataPositions) UnmarshalJSON(data []byte) error {
	type plain TouchDataPositions
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, touchDataPositionsKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TouchDataPositions(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TouchDataPositions) MarshalJSON() ([]byte, error) {
	type plain TouchDataPositions
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CompositionTreeMutationData: Mobile-specific. Incremental data carrying composition tree layer
// mutations.
type CompositionTreeMutationData struct {
	// The source of this type of incremental data.
	// Always 10.
	Source int64 `json:"source"`

	// A rendering group that groups child wireframes and child layers. Does not draw pixels itself.
	// Ordered rendering modifiers and compositing are applied to its composed output.
	Root *CompositionLayer `json:"root,omitzero"`

	// Full layer definitions for newly added layers.
	Adds []CompositionLayer `json:"adds,omitzero"`

	// Ids of layer definitions to remove. Removing a referenced layer also requires updating the
	// parent or root child list.
	Removes []int64 `json:"removes,omitzero"`

	// Sparse updates for existing layers.
	Updates []CompositionLayerUpdate `json:"updates,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var compositionTreeMutationDataKeys = map[string]struct{}{"source": {}, "root": {}, "adds": {}, "removes": {}, "updates": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CompositionTreeMutationData) UnmarshalJSON(data []byte) error {
	type plain CompositionTreeMutationData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, compositionTreeMutationDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CompositionTreeMutationData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CompositionTreeMutationData) MarshalJSON() ([]byte, error) {
	type plain CompositionTreeMutationData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CompositionLayerUpdate: Sparse update for a composition layer. Omitted fields are unchanged.
type CompositionLayerUpdate struct {
	// The id of the layer to update.
	ID int64 `json:"id"`

	// Updated X position in absolute coordinates. Uses the same coordinate space as mobile
	// wireframes.
	X *int64 `json:"x,omitzero"`

	// Updated Y position in absolute coordinates. Uses the same coordinate space as mobile
	// wireframes.
	Y *int64 `json:"y,omitzero"`

	// Updated width in pixels. Uses the same coordinate space as mobile wireframes.
	Width *int64 `json:"width,omitzero"`

	// Updated height in pixels. Uses the same coordinate space as mobile wireframes.
	Height *int64 `json:"height,omitzero"`

	// When present, replaces the full child list for this layer.
	Children []CompositionLayerChild `json:"children,omitzero"`

	// When present, replaces the full modifier list for this layer.
	Modifiers []CompositionLayerModifier `json:"modifiers,omitzero"`

	// Updated composite operation for this layer.
	// One of: "sourceOver", "destinationIn", "destinationOut", "plusDarker".
	CompositeOperation *string `json:"compositeOperation,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var compositionLayerUpdateKeys = map[string]struct{}{"id": {}, "x": {}, "y": {}, "width": {}, "height": {}, "children": {}, "modifiers": {}, "compositeOperation": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CompositionLayerUpdate) UnmarshalJSON(data []byte) error {
	type plain CompositionLayerUpdate
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, compositionLayerUpdateKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CompositionLayerUpdate(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CompositionLayerUpdate) MarshalJSON() ([]byte, error) {
	type plain CompositionLayerUpdate
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}
