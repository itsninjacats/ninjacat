// Code generated from DataDog/rum-events-format@ab3e7c5a13f1a6fed3d63bb22f1e8a9ebaa25671. DO NOT EDIT.
//
// Regenerate with: RUM_EVENTS_FORMAT=<checkout> go generate ./rumevents/...

package replay

import (
	"encoding/json"
	"github.com/itsninjacats/server/rumevents/internal/jsonx"
)

// SegmentContextApplication: Application properties.
type SegmentContextApplication struct {
	// UUID of the application.
	ID string `json:"id"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var segmentContextApplicationKeys = map[string]struct{}{"id": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *SegmentContextApplication) UnmarshalJSON(data []byte) error {
	type plain SegmentContextApplication
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, segmentContextApplicationKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = SegmentContextApplication(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o SegmentContextApplication) MarshalJSON() ([]byte, error) {
	type plain SegmentContextApplication
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// SegmentContextSession: Session properties.
type SegmentContextSession struct {
	// UUID of the session.
	ID string `json:"id"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var segmentContextSessionKeys = map[string]struct{}{"id": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *SegmentContextSession) UnmarshalJSON(data []byte) error {
	type plain SegmentContextSession
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, segmentContextSessionKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = SegmentContextSession(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o SegmentContextSession) MarshalJSON() ([]byte, error) {
	type plain SegmentContextSession
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// SegmentContextView: View properties.
type SegmentContextView struct {
	// UUID of the view.
	ID string `json:"id"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var segmentContextViewKeys = map[string]struct{}{"id": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *SegmentContextView) UnmarshalJSON(data []byte) error {
	type plain SegmentContextView
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, segmentContextViewKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = SegmentContextView(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o SegmentContextView) MarshalJSON() ([]byte, error) {
	type plain SegmentContextView
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// ViewportResizeData: Schema of a ViewportResizeData.
type ViewportResizeData struct {
	// The source of this type of incremental data.
	// Always 4.
	Source int64 `json:"source"`

	// The new width of the screen in pixels, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the width is divided by 2 to get a normalized width.
	Width int64 `json:"width"`

	// The new height of the screen in pixels, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the height is divided by 2 to get a normalized
	// height.
	Height int64 `json:"height"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var viewportResizeDataKeys = map[string]struct{}{"source": {}, "width": {}, "height": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *ViewportResizeData) UnmarshalJSON(data []byte) error {
	type plain ViewportResizeData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, viewportResizeDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = ViewportResizeData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o ViewportResizeData) MarshalJSON() ([]byte, error) {
	type plain ViewportResizeData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// PointerInteractionData: Schema of a PointerInteractionData.
type PointerInteractionData struct {
	// The source of this type of incremental data.
	// Always 9.
	Source int64 `json:"source"`

	// Schema of an PointerEventType.
	// One of: "down", "up", "move".
	PointerEventType string `json:"pointerEventType"`

	// Schema of an PointerType.
	// One of: "mouse", "touch", "pen".
	PointerType string `json:"pointerType"`

	// Id of the pointer of this PointerInteraction.
	PointerID int64 `json:"pointerId"`

	// X-axis coordinate for this PointerInteraction.
	X json.Number `json:"x,omitzero"`

	// Y-axis coordinate for this PointerInteraction.
	Y json.Number `json:"y,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var pointerInteractionDataKeys = map[string]struct{}{"source": {}, "pointerEventType": {}, "pointerType": {}, "pointerId": {}, "x": {}, "y": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *PointerInteractionData) UnmarshalJSON(data []byte) error {
	type plain PointerInteractionData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, pointerInteractionDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = PointerInteractionData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o PointerInteractionData) MarshalJSON() ([]byte, error) {
	type plain PointerInteractionData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// MetaRecord: Schema of a Record which contains the screen properties.
type MetaRecord struct {
	// Defines the UTC time in milliseconds when this Record was performed.
	Timestamp int64 `json:"timestamp"`

	// Unique ID of the slot that generated this record.
	SlotID *string `json:"slotId,omitzero"`

	// The type of this Record.
	// Always 4.
	Type int64 `json:"type"`

	// The data contained by this record.
	Data *MetaRecordData `json:"data,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var metaRecordKeys = map[string]struct{}{"timestamp": {}, "slotId": {}, "type": {}, "data": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *MetaRecord) UnmarshalJSON(data []byte) error {
	type plain MetaRecord
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, metaRecordKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = MetaRecord(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o MetaRecord) MarshalJSON() ([]byte, error) {
	type plain MetaRecord
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// MetaRecordData: The data contained by this record.
type MetaRecordData struct {
	// The width of the screen in pixels, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the normalized width is the current width divided by
	// 2.
	Width int64 `json:"width"`

	// The height of the screen in pixels, normalized based on the device pixels per inch density
	// (DPI). Example: if a device has a DPI = 2, the normalized height is the current height divided
	// by 2.
	Height int64 `json:"height"`

	// Browser-specific. URL of the view described by this record.
	Href *string `json:"href,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var metaRecordDataKeys = map[string]struct{}{"width": {}, "height": {}, "href": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *MetaRecordData) UnmarshalJSON(data []byte) error {
	type plain MetaRecordData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, metaRecordDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = MetaRecordData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o MetaRecordData) MarshalJSON() ([]byte, error) {
	type plain MetaRecordData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// FocusRecord: Schema of a Record type which contains focus information.
type FocusRecord struct {
	// Defines the UTC time in milliseconds when this Record was performed.
	Timestamp int64 `json:"timestamp"`

	// Unique ID of the slot that generated this record.
	SlotID *string `json:"slotId,omitzero"`

	// The type of this Record.
	// Always 6.
	Type int64 `json:"type"`

	Data *FocusRecordData `json:"data,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var focusRecordKeys = map[string]struct{}{"timestamp": {}, "slotId": {}, "type": {}, "data": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *FocusRecord) UnmarshalJSON(data []byte) error {
	type plain FocusRecord
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, focusRecordKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = FocusRecord(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o FocusRecord) MarshalJSON() ([]byte, error) {
	type plain FocusRecord
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// FocusRecordData
type FocusRecordData struct {
	// Whether this screen has a focus or not. For now it will always be true for mobile.
	HasFocus bool `json:"has_focus"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var focusRecordDataKeys = map[string]struct{}{"has_focus": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *FocusRecordData) UnmarshalJSON(data []byte) error {
	type plain FocusRecordData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, focusRecordDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = FocusRecordData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o FocusRecordData) MarshalJSON() ([]byte, error) {
	type plain FocusRecordData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// ViewEndRecord: Schema of a Record which signifies that view lifecycle ended.
type ViewEndRecord struct {
	// Defines the UTC time in milliseconds when this Record was performed.
	Timestamp int64 `json:"timestamp"`

	// Unique ID of the slot that generated this record.
	SlotID *string `json:"slotId,omitzero"`

	// The type of this Record.
	// Always 7.
	Type int64 `json:"type"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var viewEndRecordKeys = map[string]struct{}{"timestamp": {}, "slotId": {}, "type": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *ViewEndRecord) UnmarshalJSON(data []byte) error {
	type plain ViewEndRecord
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, viewEndRecordKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = ViewEndRecord(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o ViewEndRecord) MarshalJSON() ([]byte, error) {
	type plain ViewEndRecord
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// VisualViewportRecord: Schema of a Record which signifies that the viewport properties have
// changed.
type VisualViewportRecord struct {
	// Defines the UTC time in milliseconds when this Record was performed.
	Timestamp int64 `json:"timestamp"`

	// Unique ID of the slot that generated this record.
	SlotID *string `json:"slotId,omitzero"`

	Data *VisualViewportRecordData `json:"data,omitzero"`

	// The type of this Record.
	// Always 8.
	Type int64 `json:"type"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var visualViewportRecordKeys = map[string]struct{}{"timestamp": {}, "slotId": {}, "data": {}, "type": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *VisualViewportRecord) UnmarshalJSON(data []byte) error {
	type plain VisualViewportRecord
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, visualViewportRecordKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = VisualViewportRecord(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o VisualViewportRecord) MarshalJSON() ([]byte, error) {
	type plain VisualViewportRecord
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// VisualViewportRecordData
type VisualViewportRecordData struct {
	Height json.Number `json:"height,omitzero"`

	OffsetLeft json.Number `json:"offsetLeft,omitzero"`

	OffsetTop json.Number `json:"offsetTop,omitzero"`

	PageLeft json.Number `json:"pageLeft,omitzero"`

	PageTop json.Number `json:"pageTop,omitzero"`

	Scale json.Number `json:"scale,omitzero"`

	Width json.Number `json:"width,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var visualViewportRecordDataKeys = map[string]struct{}{"height": {}, "offsetLeft": {}, "offsetTop": {}, "pageLeft": {}, "pageTop": {}, "scale": {}, "width": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *VisualViewportRecordData) UnmarshalJSON(data []byte) error {
	type plain VisualViewportRecordData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, visualViewportRecordDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = VisualViewportRecordData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o VisualViewportRecordData) MarshalJSON() ([]byte, error) {
	type plain VisualViewportRecordData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}
