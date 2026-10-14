// How a monitor's states, reasons and methods are shown.

import { m } from '$lib/paraglide/messages';
import type { ChartRule } from '$lib/components/app/metrics/metrics-chart.svelte';
import type { Method } from './definition';

/** Datadog's group states, most severe first: a monitor shows its worst group's. */
export const STATES = ['Alert', 'Warn', 'No Data', 'OK'] as const;
export type State = (typeof STATES)[number];

/** For sorting, most severe first; any other state, null among them, comes last. */
export function severity(state: string | null): number {
	const i = STATES.indexOf(state as State);
	return i === -1 ? STATES.length : i;
}

/** null: a monitor with no open group ("no groups found"); none: a group before its first transition. */
export function stateLabel(state: string | null): string {
	switch (state) {
		case 'Alert':
			return m.monitor_state_alert();
		case 'Warn':
			return m.monitor_state_warn();
		case 'No Data':
			return m.monitor_state_no_data();
		case 'OK':
			return m.monitor_state_ok();
		case null:
			return m.monitor_state_no_groups();
		default:
			return m.monitor_state_none();
	}
}

// Whole class names, so Tailwind finds them in the source.
const BACKGROUNDS: Record<string, string> = {
	Alert: 'bg-state-alert',
	Warn: 'bg-state-warn',
	'No Data': 'bg-state-no-data',
	OK: 'bg-state-ok'
};

export function stateClass(state: string | null): string {
	return (state && BACKGROUNDS[state]) || 'bg-muted-foreground/30';
}

export function methodLabel(method: Method | null): string {
	switch (method) {
		case 'threshold':
			return m.monitor_method_threshold();
		case 'change':
			return m.monitor_method_change();
		case 'anomaly':
			return m.monitor_method_anomaly();
		case 'outlier':
			return m.monitor_method_outlier();
		case 'forecast':
			return m.monitor_method_forecast();
		default:
			return m.monitor_method_unknown();
	}
}

export function reasonLabel(reason: string): string {
	switch (reason) {
		case 'threshold':
			return m.monitor_reason_threshold();
		case 'recovery':
			return m.monitor_reason_recovery();
		case 'no_data':
			return m.monitor_reason_no_data();
		case 'no_data_shown':
			return m.monitor_reason_no_data_shown();
		case 'timeout':
			return m.monitor_reason_timeout();
		case 'expired':
			return m.monitor_reason_expired();
		case 'definition_changed':
			return m.monitor_reason_definition_changed();
		case 'deleted':
			return m.monitor_reason_deleted();
		case 'new':
			return m.monitor_reason_new();
		case 'resolved':
			return m.monitor_reason_resolved();
		default:
			return reason;
	}
}

export function thresholdRules(t: {
	critical: number | null;
	warning: number | null;
	criticalRecovery: number | null;
	warningRecovery: number | null;
}): ChartRule[] {
	const rules: ChartRule[] = [];
	const add = (value: number | null, label: string, color: string, dashed = false) => {
		if (value !== null) rules.push({ value, label, color, dashed });
	};
	add(t.critical, m.monitor_critical(), 'var(--state-alert)');
	add(t.warning, m.monitor_warning(), 'var(--state-warn)');
	add(t.criticalRecovery, m.monitor_critical_recovery(), 'var(--state-alert)', true);
	add(t.warningRecovery, m.monitor_warning_recovery(), 'var(--state-warn)', true);
	return rules;
}
