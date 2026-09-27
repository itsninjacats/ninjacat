<script lang="ts" module>
	export type ChartLine = {
		key: string;
		label: string;
		color: string;
		points: { t: number; v: number }[];
		band: { t: number; lower: number; upper: number }[];
		forecast: { t: number; v: number; lower: number; upper: number }[];
	};
</script>

<script lang="ts">
	import * as Chart from '$lib/components/ui/chart';
	import { Area, AreaChart, BarChart, LineChart, Spline } from 'layerchart';

	let {
		lines,
		display,
		window
	}: {
		lines: ChartLine[];
		display: 'line' | 'area' | 'bars';
		/** The requested range in ms: the x axis spans all of it, not just where data is. */
		window: { from: number; to: number };
	} = $props();

	// The bucket width, read off the data: the API does not send it.
	const stepMs = $derived.by(() => {
		let step = Infinity;
		for (const l of lines) {
			for (let i = 1; i < l.points.length; i++) {
				step = Math.min(step, l.points[i].t - l.points[i - 1].t);
			}
		}
		return Number.isFinite(step) && step > 0 ? step : undefined;
	});

	type Row = Record<string, number | Date | undefined> & { time: Date };

	// LayerChart wants one row per x value. Each line owns a few columns in it:
	// its value, its anomaly band (_lo, _hi) and its forecast (_f, _flo, _fhi).
	const rows = $derived.by(() => {
		const byTime: Record<number, Row> = {};
		const row = (t: number) => (byTime[t] ??= { time: new Date(t) });
		// Bars sit on a band scale, one slot per row, so empty buckets need rows
		// too — or three buckets of data would stretch across a whole week.
		const first = lines.find((l) => l.points.length > 0)?.points[0].t;
		if (display === 'bars' && stepMs && first !== undefined) {
			const start = first - Math.floor((first - window.from) / stepMs) * stepMs;
			for (let t = start; t <= window.to; t += stepMs) row(t);
		}
		for (const l of lines) {
			for (const p of l.points) row(p.t)[l.key] = p.v;
			for (const b of l.band) {
				row(b.t)[`${l.key}_lo`] = b.lower;
				row(b.t)[`${l.key}_hi`] = b.upper;
			}
			for (const f of l.forecast) {
				row(f.t)[`${l.key}_f`] = f.v;
				row(f.t)[`${l.key}_flo`] = f.lower;
				row(f.t)[`${l.key}_fhi`] = f.upper;
			}
		}
		return Object.values(byTime).sort((a, b) => a.time.getTime() - b.time.getTime());
	});

	const series = $derived(lines.map((l) => ({ key: l.key, label: l.label, color: l.color })));

	const config = $derived(
		Object.fromEntries(
			lines.map((l) => [l.key, { label: l.label, color: l.color }])
		) as Chart.ChartConfig
	);

	// The chart sizes its y axis from the series alone; bands and forecasts
	// reach further, so the domain is taken over everything drawn.
	const yDomain = $derived.by(() => {
		let min = Infinity;
		let max = -Infinity;
		for (const l of lines) {
			for (const p of l.points) [min, max] = [Math.min(min, p.v), Math.max(max, p.v)];
			for (const b of l.band) [min, max] = [Math.min(min, b.lower), Math.max(max, b.upper)];
			for (const f of l.forecast) [min, max] = [Math.min(min, f.lower), Math.max(max, f.upper)];
		}
		if (!Number.isFinite(min)) return undefined;
		if (min === max) return [min - 1, max + 1];
		return [min, max];
	});

	const value = (key: string) => (d: Row) => d[key] as number;
	const has = (key: string) => (d: Row) => d[key] !== undefined;

	// Line and area charts are on a time scale: show the whole window, and a
	// forecast past its end.
	const xDomain = $derived.by(() => {
		const forecastEnd = Math.max(window.to, ...lines.flatMap((l) => l.forecast.map((f) => f.t)));
		return [new Date(window.from), new Date(forecastEnd)];
	});

	const longWindow = $derived(window.to - window.from > 86_400_000);

	const timeFormat = (d: Date) =>
		longWindow
			? d.toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
			: d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

	// Time labels are wide; about eight of them fit without overlapping.
	const timeAxis = $derived({ format: timeFormat, ticks: longWindow ? 6 : 8 });

	const tooltipTime = (v: unknown) =>
		v instanceof Date
			? v.toLocaleString([], {
					month: 'short',
					day: 'numeric',
					hour: '2-digit',
					minute: '2-digit',
					second: '2-digit'
				})
			: String(v);

	// 11 771 153 846 does not fit beside the chart; 11.8B does. English on
	// purpose: other locales spell the suffix out ("11,8 mld") and get clipped.
	const compact = new Intl.NumberFormat('en', {
		notation: 'compact',
		maximumFractionDigits: 1
	});
	const valueAxis = { format: (v: number) => compact.format(v) };

	// A band scale would label every bar; keep about eight.
	const barTicks = (scale: { domain: () => unknown[] }) => {
		const all = scale.domain();
		const every = Math.max(1, Math.ceil(all.length / 8));
		return all.filter((_, i) => i % every === 0);
	};
</script>

<Chart.Container {config} class="h-[320px] w-full">
	{#if display === 'line'}
		<LineChart
			data={rows}
			x="time"
			{xDomain}
			{series}
			{yDomain}
			props={{ xAxis: timeAxis, yAxis: valueAxis }}
		>
			{#snippet marks()}
				{#each lines as l (l.key)}
					{#if l.band.length > 0}
						<Area
							y0={value(`${l.key}_lo`)}
							y1={value(`${l.key}_hi`)}
							defined={has(`${l.key}_lo`)}
							fill={() => l.color}
							style="opacity: 0.15"
						/>
					{/if}
					{#if l.forecast.length > 0}
						<Area
							y0={value(`${l.key}_flo`)}
							y1={value(`${l.key}_fhi`)}
							defined={has(`${l.key}_flo`)}
							fill={() => l.color}
							style="opacity: 0.12"
						/>
						<Spline
							y={value(`${l.key}_f`)}
							defined={has(`${l.key}_f`)}
							stroke={() => l.color}
							stroke-width={2}
							stroke-dasharray="4 4"
						/>
					{/if}
					<Spline seriesKey={l.key} defined={has(l.key)} stroke-width={2} />
				{/each}
			{/snippet}
			{#snippet tooltip()}
				<Chart.Tooltip labelFormatter={tooltipTime} />
			{/snippet}
		</LineChart>
	{:else if display === 'area'}
		<AreaChart
			data={rows}
			x="time"
			{xDomain}
			{series}
			seriesLayout="overlap"
			props={{
				xAxis: timeAxis,
				yAxis: valueAxis,
				area: { opacity: 0.25, line: { 'stroke-width': 2 } }
			}}
		>
			{#snippet tooltip()}
				<Chart.Tooltip labelFormatter={tooltipTime} />
			{/snippet}
		</AreaChart>
	{:else}
		<BarChart
			data={rows}
			x="time"
			{series}
			seriesLayout="stack"
			props={{
				xAxis: { ticks: barTicks, format: timeFormat },
				yAxis: valueAxis,
				bars: { stroke: 'none' }
			}}
		>
			{#snippet tooltip()}
				<Chart.Tooltip labelFormatter={tooltipTime} />
			{/snippet}
		</BarChart>
	{/if}
</Chart.Container>

<div class="flex flex-wrap gap-x-4 gap-y-1.5 border-t pt-3">
	{#each lines as l (l.key)}
		<span class="flex items-center gap-1.5 font-mono text-xs">
			<span class="inline-block size-2 shrink-0 rounded-full" style="background: {l.color}"></span>
			{l.label}
			{#if l.band.length > 0}<span class="text-muted-foreground">· expected range</span>{/if}
			{#if l.forecast.length > 0}<span class="text-muted-foreground">· forecast (dashed)</span>{/if}
		</span>
	{/each}
</div>
