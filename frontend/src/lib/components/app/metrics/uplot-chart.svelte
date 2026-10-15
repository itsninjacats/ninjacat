<script lang="ts" module>
	import type * as LayerChart from './metrics-chart.svelte';

	// The same props as metrics-chart, so either can be imported.
	export type ChartLine = LayerChart.ChartLine;
	export type ChartRule = LayerChart.ChartRule;
	export type ChartZone = LayerChart.ChartZone;
</script>

<script lang="ts">
	import { onMount } from 'svelte';
	import uPlot from 'uplot';
	import 'uplot/dist/uPlot.min.css';

	let {
		lines,
		display,
		window,
		rules = [],
		zone = null
	}: {
		lines: ChartLine[];
		display: 'line' | 'area' | 'bars';
		/** The requested range in ms: the x axis spans all of it, not just where data is. */
		window: { from: number; to: number };
		/** Drawn on line charts only. */
		rules?: ChartRule[];
		zone?: ChartZone | null;
	} = $props();

	const HEIGHT = 320;

	let el: HTMLDivElement;
	let probe: HTMLSpanElement;
	let plot: uPlot | null = null;
	let plotShape = '';
	let hover = $state<{ idx: number; left: number; top: number } | null>(null);

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
		const xs = times.filter((t, i) => t !== times[i - 1]);
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
		if (min === max) return [min - 1, max + 1];
		return [min, max];
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

	// 11 771 153 846 does not fit beside the chart; 11.8B does. English on
	// purpose: other locales spell the suffix out ("11,8 mld") and get clipped.
	const compact = new Intl.NumberFormat('en', {
		notation: 'compact',
		maximumFractionDigits: 1
	});

	// A canvas cannot read CSS variables, so a colour is resolved through an element.
	function resolve(color: string) {
		probe.style.color = color;
		return getComputedStyle(probe).color;
	}

	const faded = (color: string, percent: number) =>
		resolve(`color-mix(in oklch, ${color} ${percent}%, transparent)`);

	function options(): uPlot.Options {
		const font = `12px ${getComputedStyle(el).fontFamily}`;
		const muted = resolve('var(--muted-foreground)');
		const axis = { stroke: muted, font, ticks: { show: false } };

		const series: uPlot.Series[] = [{}];
		const hidden = { stroke: 'transparent', width: 0, points: { show: false } };
		const bands: uPlot.Band[] = [];

		if (display === 'bars') {
			const bars = uPlot.paths.bars!({ size: [0.6, Infinity] });
			for (const l of lines.toReversed()) {
				const c = resolve(l.color);
				series.push({ stroke: c, fill: c, width: 0, paths: bars, points: { show: false } });
			}
		} else if (display === 'area') {
			for (const l of lines) {
				series.push({
					stroke: resolve(l.color),
					fill: faded(l.color, 25),
					width: 2,
					points: { show: false }
				});
			}
		} else {
			for (const l of lines) {
				series.push({ stroke: resolve(l.color), width: 2, points: { show: false } });
				if (l.band.length > 0) {
					series.push(hidden, hidden);
					bands.push({ series: [series.length - 1, series.length - 2], fill: faded(l.color, 15) });
				}
				if (l.forecast.length > 0) {
					series.push(
						{ stroke: resolve(l.color), width: 2, dash: [4, 4], points: { show: false } },
						hidden,
						hidden
					);
					bands.push({ series: [series.length - 1, series.length - 2], fill: faded(l.color, 12) });
				}
			}
		}

		return {
			width: el.clientWidth,
			height: HEIGHT,
			ms: 1,
			legend: { show: false },
			cursor: {
				x: false,
				y: false,
				drag: { x: false, y: false, setScale: false },
				// uPlot takes a hover dot's colour from the drawn points, and none are drawn.
				points:
					display === 'bars'
						? { show: false }
						: {
								fill: (u, i) => series[i].stroke as string,
								stroke: (u, i) => series[i].stroke as string
							}
			},
			// Room for the last time label, which uPlot would cut at the canvas edge.
			padding: [8, 32, 0, 0],
			scales: { x: { time: true, range: xDomain }, y: { range: yDomain } },
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
					// d3's 1, 2, 5 steps: uPlot's own 2.5 steps round to wrong labels.
					incrs: Array.from({ length: 40 }, (_, e) =>
						[1, 2, 5].map((m) => m * 10 ** (e - 20))
					).flat(),
					values: (u, splits) => splits.map((v) => compact.format(v))
				}
			],
			hooks: {
				drawAxes: [drawMarks],
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

	// Beneath the series, as LayerChart draws them: the alert zone and the rules.
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
			ctx.fillStyle = faded('var(--state-alert)', 10);
			ctx.fillRect(left, Math.min(y0, y1), width, Math.abs(y0 - y1));
		}
		ctx.font = `${10 * px}px ${getComputedStyle(el).fontFamily}`;
		ctx.textBaseline = 'bottom';
		for (const r of rules) {
			const y = u.valToPos(r.value, 'y', true);
			const color = resolve(r.color);
			ctx.strokeStyle = color;
			ctx.fillStyle = color;
			ctx.lineWidth = 1.5 * px;
			ctx.setLineDash(r.dashed ? [6 * px, 4 * px] : []);
			ctx.beginPath();
			ctx.moveTo(left, y);
			ctx.lineTo(left + width, y);
			ctx.stroke();
			ctx.fillText(r.label, left + 4 * px, y - 2 * px);
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
		const rows = lines
			.map((l, i) => ({ label: l.label, color: l.color, value: values[i][idx] }))
			.filter((r) => r.value !== null);
		if (rows.length === 0) return null;
		const flip = left > el.clientWidth / 2;
		return { time: tooltipTime(table.xs[idx]), rows, left, top, flip };
	});
</script>

<div class="relative w-full text-xs">
	<div bind:this={el} class="h-[320px] w-full"></div>
	<span bind:this={probe} class="hidden"></span>
	{#if hovered}
		<div
			class="pointer-events-none absolute z-10 grid min-w-32 items-start gap-1.5 rounded-lg border border-border/50 bg-background px-2.5 py-1.5 text-xs shadow-xl"
			style:left="{hovered.left}px"
			style:top="{hovered.top}px"
			style:transform="translate({hovered.flip ? 'calc(-100% - 12px)' : '12px'}, 12px)"
		>
			<div class="font-medium">{hovered.time}</div>
			<div class="grid gap-1.5">
				{#each hovered.rows as r, i (i)}
					<div class="flex w-full items-center gap-2">
						<div class="size-2.5 shrink-0 rounded-[2px]" style="background: {r.color}"></div>
						<div class="flex flex-1 shrink-0 items-center justify-between gap-4 leading-none">
							<span class="text-muted-foreground">{r.label}</span>
							<span class="font-mono font-medium text-foreground tabular-nums">
								{r.value!.toLocaleString()}
							</span>
						</div>
					</div>
				{/each}
			</div>
		</div>
	{/if}
</div>

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
