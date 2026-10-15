<script lang="ts" module>
	export type ChartLine = {
		key: string;
		label: string;
		color: string;
		points: { t: number; v: number }[];
		band: { t: number; lower: number; upper: number }[];
		forecast: { t: number; v: number; lower: number; upper: number }[];
	};

	/** A horizontal line across the chart, such as a monitor's threshold. */
	export type ChartRule = { value: number; label: string; color: string; dashed?: boolean };

	/** The values a monitor alerts on, drawn in its alert colour; null runs to the chart's edge. */
	export type ChartZone = { from: number | null; to: number | null };

	/** A series keeps its colour by its place in the query's answer, not in a sorted list. */
	export const seriesColor = (i: number) => `var(--chart-${(i % 8) + 1})`;
</script>

<script lang="ts">
	import { onMount } from 'svelte';
	import uPlot from 'uplot';
	import 'uplot/dist/uPlot.min.css';
	import { formatValue } from '$lib/metrics/format';
	import { m } from '$lib/paraglide/messages';

	let {
		lines,
		display,
		window,
		rules = [],
		zone = null,
		onzoom,
		onreset
	}: {
		lines: ChartLine[];
		display: 'line' | 'area' | 'bars';
		/** The requested range in ms: the x axis spans all of it, not just where data is. */
		window: { from: number; to: number };
		/** Drawn on line charts only. */
		rules?: ChartRule[];
		zone?: ChartZone | null;
		/** Called with the range dragged across, in ms; without it the chart cannot be dragged. */
		onzoom?: (from: number, to: number) => void;
		/** Called on a double-click, to undo a zoom; given only while zoomed. */
		onreset?: () => void;
	} = $props();

	const HEIGHT = 320;
	const LEGEND_ROWS = 10;
	const TOOLTIP_ROWS = 8;
	// d3's 1, 2, 5 steps: uPlot's own 2.5 steps round to wrong labels.
	const INCRS = Array.from({ length: 40 }, (_, e) =>
		[1, 2, 5].map((m) => m * 10 ** (e - 20))
	).flat();

	let el: HTMLDivElement;
	let probe: HTMLSpanElement;
	let plot: uPlot | null = null;
	let plotShape = '';
	/** For each uPlot series, the line it draws; null for the helpers behind bands and forecasts. */
	let owner: (number | null)[] = [];
	let hover = $state<{ idx: number; left: number; top: number } | null>(null);
	/** The line under the cursor or the legend's pointer. */
	let focused = $state<number | null>(null);
	let tooltipWidth = $state(0);

	// The bucket width, read off the data: the API does not send it. The median,
	// not the least: a monitor whose frequency changed has two, and the smaller
	// would break the line at every interval of the larger.
	const stepMs = $derived.by(() => {
		const ds: number[] = [];
		for (const l of lines) {
			for (let i = 1; i < l.points.length; i++) {
				// Not every caller sorts its points.
				const d = Math.abs(l.points[i].t - l.points[i - 1].t);
				if (d > 0) ds.push(d);
			}
		}
		return ds.length > 0 ? Float64Array.from(ds).sort()[ds.length >> 1] : undefined;
	});

	// uPlot wants one x array shared by every series. A line with no point at
	// an x gets null there, which uPlot draws as a gap.
	const table = $derived.by(() => {
		const times: number[] = [];
		const first = lines.find((l) => l.points.length > 0)?.points[0].t;
		// Empty buckets get an x too, so bars keep their width across gaps.
		if (display === 'bars' && stepMs && first !== undefined) {
			const start = first - Math.floor((first - window.from) / stepMs) * stepMs;
			for (let t = start; t <= window.to; t += stepMs) times.push(t);
		}
		for (const l of lines) {
			for (const p of l.points) times.push(p.t);
			for (const b of l.band) times.push(b.t);
			for (const f of l.forecast) times.push(f.t);
		}
		times.sort((a, b) => a - b);
		const xs: number[] = [];
		for (const t of times) {
			const last = xs[xs.length - 1];
			if (t === last) continue;
			// A stretch where no line has a point gets one x with nothing at it, so
			// the lines break there instead of joining across.
			if (stepMs && t - last > 1.5 * stepMs) xs.push((t + last) / 2);
			xs.push(t);
		}
		const at = new Map(xs.map((t, i) => [t, i]));
		const column = <T extends { t: number }>(items: T[], value: (item: T) => number) => {
			const c: (number | null)[] = new Array(xs.length).fill(null);
			for (const item of items) c[at.get(item.t)!] = value(item);
			return c;
		};
		return { xs, column };
	});

	const values = $derived(lines.map((l) => table.column(l.points, (p) => p.v)));

	// The columns in the order options() lays out the series.
	const plotData = $derived.by((): uPlot.AlignedData => {
		const { xs, column } = table;
		if (display === 'area') return [xs, ...values];
		if (display === 'bars') {
			// Stacked by hand: each bar reaches the running total and the tallest is
			// drawn first, so the ones below it stay visible.
			const sums = new Array(xs.length).fill(0);
			const stacked = values.map((vs) => vs.map((v, i) => (v === null ? null : (sums[i] += v))));
			return [xs, ...stacked.toReversed()];
		}
		const columns: (number | null)[][] = [];
		lines.forEach((l, i) => {
			columns.push(values[i]);
			if (l.band.length > 0) {
				columns.push(
					column(l.band, (b) => b.lower),
					column(l.band, (b) => b.upper)
				);
			}
			if (l.forecast.length > 0) {
				columns.push(
					column(l.forecast, (f) => f.v),
					column(l.forecast, (f) => f.lower),
					column(l.forecast, (f) => f.upper)
				);
			}
		});
		return [xs, ...columns];
	});

	const xDomain = $derived.by((): [number, number] => {
		const xs = table.xs;
		if (display === 'bars' && xs.length > 0) {
			// One bucket has no step to read: centre it in the window instead.
			const half = (stepMs ?? window.to - window.from) / 2;
			return [xs[0] - half, xs[xs.length - 1] + half];
		}
		const forecastEnd = Math.max(window.to, ...lines.flatMap((l) => l.forecast.map((f) => f.t)));
		return [window.from, forecastEnd];
	});

	// Over everything drawn: bands, forecasts and rules reach past the series.
	const yDomain = $derived.by((): [number, number] => {
		let min = display === 'line' ? Infinity : 0;
		let max = display === 'line' ? -Infinity : 0;
		for (let c = 1; c < plotData.length; c++) {
			for (const v of plotData[c]) {
				if (v === null || v === undefined) continue;
				if (v < min) min = v;
				if (v > max) max = v;
			}
		}
		if (display === 'line') {
			for (const r of rules) [min, max] = [Math.min(min, r.value), Math.max(max, r.value)];
		}
		if (!Number.isFinite(min)) return [0, 1];
		// From zero, so a small wobble does not read as a cliff.
		min = Math.min(min, 0);
		max = Math.max(max, 0);
		if (min === max) return [min - 1, max + 1];
		// Out to the next round number, so a peak clears the top edge and the top
		// gets a label.
		const step = INCRS.find((s) => s >= (max - min) / 5)!;
		return [
			min < 0 ? Math.floor(min / step - 0.05) * step : 0,
			Math.ceil(max / step + 0.05) * step
		];
	});

	// What the plot is built from; anything else is new data on the same plot.
	const shape = $derived(
		JSON.stringify({
			display,
			lines: lines.map((l) => [l.color, l.band.length > 0, l.forecast.length > 0])
		})
	);

	const longWindow = $derived(window.to - window.from > 86_400_000);

	const timeFormat = (d: Date) =>
		longWindow
			? d.toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
			: d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

	const tooltipTime = (t: number) =>
		new Date(t).toLocaleString([], {
			month: 'short',
			day: 'numeric',
			hour: '2-digit',
			minute: '2-digit',
			second: '2-digit'
		});

	// A canvas cannot read CSS variables, so a colour is resolved through an element.
	function resolve(color: string) {
		probe.style.color = color;
		return getComputedStyle(probe).color;
	}

	const faded = (color: string, percent: number) =>
		resolve(`color-mix(in oklch, ${color} ${percent}%, transparent)`);

	function options(): uPlot.Options {
		const font = `11px ${getComputedStyle(el).fontFamily}`;
		const muted = resolve('var(--muted-foreground)');
		const axis = { stroke: muted, font, ticks: { show: false } };

		const series: uPlot.Series[] = [{}];
		const hidden = { stroke: 'transparent', width: 0, points: { show: false } };
		const bands: uPlot.Band[] = [];
		owner = [null];
		const add = (line: number | null, ...s: uPlot.Series[]) => {
			series.push(...s);
			owner.push(...s.map(() => line));
		};

		if (display === 'bars') {
			const bars = uPlot.paths.bars!({ size: [0.6, Infinity] });
			for (let i = lines.length - 1; i >= 0; i--) {
				const c = resolve(lines[i].color);
				add(i, { stroke: c, fill: c, width: 0, paths: bars, points: { show: false } });
			}
		} else if (display === 'area') {
			lines.forEach((l, i) =>
				add(i, {
					stroke: resolve(l.color),
					fill: faded(l.color, 18),
					width: 1.5,
					points: { show: false }
				})
			);
		} else {
			lines.forEach((l, i) => {
				add(i, { stroke: resolve(l.color), width: 1.5, points: { show: false } });
				if (l.band.length > 0) {
					add(null, hidden, hidden);
					bands.push({ series: [series.length - 1, series.length - 2], fill: faded(l.color, 12) });
				}
				if (l.forecast.length > 0) {
					add(
						null,
						{ stroke: resolve(l.color), width: 1.5, dash: [4, 3], points: { show: false } },
						hidden,
						hidden
					);
					bands.push({ series: [series.length - 1, series.length - 2], fill: faded(l.color, 10) });
				}
			});
		}

		const surface = resolve('var(--background)');
		// A hand-stacked bar has no line nearest the cursor, and a lone line has
		// nothing to stand out from but its own band.
		const prox = display === 'bars' || lines.length < 2 ? -1 : 16;

		return {
			width: el.clientWidth,
			height: HEIGHT,
			ms: 1,
			legend: { show: false },
			// Dimming redraws every line on each change of focus, which past a hundred
			// lines lags the cursor; the tooltip still names the focused one.
			focus: { alpha: lines.length > 100 ? 1 : 0.25 },
			cursor: {
				x: true,
				y: false,
				drag: { x: !!onzoom, y: false, setScale: false, dist: 5 },
				// In place of uPlot's own, which rescales x to the data rather than the window.
				bind: {
					dblclick: () => () => {
						onreset?.();
						return null;
					}
				},
				// The line stands on the time the tooltip shows, not between two.
				move: (u, l, t) =>
					l < 0 || u.data[0].length === 0 ? [l, t] : [u.valToPos(u.data[0][u.posToIdx(l)], 'x'), t],
				focus: {
					prox,
					dist: (u, i, idx, valPos, curPos) => (owner[i] === null ? Infinity : valPos - curPos)
				},
				// uPlot takes a hover dot's colour from the drawn points, and none are drawn.
				points:
					display === 'bars'
						? { show: false }
						: {
								one: true,
								size: 7,
								width: 2,
								fill: (u, i) => series[i].stroke as string,
								stroke: (u, i) => (owner[i] === null ? 'transparent' : surface)
							}
			},
			// Room for the last time label, which uPlot would cut at the canvas edge.
			padding: [8, 32, 0, 0],
			// A function, not the array: uPlot reads an array once and keeps it.
			scales: { x: { time: true, range: () => xDomain }, y: { range: yDomain } },
			series,
			bands,
			axes: [
				{
					...axis,
					grid: { show: false },
					size: 28,
					// Time labels are wide: one per 100 px, or 150 px with the date, and
					// never more than eight.
					space: (u, i, min, max, dim) =>
						Math.max(longWindow ? 150 : 100, dim / (longWindow ? 6 : 8)),
					values: (u, splits) => splits.map((v) => timeFormat(new Date(v)))
				},
				{
					...axis,
					grid: { stroke: resolve('var(--border)'), width: 1 },
					space: 48,
					incrs: INCRS,
					values: (u, splits) => splits.map(formatValue)
				}
			],
			hooks: {
				drawAxes: [drawMarks],
				setSelect: [
					(u) => {
						// Only the requested window can be asked for again: not the forecast
						// past it, nor the half bucket the bars are padded with.
						const from = Math.max(window.from, Math.round(u.posToVal(u.select.left, 'x')));
						const to = Math.min(
							window.to,
							Math.round(u.posToVal(u.select.left + u.select.width, 'x'))
						);
						u.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);
						if (to > from) onzoom?.(from, to);
					}
				],
				setSeries: [
					(u, i) => {
						focused = i === null ? null : owner[i];
						// A line's band and forecast follow it, so they stay as bright as it does.
						if (i !== null) for (let j = i + 1; owner[j] === null; j++) u.series[j].alpha = 1;
					}
				],
				setCursor: [
					(u) => {
						const { idx, left, top } = u.cursor;
						hover =
							idx == null || left == null || top == null || left < 0
								? null
								: {
										idx,
										left: left + u.bbox.left / uPlot.pxRatio,
										top: top + u.bbox.top / uPlot.pxRatio
									};
					}
				]
			}
		};
	}

	// Beneath the series: the alert zone and the rules.
	function drawMarks(u: uPlot) {
		if (display !== 'line' || (!zone && rules.length === 0)) return;
		const { ctx } = u;
		const { left, top, width, height } = u.bbox;
		const px = uPlot.pxRatio;
		ctx.save();
		ctx.beginPath();
		ctx.rect(left, top, width, height);
		ctx.clip();
		if (zone) {
			const y1 = zone.to === null ? top : u.valToPos(zone.to, 'y', true);
			const y0 = zone.from === null ? top + height : u.valToPos(zone.from, 'y', true);
			ctx.fillStyle = faded('var(--state-alert)', 8);
			ctx.fillRect(left, Math.min(y0, y1), width, Math.abs(y0 - y1));
		}
		ctx.font = `500 ${10 * px}px ${getComputedStyle(el).fontFamily}`;
		// uPlot leaves the y axis's right alignment behind.
		ctx.textAlign = 'left';
		// A label too close to one already drawn would print over it: the first rules matter most.
		const labelled: number[] = [];
		for (const r of rules) {
			const y = u.valToPos(r.value, 'y', true);
			const color = resolve(r.color);
			ctx.strokeStyle = color;
			ctx.fillStyle = color;
			ctx.lineWidth = px;
			// A recovery is dotted, its threshold dashed.
			ctx.setLineDash(r.dashed ? [2 * px, 3 * px] : [6 * px, 4 * px]);
			ctx.beginPath();
			ctx.moveTo(left, y);
			ctx.lineTo(left + width, y);
			ctx.stroke();
			// A rule at the top edge would lose its label above the line; it goes below.
			const below = y - top < 14 * px;
			if (labelled.some((l) => Math.abs(l - y) < 12 * px)) continue;
			labelled.push(y);
			ctx.textBaseline = below ? 'top' : 'bottom';
			ctx.fillText(r.label, left + 4 * px, below ? y + 3 * px : y - 3 * px);
		}
		ctx.restore();
	}

	$effect(() => {
		const data = plotData;
		const [x0, x1] = xDomain;
		const [y0, y1] = yDomain;
		// drawMarks reads the zone when uPlot draws; a new one needs a redraw.
		void zone;
		if (plot && plotShape === shape) {
			const p = plot;
			p.batch(() => {
				p.setData(data, false);
				p.setScale('x', { min: x0, max: x1 });
				p.setScale('y', { min: y0, max: y1 });
			});
			return;
		}
		plot?.destroy();
		hover = null;
		focused = null;
		plot = new uPlot(options(), data, el);
		plotShape = shape;
	});

	onMount(() => {
		const resize = new ResizeObserver(() => {
			if (plot && plot.width !== el.clientWidth) {
				plot.setSize({ width: el.clientWidth, height: HEIGHT });
			}
		});
		resize.observe(el);
		return () => {
			resize.disconnect();
			plot?.destroy();
			plot = null;
		};
	});

	const hovered = $derived.by(() => {
		if (!hover || table.xs[hover.idx] === undefined) return null;
		const { idx, left, top } = hover;
		const t = table.xs[idx];
		// Bands and forecasts are drawn on line charts only.
		const line = display === 'line';
		const all: { i: number; value: number; forecast: boolean }[] = [];
		values.forEach((vs, i) => {
			const v = vs[idx];
			if (v !== null) all.push({ i, value: v, forecast: false });
			else if (line && lines[i].forecast.length > 0) {
				const f = lines[i].forecast.find((p) => p.t === t);
				if (f) all.push({ i, value: f.v, forecast: true });
			}
		});
		if (all.length === 0) return null;
		// Largest first, and only a few: the line being looked at always among them.
		all.sort((a, b) => b.value - a.value);
		const rows = all.slice(0, TOOLTIP_ROWS);
		const f = all.findIndex((r) => r.i === focused);
		if (f >= TOOLTIP_ROWS) rows[TOOLTIP_ROWS - 1] = all[f];
		// Kept inside the chart, which clips whatever sticks out.
		const x =
			left > el.clientWidth / 2
				? Math.max(0, left - tooltipWidth - 12)
				: Math.min(left + 12, el.clientWidth - tooltipWidth);
		const more = all.length - rows.length;
		// Searched for the shown rows alone: a grouped anomaly query can bring a thousand bands.
		const bounds = (r: (typeof all)[number]) => {
			const p = line && (r.forecast ? lines[r.i].forecast : lines[r.i].band).find((x) => x.t === t);
			return p ? [p.lower, p.upper] : null;
		};
		return {
			time: tooltipTime(table.xs[idx]),
			rows: rows.map((r) => ({ ...r, range: bounds(r) })),
			more,
			left: x,
			top: top + 12
		};
	});

	function emphasise(line: number | null) {
		plot?.setSeries(line === null ? null : owner.indexOf(line), { focus: true });
	}

	// Largest mean first, and only the first rows drawn until asked: a group-by
	// can return a thousand series, and their DOM costs more than the chart.
	const legend = $derived(
		lines
			.map((l, i) => ({
				l,
				i,
				mean:
					l.points.length === 0
						? -Infinity
						: l.points.reduce((s, p) => s + p.v, 0) / l.points.length
			}))
			.toSorted((a, b) => b.mean - a.mean)
	);
	let legendOpen = $state(false);
</script>

<div class="relative w-full text-xs">
	<div bind:this={el} class="chart h-[320px] w-full"></div>
	<span bind:this={probe} class="hidden"></span>
	{#if hovered}
		<div
			bind:clientWidth={tooltipWidth}
			class="pointer-events-none absolute z-10 max-w-[min(20rem,100%)] min-w-40 rounded-md border bg-popover px-2 py-1.5 text-[11px] text-popover-foreground shadow-sm"
			style:left="{hovered.left}px"
			style:top="{hovered.top}px"
		>
			<div class="mb-1 text-muted-foreground">{hovered.time}</div>
			{#each hovered.rows as r (r.i)}
				<div
					class="flex items-center gap-2 {r.i === focused
						? 'font-medium text-foreground'
						: 'text-muted-foreground'}"
				>
					<span class="h-0.5 w-2.5 shrink-0" style:background={lines[r.i].color}></span>
					<span class="min-w-0 flex-1 truncate">
						{lines[r.i].label}
						{#if r.forecast}<span class="ml-1 text-muted-foreground"
								>· {m.chart_tooltip_forecast()}</span
							>{/if}
					</span>
					<span class="font-mono text-foreground tabular-nums">{formatValue(r.value)}</span>
					{#if r.range}
						<span class="font-mono text-muted-foreground tabular-nums">
							{formatValue(r.range[0])}–{formatValue(r.range[1])}
						</span>
					{/if}
				</div>
			{/each}
			{#if hovered.more > 0}
				<div class="text-muted-foreground">{m.chart_more_series({ count: hovered.more })}</div>
			{/if}
		</div>
	{/if}
</div>

{#snippet entry({ l, i }: { l: ChartLine; i: number })}
	<span
		role="presentation"
		class="flex items-center gap-1.5"
		onmouseenter={() => emphasise(i)}
		onmouseleave={() => emphasise(null)}
	>
		<span class="h-0.5 w-2.5 shrink-0" style:background={l.color}></span>
		{l.label}
		{#if l.band.length > 0}
			<span class="text-muted-foreground">· {m.chart_expected_range()}</span>
		{/if}
		{#if l.forecast.length > 0}
			<span class="text-muted-foreground">· {m.chart_forecast()}</span>
		{/if}
	</span>
{/snippet}

<div class="flex flex-wrap gap-x-4 gap-y-1 border-t pt-2 font-mono text-[11px]">
	{#each legend.slice(0, LEGEND_ROWS) as e (e.l.key)}
		{@render entry(e)}
	{/each}
	{#if legend.length > LEGEND_ROWS && !legendOpen}
		<button
			type="button"
			class="text-muted-foreground hover:text-foreground"
			onclick={() => (legendOpen = true)}
		>
			{m.chart_more_series({ count: legend.length - LEGEND_ROWS })}
		</button>
	{/if}
	{#if legendOpen}
		<div class="flex max-h-40 basis-full flex-wrap gap-x-4 gap-y-1 overflow-y-auto">
			{#each legend.slice(LEGEND_ROWS) as e (e.l.key)}
				{@render entry(e)}
			{/each}
		</div>
	{/if}
</div>

<style>
	.chart :global(.u-select) {
		background: color-mix(in oklch, var(--muted-foreground) 15%, transparent);
	}
	.chart :global(.u-cursor-x) {
		border-right: 1px solid color-mix(in oklch, var(--muted-foreground) 60%, transparent);
	}
</style>
