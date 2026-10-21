<script lang="ts">
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as NativeSelect from '$lib/components/ui/native-select';
	import NumberInput from './number-input.svelte';
	import OptionSelect from './option-select.svelte';
	import {
		ANOMALY_ALGORITHMS,
		COMPARATORS,
		DIRECTIONS,
		FORECAST_ALGORITHMS,
		FORECAST_MODELS,
		HISTORIES,
		HORIZONS,
		MISSING,
		OUTLIER_ALGORITHMS,
		SEASONALITIES,
		SHIFTS,
		TIME_AGGREGATORS,
		WINDOWS,
		type Missing,
		type MonitorView
	} from '$lib/monitors/definition';
	import { m } from '$lib/paraglide/messages';

	let { view = $bindable(), multi }: { view: MonitorView; multi: boolean } = $props();

	// A change looks at most two days back.
	const changeWindows = WINDOWS.slice(0, WINDOWS.indexOf('last_2d') + 1);
	const alertWindows = WINDOWS.slice(1, WINDOWS.indexOf('last_1w') + 1);

	const missingLabel: Record<Missing, () => string> = {
		keep: m.monitor_missing_keep,
		notify: m.monitor_missing_notify,
		default: m.monitor_missing_default,
		show_no_data: m.monitor_missing_show_no_data,
		show_and_notify_no_data: m.monitor_missing_show_and_notify_no_data,
		resolve: m.monitor_missing_resolve
	};

	const fractions = $derived(view.method === 'anomaly');
	// Anomalies and forecasts each have one; the field shows the current method's.
	const timezone = {
		get: () => (view.method === 'anomaly' ? view.anomaly.timezone : view.forecast.timezone),
		set: (zone: string) => {
			if (view.method === 'anomaly') view.anomaly.timezone = zone;
			else view.forecast.timezone = zone;
		}
	};

	const check = $derived(view.method === 'check');
	// A check has no on_missing_data.
	const missingOptions = $derived(check ? (['keep', 'notify'] as const) : MISSING);

	const comparators = $derived(view.method === 'forecast' ? (['>=', '<='] as const) : COMPARATORS);
</script>

<div class="@container space-y-6">
	<!-- what is evaluated -->
	<div class="grid grid-cols-1 gap-3 @md:grid-cols-2 @3xl:grid-cols-4" class:hidden={check}>
		{#if view.method === 'change'}
			<div class="space-y-1">
				<Label for="monitor-measure">{m.monitor_change_measure()}</Label>
				<NativeSelect.Root
					id="monitor-measure"
					bind:value={view.change.measure}
					size="sm"
					class="w-full [&>select]:text-xs"
				>
					<NativeSelect.Option value="change">{m.monitor_change_absolute()}</NativeSelect.Option>
					<NativeSelect.Option value="pct_change">{m.monitor_change_percent()}</NativeSelect.Option>
				</NativeSelect.Root>
			</div>
		{/if}

		{#if view.method !== 'forecast'}
			<div class="space-y-1">
				<Label for="monitor-aggregator">{m.monitor_aggregator()}</Label>
				<OptionSelect
					id="monitor-aggregator"
					bind:value={view.aggregator}
					options={TIME_AGGREGATORS}
				/>
			</div>
			<div class="space-y-1">
				<Label for="monitor-window">
					{view.method === 'anomaly' ? m.monitor_graph_window() : m.monitor_window()}
				</Label>
				<OptionSelect
					id="monitor-window"
					bind:value={view.window}
					options={view.method === 'change' ? changeWindows : WINDOWS}
				/>
			</div>
		{/if}

		{#if view.method === 'change'}
			<div class="space-y-1">
				<Label for="monitor-shift">{m.monitor_change_shift()}</Label>
				<OptionSelect id="monitor-shift" bind:value={view.change.shift} options={SHIFTS} />
			</div>
		{/if}

		{#if view.method === 'anomaly'}
			<div class="space-y-1">
				<Label for="monitor-anomaly-algorithm">{m.monitor_algorithm()}</Label>
				<OptionSelect
					id="monitor-anomaly-algorithm"
					bind:value={view.anomaly.algorithm}
					options={ANOMALY_ALGORITHMS}
				/>
			</div>
			<div class="space-y-1">
				<Label for="monitor-bounds">{m.monitor_anomaly_bounds()}</Label>
				<NumberInput id="monitor-bounds" bind:value={view.anomaly.bounds} min="0" step="0.5" />
			</div>
			<div class="space-y-1">
				<Label for="monitor-direction">{m.monitor_anomaly_direction()}</Label>
				<NativeSelect.Root
					id="monitor-direction"
					bind:value={view.anomaly.direction}
					size="sm"
					class="w-full [&>select]:text-xs"
				>
					{#each DIRECTIONS as d (d)}
						<NativeSelect.Option value={d}>
							{d === 'both'
								? m.monitor_direction_both()
								: d === 'above'
									? m.monitor_direction_above()
									: m.monitor_direction_below()}
						</NativeSelect.Option>
					{/each}
				</NativeSelect.Root>
			</div>
			{#if view.anomaly.algorithm !== 'basic'}
				<div class="space-y-1">
					<Label for="monitor-anomaly-seasonality">{m.monitor_seasonality()}</Label>
					<OptionSelect
						id="monitor-anomaly-seasonality"
						bind:value={view.anomaly.seasonality}
						options={SEASONALITIES}
					/>
				</div>
			{/if}
			<div class="space-y-1">
				<Label for="monitor-alert-window">{m.monitor_alert_window()}</Label>
				<OptionSelect
					id="monitor-alert-window"
					bind:value={view.anomaly.alertWindow}
					options={alertWindows}
				/>
			</div>
			<div class="space-y-1">
				<Label for="monitor-recovery-window">{m.monitor_recovery_window()}</Label>
				<OptionSelect
					id="monitor-recovery-window"
					bind:value={view.anomaly.recoveryWindow}
					options={alertWindows}
				/>
			</div>
			<div class="space-y-1">
				<Label for="monitor-anomaly-interval">{m.monitor_interval()}</Label>
				<NumberInput
					id="monitor-anomaly-interval"
					bind:value={view.anomaly.interval}
					min="1"
					placeholder={m.monitor_interval_auto()}
				/>
			</div>
			<div class="flex items-center gap-2 self-end pb-1.5">
				<Checkbox id="monitor-count-default-zero" bind:checked={view.anomaly.countDefaultZero} />
				<Label for="monitor-count-default-zero" class="text-xs font-normal">
					{m.monitor_count_default_zero()}
				</Label>
			</div>
		{/if}

		{#if view.method === 'forecast'}
			<div class="space-y-1">
				<Label for="monitor-forecast-algorithm">{m.monitor_algorithm()}</Label>
				<OptionSelect
					id="monitor-forecast-algorithm"
					bind:value={view.forecast.algorithm}
					options={FORECAST_ALGORITHMS}
				/>
			</div>
			<div class="space-y-1">
				<Label for="monitor-deviations">{m.monitor_forecast_deviations()}</Label>
				<NumberInput
					id="monitor-deviations"
					bind:value={view.forecast.deviations}
					min="1"
					step="0.5"
				/>
			</div>
			<div class="space-y-1">
				<Label for="monitor-horizon">{m.monitor_forecast_horizon()}</Label>
				<OptionSelect id="monitor-horizon" bind:value={view.forecast.horizon} options={HORIZONS} />
			</div>
			{#if view.forecast.algorithm === 'linear'}
				<div class="space-y-1">
					<Label for="monitor-model">{m.monitor_forecast_model()}</Label>
					<OptionSelect
						id="monitor-model"
						bind:value={view.forecast.model}
						options={FORECAST_MODELS}
					/>
				</div>
				<div class="space-y-1">
					<Label for="monitor-history">{m.monitor_forecast_history()}</Label>
					<OptionSelect
						id="monitor-history"
						bind:value={view.forecast.history}
						options={HISTORIES}
					/>
				</div>
			{:else}
				<div class="space-y-1">
					<Label for="monitor-forecast-seasonality">{m.monitor_seasonality()}</Label>
					<OptionSelect
						id="monitor-forecast-seasonality"
						bind:value={view.forecast.seasonality}
						options={SEASONALITIES}
					/>
				</div>
			{/if}
			<div class="space-y-1">
				<Label for="monitor-forecast-interval">{m.monitor_interval()}</Label>
				<NumberInput
					id="monitor-forecast-interval"
					bind:value={view.forecast.interval}
					min="1"
					placeholder={m.monitor_interval_auto()}
				/>
			</div>
		{/if}

		{#if view.method === 'outlier'}
			<div class="space-y-1">
				<Label for="monitor-outlier-algorithm">{m.monitor_algorithm()}</Label>
				<OptionSelect
					id="monitor-outlier-algorithm"
					bind:value={view.outlier.algorithm}
					options={OUTLIER_ALGORITHMS}
				/>
			</div>
			<div class="space-y-1">
				<Label for="monitor-tolerance">{m.monitor_outlier_tolerance()}</Label>
				<NumberInput
					id="monitor-tolerance"
					bind:value={view.outlier.tolerance}
					min="0"
					step="0.5"
				/>
			</div>
			{#if view.outlier.algorithm.endsWith('mad')}
				<div class="space-y-1">
					<Label for="monitor-percentage">{m.monitor_outlier_percentage()}</Label>
					<NumberInput
						id="monitor-percentage"
						bind:value={view.outlier.percentage}
						min="0"
						max="100"
					/>
				</div>
			{/if}
		{/if}
	</div>

	{#if view.method === 'anomaly' || view.method === 'forecast'}
		<div class="grid grid-cols-1 gap-1 @md:grid-cols-2">
			<div class="space-y-1">
				<Label for="monitor-timezone">{m.monitor_timezone()}</Label>
				<Input
					id="monitor-timezone"
					bind:value={timezone.get, timezone.set}
					placeholder="Europe/Warsaw"
					class="h-8 font-mono text-xs md:text-xs"
				/>
			</div>
			<p class="self-end text-xs text-muted-foreground @md:pb-1">
				{m.monitor_timezone_deviation()}
			</p>
		</div>
	{/if}

	<!-- thresholds -->
	<div class="space-y-2">
		<h3 class="text-sm font-medium">{m.monitor_thresholds()}</h3>
		{#if view.method === 'outlier'}
			<p class="text-xs text-muted-foreground">{m.monitor_outlier_explained()}</p>
		{:else if check}
			<p class="text-xs text-muted-foreground">{m.monitor_check_explained()}</p>
			<div class="grid grid-cols-1 gap-3 @md:grid-cols-2 @3xl:grid-cols-4">
				<div class="space-y-1">
					<Label for="monitor-critical">{m.monitor_check_critical()}</Label>
					<NumberInput id="monitor-critical" bind:value={view.critical} min="1" max="100" />
					<p class="text-xs text-muted-foreground">{m.monitor_check_critical_hint()}</p>
				</div>
				<div class="space-y-1">
					<Label for="monitor-warning">{m.monitor_check_warning()}</Label>
					<NumberInput
						id="monitor-warning"
						bind:value={view.warning}
						min="1"
						max="100"
						placeholder="1"
					/>
					<p class="text-xs text-muted-foreground">{m.monitor_check_warning_hint()}</p>
				</div>
				<div class="space-y-1">
					<Label for="monitor-check-ok">{m.monitor_check_recover()}</Label>
					<NumberInput
						id="monitor-check-ok"
						bind:value={view.check.ok}
						min="1"
						max="100"
						placeholder="1"
					/>
					<p class="text-xs text-muted-foreground">{m.monitor_check_recover_hint()}</p>
				</div>
				<div class="space-y-1">
					<Label for="monitor-check-unknown">{m.monitor_check_unknown()}</Label>
					<NumberInput
						id="monitor-check-unknown"
						bind:value={view.check.unknown}
						min="1"
						max="100"
					/>
					<p class="text-xs text-muted-foreground">{m.monitor_check_unknown_hint()}</p>
				</div>
			</div>
		{:else}
			{#if fractions}
				<p class="text-xs text-muted-foreground">{m.monitor_anomaly_thresholds_explained()}</p>
			{/if}
			<div class="grid grid-cols-1 gap-3 @md:grid-cols-2 @3xl:grid-cols-5">
				{#if !fractions}
					<div class="space-y-1">
						<Label for="monitor-comparator">{m.monitor_comparator()}</Label>
						<OptionSelect
							id="monitor-comparator"
							bind:value={view.comparator}
							options={comparators}
						/>
					</div>
				{/if}
				<div class="space-y-1">
					<Label for="monitor-critical">{m.monitor_critical()}</Label>
					<NumberInput
						id="monitor-critical"
						bind:value={view.critical}
						step="any"
						min={fractions ? '0' : undefined}
						max={fractions ? '1' : undefined}
					/>
				</div>
				<div class="space-y-1">
					<Label for="monitor-warning">{m.monitor_warning()}</Label>
					<NumberInput id="monitor-warning" bind:value={view.warning} step="any" />
				</div>
				<div class="space-y-1">
					<Label for="monitor-critical-recovery">{m.monitor_critical_recovery()}</Label>
					<NumberInput
						id="monitor-critical-recovery"
						bind:value={view.criticalRecovery}
						step="any"
					/>
				</div>
				{#if !fractions}
					<div class="space-y-1">
						<Label for="monitor-warning-recovery">{m.monitor_warning_recovery()}</Label>
						<NumberInput
							id="monitor-warning-recovery"
							bind:value={view.warningRecovery}
							step="any"
						/>
					</div>
				{/if}
			</div>
		{/if}
	</div>

	<!-- missing data -->
	<div class="grid grid-cols-1 gap-3 @md:grid-cols-2">
		<div class="space-y-1">
			<Label for="monitor-missing">{m.monitor_missing()}</Label>
			<NativeSelect.Root
				id="monitor-missing"
				bind:value={view.missing}
				size="sm"
				class="w-full [&>select]:text-xs"
			>
				{#each missingOptions as option (option)}
					<NativeSelect.Option value={option}>{missingLabel[option]()}</NativeSelect.Option>
				{/each}
			</NativeSelect.Root>
		</div>
		{#if view.missing === 'notify'}
			<div class="space-y-1">
				<Label for="monitor-no-data-timeframe">{m.monitor_no_data_timeframe()}</Label>
				<NumberInput
					id="monitor-no-data-timeframe"
					bind:value={view.noDataTimeframe}
					min="1"
					placeholder={check
						? m.monitor_no_data_timeframe_default_check()
						: m.monitor_no_data_timeframe_default()}
				/>
			</div>
		{/if}
	</div>

	<!-- advanced -->
	<div class="space-y-2">
		<h3 class="text-sm font-medium">{m.monitor_advanced()}</h3>
		<div class="grid grid-cols-1 gap-3 @md:grid-cols-2 @3xl:grid-cols-4">
			<div class="space-y-1">
				<Label for="monitor-evaluation-delay">{m.monitor_evaluation_delay()}</Label>
				<NumberInput
					id="monitor-evaluation-delay"
					bind:value={view.evaluationDelay}
					min="0"
					max="86400"
					placeholder="0"
				/>
			</div>
			{#if multi}
				<div class="space-y-1">
					<Label for="monitor-new-group-delay">{m.monitor_new_group_delay()}</Label>
					<NumberInput
						id="monitor-new-group-delay"
						bind:value={view.newGroupDelay}
						min="0"
						max="86400"
						placeholder="60"
					/>
				</div>
			{/if}
			<div class="space-y-1">
				<Label for="monitor-timeout">{m.monitor_timeout()}</Label>
				<NumberInput
					id="monitor-timeout"
					bind:value={view.timeoutH}
					min="0"
					max="24"
					placeholder={m.monitor_timeout_never()}
				/>
			</div>
			{#if !check}
				<div class="flex items-center gap-2 self-end pb-1.5">
					<Checkbox id="monitor-full-window" bind:checked={view.requireFullWindow} />
					<Label for="monitor-full-window" class="text-xs font-normal">
						{m.monitor_require_full_window()}
					</Label>
				</div>
			{/if}
		</div>
	</div>
</div>
