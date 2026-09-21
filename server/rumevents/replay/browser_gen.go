// Code generated from DataDog/rum-events-format@ab3e7c5a13f1a6fed3d63bb22f1e8a9ebaa25671. DO NOT EDIT.
//
// Regenerate with: RUM_EVENTS_FORMAT=<checkout> go generate ./rumevents/...

package replay

import (
	"encoding/json"
	"github.com/itsninjacats/server/rumevents/internal/jsonx"
)

// BrowserSegment: Browser-specific. Schema of a Session Replay data Segment.
//
// Listed by: session-replay-browser-schema.json.
type BrowserSegment struct {
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
	// Always "browser".
	Source string `json:"source"`

	// The reason this Segment was created. For mobile there is only one possible value for this,
	// which is always the default value.
	// One of: "init", "segment_duration_limit", "segment_bytes_limit", "view_change",
	// "before_unload", "visibility_hidden", "page_frozen".
	CreationReason string `json:"creation_reason"`

	// The records contained by this Segment.
	Records []BrowserRecord `json:"records,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var browserSegmentKeys = map[string]struct{}{"application": {}, "session": {}, "view": {}, "start": {}, "end": {}, "records_count": {}, "index_in_view": {}, "has_full_snapshot": {}, "source": {}, "creation_reason": {}, "records": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *BrowserSegment) UnmarshalJSON(data []byte) error {
	type plain BrowserSegment
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, browserSegmentKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = BrowserSegment(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o BrowserSegment) MarshalJSON() ([]byte, error) {
	type plain BrowserSegment
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// BrowserRecord: Browser-specific. Schema of a Session Replay Record.
//
// Discriminated union selected by type, format: exactly one variant field is set. When no
// variant's discriminator matches (a record type newer than the schema), Raw keeps the original
// JSON so nothing is lost.
type BrowserRecord struct {
	BrowserFullSnapshotV1Record      *BrowserFullSnapshotV1Record
	BrowserFullSnapshotChangeRecord  *BrowserFullSnapshotChangeRecord
	BrowserIncrementalSnapshotRecord *BrowserIncrementalSnapshotRecord
	MetaRecord                       *MetaRecord
	FocusRecord                      *FocusRecord
	ViewEndRecord                    *ViewEndRecord
	VisualViewportRecord             *VisualViewportRecord
	FrustrationRecord                *FrustrationRecord
	BrowserChangeRecord              *BrowserChangeRecord

	// Raw holds the JSON of an alternative that matched no variant.
	Raw json.RawMessage
}

var browserRecordVariants = []jsonx.Variant{
	{Name: "BrowserFullSnapshotV1Record", New: func() any { return new(BrowserFullSnapshotV1Record) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("2")}, Required: true}, {Path: []string{"format"}, Values: []any{json.Number("0")}, Required: false}}},
	{Name: "BrowserFullSnapshotChangeRecord", New: func() any { return new(BrowserFullSnapshotChangeRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("2")}, Required: true}, {Path: []string{"format"}, Values: []any{json.Number("1")}, Required: true}}},
	{Name: "BrowserIncrementalSnapshotRecord", New: func() any { return new(BrowserIncrementalSnapshotRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("3")}, Required: true}}},
	{Name: "MetaRecord", New: func() any { return new(MetaRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("4")}, Required: true}}},
	{Name: "FocusRecord", New: func() any { return new(FocusRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("6")}, Required: true}}},
	{Name: "ViewEndRecord", New: func() any { return new(ViewEndRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("7")}, Required: true}}},
	{Name: "VisualViewportRecord", New: func() any { return new(VisualViewportRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("8")}, Required: true}}},
	{Name: "FrustrationRecord", New: func() any { return new(FrustrationRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("9")}, Required: true}}},
	{Name: "BrowserChangeRecord", New: func() any { return new(BrowserChangeRecord) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("12")}, Required: true}}},
}

// UnmarshalJSON selects the variant by its discriminator; an unmatched alternative is kept in Raw.
func (u *BrowserRecord) UnmarshalJSON(data []byte) error {
	*u = BrowserRecord{}
	if jsonx.IsNull(data) {
		return nil
	}
	i, v, err := jsonx.DecodeVariant(data, browserRecordVariants)
	if err != nil {
		return err
	}
	switch i {
	case 0:
		u.BrowserFullSnapshotV1Record = v.(*BrowserFullSnapshotV1Record)
	case 1:
		u.BrowserFullSnapshotChangeRecord = v.(*BrowserFullSnapshotChangeRecord)
	case 2:
		u.BrowserIncrementalSnapshotRecord = v.(*BrowserIncrementalSnapshotRecord)
	case 3:
		u.MetaRecord = v.(*MetaRecord)
	case 4:
		u.FocusRecord = v.(*FocusRecord)
	case 5:
		u.ViewEndRecord = v.(*ViewEndRecord)
	case 6:
		u.VisualViewportRecord = v.(*VisualViewportRecord)
	case 7:
		u.FrustrationRecord = v.(*FrustrationRecord)
	case 8:
		u.BrowserChangeRecord = v.(*BrowserChangeRecord)
	default:
		u.Raw = append(json.RawMessage(nil), data...)
	}
	return nil
}

// MarshalJSON encodes the set variant, or Raw, or null when neither is set.
func (u BrowserRecord) MarshalJSON() ([]byte, error) {
	if v := u.Variant(); v != nil {
		return jsonx.Marshal(v)
	}
	if u.Raw != nil {
		return u.Raw, nil
	}
	return []byte("null"), nil
}

// Variant returns the populated alternative as a pointer to its concrete type, or nil.
func (u *BrowserRecord) Variant() any {
	switch {
	case u.BrowserFullSnapshotV1Record != nil:
		return u.BrowserFullSnapshotV1Record
	case u.BrowserFullSnapshotChangeRecord != nil:
		return u.BrowserFullSnapshotChangeRecord
	case u.BrowserIncrementalSnapshotRecord != nil:
		return u.BrowserIncrementalSnapshotRecord
	case u.MetaRecord != nil:
		return u.MetaRecord
	case u.FocusRecord != nil:
		return u.FocusRecord
	case u.ViewEndRecord != nil:
		return u.ViewEndRecord
	case u.VisualViewportRecord != nil:
		return u.VisualViewportRecord
	case u.FrustrationRecord != nil:
		return u.FrustrationRecord
	case u.BrowserChangeRecord != nil:
		return u.BrowserChangeRecord
	}
	return nil
}

// BrowserFullSnapshotV1Record: Browser-specific. Schema of a Record type which contains a full
// snapshot of a document in V1 format.
type BrowserFullSnapshotV1Record struct {
	// Defines the UTC time in milliseconds when this Record was performed.
	Timestamp int64 `json:"timestamp"`

	// Unique ID of the slot that generated this record.
	SlotID *string `json:"slotId,omitzero"`

	// The type of this Record.
	// Always 2.
	Type int64 `json:"type"`

	// The V1 snapshot format.
	// Always 0.
	Format *int64 `json:"format,omitzero"`

	// Schema of a Node type.
	Data *BrowserNode `json:"data,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var browserFullSnapshotV1RecordKeys = map[string]struct{}{"timestamp": {}, "slotId": {}, "type": {}, "format": {}, "data": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *BrowserFullSnapshotV1Record) UnmarshalJSON(data []byte) error {
	type plain BrowserFullSnapshotV1Record
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, browserFullSnapshotV1RecordKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = BrowserFullSnapshotV1Record(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o BrowserFullSnapshotV1Record) MarshalJSON() ([]byte, error) {
	type plain BrowserFullSnapshotV1Record
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// BrowserNode: Schema of a Node type.
type BrowserNode struct {
	// Serialized node contained by this Record.
	Node *SerializedNodeWithId `json:"node,omitzero"`

	// Initial node offset position.
	InitialOffset *BrowserNodeInitialOffset `json:"initialOffset,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var browserNodeKeys = map[string]struct{}{"node": {}, "initialOffset": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *BrowserNode) UnmarshalJSON(data []byte) error {
	type plain BrowserNode
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, browserNodeKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = BrowserNode(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o BrowserNode) MarshalJSON() ([]byte, error) {
	type plain BrowserNode
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// SerializedNodeWithId: Serialized node contained by this Record.
//
// Discriminated union selected by type: exactly one variant field is set. When no variant's
// discriminator matches (a record type newer than the schema), Raw keeps the original JSON so
// nothing is lost.
type SerializedNodeWithId struct {
	DocumentNodeWithId         *DocumentNodeWithId
	DocumentFragmentNodeWithId *DocumentFragmentNodeWithId
	DocumentTypeNodeWithId     *DocumentTypeNodeWithId
	ElementNodeWithId          *ElementNodeWithId
	TextNodeWithId             *TextNodeWithId
	CDataNodeWithId            *CDataNodeWithId

	// Raw holds the JSON of an alternative that matched no variant.
	Raw json.RawMessage
}

var serializedNodeWithIdVariants = []jsonx.Variant{
	{Name: "DocumentNodeWithId", New: func() any { return new(DocumentNodeWithId) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("0")}, Required: true}}},
	{Name: "DocumentFragmentNodeWithId", New: func() any { return new(DocumentFragmentNodeWithId) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("11")}, Required: true}}},
	{Name: "DocumentTypeNodeWithId", New: func() any { return new(DocumentTypeNodeWithId) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("1")}, Required: true}}},
	{Name: "ElementNodeWithId", New: func() any { return new(ElementNodeWithId) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("2")}, Required: true}}},
	{Name: "TextNodeWithId", New: func() any { return new(TextNodeWithId) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("3")}, Required: true}}},
	{Name: "CDataNodeWithId", New: func() any { return new(CDataNodeWithId) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{json.Number("4")}, Required: true}}},
}

// UnmarshalJSON selects the variant by its discriminator; an unmatched alternative is kept in Raw.
func (u *SerializedNodeWithId) UnmarshalJSON(data []byte) error {
	*u = SerializedNodeWithId{}
	if jsonx.IsNull(data) {
		return nil
	}
	i, v, err := jsonx.DecodeVariant(data, serializedNodeWithIdVariants)
	if err != nil {
		return err
	}
	switch i {
	case 0:
		u.DocumentNodeWithId = v.(*DocumentNodeWithId)
	case 1:
		u.DocumentFragmentNodeWithId = v.(*DocumentFragmentNodeWithId)
	case 2:
		u.DocumentTypeNodeWithId = v.(*DocumentTypeNodeWithId)
	case 3:
		u.ElementNodeWithId = v.(*ElementNodeWithId)
	case 4:
		u.TextNodeWithId = v.(*TextNodeWithId)
	case 5:
		u.CDataNodeWithId = v.(*CDataNodeWithId)
	default:
		u.Raw = append(json.RawMessage(nil), data...)
	}
	return nil
}

// MarshalJSON encodes the set variant, or Raw, or null when neither is set.
func (u SerializedNodeWithId) MarshalJSON() ([]byte, error) {
	if v := u.Variant(); v != nil {
		return jsonx.Marshal(v)
	}
	if u.Raw != nil {
		return u.Raw, nil
	}
	return []byte("null"), nil
}

// Variant returns the populated alternative as a pointer to its concrete type, or nil.
func (u *SerializedNodeWithId) Variant() any {
	switch {
	case u.DocumentNodeWithId != nil:
		return u.DocumentNodeWithId
	case u.DocumentFragmentNodeWithId != nil:
		return u.DocumentFragmentNodeWithId
	case u.DocumentTypeNodeWithId != nil:
		return u.DocumentTypeNodeWithId
	case u.ElementNodeWithId != nil:
		return u.ElementNodeWithId
	case u.TextNodeWithId != nil:
		return u.TextNodeWithId
	case u.CDataNodeWithId != nil:
		return u.CDataNodeWithId
	}
	return nil
}

// DocumentNodeWithId: Serialized node contained by this Record.
type DocumentNodeWithId struct {
	ID int64 `json:"id"`

	// The type of this Node.
	// Always 0.
	Type int64 `json:"type"`

	// Stylesheet added dynamically.
	AdoptedStyleSheets []StyleSheet `json:"adoptedStyleSheets,omitzero"`

	ChildNodes []SerializedNodeWithId `json:"childNodes,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var documentNodeWithIdKeys = map[string]struct{}{"id": {}, "type": {}, "adoptedStyleSheets": {}, "childNodes": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *DocumentNodeWithId) UnmarshalJSON(data []byte) error {
	type plain DocumentNodeWithId
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, documentNodeWithIdKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = DocumentNodeWithId(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o DocumentNodeWithId) MarshalJSON() ([]byte, error) {
	type plain DocumentNodeWithId
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// StyleSheet: Browser-specific. Schema of a StyleSheet.
type StyleSheet struct {
	// CSS rules applied (rule.cssText).
	CSSRules []string `json:"cssRules,omitzero"`

	// MediaList of the stylesheet.
	Media []string `json:"media,omitzero"`

	// Is the stylesheet disabled.
	Disabled *bool `json:"disabled,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var styleSheetKeys = map[string]struct{}{"cssRules": {}, "media": {}, "disabled": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *StyleSheet) UnmarshalJSON(data []byte) error {
	type plain StyleSheet
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, styleSheetKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = StyleSheet(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o StyleSheet) MarshalJSON() ([]byte, error) {
	type plain StyleSheet
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// DocumentFragmentNodeWithId: Serialized node contained by this Record.
type DocumentFragmentNodeWithId struct {
	ID int64 `json:"id"`

	// The type of this Node.
	// Always 11.
	Type int64 `json:"type"`

	// Stylesheet added dynamically.
	AdoptedStyleSheets []StyleSheet `json:"adoptedStyleSheets,omitzero"`

	// Is this node a shadow root or not.
	IsShadowRoot bool `json:"isShadowRoot"`

	ChildNodes []SerializedNodeWithId `json:"childNodes,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var documentFragmentNodeWithIdKeys = map[string]struct{}{"id": {}, "type": {}, "adoptedStyleSheets": {}, "isShadowRoot": {}, "childNodes": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *DocumentFragmentNodeWithId) UnmarshalJSON(data []byte) error {
	type plain DocumentFragmentNodeWithId
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, documentFragmentNodeWithIdKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = DocumentFragmentNodeWithId(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o DocumentFragmentNodeWithId) MarshalJSON() ([]byte, error) {
	type plain DocumentFragmentNodeWithId
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// DocumentTypeNodeWithId: Serialized node contained by this Record.
type DocumentTypeNodeWithId struct {
	ID int64 `json:"id"`

	// The type of this Node.
	// Always 1.
	Type int64 `json:"type"`

	// Name for this DocumentType.
	Name string `json:"name"`

	// PublicId for this DocumentType.
	PublicID string `json:"publicId"`

	// SystemId for this DocumentType.
	SystemID string `json:"systemId"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var documentTypeNodeWithIdKeys = map[string]struct{}{"id": {}, "type": {}, "name": {}, "publicId": {}, "systemId": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *DocumentTypeNodeWithId) UnmarshalJSON(data []byte) error {
	type plain DocumentTypeNodeWithId
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, documentTypeNodeWithIdKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = DocumentTypeNodeWithId(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o DocumentTypeNodeWithId) MarshalJSON() ([]byte, error) {
	type plain DocumentTypeNodeWithId
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// ElementNodeWithId: Serialized node contained by this Record.
type ElementNodeWithId struct {
	ID int64 `json:"id"`

	// The type of this Node.
	// Always 2.
	Type int64 `json:"type"`

	// TagName for this Node.
	TagName string `json:"tagName"`

	// Schema of an Attributes type.
	// Values: (string | number | boolean).
	Attributes map[string]any `json:"attributes,omitzero"`

	ChildNodes []SerializedNodeWithId `json:"childNodes,omitzero"`

	// Is this node a SVG instead of a HTML.
	// Always true.
	IsSVG *bool `json:"isSVG,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var elementNodeWithIdKeys = map[string]struct{}{"id": {}, "type": {}, "tagName": {}, "attributes": {}, "childNodes": {}, "isSVG": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *ElementNodeWithId) UnmarshalJSON(data []byte) error {
	type plain ElementNodeWithId
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, elementNodeWithIdKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = ElementNodeWithId(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o ElementNodeWithId) MarshalJSON() ([]byte, error) {
	type plain ElementNodeWithId
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TextNodeWithId: Serialized node contained by this Record.
type TextNodeWithId struct {
	ID int64 `json:"id"`

	// The type of this Node.
	// Always 3.
	Type int64 `json:"type"`

	// Text value for this Text Node.
	TextContent string `json:"textContent"`

	// Always true.
	IsStyle *bool `json:"isStyle,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var textNodeWithIdKeys = map[string]struct{}{"id": {}, "type": {}, "textContent": {}, "isStyle": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TextNodeWithId) UnmarshalJSON(data []byte) error {
	type plain TextNodeWithId
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, textNodeWithIdKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TextNodeWithId(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TextNodeWithId) MarshalJSON() ([]byte, error) {
	type plain TextNodeWithId
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// CDataNodeWithId: Serialized node contained by this Record.
type CDataNodeWithId struct {
	ID int64 `json:"id"`

	// The type of this Node.
	// Always 4.
	Type int64 `json:"type"`

	// Always "".
	TextContent string `json:"textContent"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var cDataNodeWithIdKeys = map[string]struct{}{"id": {}, "type": {}, "textContent": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *CDataNodeWithId) UnmarshalJSON(data []byte) error {
	type plain CDataNodeWithId
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, cDataNodeWithIdKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = CDataNodeWithId(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o CDataNodeWithId) MarshalJSON() ([]byte, error) {
	type plain CDataNodeWithId
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// BrowserNodeInitialOffset: Initial node offset position.
type BrowserNodeInitialOffset struct {
	// Top position offset for this node.
	Top json.Number `json:"top,omitzero"`

	// Left position offset for this node.
	Left json.Number `json:"left,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var browserNodeInitialOffsetKeys = map[string]struct{}{"top": {}, "left": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *BrowserNodeInitialOffset) UnmarshalJSON(data []byte) error {
	type plain BrowserNodeInitialOffset
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, browserNodeInitialOffsetKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = BrowserNodeInitialOffset(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o BrowserNodeInitialOffset) MarshalJSON() ([]byte, error) {
	type plain BrowserNodeInitialOffset
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// BrowserFullSnapshotChangeRecord: Browser-specific. Schema of a Record type which contains a full
// snapshot of a document in Change format.
type BrowserFullSnapshotChangeRecord struct {
	// Defines the UTC time in milliseconds when this Record was performed.
	Timestamp int64 `json:"timestamp"`

	// Unique ID of the slot that generated this record.
	SlotID *string `json:"slotId,omitzero"`

	// The type of this Record.
	// Always 2.
	Type int64 `json:"type"`

	// The Change snapshot format.
	// Always 1.
	Format int64 `json:"format"`

	// Items: Change. Decoded as any (numbers as json.Number).
	Data []any `json:"data,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var browserFullSnapshotChangeRecordKeys = map[string]struct{}{"timestamp": {}, "slotId": {}, "type": {}, "format": {}, "data": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *BrowserFullSnapshotChangeRecord) UnmarshalJSON(data []byte) error {
	type plain BrowserFullSnapshotChangeRecord
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, browserFullSnapshotChangeRecordKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = BrowserFullSnapshotChangeRecord(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o BrowserFullSnapshotChangeRecord) MarshalJSON() ([]byte, error) {
	type plain BrowserFullSnapshotChangeRecord
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// BrowserIncrementalSnapshotRecord: Browser-specific. Schema of a Record type which contains
// mutations of a screen.
type BrowserIncrementalSnapshotRecord struct {
	// Defines the UTC time in milliseconds when this Record was performed.
	Timestamp int64 `json:"timestamp"`

	// Unique ID of the slot that generated this record.
	SlotID *string `json:"slotId,omitzero"`

	// The type of this Record.
	// Always 3.
	Type int64 `json:"type"`

	// Browser-specific. Schema of a Session Replay IncrementalData type.
	Data *BrowserIncrementalData `json:"data,omitzero"`

	ID *int64 `json:"id,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var browserIncrementalSnapshotRecordKeys = map[string]struct{}{"timestamp": {}, "slotId": {}, "type": {}, "data": {}, "id": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *BrowserIncrementalSnapshotRecord) UnmarshalJSON(data []byte) error {
	type plain BrowserIncrementalSnapshotRecord
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, browserIncrementalSnapshotRecordKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = BrowserIncrementalSnapshotRecord(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o BrowserIncrementalSnapshotRecord) MarshalJSON() ([]byte, error) {
	type plain BrowserIncrementalSnapshotRecord
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// BrowserIncrementalData: Browser-specific. Schema of a Session Replay IncrementalData type.
//
// Discriminated union selected by source: exactly one variant field is set. When no variant's
// discriminator matches (a record type newer than the schema), Raw keeps the original JSON so
// nothing is lost.
type BrowserIncrementalData struct {
	BrowserMutationData    *BrowserMutationData
	MousemoveData          *MousemoveData
	MouseInteractionData   *MouseInteractionData
	ScrollData             *ScrollData
	InputData              *InputData
	MediaInteractionData   *MediaInteractionData
	StyleSheetRuleData     *StyleSheetRuleData
	ViewportResizeData     *ViewportResizeData
	PointerInteractionData *PointerInteractionData

	// Raw holds the JSON of an alternative that matched no variant.
	Raw json.RawMessage
}

var browserIncrementalDataVariants = []jsonx.Variant{
	{Name: "BrowserMutationData", New: func() any { return new(BrowserMutationData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("0")}, Required: true}}},
	{Name: "MousemoveData", New: func() any { return new(MousemoveData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("1"), json.Number("6")}, Required: true}}},
	{Name: "MouseInteractionData", New: func() any { return new(MouseInteractionData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("2")}, Required: true}}},
	{Name: "ScrollData", New: func() any { return new(ScrollData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("3")}, Required: true}}},
	{Name: "InputData", New: func() any { return new(InputData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("5")}, Required: true}}},
	{Name: "MediaInteractionData", New: func() any { return new(MediaInteractionData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("7")}, Required: true}}},
	{Name: "StyleSheetRuleData", New: func() any { return new(StyleSheetRuleData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("8")}, Required: true}}},
	{Name: "ViewportResizeData", New: func() any { return new(ViewportResizeData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("4")}, Required: true}}},
	{Name: "PointerInteractionData", New: func() any { return new(PointerInteractionData) }, Match: []jsonx.Discriminator{{Path: []string{"source"}, Values: []any{json.Number("9")}, Required: true}}},
}

// UnmarshalJSON selects the variant by its discriminator; an unmatched alternative is kept in Raw.
func (u *BrowserIncrementalData) UnmarshalJSON(data []byte) error {
	*u = BrowserIncrementalData{}
	if jsonx.IsNull(data) {
		return nil
	}
	i, v, err := jsonx.DecodeVariant(data, browserIncrementalDataVariants)
	if err != nil {
		return err
	}
	switch i {
	case 0:
		u.BrowserMutationData = v.(*BrowserMutationData)
	case 1:
		u.MousemoveData = v.(*MousemoveData)
	case 2:
		u.MouseInteractionData = v.(*MouseInteractionData)
	case 3:
		u.ScrollData = v.(*ScrollData)
	case 4:
		u.InputData = v.(*InputData)
	case 5:
		u.MediaInteractionData = v.(*MediaInteractionData)
	case 6:
		u.StyleSheetRuleData = v.(*StyleSheetRuleData)
	case 7:
		u.ViewportResizeData = v.(*ViewportResizeData)
	case 8:
		u.PointerInteractionData = v.(*PointerInteractionData)
	default:
		u.Raw = append(json.RawMessage(nil), data...)
	}
	return nil
}

// MarshalJSON encodes the set variant, or Raw, or null when neither is set.
func (u BrowserIncrementalData) MarshalJSON() ([]byte, error) {
	if v := u.Variant(); v != nil {
		return jsonx.Marshal(v)
	}
	if u.Raw != nil {
		return u.Raw, nil
	}
	return []byte("null"), nil
}

// Variant returns the populated alternative as a pointer to its concrete type, or nil.
func (u *BrowserIncrementalData) Variant() any {
	switch {
	case u.BrowserMutationData != nil:
		return u.BrowserMutationData
	case u.MousemoveData != nil:
		return u.MousemoveData
	case u.MouseInteractionData != nil:
		return u.MouseInteractionData
	case u.ScrollData != nil:
		return u.ScrollData
	case u.InputData != nil:
		return u.InputData
	case u.MediaInteractionData != nil:
		return u.MediaInteractionData
	case u.StyleSheetRuleData != nil:
		return u.StyleSheetRuleData
	case u.ViewportResizeData != nil:
		return u.ViewportResizeData
	case u.PointerInteractionData != nil:
		return u.PointerInteractionData
	}
	return nil
}

// BrowserMutationData: Browser-specific. Schema of a MutationData.
type BrowserMutationData struct {
	// The source of this type of incremental data.
	// Always 0.
	Source int64 `json:"source"`

	// Contains the newly added nodes.
	Adds []AddedNodeMutation `json:"adds,omitzero"`

	// Contains the removed nodes.
	Removes []RemovedNodeMutation `json:"removes,omitzero"`

	// Contains the updated attribute mutations.
	Attributes []AttributeMutation `json:"attributes,omitzero"`

	// Contains the updated text mutations.
	Texts []TextMutation `json:"texts,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var browserMutationDataKeys = map[string]struct{}{"source": {}, "adds": {}, "removes": {}, "attributes": {}, "texts": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *BrowserMutationData) UnmarshalJSON(data []byte) error {
	type plain BrowserMutationData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, browserMutationDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = BrowserMutationData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o BrowserMutationData) MarshalJSON() ([]byte, error) {
	type plain BrowserMutationData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// AddedNodeMutation: Schema of an AddedNodeMutation.
type AddedNodeMutation struct {
	// Serialized node contained by this Record.
	Node *SerializedNodeWithId `json:"node,omitzero"`

	// Id for the parent node for this AddedNodeMutation.
	ParentID int64 `json:"parentId"`

	// Wire form: integer | (null). Decoded as any (numbers as json.Number).
	NextID any `json:"nextId"`

	// Wire form: integer | (null). Decoded as any (numbers as json.Number).
	PreviousID any `json:"previousId,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var addedNodeMutationKeys = map[string]struct{}{"node": {}, "parentId": {}, "nextId": {}, "previousId": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *AddedNodeMutation) UnmarshalJSON(data []byte) error {
	type plain AddedNodeMutation
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, addedNodeMutationKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = AddedNodeMutation(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o AddedNodeMutation) MarshalJSON() ([]byte, error) {
	type plain AddedNodeMutation
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// RemovedNodeMutation: Schema of a RemovedNodeMutation.
type RemovedNodeMutation struct {
	// Id of the mutated node.
	ID int64 `json:"id"`

	// Id for the parent node for this RemovedNodeMutation.
	ParentID int64 `json:"parentId"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var removedNodeMutationKeys = map[string]struct{}{"id": {}, "parentId": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *RemovedNodeMutation) UnmarshalJSON(data []byte) error {
	type plain RemovedNodeMutation
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, removedNodeMutationKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = RemovedNodeMutation(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o RemovedNodeMutation) MarshalJSON() ([]byte, error) {
	type plain RemovedNodeMutation
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// AttributeMutation: Schema of an AttributeMutation.
type AttributeMutation struct {
	// Id of the mutated node.
	ID int64 `json:"id"`

	// Attributes for this AttributeMutation.
	// Values: (string | null).
	Attributes map[string]any `json:"attributes,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var attributeMutationKeys = map[string]struct{}{"id": {}, "attributes": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *AttributeMutation) UnmarshalJSON(data []byte) error {
	type plain AttributeMutation
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, attributeMutationKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = AttributeMutation(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o AttributeMutation) MarshalJSON() ([]byte, error) {
	type plain AttributeMutation
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TextMutation: Schema of a TextMutation.
type TextMutation struct {
	// Id of the mutated node.
	ID int64 `json:"id"`

	// Value for this TextMutation.
	// Wire form: (null) | string. Decoded as any (numbers as json.Number).
	Value any `json:"value"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var textMutationKeys = map[string]struct{}{"id": {}, "value": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TextMutation) UnmarshalJSON(data []byte) error {
	type plain TextMutation
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, textMutationKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TextMutation(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TextMutation) MarshalJSON() ([]byte, error) {
	type plain TextMutation
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// MousemoveData: Browser-specific. Schema of a MousemoveData.
type MousemoveData struct {
	// The source of this type of incremental data.
	// One of: 1, 6.
	Source int64 `json:"source"`

	// Positions reported for this MousemoveData.
	Positions []MousePosition `json:"positions,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var mousemoveDataKeys = map[string]struct{}{"source": {}, "positions": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *MousemoveData) UnmarshalJSON(data []byte) error {
	type plain MousemoveData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, mousemoveDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = MousemoveData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o MousemoveData) MarshalJSON() ([]byte, error) {
	type plain MousemoveData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// MousePosition: Browser-specific. Schema of a MousePosition.
type MousePosition struct {
	// X-axis coordinate for this MousePosition.
	X json.Number `json:"x,omitzero"`

	// Y-axis coordinate for this MousePosition.
	Y json.Number `json:"y,omitzero"`

	// Id for the target node for this MousePosition.
	ID int64 `json:"id"`

	// Observed time offset for this MousePosition.
	TimeOffset int64 `json:"timeOffset"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var mousePositionKeys = map[string]struct{}{"x": {}, "y": {}, "id": {}, "timeOffset": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *MousePosition) UnmarshalJSON(data []byte) error {
	type plain MousePosition
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, mousePositionKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = MousePosition(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o MousePosition) MarshalJSON() ([]byte, error) {
	type plain MousePosition
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// MouseInteractionData: Browser-specific. Schema of a MouseInteractionData.
//
// Flattened oneOf of: , . All alternative-specific fields are optional.
type MouseInteractionData struct {
	// The source of this type of incremental data.
	// Always 2.
	Source int64 `json:"source"`

	// One of: 0, 1, 2, 3, 4, 7, 9, 5, 6.
	Type int64 `json:"type"`

	// Id for the target node for this MouseInteraction.
	ID int64 `json:"id"`

	// X-axis coordinate for this MouseInteraction.
	X json.Number `json:"x,omitzero"`

	// Y-axis coordinate for this MouseInteraction.
	Y json.Number `json:"y,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var mouseInteractionDataKeys = map[string]struct{}{"source": {}, "type": {}, "id": {}, "x": {}, "y": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *MouseInteractionData) UnmarshalJSON(data []byte) error {
	type plain MouseInteractionData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, mouseInteractionDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = MouseInteractionData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o MouseInteractionData) MarshalJSON() ([]byte, error) {
	type plain MouseInteractionData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// ScrollData: Browser-specific. Schema of a ScrollData.
type ScrollData struct {
	// The source of this type of incremental data.
	// Always 3.
	Source int64 `json:"source"`

	// Id for the target node for this ScrollPosition.
	ID int64 `json:"id"`

	// X-axis coordinate for this ScrollPosition.
	X json.Number `json:"x,omitzero"`

	// Y-axis coordinate for this ScrollPosition.
	Y json.Number `json:"y,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var scrollDataKeys = map[string]struct{}{"source": {}, "id": {}, "x": {}, "y": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *ScrollData) UnmarshalJSON(data []byte) error {
	type plain ScrollData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, scrollDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = ScrollData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o ScrollData) MarshalJSON() ([]byte, error) {
	type plain ScrollData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// InputData: Browser-specific. Schema of an InputData.
//
// Flattened oneOf of: , . All alternative-specific fields are optional.
type InputData struct {
	// The source of this type of incremental data.
	// Always 5.
	Source int64 `json:"source"`

	// Id for the target node for this InputData.
	ID int64 `json:"id"`

	// Text value for this InputState.
	Text *string `json:"text,omitzero"`

	// Checked state for this InputState.
	IsChecked *bool `json:"isChecked,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var inputDataKeys = map[string]struct{}{"source": {}, "id": {}, "text": {}, "isChecked": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *InputData) UnmarshalJSON(data []byte) error {
	type plain InputData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, inputDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = InputData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o InputData) MarshalJSON() ([]byte, error) {
	type plain InputData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// MediaInteractionData: Browser-specific. Schema of a MediaInteractionData.
type MediaInteractionData struct {
	// The source of this type of incremental data.
	// Always 7.
	Source int64 `json:"source"`

	// Id for the target node for this MediaInteraction.
	ID int64 `json:"id"`

	// The type of MediaInteraction.
	// One of: 0, 1.
	Type int64 `json:"type"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var mediaInteractionDataKeys = map[string]struct{}{"source": {}, "id": {}, "type": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *MediaInteractionData) UnmarshalJSON(data []byte) error {
	type plain MediaInteractionData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, mediaInteractionDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = MediaInteractionData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o MediaInteractionData) MarshalJSON() ([]byte, error) {
	type plain MediaInteractionData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// StyleSheetRuleData: Browser-specific. Schema of a StyleSheetRuleData.
type StyleSheetRuleData struct {
	// The source of this type of incremental data.
	// Always 8.
	Source int64 `json:"source"`

	// Id of the owner node for this StyleSheetRule.
	ID int64 `json:"id"`

	// Rules added to this StyleSheetRule.
	Adds []StyleSheetAddRule `json:"adds,omitzero"`

	// Rules deleted from this StyleSheetRule.
	Removes []StyleSheetDeleteRule `json:"removes,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var styleSheetRuleDataKeys = map[string]struct{}{"source": {}, "id": {}, "adds": {}, "removes": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *StyleSheetRuleData) UnmarshalJSON(data []byte) error {
	type plain StyleSheetRuleData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, styleSheetRuleDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = StyleSheetRuleData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o StyleSheetRuleData) MarshalJSON() ([]byte, error) {
	type plain StyleSheetRuleData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// StyleSheetAddRule: Browser-specific. Schema of a StyleSheetAddRule.
type StyleSheetAddRule struct {
	// Text content for this StyleSheetAddRule.
	Rule string `json:"rule"`

	// Index of this StyleSheetAddRule in its StyleSheet.
	// Wire form: integer | array of integer. Decoded as any (numbers as json.Number).
	Index any `json:"index,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var styleSheetAddRuleKeys = map[string]struct{}{"rule": {}, "index": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *StyleSheetAddRule) UnmarshalJSON(data []byte) error {
	type plain StyleSheetAddRule
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, styleSheetAddRuleKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = StyleSheetAddRule(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o StyleSheetAddRule) MarshalJSON() ([]byte, error) {
	type plain StyleSheetAddRule
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// StyleSheetDeleteRule: Browser-specific. Schema of a StyleSheetDeleteRule.
type StyleSheetDeleteRule struct {
	// Index of this StyleSheetDeleteRule in its StyleSheet.
	// Wire form: integer | array of integer. Decoded as any (numbers as json.Number).
	Index any `json:"index"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var styleSheetDeleteRuleKeys = map[string]struct{}{"index": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *StyleSheetDeleteRule) UnmarshalJSON(data []byte) error {
	type plain StyleSheetDeleteRule
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, styleSheetDeleteRuleKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = StyleSheetDeleteRule(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o StyleSheetDeleteRule) MarshalJSON() ([]byte, error) {
	type plain StyleSheetDeleteRule
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// FrustrationRecord: Schema of a Record which signifies a collection of frustration signals.
type FrustrationRecord struct {
	// Defines the UTC time in milliseconds when this Record was performed.
	Timestamp int64 `json:"timestamp"`

	// Unique ID of the slot that generated this record.
	SlotID *string `json:"slotId,omitzero"`

	// The type of this Record.
	// Always 9.
	Type int64 `json:"type"`

	// Schema of a Session Replay FrustrationRecord data structure type.
	Data *FrustrationRecordData `json:"data,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var frustrationRecordKeys = map[string]struct{}{"timestamp": {}, "slotId": {}, "type": {}, "data": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *FrustrationRecord) UnmarshalJSON(data []byte) error {
	type plain FrustrationRecord
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, frustrationRecordKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = FrustrationRecord(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o FrustrationRecord) MarshalJSON() ([]byte, error) {
	type plain FrustrationRecord
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// FrustrationRecordData: Schema of a Session Replay FrustrationRecord data structure type.
type FrustrationRecordData struct {
	// Collection of frustration signal types.
	FrustrationTypes []string `json:"frustrationTypes,omitzero"`

	// Collection of frustration signal event IDs.
	RecordIds []int64 `json:"recordIds,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var frustrationRecordDataKeys = map[string]struct{}{"frustrationTypes": {}, "recordIds": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *FrustrationRecordData) UnmarshalJSON(data []byte) error {
	type plain FrustrationRecordData
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, frustrationRecordDataKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = FrustrationRecordData(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o FrustrationRecordData) MarshalJSON() ([]byte, error) {
	type plain FrustrationRecordData
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// BrowserChangeRecord: Browser-specific. Schema of a record type which represents changes using a
// compact encoding. (Experimental; subject to change.).
type BrowserChangeRecord struct {
	// Defines the UTC time in milliseconds when this Record was performed.
	Timestamp int64 `json:"timestamp"`

	// Unique ID of the slot that generated this record.
	SlotID *string `json:"slotId,omitzero"`

	// The type of this Record.
	// Always 12.
	Type int64 `json:"type"`

	// Items: Change. Decoded as any (numbers as json.Number).
	Data []any `json:"data,omitzero"`

	ID *int64 `json:"id,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var browserChangeRecordKeys = map[string]struct{}{"timestamp": {}, "slotId": {}, "type": {}, "data": {}, "id": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *BrowserChangeRecord) UnmarshalJSON(data []byte) error {
	type plain BrowserChangeRecord
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, browserChangeRecordKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = BrowserChangeRecord(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o BrowserChangeRecord) MarshalJSON() ([]byte, error) {
	type plain BrowserChangeRecord
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}
