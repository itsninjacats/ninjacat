// Code generated from DataDog/rum-events-format@ab3e7c5a13f1a6fed3d63bb22f1e8a9ebaa25671. DO NOT EDIT.
//
// Regenerate with: RUM_EVENTS_FORMAT=<checkout> go generate ./rumevents/...

package rumevents

import (
	"github.com/itsninjacats/server/rumevents/internal/jsonx"
)

// Event: Any event accepted on the RUM intake track.
//
// Implemented by: RumEvent, TelemetryEvent, RumTimeseriesEvent.
type Event interface {
	// EventType returns the value of the top-level "type" field that selects the variant.
	EventType() string
	// SchemaRoots lists the platform root schemas whose union contains the variant, i.e. which SDK
	// families can send it.
	SchemaRoots() []string
	isEvent()
}

// RumEvent: Schema of all properties of a RUM event.
//
// Implemented by: RumActionEvent, RumTransitionEvent, RumErrorEvent, RumLongTaskEvent,
// RumResourceEvent, RumViewEvent, RumViewUpdateEvent, RumVitalEvent.
type RumEvent interface {
	Event
	isRumEvent()
}

// RumVitalEvent
//
// Implemented by: RumVitalDurationEvent, RumVitalOperationStepEvent, RumVitalAppLaunchEvent.
type RumVitalEvent interface {
	RumEvent
	isRumVitalEvent()
}

// TelemetryEvent: Schema of all properties of a telemetry event.
//
// Implemented by: TelemetryErrorEvent, TelemetryDebugEvent, TelemetryConfigurationEvent,
// TelemetryUsageEvent.
type TelemetryEvent interface {
	Event
	isTelemetryEvent()
}

// RumTimeseriesEvent: Schema of all properties of a timeseries event.
//
// Implemented by: RumTimeseriesMemoryEvent, RumTimeseriesCpuEvent.
type RumTimeseriesEvent interface {
	Event
	isRumTimeseriesEvent()
}

// EventType reports the value of the "type" discriminator for RumActionEvent.
func (*RumActionEvent) EventType() string { return "action" }

// SchemaRoots reports the platform root schemas that list RumActionEvent.
func (*RumActionEvent) SchemaRoots() []string {
	return []string{"rum-events-schema.json", "rum-events-browser-schema.json", "rum-events-mobile-schema.json"}
}
func (*RumActionEvent) isEvent()    {}
func (*RumActionEvent) isRumEvent() {}

// EventType reports the value of the "type" discriminator for RumTransitionEvent.
func (*RumTransitionEvent) EventType() string { return "transition" }

// SchemaRoots reports the platform root schemas that list RumTransitionEvent.
func (*RumTransitionEvent) SchemaRoots() []string {
	return []string{"rum-events-schema.json", "rum-events-browser-schema.json"}
}
func (*RumTransitionEvent) isEvent()    {}
func (*RumTransitionEvent) isRumEvent() {}

// EventType reports the value of the "type" discriminator for RumErrorEvent.
func (*RumErrorEvent) EventType() string { return "error" }

// SchemaRoots reports the platform root schemas that list RumErrorEvent.
func (*RumErrorEvent) SchemaRoots() []string {
	return []string{"rum-events-schema.json", "rum-events-browser-schema.json", "rum-events-mobile-schema.json", "rum-events-electron-schema.json"}
}
func (*RumErrorEvent) isEvent()    {}
func (*RumErrorEvent) isRumEvent() {}

// EventType reports the value of the "type" discriminator for RumLongTaskEvent.
func (*RumLongTaskEvent) EventType() string { return "long_task" }

// SchemaRoots reports the platform root schemas that list RumLongTaskEvent.
func (*RumLongTaskEvent) SchemaRoots() []string {
	return []string{"rum-events-schema.json", "rum-events-browser-schema.json", "rum-events-mobile-schema.json"}
}
func (*RumLongTaskEvent) isEvent()    {}
func (*RumLongTaskEvent) isRumEvent() {}

// EventType reports the value of the "type" discriminator for RumResourceEvent.
func (*RumResourceEvent) EventType() string { return "resource" }

// SchemaRoots reports the platform root schemas that list RumResourceEvent.
func (*RumResourceEvent) SchemaRoots() []string {
	return []string{"rum-events-schema.json", "rum-events-browser-schema.json", "rum-events-mobile-schema.json", "rum-events-electron-schema.json"}
}
func (*RumResourceEvent) isEvent()    {}
func (*RumResourceEvent) isRumEvent() {}

// EventType reports the value of the "type" discriminator for RumViewEvent.
func (*RumViewEvent) EventType() string { return "view" }

// SchemaRoots reports the platform root schemas that list RumViewEvent.
func (*RumViewEvent) SchemaRoots() []string {
	return []string{"rum-events-schema.json", "rum-events-browser-schema.json", "rum-events-mobile-schema.json", "rum-events-electron-schema.json"}
}
func (*RumViewEvent) isEvent()    {}
func (*RumViewEvent) isRumEvent() {}

// EventType reports the value of the "type" discriminator for RumViewUpdateEvent.
func (*RumViewUpdateEvent) EventType() string { return "view_update" }

// SchemaRoots reports the platform root schemas that list RumViewUpdateEvent.
func (*RumViewUpdateEvent) SchemaRoots() []string {
	return []string{"rum-events-schema.json", "rum-events-browser-schema.json", "rum-events-mobile-schema.json", "rum-events-electron-schema.json"}
}
func (*RumViewUpdateEvent) isEvent()    {}
func (*RumViewUpdateEvent) isRumEvent() {}

// EventType reports the value of the "type" discriminator for RumVitalDurationEvent.
func (*RumVitalDurationEvent) EventType() string { return "vital" }

// SchemaRoots reports the platform root schemas that list RumVitalDurationEvent.
func (*RumVitalDurationEvent) SchemaRoots() []string {
	return []string{"rum-events-schema.json", "rum-events-browser-schema.json", "rum-events-mobile-schema.json", "rum-events-electron-schema.json"}
}
func (*RumVitalDurationEvent) isEvent()         {}
func (*RumVitalDurationEvent) isRumEvent()      {}
func (*RumVitalDurationEvent) isRumVitalEvent() {}

// EventType reports the value of the "type" discriminator for RumVitalOperationStepEvent.
func (*RumVitalOperationStepEvent) EventType() string { return "vital" }

// SchemaRoots reports the platform root schemas that list RumVitalOperationStepEvent.
func (*RumVitalOperationStepEvent) SchemaRoots() []string {
	return []string{"rum-events-schema.json", "rum-events-browser-schema.json", "rum-events-mobile-schema.json", "rum-events-electron-schema.json"}
}
func (*RumVitalOperationStepEvent) isEvent()         {}
func (*RumVitalOperationStepEvent) isRumEvent()      {}
func (*RumVitalOperationStepEvent) isRumVitalEvent() {}

// EventType reports the value of the "type" discriminator for RumVitalAppLaunchEvent.
func (*RumVitalAppLaunchEvent) EventType() string { return "vital" }

// SchemaRoots reports the platform root schemas that list RumVitalAppLaunchEvent.
func (*RumVitalAppLaunchEvent) SchemaRoots() []string {
	return []string{"rum-events-schema.json", "rum-events-mobile-schema.json"}
}
func (*RumVitalAppLaunchEvent) isEvent()         {}
func (*RumVitalAppLaunchEvent) isRumEvent()      {}
func (*RumVitalAppLaunchEvent) isRumVitalEvent() {}

// EventType reports the value of the "type" discriminator for TelemetryErrorEvent.
func (*TelemetryErrorEvent) EventType() string { return "telemetry" }

// SchemaRoots reports the platform root schemas that list TelemetryErrorEvent.
func (*TelemetryErrorEvent) SchemaRoots() []string { return []string{"rum-events-mobile-schema.json"} }
func (*TelemetryErrorEvent) isEvent()              {}
func (*TelemetryErrorEvent) isTelemetryEvent()     {}

// EventType reports the value of the "type" discriminator for TelemetryDebugEvent.
func (*TelemetryDebugEvent) EventType() string { return "telemetry" }

// SchemaRoots reports the platform root schemas that list TelemetryDebugEvent.
func (*TelemetryDebugEvent) SchemaRoots() []string { return []string{"rum-events-mobile-schema.json"} }
func (*TelemetryDebugEvent) isEvent()              {}
func (*TelemetryDebugEvent) isTelemetryEvent()     {}

// EventType reports the value of the "type" discriminator for TelemetryConfigurationEvent.
func (*TelemetryConfigurationEvent) EventType() string { return "telemetry" }

// SchemaRoots reports the platform root schemas that list TelemetryConfigurationEvent.
func (*TelemetryConfigurationEvent) SchemaRoots() []string {
	return []string{"rum-events-mobile-schema.json"}
}
func (*TelemetryConfigurationEvent) isEvent()          {}
func (*TelemetryConfigurationEvent) isTelemetryEvent() {}

// EventType reports the value of the "type" discriminator for TelemetryUsageEvent.
func (*TelemetryUsageEvent) EventType() string { return "telemetry" }

// SchemaRoots reports the platform root schemas that list TelemetryUsageEvent.
func (*TelemetryUsageEvent) SchemaRoots() []string { return []string{"rum-events-mobile-schema.json"} }
func (*TelemetryUsageEvent) isEvent()              {}
func (*TelemetryUsageEvent) isTelemetryEvent()     {}

// EventType reports the value of the "type" discriminator for RumTimeseriesMemoryEvent.
func (*RumTimeseriesMemoryEvent) EventType() string { return "timeseries" }

// SchemaRoots reports the platform root schemas that list RumTimeseriesMemoryEvent.
func (*RumTimeseriesMemoryEvent) SchemaRoots() []string {
	return []string{"rum-events-mobile-schema.json"}
}
func (*RumTimeseriesMemoryEvent) isEvent()              {}
func (*RumTimeseriesMemoryEvent) isRumTimeseriesEvent() {}

// EventType reports the value of the "type" discriminator for RumTimeseriesCpuEvent.
func (*RumTimeseriesCpuEvent) EventType() string { return "timeseries" }

// SchemaRoots reports the platform root schemas that list RumTimeseriesCpuEvent.
func (*RumTimeseriesCpuEvent) SchemaRoots() []string {
	return []string{"rum-events-mobile-schema.json"}
}
func (*RumTimeseriesCpuEvent) isEvent()              {}
func (*RumTimeseriesCpuEvent) isRumTimeseriesEvent() {}

// eventVariants lists every concrete top-level type in schema order together with the
// discriminator constraints that select it. A constraint on an optional path matches when the path
// is absent; a constraint on a required path does not. The first matching variant wins.
var eventVariants = []jsonx.Variant{
	{Name: "RumActionEvent", New: func() any { return new(RumActionEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"action"}, Required: true}}},
	{Name: "RumTransitionEvent", New: func() any { return new(RumTransitionEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"transition"}, Required: true}}},
	{Name: "RumErrorEvent", New: func() any { return new(RumErrorEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"error"}, Required: true}}},
	{Name: "RumLongTaskEvent", New: func() any { return new(RumLongTaskEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"long_task"}, Required: true}}},
	{Name: "RumResourceEvent", New: func() any { return new(RumResourceEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"resource"}, Required: true}}},
	{Name: "RumViewEvent", New: func() any { return new(RumViewEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"view"}, Required: true}}},
	{Name: "RumViewUpdateEvent", New: func() any { return new(RumViewUpdateEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"view_update"}, Required: true}}},
	{Name: "RumVitalDurationEvent", New: func() any { return new(RumVitalDurationEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"vital"}, Required: true}, {Path: []string{"vital", "type"}, Values: []any{"duration"}, Required: true}}},
	{Name: "RumVitalOperationStepEvent", New: func() any { return new(RumVitalOperationStepEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"vital"}, Required: true}, {Path: []string{"vital", "type"}, Values: []any{"operation_step"}, Required: true}}},
	{Name: "RumVitalAppLaunchEvent", New: func() any { return new(RumVitalAppLaunchEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"vital"}, Required: true}, {Path: []string{"vital", "type"}, Values: []any{"app_launch"}, Required: true}}},
	{Name: "TelemetryErrorEvent", New: func() any { return new(TelemetryErrorEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"telemetry"}, Required: true}, {Path: []string{"telemetry", "type"}, Values: []any{"log"}, Required: false}, {Path: []string{"telemetry", "status"}, Values: []any{"error"}, Required: true}}},
	{Name: "TelemetryDebugEvent", New: func() any { return new(TelemetryDebugEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"telemetry"}, Required: true}, {Path: []string{"telemetry", "type"}, Values: []any{"log"}, Required: false}, {Path: []string{"telemetry", "status"}, Values: []any{"debug"}, Required: true}}},
	{Name: "TelemetryConfigurationEvent", New: func() any { return new(TelemetryConfigurationEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"telemetry"}, Required: true}, {Path: []string{"telemetry", "type"}, Values: []any{"configuration"}, Required: true}}},
	{Name: "TelemetryUsageEvent", New: func() any { return new(TelemetryUsageEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"telemetry"}, Required: true}, {Path: []string{"telemetry", "type"}, Values: []any{"usage"}, Required: true}}},
	{Name: "RumTimeseriesMemoryEvent", New: func() any { return new(RumTimeseriesMemoryEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"timeseries"}, Required: true}, {Path: []string{"timeseries", "name"}, Values: []any{"memory"}, Required: true}}},
	{Name: "RumTimeseriesCpuEvent", New: func() any { return new(RumTimeseriesCpuEvent) }, Match: []jsonx.Discriminator{{Path: []string{"type"}, Values: []any{"timeseries"}, Required: true}, {Path: []string{"timeseries", "name"}, Values: []any{"cpu"}, Required: true}}},
}
