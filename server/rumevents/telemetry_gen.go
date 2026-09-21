// Code generated from DataDog/rum-events-format@ab3e7c5a13f1a6fed3d63bb22f1e8a9ebaa25671. DO NOT EDIT.
//
// Regenerate with: RUM_EVENTS_FORMAT=<checkout> go generate ./rumevents/...

package rumevents

import (
	"encoding/json"
	"github.com/itsninjacats/server/rumevents/internal/jsonx"
)

// TelemetryErrorEvent: Schema of all properties of a telemetry error event.
//
// Listed by: rum-events-mobile-schema.json.
type TelemetryErrorEvent struct {
	// Internal properties.
	DD *TelemetryCommonDD `json:"_dd,omitzero"`

	// Telemetry event type. Should specify telemetry only.
	// Always "telemetry".
	Type string `json:"type"`

	// Start of the event in ms from epoch.
	Date int64 `json:"date"`

	// The SDK generating the telemetry event.
	Service string `json:"service"`

	// The source of this event.
	// One of: "android", "ios", "browser", "flutter", "react-native", "unity",
	// "kotlin-multiplatform", "electron", "cpp", "maui".
	Source string `json:"source"`

	// The version of the SDK generating the telemetry event.
	Version string `json:"version"`

	// Application properties.
	Application *TelemetryCommonApplication `json:"application,omitzero"`

	// Session properties.
	Session *TelemetryCommonSession `json:"session,omitzero"`

	// View properties.
	View *TelemetryCommonView `json:"view,omitzero"`

	// Action properties.
	Action *TelemetryCommonAction `json:"action,omitzero"`

	// The actual percentage of telemetry usage per event.
	EffectiveSampleRate json.Number `json:"effective_sample_rate,omitzero"`

	// Enabled experimental features.
	ExperimentalFeatures []string `json:"experimental_features,omitzero"`

	// The telemetry log information.
	Telemetry *TelemetryErrorEventTelemetry `json:"telemetry,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryErrorEventKeys = map[string]struct{}{"_dd": {}, "type": {}, "date": {}, "service": {}, "source": {}, "version": {}, "application": {}, "session": {}, "view": {}, "action": {}, "effective_sample_rate": {}, "experimental_features": {}, "telemetry": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryErrorEvent) UnmarshalJSON(data []byte) error {
	type plain TelemetryErrorEvent
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryErrorEventKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryErrorEvent(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryErrorEvent) MarshalJSON() ([]byte, error) {
	type plain TelemetryErrorEvent
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryCommonDD: Internal properties.
type TelemetryCommonDD struct {
	// Version of the RUM event format.
	// Always 2.
	FormatVersion int64 `json:"format_version"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryCommonDDKeys = map[string]struct{}{"format_version": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryCommonDD) UnmarshalJSON(data []byte) error {
	type plain TelemetryCommonDD
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryCommonDDKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryCommonDD(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryCommonDD) MarshalJSON() ([]byte, error) {
	type plain TelemetryCommonDD
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryCommonApplication: Application properties.
type TelemetryCommonApplication struct {
	// UUID of the application.
	ID string `json:"id"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryCommonApplicationKeys = map[string]struct{}{"id": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryCommonApplication) UnmarshalJSON(data []byte) error {
	type plain TelemetryCommonApplication
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryCommonApplicationKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryCommonApplication(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryCommonApplication) MarshalJSON() ([]byte, error) {
	type plain TelemetryCommonApplication
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryCommonSession: Session properties.
type TelemetryCommonSession struct {
	// UUID of the session.
	ID string `json:"id"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryCommonSessionKeys = map[string]struct{}{"id": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryCommonSession) UnmarshalJSON(data []byte) error {
	type plain TelemetryCommonSession
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryCommonSessionKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryCommonSession(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryCommonSession) MarshalJSON() ([]byte, error) {
	type plain TelemetryCommonSession
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryCommonView: View properties.
type TelemetryCommonView struct {
	// UUID of the view.
	ID string `json:"id"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryCommonViewKeys = map[string]struct{}{"id": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryCommonView) UnmarshalJSON(data []byte) error {
	type plain TelemetryCommonView
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryCommonViewKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryCommonView(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryCommonView) MarshalJSON() ([]byte, error) {
	type plain TelemetryCommonView
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryCommonAction: Action properties.
type TelemetryCommonAction struct {
	// UUID of the action.
	// Wire form: string | array of string. Decoded as any (numbers as json.Number).
	ID any `json:"id"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryCommonActionKeys = map[string]struct{}{"id": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryCommonAction) UnmarshalJSON(data []byte) error {
	type plain TelemetryCommonAction
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryCommonActionKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryCommonAction(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryCommonAction) MarshalJSON() ([]byte, error) {
	type plain TelemetryCommonAction
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryErrorEventTelemetry: The telemetry log information.
type TelemetryErrorEventTelemetry struct {
	// Device properties.
	Device *TelemetryCommonTelemetryDevice `json:"device,omitzero"`

	// OS properties.
	OS *TelemetryCommonTelemetryOS `json:"os,omitzero"`

	// Telemetry type.
	// Always "log".
	Type *string `json:"type,omitzero"`

	// Level/severity of the log.
	// Always "error".
	Status string `json:"status"`

	// Body of the log.
	Message string `json:"message"`

	// Error properties.
	Error *TelemetryErrorEventTelemetryError `json:"error,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryErrorEventTelemetryKeys = map[string]struct{}{"device": {}, "os": {}, "type": {}, "status": {}, "message": {}, "error": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryErrorEventTelemetry) UnmarshalJSON(data []byte) error {
	type plain TelemetryErrorEventTelemetry
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryErrorEventTelemetryKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryErrorEventTelemetry(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryErrorEventTelemetry) MarshalJSON() ([]byte, error) {
	type plain TelemetryErrorEventTelemetry
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryCommonTelemetryDevice: Device properties.
type TelemetryCommonTelemetryDevice struct {
	// Architecture of the device.
	Architecture *string `json:"architecture,omitzero"`

	// Brand of the device.
	Brand *string `json:"brand,omitzero"`

	// Model of the device.
	Model *string `json:"model,omitzero"`

	// Number of logical CPU cores available for scheduling on the device at runtime, as reported by
	// the operating system.
	LogicalCPUCount json.Number `json:"logical_cpu_count,omitzero"`

	// Total RAM in megabytes.
	TotalRAM json.Number `json:"total_ram,omitzero"`

	// Whether the device is considered a low RAM device (Android).
	IsLowRAM *bool `json:"is_low_ram,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryCommonTelemetryDeviceKeys = map[string]struct{}{"architecture": {}, "brand": {}, "model": {}, "logical_cpu_count": {}, "total_ram": {}, "is_low_ram": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryCommonTelemetryDevice) UnmarshalJSON(data []byte) error {
	type plain TelemetryCommonTelemetryDevice
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryCommonTelemetryDeviceKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryCommonTelemetryDevice(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryCommonTelemetryDevice) MarshalJSON() ([]byte, error) {
	type plain TelemetryCommonTelemetryDevice
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryCommonTelemetryOS: OS properties.
type TelemetryCommonTelemetryOS struct {
	// Build of the OS.
	Build *string `json:"build,omitzero"`

	// Name of the OS.
	Name *string `json:"name,omitzero"`

	// Version of the OS.
	Version *string `json:"version,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryCommonTelemetryOSKeys = map[string]struct{}{"build": {}, "name": {}, "version": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryCommonTelemetryOS) UnmarshalJSON(data []byte) error {
	type plain TelemetryCommonTelemetryOS
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryCommonTelemetryOSKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryCommonTelemetryOS(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryCommonTelemetryOS) MarshalJSON() ([]byte, error) {
	type plain TelemetryCommonTelemetryOS
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryErrorEventTelemetryError: Error properties.
type TelemetryErrorEventTelemetryError struct {
	// The stack trace or the complementary information about the error.
	Stack *string `json:"stack,omitzero"`

	// The error type or kind (or code in some cases).
	Kind *string `json:"kind,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryErrorEventTelemetryErrorKeys = map[string]struct{}{"stack": {}, "kind": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryErrorEventTelemetryError) UnmarshalJSON(data []byte) error {
	type plain TelemetryErrorEventTelemetryError
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryErrorEventTelemetryErrorKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryErrorEventTelemetryError(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryErrorEventTelemetryError) MarshalJSON() ([]byte, error) {
	type plain TelemetryErrorEventTelemetryError
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryDebugEvent: Schema of all properties of a telemetry debug event.
//
// Listed by: rum-events-mobile-schema.json.
type TelemetryDebugEvent struct {
	// Internal properties.
	DD *TelemetryCommonDD `json:"_dd,omitzero"`

	// Telemetry event type. Should specify telemetry only.
	// Always "telemetry".
	Type string `json:"type"`

	// Start of the event in ms from epoch.
	Date int64 `json:"date"`

	// The SDK generating the telemetry event.
	Service string `json:"service"`

	// The source of this event.
	// One of: "android", "ios", "browser", "flutter", "react-native", "unity",
	// "kotlin-multiplatform", "electron", "cpp", "maui".
	Source string `json:"source"`

	// The version of the SDK generating the telemetry event.
	Version string `json:"version"`

	// Application properties.
	Application *TelemetryCommonApplication `json:"application,omitzero"`

	// Session properties.
	Session *TelemetryCommonSession `json:"session,omitzero"`

	// View properties.
	View *TelemetryCommonView `json:"view,omitzero"`

	// Action properties.
	Action *TelemetryCommonAction `json:"action,omitzero"`

	// The actual percentage of telemetry usage per event.
	EffectiveSampleRate json.Number `json:"effective_sample_rate,omitzero"`

	// Enabled experimental features.
	ExperimentalFeatures []string `json:"experimental_features,omitzero"`

	// The telemetry log information.
	Telemetry *TelemetryDebugEventTelemetry `json:"telemetry,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryDebugEventKeys = map[string]struct{}{"_dd": {}, "type": {}, "date": {}, "service": {}, "source": {}, "version": {}, "application": {}, "session": {}, "view": {}, "action": {}, "effective_sample_rate": {}, "experimental_features": {}, "telemetry": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryDebugEvent) UnmarshalJSON(data []byte) error {
	type plain TelemetryDebugEvent
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryDebugEventKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryDebugEvent(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryDebugEvent) MarshalJSON() ([]byte, error) {
	type plain TelemetryDebugEvent
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryDebugEventTelemetry: The telemetry log information.
type TelemetryDebugEventTelemetry struct {
	// Device properties.
	Device *TelemetryCommonTelemetryDevice `json:"device,omitzero"`

	// OS properties.
	OS *TelemetryCommonTelemetryOS `json:"os,omitzero"`

	// Telemetry type.
	// Always "log".
	Type *string `json:"type,omitzero"`

	// Level/severity of the log.
	// Always "debug".
	Status string `json:"status"`

	// Body of the log.
	Message string `json:"message"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryDebugEventTelemetryKeys = map[string]struct{}{"device": {}, "os": {}, "type": {}, "status": {}, "message": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryDebugEventTelemetry) UnmarshalJSON(data []byte) error {
	type plain TelemetryDebugEventTelemetry
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryDebugEventTelemetryKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryDebugEventTelemetry(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryDebugEventTelemetry) MarshalJSON() ([]byte, error) {
	type plain TelemetryDebugEventTelemetry
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryConfigurationEvent: Schema of all properties of a telemetry configuration event.
//
// Listed by: rum-events-mobile-schema.json.
type TelemetryConfigurationEvent struct {
	// Internal properties.
	DD *TelemetryCommonDD `json:"_dd,omitzero"`

	// Telemetry event type. Should specify telemetry only.
	// Always "telemetry".
	Type string `json:"type"`

	// Start of the event in ms from epoch.
	Date int64 `json:"date"`

	// The SDK generating the telemetry event.
	Service string `json:"service"`

	// The source of this event.
	// One of: "android", "ios", "browser", "flutter", "react-native", "unity",
	// "kotlin-multiplatform", "electron", "cpp", "maui".
	Source string `json:"source"`

	// The version of the SDK generating the telemetry event.
	Version string `json:"version"`

	// Application properties.
	Application *TelemetryCommonApplication `json:"application,omitzero"`

	// Session properties.
	Session *TelemetryCommonSession `json:"session,omitzero"`

	// View properties.
	View *TelemetryCommonView `json:"view,omitzero"`

	// Action properties.
	Action *TelemetryCommonAction `json:"action,omitzero"`

	// The actual percentage of telemetry usage per event.
	EffectiveSampleRate json.Number `json:"effective_sample_rate,omitzero"`

	// Enabled experimental features.
	ExperimentalFeatures []string `json:"experimental_features,omitzero"`

	// The telemetry configuration information.
	Telemetry *TelemetryConfigurationEventTelemetry `json:"telemetry,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryConfigurationEventKeys = map[string]struct{}{"_dd": {}, "type": {}, "date": {}, "service": {}, "source": {}, "version": {}, "application": {}, "session": {}, "view": {}, "action": {}, "effective_sample_rate": {}, "experimental_features": {}, "telemetry": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryConfigurationEvent) UnmarshalJSON(data []byte) error {
	type plain TelemetryConfigurationEvent
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryConfigurationEventKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryConfigurationEvent(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryConfigurationEvent) MarshalJSON() ([]byte, error) {
	type plain TelemetryConfigurationEvent
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryConfigurationEventTelemetry: The telemetry configuration information.
type TelemetryConfigurationEventTelemetry struct {
	// Device properties.
	Device *TelemetryCommonTelemetryDevice `json:"device,omitzero"`

	// OS properties.
	OS *TelemetryCommonTelemetryOS `json:"os,omitzero"`

	// Telemetry type.
	// Always "configuration".
	Type string `json:"type"`

	// Configuration properties.
	Configuration *TelemetryConfigurationEventTelemetryConfiguration `json:"configuration,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryConfigurationEventTelemetryKeys = map[string]struct{}{"device": {}, "os": {}, "type": {}, "configuration": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryConfigurationEventTelemetry) UnmarshalJSON(data []byte) error {
	type plain TelemetryConfigurationEventTelemetry
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryConfigurationEventTelemetryKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryConfigurationEventTelemetry(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryConfigurationEventTelemetry) MarshalJSON() ([]byte, error) {
	type plain TelemetryConfigurationEventTelemetry
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryConfigurationEventTelemetryConfiguration: Configuration properties.
type TelemetryConfigurationEventTelemetryConfiguration struct {
	// The percentage of sessions tracked.
	SessionSampleRate *int64 `json:"session_sample_rate,omitzero"`

	// The percentage of telemetry events sent.
	TelemetrySampleRate *int64 `json:"telemetry_sample_rate,omitzero"`

	// The percentage of telemetry configuration events sent after being sampled by
	// telemetry_sample_rate.
	TelemetryConfigurationSampleRate *int64 `json:"telemetry_configuration_sample_rate,omitzero"`

	// The percentage of telemetry usage events sent after being sampled by telemetry_sample_rate.
	TelemetryUsageSampleRate *int64 `json:"telemetry_usage_sample_rate,omitzero"`

	// The percentage of requests traced.
	TraceSampleRate *int64 `json:"trace_sample_rate,omitzero"`

	// The opt-in configuration to add trace context.
	// One of: "all", "sampled".
	TraceContextInjection *string `json:"trace_context_injection,omitzero"`

	// The percentage of sessions with Browser RUM & Session Replay pricing tracked (deprecated in
	// favor of session_replay_sample_rate).
	PremiumSampleRate *int64 `json:"premium_sample_rate,omitzero"`

	// The percentage of sessions with Browser RUM & Session Replay pricing tracked (deprecated in
	// favor of session_replay_sample_rate).
	ReplaySampleRate *int64 `json:"replay_sample_rate,omitzero"`

	// The percentage of sessions with RUM & Session Replay pricing tracked.
	SessionReplaySampleRate *int64 `json:"session_replay_sample_rate,omitzero"`

	// The initial tracking consent value.
	// One of: "granted", "not-granted", "pending".
	TrackingConsent *string `json:"tracking_consent,omitzero"`

	// Whether the session replay start is handled manually.
	StartSessionReplayRecordingManually *bool `json:"start_session_replay_recording_manually,omitzero"`

	// Whether Session Replay should automatically start a recording when enabled.
	StartRecordingImmediately *bool `json:"start_recording_immediately,omitzero"`

	// Whether a proxy is used.
	UseProxy *bool `json:"use_proxy,omitzero"`

	// Whether beforeSend callback function is used.
	UseBeforeSend *bool `json:"use_before_send,omitzero"`

	// Whether initialization fails silently if the SDK is already initialized.
	SilentMultipleInit *bool `json:"silent_multiple_init,omitzero"`

	// Whether sessions across subdomains for the same site are tracked.
	TrackSessionAcrossSubdomains *bool `json:"track_session_across_subdomains,omitzero"`

	// Whether resources are tracked.
	TrackResources *bool `json:"track_resources,omitzero"`

	// Whether early requests are tracked.
	TrackEarlyRequests *bool `json:"track_early_requests,omitzero"`

	// Whether long tasks are tracked.
	TrackLongTask *bool `json:"track_long_task,omitzero"`

	// Whether views loaded from the bfcache are tracked.
	TrackBfcacheViews *bool `json:"track_bfcache_views,omitzero"`

	// Whether a secure cross-site session cookie is used (deprecated).
	UseCrossSiteSessionCookie *bool `json:"use_cross_site_session_cookie,omitzero"`

	// Whether a partitioned secure cross-site session cookie is used.
	UsePartitionedCrossSiteSessionCookie *bool `json:"use_partitioned_cross_site_session_cookie,omitzero"`

	// Whether a secure session cookie is used.
	UseSecureSessionCookie *bool `json:"use_secure_session_cookie,omitzero"`

	// Whether it is allowed to use LocalStorage when cookies are not available (deprecated in favor
	// of session_persistence).
	AllowFallbackToLocalStorage *bool `json:"allow_fallback_to_local_storage,omitzero"`

	// Configure the storage strategy for persisting sessions.
	// One of: "local-storage", "cookie", "memory".
	SessionPersistence *string `json:"session_persistence,omitzero"`

	// Whether contexts are stored in local storage.
	StoreContextsAcrossPages *bool `json:"store_contexts_across_pages,omitzero"`

	// Whether untrusted events are allowed.
	AllowUntrustedEvents *bool `json:"allow_untrusted_events,omitzero"`

	// Attribute to be used to name actions.
	ActionNameAttribute *string `json:"action_name_attribute,omitzero"`

	// Whether the allowed tracing origins list is used (deprecated in favor of
	// use_allowed_tracing_urls).
	UseAllowedTracingOrigins *bool `json:"use_allowed_tracing_origins,omitzero"`

	// Whether the allowed tracing urls list is used.
	UseAllowedTracingUrls *bool `json:"use_allowed_tracing_urls,omitzero"`

	// Whether the allowed GraphQL urls list is used.
	UseAllowedGraphQlUrls *bool `json:"use_allowed_graph_ql_urls,omitzero"`

	// Whether GraphQL payload tracking is used for at least one GraphQL endpoint.
	UseTrackGraphQlPayload *bool `json:"use_track_graph_ql_payload,omitzero"`

	// Whether GraphQL response errors tracking is used for at least one GraphQL endpoint.
	UseTrackGraphQlResponseErrors *bool `json:"use_track_graph_ql_response_errors,omitzero"`

	// A list of selected tracing propagators.
	SelectedTracingPropagators []string `json:"selected_tracing_propagators,omitzero"`

	// Session replay default privacy level.
	DefaultPrivacyLevel *string `json:"default_privacy_level,omitzero"`

	// Session replay text and input privacy level.
	TextAndInputPrivacyLevel *string `json:"text_and_input_privacy_level,omitzero"`

	// Session replay image privacy level.
	ImagePrivacyLevel *string `json:"image_privacy_level,omitzero"`

	// Session replay touch privacy level.
	TouchPrivacyLevel *string `json:"touch_privacy_level,omitzero"`

	// Privacy control for action name.
	EnablePrivacyForActionName *bool `json:"enable_privacy_for_action_name,omitzero"`

	// Whether the request origins list to ignore when computing the page activity is used.
	UseExcludedActivityUrls *bool `json:"use_excluded_activity_urls,omitzero"`

	// Whether the Worker is loaded from an external URL.
	UseWorkerURL *bool `json:"use_worker_url,omitzero"`

	// Whether intake requests are compressed.
	CompressIntakeRequests *bool `json:"compress_intake_requests,omitzero"`

	// Whether user frustrations are tracked.
	TrackFrustrations *bool `json:"track_frustrations,omitzero"`

	// Whether the RUM views creation is handled manually.
	TrackViewsManually *bool `json:"track_views_manually,omitzero"`

	// Whether user actions are tracked (deprecated in favor of track_user_interactions).
	TrackInteractions *bool `json:"track_interactions,omitzero"`

	// Whether user actions are tracked.
	TrackUserInteractions *bool `json:"track_user_interactions,omitzero"`

	// Whether console.error logs, uncaught exceptions and network errors are tracked.
	ForwardErrorsToLogs *bool `json:"forward_errors_to_logs,omitzero"`

	// The number of displays available to the device.
	NumberOfDisplays *int64 `json:"number_of_displays,omitzero"`

	// The console.* tracked.
	// Wire form: array of string | "all". Decoded as any (numbers as json.Number).
	ForwardConsoleLogs any `json:"forward_console_logs,omitzero"`

	// The reports from the Reporting API tracked.
	// Wire form: array of string | "all". Decoded as any (numbers as json.Number).
	ForwardReports any `json:"forward_reports,omitzero"`

	// Whether local encryption is used.
	UseLocalEncryption *bool `json:"use_local_encryption,omitzero"`

	// View tracking strategy.
	// One of: "ActivityViewTrackingStrategy", "FragmentViewTrackingStrategy",
	// "MixedViewTrackingStrategy", "NavigationViewTrackingStrategy".
	ViewTrackingStrategy *string `json:"view_tracking_strategy,omitzero"`

	// Whether SwiftUI view instrumentation is enabled.
	SwiftuiViewTrackingEnabled *bool `json:"swiftui_view_tracking_enabled,omitzero"`

	// Whether SwiftUI action instrumentation is enabled.
	SwiftuiActionTrackingEnabled *bool `json:"swiftui_action_tracking_enabled,omitzero"`

	// Whether RUM events are tracked when the application is in Background.
	TrackBackgroundEvents *bool `json:"track_background_events,omitzero"`

	// The period between each Mobile Vital sample (in milliseconds).
	MobileVitalsUpdatePeriod *int64 `json:"mobile_vitals_update_period,omitzero"`

	// Whether error monitoring & crash reporting is enabled for the source platform.
	TrackErrors *bool `json:"track_errors,omitzero"`

	// Whether automatic collection of network requests is enabled.
	TrackNetworkRequests *bool `json:"track_network_requests,omitzero"`

	// Whether tracing features are enabled.
	UseTracing *bool `json:"use_tracing,omitzero"`

	// Whether native views are tracked (for cross platform SDKs).
	TrackNativeViews *bool `json:"track_native_views,omitzero"`

	// Whether native error monitoring & crash reporting is enabled (for cross platform SDKs).
	TrackNativeErrors *bool `json:"track_native_errors,omitzero"`

	// Whether long task tracking is performed automatically.
	TrackNativeLongTasks *bool `json:"track_native_long_tasks,omitzero"`

	// Whether long task tracking is performed automatically for cross platform SDKs.
	TrackCrossPlatformLongTasks *bool `json:"track_cross_platform_long_tasks,omitzero"`

	// Whether the client has provided a list of first party hosts.
	UseFirstPartyHosts *bool `json:"use_first_party_hosts,omitzero"`

	// The type of initialization the SDK used, in case multiple are supported.
	InitializationType *string `json:"initialization_type,omitzero"`

	// Whether Flutter build and raster time tracking is enabled.
	TrackFlutterPerformance *bool `json:"track_flutter_performance,omitzero"`

	// The window duration for batches sent by the SDK (in milliseconds).
	BatchSize *int64 `json:"batch_size,omitzero"`

	// The upload frequency of batches (in milliseconds).
	BatchUploadFrequency *int64 `json:"batch_upload_frequency,omitzero"`

	// Maximum number of batches processed sequentially without a delay.
	BatchProcessingLevel *int64 `json:"batch_processing_level,omitzero"`

	// Whether UIApplication background tasks are enabled.
	BackgroundTasksEnabled *bool `json:"background_tasks_enabled,omitzero"`

	// The version of React used in a ReactNative application.
	ReactVersion *string `json:"react_version,omitzero"`

	// The version of ReactNative used in a ReactNative application.
	ReactNativeVersion *string `json:"react_native_version,omitzero"`

	// The version of Dart used in a Flutter application.
	DartVersion *string `json:"dart_version,omitzero"`

	// The version of Unity used in a Unity application.
	UnityVersion *string `json:"unity_version,omitzero"`

	// The version of MAUI used in a .NET MAUI application.
	MauiVersion *string `json:"maui_version,omitzero"`

	// The threshold used for iOS App Hangs monitoring (in milliseconds).
	AppHangThreshold *int64 `json:"app_hang_threshold,omitzero"`

	// Whether logs are sent to the PCI-compliant intake.
	UsePciIntake *bool `json:"use_pci_intake,omitzero"`

	// The tracer API used by the SDK. Possible values: 'Datadog', 'OpenTelemetry', 'OpenTracing'.
	TracerAPI *string `json:"tracer_api,omitzero"`

	// The version of the tracer API used by the SDK. Eg. '0.1.0'.
	TracerAPIVersion *string `json:"tracer_api_version,omitzero"`

	// Whether logs are sent after the session expiration.
	SendLogsAfterSessionExpiration *bool `json:"send_logs_after_session_expiration,omitzero"`

	// The list of plugins enabled.
	Plugins []TelemetryConfigurationEventTelemetryConfigurationPlugins `json:"plugins,omitzero"`

	// Whether the SDK is initialised on the application's main or a secondary process.
	IsMainProcess *bool `json:"is_main_process,omitzero"`

	// Interval in milliseconds when the last action is considered as the action that created the next
	// view. Only sent if a time based strategy has been used.
	InvTimeThresholdMs *int64 `json:"inv_time_threshold_ms,omitzero"`

	// The interval in milliseconds during which all network requests will be considered as initial,
	// i.e. caused by the creation of this view. Only sent if a time based strategy has been used.
	TnsTimeThresholdMs *int64 `json:"tns_time_threshold_ms,omitzero"`

	// The list of events that include feature flags collection. The tracking is always enabled for
	// views and errors.
	TrackFeatureFlagsForEvents []string `json:"track_feature_flags_for_events,omitzero"`

	// Whether the anonymous users are tracked.
	TrackAnonymousUser *bool `json:"track_anonymous_user,omitzero"`

	// Whether a list of allowed origins is used to control SDK execution in browser extension
	// contexts. When enabled, the SDK will check if the current origin matches the allowed origins
	// list before running.
	UseAllowedTrackingOrigins *bool `json:"use_allowed_tracking_origins,omitzero"`

	// The version of the SDK that is running.
	SDKVersion *string `json:"sdk_version,omitzero"`

	// The source of the SDK, e.g., 'browser', 'ios', 'android', 'flutter', 'react-native', 'unity',
	// 'kotlin-multiplatform', 'maui'.
	Source *string `json:"source,omitzero"`

	// The variant of the SDK build (e.g., standard, lite, etc.).
	Variant *string `json:"variant,omitzero"`

	// The id of the remote configuration.
	RemoteConfigurationID *string `json:"remote_configuration_id,omitzero"`

	// Whether a proxy is used for remote configuration.
	UseRemoteConfigurationProxy *bool `json:"use_remote_configuration_proxy,omitzero"`

	// Metadata of the remote configuration currently applied for this session.
	RemoteConfiguration *TelemetryConfigurationEventTelemetryConfigurationRemoteConfiguration `json:"remote_configuration,omitzero"`

	// The percentage of sessions with Profiling enabled.
	ProfilingSampleRate json.Number `json:"profiling_sample_rate,omitzero"`

	// Whether trace baggage is propagated to child spans.
	PropagateTraceBaggage *bool `json:"propagate_trace_baggage,omitzero"`

	// How the SDK tracks resource request/response headers.
	// One of: "default_headers", "custom".
	TrackResourceHeaders *string `json:"track_resource_headers,omitzero"`

	// Whether the beta encode cookie options is enabled.
	BetaEncodeCookieOptions *bool `json:"beta_encode_cookie_options,omitzero"`

	// Whether the beta partial view updates feature is enabled.
	BetaEnableViewUpdates *bool `json:"beta_enable_view_updates,omitzero"`

	// Whether the beta track WebSockets feature is enabled.
	BetaTrackWebSockets *bool `json:"beta_track_web_sockets,omitzero"`

	// Whether tracing feature's client-side-stats generation is enabled.
	UseClientSideStats *bool `json:"use_client_side_stats,omitzero"`

	// Whether trace sampling rules are configured.
	UseTraceSamplingRules *bool `json:"use_trace_sampling_rules,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryConfigurationEventTelemetryConfigurationKeys = map[string]struct{}{"session_sample_rate": {}, "telemetry_sample_rate": {}, "telemetry_configuration_sample_rate": {}, "telemetry_usage_sample_rate": {}, "trace_sample_rate": {}, "trace_context_injection": {}, "premium_sample_rate": {}, "replay_sample_rate": {}, "session_replay_sample_rate": {}, "tracking_consent": {}, "start_session_replay_recording_manually": {}, "start_recording_immediately": {}, "use_proxy": {}, "use_before_send": {}, "silent_multiple_init": {}, "track_session_across_subdomains": {}, "track_resources": {}, "track_early_requests": {}, "track_long_task": {}, "track_bfcache_views": {}, "use_cross_site_session_cookie": {}, "use_partitioned_cross_site_session_cookie": {}, "use_secure_session_cookie": {}, "allow_fallback_to_local_storage": {}, "session_persistence": {}, "store_contexts_across_pages": {}, "allow_untrusted_events": {}, "action_name_attribute": {}, "use_allowed_tracing_origins": {}, "use_allowed_tracing_urls": {}, "use_allowed_graph_ql_urls": {}, "use_track_graph_ql_payload": {}, "use_track_graph_ql_response_errors": {}, "selected_tracing_propagators": {}, "default_privacy_level": {}, "text_and_input_privacy_level": {}, "image_privacy_level": {}, "touch_privacy_level": {}, "enable_privacy_for_action_name": {}, "use_excluded_activity_urls": {}, "use_worker_url": {}, "compress_intake_requests": {}, "track_frustrations": {}, "track_views_manually": {}, "track_interactions": {}, "track_user_interactions": {}, "forward_errors_to_logs": {}, "number_of_displays": {}, "forward_console_logs": {}, "forward_reports": {}, "use_local_encryption": {}, "view_tracking_strategy": {}, "swiftui_view_tracking_enabled": {}, "swiftui_action_tracking_enabled": {}, "track_background_events": {}, "mobile_vitals_update_period": {}, "track_errors": {}, "track_network_requests": {}, "use_tracing": {}, "track_native_views": {}, "track_native_errors": {}, "track_native_long_tasks": {}, "track_cross_platform_long_tasks": {}, "use_first_party_hosts": {}, "initialization_type": {}, "track_flutter_performance": {}, "batch_size": {}, "batch_upload_frequency": {}, "batch_processing_level": {}, "background_tasks_enabled": {}, "react_version": {}, "react_native_version": {}, "dart_version": {}, "unity_version": {}, "maui_version": {}, "app_hang_threshold": {}, "use_pci_intake": {}, "tracer_api": {}, "tracer_api_version": {}, "send_logs_after_session_expiration": {}, "plugins": {}, "is_main_process": {}, "inv_time_threshold_ms": {}, "tns_time_threshold_ms": {}, "track_feature_flags_for_events": {}, "track_anonymous_user": {}, "use_allowed_tracking_origins": {}, "sdk_version": {}, "source": {}, "variant": {}, "remote_configuration_id": {}, "use_remote_configuration_proxy": {}, "remote_configuration": {}, "profiling_sample_rate": {}, "propagate_trace_baggage": {}, "track_resource_headers": {}, "beta_encode_cookie_options": {}, "beta_enable_view_updates": {}, "beta_track_web_sockets": {}, "use_client_side_stats": {}, "use_trace_sampling_rules": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryConfigurationEventTelemetryConfiguration) UnmarshalJSON(data []byte) error {
	type plain TelemetryConfigurationEventTelemetryConfiguration
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryConfigurationEventTelemetryConfigurationKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryConfigurationEventTelemetryConfiguration(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryConfigurationEventTelemetryConfiguration) MarshalJSON() ([]byte, error) {
	type plain TelemetryConfigurationEventTelemetryConfiguration
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryConfigurationEventTelemetryConfigurationPlugins
type TelemetryConfigurationEventTelemetryConfigurationPlugins struct {
	// The name of the plugin.
	Name string `json:"name"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryConfigurationEventTelemetryConfigurationPluginsKeys = map[string]struct{}{"name": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryConfigurationEventTelemetryConfigurationPlugins) UnmarshalJSON(data []byte) error {
	type plain TelemetryConfigurationEventTelemetryConfigurationPlugins
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryConfigurationEventTelemetryConfigurationPluginsKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryConfigurationEventTelemetryConfigurationPlugins(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryConfigurationEventTelemetryConfigurationPlugins) MarshalJSON() ([]byte, error) {
	type plain TelemetryConfigurationEventTelemetryConfigurationPlugins
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryConfigurationEventTelemetryConfigurationRemoteConfiguration: Metadata of the remote
// configuration currently applied for this session.
type TelemetryConfigurationEventTelemetryConfigurationRemoteConfiguration struct {
	// Identifier of the remote configuration bundle this metadata belongs to.
	ConfigID *string `json:"config_id,omitzero"`

	// CDN version identifier of the applied configuration.
	VersionID *string `json:"version_id,omitzero"`

	// CDN publish timestamp of the applied configuration, in ms from epoch.
	LastModified *int64 `json:"last_modified,omitzero"`

	// Timestamp at which the device fetched and cached this configuration version, in ms from epoch.
	LastSynced *int64 `json:"last_synced,omitzero"`

	// Timestamp at which this configuration version was first observed as applied by the device, in
	// ms from epoch. Stamped once and reused on every subsequent session that runs on the same
	// version.
	FirstApplied *int64 `json:"first_applied,omitzero"`

	// Identifier of the sync that produced this configuration version, used to deduplicate repeat
	// sessions from the same device without a persistent identifier.
	SyncID *string `json:"sync_id,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryConfigurationEventTelemetryConfigurationRemoteConfigurationKeys = map[string]struct{}{"config_id": {}, "version_id": {}, "last_modified": {}, "last_synced": {}, "first_applied": {}, "sync_id": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryConfigurationEventTelemetryConfigurationRemoteConfiguration) UnmarshalJSON(data []byte) error {
	type plain TelemetryConfigurationEventTelemetryConfigurationRemoteConfiguration
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryConfigurationEventTelemetryConfigurationRemoteConfigurationKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryConfigurationEventTelemetryConfigurationRemoteConfiguration(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryConfigurationEventTelemetryConfigurationRemoteConfiguration) MarshalJSON() ([]byte, error) {
	type plain TelemetryConfigurationEventTelemetryConfigurationRemoteConfiguration
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryUsageEvent: Schema of all properties of a telemetry usage event.
//
// Listed by: rum-events-mobile-schema.json.
type TelemetryUsageEvent struct {
	// Internal properties.
	DD *TelemetryCommonDD `json:"_dd,omitzero"`

	// Telemetry event type. Should specify telemetry only.
	// Always "telemetry".
	Type string `json:"type"`

	// Start of the event in ms from epoch.
	Date int64 `json:"date"`

	// The SDK generating the telemetry event.
	Service string `json:"service"`

	// The source of this event.
	// One of: "android", "ios", "browser", "flutter", "react-native", "unity",
	// "kotlin-multiplatform", "electron", "cpp", "maui".
	Source string `json:"source"`

	// The version of the SDK generating the telemetry event.
	Version string `json:"version"`

	// Application properties.
	Application *TelemetryCommonApplication `json:"application,omitzero"`

	// Session properties.
	Session *TelemetryCommonSession `json:"session,omitzero"`

	// View properties.
	View *TelemetryCommonView `json:"view,omitzero"`

	// Action properties.
	Action *TelemetryCommonAction `json:"action,omitzero"`

	// The actual percentage of telemetry usage per event.
	EffectiveSampleRate json.Number `json:"effective_sample_rate,omitzero"`

	// Enabled experimental features.
	ExperimentalFeatures []string `json:"experimental_features,omitzero"`

	// The telemetry usage information.
	Telemetry *TelemetryUsageEventTelemetry `json:"telemetry,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryUsageEventKeys = map[string]struct{}{"_dd": {}, "type": {}, "date": {}, "service": {}, "source": {}, "version": {}, "application": {}, "session": {}, "view": {}, "action": {}, "effective_sample_rate": {}, "experimental_features": {}, "telemetry": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryUsageEvent) UnmarshalJSON(data []byte) error {
	type plain TelemetryUsageEvent
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryUsageEventKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryUsageEvent(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryUsageEvent) MarshalJSON() ([]byte, error) {
	type plain TelemetryUsageEvent
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryUsageEventTelemetry: The telemetry usage information.
type TelemetryUsageEventTelemetry struct {
	// Device properties.
	Device *TelemetryCommonTelemetryDevice `json:"device,omitzero"`

	// OS properties.
	OS *TelemetryCommonTelemetryOS `json:"os,omitzero"`

	// Telemetry type.
	// Always "usage".
	Type string `json:"type"`

	Usage *TelemetryUsageEventTelemetryUsage `json:"usage,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryUsageEventTelemetryKeys = map[string]struct{}{"device": {}, "os": {}, "type": {}, "usage": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryUsageEventTelemetry) UnmarshalJSON(data []byte) error {
	type plain TelemetryUsageEventTelemetry
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryUsageEventTelemetryKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryUsageEventTelemetry(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryUsageEventTelemetry) MarshalJSON() ([]byte, error) {
	type plain TelemetryUsageEventTelemetry
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}

// TelemetryUsageEventTelemetryUsage: Schema of features usage common across SDKs.
//
// Flattened oneOf of: SetTrackingConsent, StopSession, StartView, SetViewContext,
// SetViewContextProperty, SetViewName, GetViewContext, AddAction, AddError, GetGlobalContext,
// SetGlobalContext, SetGlobalContextProperty, RemoveGlobalContextProperty, ClearGlobalContext,
// GetUser, SetUser, SetUserProperty, RemoveUserProperty, ClearUser, GetAccount, SetAccount,
// SetAccountProperty, RemoveAccountProperty, ClearAccount, AddFeatureFlagEvaluation,
// AddOperationStepVital, GraphQLRequest, AddViewLoadingTime, TelemetryCommonFeaturesUsage,
// TelemetryBrowserFeaturesUsage, TelemetryMobileFeaturesUsage. All alternative-specific fields are
// optional.
type TelemetryUsageEventTelemetryUsage struct {
	// One of: "set-tracking-consent", "stop-session", "start-view", "set-view-context",
	// "set-view-context-property", "set-view-name", "get-view-context", "add-action", "add-error",
	// "get-global-context", "set-global-context", "set-global-context-property",
	// "remove-global-context-property", "clear-global-context", "get-user", "set-user",
	// "set-user-property", "remove-user-property", "clear-user", "get-account", "set-account",
	// "set-account-property", "remove-account-property", "clear-account",
	// "add-feature-flag-evaluation", "add-operation-step-vital", "graphql-request",
	// "addViewLoadingTime", "start-session-replay-recording", "start-duration-vital",
	// "stop-duration-vital", "add-duration-vital", "start-action", "stop-action", "start-resource",
	// "stop-resource", "source-code-context", "trackWebView", "timeseries",
	// "androidNetworkInstrumentation".
	Feature string `json:"feature"`

	// The tracking consent value set by the user.
	// One of: "granted", "not-granted", "pending".
	TrackingConsent *string `json:"tracking_consent,omitzero"`

	// Operations step type.
	// One of: "start", "succeed", "fail".
	ActionType *string `json:"action_type,omitzero"`

	// Whether the view is not available.
	NoView *bool `json:"no_view,omitzero"`

	// Whether the available view is not active.
	NoActiveView *bool `json:"no_active_view,omitzero"`

	// Whether this call overwrote a previously set loading time.
	Overwritten *bool `json:"overwritten,omitzero"`

	// Whether the recording is allowed to start even on sessions sampled out of replay.
	IsForced *bool `json:"is_forced,omitzero"`

	// The network instrumentation API used.
	// One of: "CRONET", "OKHTTP", "LEGACY_OKHTTP".
	Type *string `json:"type,omitzero"`

	// AdditionalProperties holds every key of the object that the schema does not declare. Nothing is
	// dropped on decode and everything is written back on encode.
	AdditionalProperties map[string]any `json:"-"`
}

var telemetryUsageEventTelemetryUsageKeys = map[string]struct{}{"feature": {}, "tracking_consent": {}, "action_type": {}, "no_view": {}, "no_active_view": {}, "overwritten": {}, "is_forced": {}, "type": {}}

// UnmarshalJSON decodes the declared fields and keeps unknown keys in AdditionalProperties.
func (o *TelemetryUsageEventTelemetryUsage) UnmarshalJSON(data []byte) error {
	type plain TelemetryUsageEventTelemetryUsage
	var p plain
	extra, err := jsonx.UnmarshalObject(data, &p, telemetryUsageEventTelemetryUsageKeys)
	if err != nil {
		return err
	}
	p.AdditionalProperties = extra
	*o = TelemetryUsageEventTelemetryUsage(p)
	return nil
}

// MarshalJSON encodes the declared fields together with AdditionalProperties.
func (o TelemetryUsageEventTelemetryUsage) MarshalJSON() ([]byte, error) {
	type plain TelemetryUsageEventTelemetryUsage
	return jsonx.MarshalObject(plain(o), o.AdditionalProperties)
}
