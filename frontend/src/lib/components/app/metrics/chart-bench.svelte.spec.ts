// A benchmark of the chart, not a test of it: it runs only with VITE_CHART_BENCH=1
// and prints one `CHART_BENCH {...}` line.
//   VITE_CHART_BENCH=1 bunx vitest run --project client --silent=false --reporter=verbose chart-bench
// Times are whole frames: anything at frameMs (hover) or two of them (paint) is
// the floor, not a measured speed.
import { describe, expect, it } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import '../../../../routes/layout.css';
import Chart, { seriesColor, type ChartLine } from './uplot-chart.svelte';

const POINTS = 300;
const STEP = 60_000;
const END = Date.UTC(2026, 9, 15, 12);
const window = { from: END - POINTS * STEP, to: END };

function lines(count: number, shift = 0): ChartLine[] {
	return Array.from({ length: count }, (_, s) => ({
		key: `s${s}`,
		label: `host:bench-${s}`,
		color: seriesColor(s),
		points: Array.from({ length: POINTS }, (_, i) => ({
			t: window.from + (i + 1) * STEP,
			v: 100 + s + 20 * Math.sin((i + shift + s) / 10)
		})),
		band: [],
		forecast: []
	}));
}

const frame = () => new Promise<number>((r) => requestAnimationFrame(r));
const painted = async () => {
	await frame();
	return frame();
};
const percentile = (xs: number[], p: number) => {
	const sorted = xs.toSorted((a, b) => a - b);
	return sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))];
};
const round = (x: number) => Math.round(x * 10) / 10;
const median = (xs: number[]) => percentile(xs, 0.5);

// One frame with nothing to draw: what every time here is counted in.
async function idleFrame() {
	const gaps = [];
	let last = await frame();
	for (let i = 0; i < 30; i++) {
		const now = await frame();
		gaps.push(now - last);
		last = now;
	}
	return median(gaps);
}

// 60 moves across the middle of the plot, one per frame.
async function hover(chart: HTMLElement) {
	const area = chart.querySelector<HTMLElement>('.u-over')!;
	const box = area.getBoundingClientRect();
	const y = box.top + box.height / 2;
	const frames: number[] = [];
	let last = await frame();
	for (let i = 0; i < 60; i++) {
		const x = box.left + box.width * (0.1 + (0.8 * i) / 59);
		area.dispatchEvent(new MouseEvent('mousemove', { clientX: x, clientY: y, bubbles: true }));
		const now = await frame();
		frames.push(now - last);
		last = now;
	}
	return { hoverP50: percentile(frames, 0.5), hoverP95: percentile(frames, 0.95) };
}

// A series, not just axes: the series are coloured, the axes and grid grey.
function drewSeries(chart: HTMLElement) {
	const canvas = chart.querySelector('canvas')!;
	const px = canvas.getContext('2d')!.getImageData(0, 0, canvas.width, canvas.height).data;
	for (let i = 0; i < px.length; i += 4) {
		if (
			px[i + 3] > 0 &&
			Math.max(px[i], px[i + 1], px[i + 2]) - Math.min(px[i], px[i + 1], px[i + 2]) > 60
		) {
			return true;
		}
	}
	return false;
}

async function scenario(charts: number, series: number) {
	const grid = document.createElement('div');
	grid.style.width = '1000px';
	document.body.append(grid);

	const start = performance.now();
	const mounted = [];
	for (let i = 0; i < charts; i++) {
		const target = grid.appendChild(document.createElement('div'));
		mounted.push(
			render(Chart, { target, props: { lines: lines(series), display: 'line', window } })
		);
	}
	await painted();
	const firstPaint = performance.now() - start;
	const drew = drewSeries(grid.firstElementChild as HTMLElement);

	const updateStart = performance.now();
	for (const m of mounted) await m.rerender({ lines: lines(series, 7) });
	await painted();
	const update = performance.now() - updateStart;

	const result = {
		firstPaintMs: firstPaint,
		updateMs: update,
		domNodes: grid.querySelectorAll('*').length,
		...(await hover(grid.firstElementChild as HTMLElement)),
		drew
	};
	for (const m of mounted) await m.unmount();
	grid.remove();
	return result;
}

// The median of three runs, each figure on its own.
async function measure(charts: number, series: number) {
	const runs = [];
	for (let i = 0; i < 3; i++) runs.push(await scenario(charts, series));
	return {
		charts,
		series,
		points: POINTS,
		firstPaintMs: round(median(runs.map((r) => r.firstPaintMs))),
		updateMs: round(median(runs.map((r) => r.updateMs))),
		domNodes: runs[0].domNodes,
		hoverP50: round(median(runs.map((r) => r.hoverP50))),
		hoverP95: round(median(runs.map((r) => r.hoverP95))),
		drew: runs.every((r) => r.drew)
	};
}

describe.skipIf(!import.meta.env.VITE_CHART_BENCH)('chart bench', () => {
	it('uplot', { timeout: 1_800_000 }, async () => {
		await page.viewport(1280, 900);
		// Untimed, so the measured runs do not pay for loading the page's CSS and fonts.
		await scenario(1, 10);
		const frameMs = round(await idleFrame());
		const results = [];
		for (const series of [10, 100, 300, 1000]) results.push(await measure(1, series));
		results.push(await measure(20, 20));
		console.log(`CHART_BENCH ${JSON.stringify({ frameMs, results })}`);
		for (const r of results) expect(r.drew, `drew ${r.charts}×${r.series}`).toBe(true);
	});
});
