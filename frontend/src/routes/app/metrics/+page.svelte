<script lang="ts">
	import { afterNavigate, replaceState } from '$app/navigation';
	import { page } from '$app/state';
	import ChartAreaIcon from '@lucide/svelte/icons/chart-area';
	import ChartColumnIcon from '@lucide/svelte/icons/chart-column';
	import ChartLineIcon from '@lucide/svelte/icons/chart-line';
	import EyeIcon from '@lucide/svelte/icons/eye';
	import EyeOffIcon from '@lucide/svelte/icons/eye-off';
	import HashIcon from '@lucide/svelte/icons/hash';
	import ListIcon from '@lucide/svelte/icons/chart-bar-decreasing';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RefreshIcon from '@lucide/svelte/icons/refresh-cw';
	import XIcon from '@lucide/svelte/icons/x';
	import * as Card from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Separator } from '$lib/components/ui/separator';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import FunctionChips from '$lib/components/app/metrics/function-chips.svelte';
	import MetricsChart, { type ChartLine } from '$lib/components/app/metrics/metrics-chart.svelte';
	import QueryRow from '$lib/components/app/metrics/query-row.svelte';
	import ScalarView, { type ScalarTable } from '$lib/components/app/metrics/scalar-view.svelte';
	import {
		SCALAR_AGGREGATORS,
		emptyFormula,
		emptyQuery,
		nextQueryName,
		rangeMs,
		readView,
		requestOf,
		writeView,
		type Display
	} from '$lib/metrics/query';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const RANGES = [
		{ value: '-15m', label: '15m' },
		{ value: '-1h', label: '1h' },
		{ value: '-6h', label: '6h' },
		{ value: '-24h', label: '24h' },
		{ value: '-7d', label: '7d' }
	];

	const DISPLAY_OPTIONS: { value: Display; label: string; icon: typeof ChartLineIcon }[] = [
		{ value: 'line', label: 'Line', icon: ChartLineIcon },
		{ value: 'area', label: 'Area', icon: ChartAreaIcon },
		{ value: 'bars', label: 'Bars', icon: ChartColumnIcon },
		{ value: 'toplist', label: 'Top list', icon: ListIcon },
		{ value: 'value', label: 'Query value', icon: HashIcon }
	];

	const color = (i: number) => `var(--chart-${(i % 5) + 1})`;

	// ---- the view, seeded from the URL so a pasted address reproduces it ----

	const view = $state(readView(page.url.searchParams));

	// Start with a metric on screen, unless the URL asked for something.
	$effect(() => {
		const first = view.queries[0];
		if (
			view.queries.length === 1 &&
			first.mode === 'builder' &&
			!first.builder.metric &&
			data.metrics.length > 0
		) {
			first.builder.metric = data.metrics[0];
		}
	});

	// replaceState throws until the router has started, which is after this
	// page's first effects run; afterNavigate fires once it has.
	let routerReady = $state(false);
	afterNavigate(() => (routerReady = true));

	$effect(() => {
		const target = `?${writeView(view)}`;
		if (!routerReady) return;
		// replaceState rather than goto: the address follows the controls
		// without piling an entry onto history for every keystroke.
		// Only the query string changes, on this same route, so there is no path to resolve.
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		if (page.url.search !== target) replaceState(target, {});
	});

	const request = $derived(requestOf(view));
	const scalar = $derived(view.display === 'toplist' || view.display === 'value');

	// ---- the query ----

	let lines = $state<ChartLine[]>([]);
	/** The range the lines on screen were asked for. */
	let shownWindow = $state({ from: 0, to: 0 });
	let table = $state<ScalarTable | null>(null);
	let loading = $state(false);
	let problems = $state<string[]>([]);
	let refreshes = $state(0);

	// Everything the answer depends on, as one string: the effect below re-runs
	// only when it changes, and waits for typing to pause before asking.
	const requestKey = $derived(
		JSON.stringify({
			request,
			range: view.range,
			scalar,
			aggregator: scalar ? view.aggregator : null,
			refreshes
		})
	);

	$effect(() => {
		const { queries, outputs } = JSON.parse(requestKey).request as typeof request;
		const isScalar = scalar;
		const aggregator = view.aggregator;

		if (outputs.length === 0) {
			lines = [];
			table = null;
			problems = [];
			return;
		}

		const controller = new AbortController();
		const timer = setTimeout(() => {
			const to = Date.now();
			const from = to - rangeMs(view.range);
			loading = true;

			fetch('/app/metrics/query', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				signal: controller.signal,
				body: JSON.stringify({
					kind: isScalar ? 'scalar' : 'timeseries',
					from,
					to,
					queries,
					formulas: outputs.map((o) => o.formula),
					aggregator
				})
			})
				.then(async (r) => {
					const body = await r.json();
					if (!r.ok) throw body?.errors ?? [`HTTP ${r.status}`];
					return body;
				})
				.then((body) => {
					problems = [];
					if (isScalar) {
						table = body as ScalarTable;
						lines = [];
					} else {
						shownWindow = { from, to };
						lines = toLines(
							body.series,
							outputs.map((o) => o.label)
						);
						table = null;
					}
				})
				.catch((e) => {
					if (e?.name === 'AbortError') return;
					problems = Array.isArray(e) ? e.map((p) => named(p, outputs)) : [String(e?.message ?? e)];
					lines = [];
					table = null;
				})
				.finally(() => (loading = false));
		}, 400);

		// A newer request replaces this one, so a slow answer cannot
		// overwrite a fresher view.
		return () => {
			clearTimeout(timer);
			controller.abort();
		};
	});

	type ApiSeries = {
		output: number;
		tags: string[];
		points: { t: number; v: number }[];
		band: { t: number; lower: number; upper: number }[];
		forecast: { t: number; v: number; lower: number; upper: number }[];
	};

	function toLines(series: ApiSeries[], labels: string[]): ChartLine[] {
		const oneOutput = labels.length === 1;
		return series.map((s, i) => {
			const group = s.tags.join(', ');
			const label = !group
				? labels[s.output]
				: oneOutput
					? group
					: `${labels[s.output]} · ${group}`;
			return { key: `s${i}`, label, color: color(i), ...s };
		});
	}

	/** The API names formulas by position; on screen they are their text. */
	function named(problem: string, outputs: { label: string }[]): string {
		return problem.replace(/^formulas\[(\d+)\]/, (all, i) => outputs[Number(i)]?.label ?? all);
	}

	const stepSeconds = $derived.by(() => {
		const points = lines.find((l) => l.points.length > 1)?.points;
		return points ? (points[1].t - points[0].t) / 1000 : 0;
	});

	const totalPoints = $derived(lines.reduce((n, l) => n + l.points.length, 0));

	function formatStep(s: number) {
		if (s >= 3600) return `${Math.round(s / 3600)}h`;
		if (s >= 60) return `${Math.round(s / 60)}m`;
		return `${s}s`;
	}

	// ---- rows ----

	function addQuery() {
		view.queries.push(emptyQuery(nextQueryName(view.queries.map((q) => q.name))));
	}

	function addFormula() {
		const names = view.queries.map((q) => q.name);
		view.formulas.push(emptyFormula(names.length > 1 ? `${names[0]} / ${names[1]}` : names[0]));
	}
</script>

<svelte:head><title>Metrics — ninjacat</title></svelte:head>

<div class="mx-auto max-w-6xl px-6 py-8">
	<div class="mb-6">
		<h1 class="font-heading text-2xl font-semibold tracking-tight">Metrics</h1>
		<p class="text-sm text-muted-foreground">
			Datadog's query language: queries, formulas and functions, straight from ClickHouse.
		</p>
	</div>

	{#if data.error}
		<Card.Root class="border-destructive/40">
			<Card.Header>
				<Card.Title class="text-base">Cannot reach the server</Card.Title>
				<Card.Description>{data.error}</Card.Description>
			</Card.Header>
			<Card.Content class="text-sm text-muted-foreground">
				Check that the <code class="font-mono">ninjacat</code> process is running and answering on port
				8081.
			</Card.Content>
		</Card.Root>
	{:else}
		<div class="space-y-4">
			<!-- Chart -->
			<Card.Root>
				<Card.Header class="pb-4">
					<div class="flex flex-wrap items-center justify-between gap-3">
						<div class="flex items-center gap-1">
							{#each DISPLAY_OPTIONS as d (d.value)}
								<Button
									size="sm"
									variant={view.display === d.value ? 'default' : 'ghost'}
									onclick={() => (view.display = d.value)}
									title={d.label}
								>
									<d.icon class="size-4" />
									<span class="hidden sm:inline">{d.label}</span>
								</Button>
							{/each}
						</div>

						<div class="flex items-center gap-1">
							{#if scalar}
								<select
									bind:value={view.aggregator}
									class="h-8 rounded-md border bg-transparent px-2 font-mono text-xs"
									aria-label="Reduce the window by"
									title="How the window becomes one value"
								>
									{#each SCALAR_AGGREGATORS as a (a)}
										<option value={a}>{a}</option>
									{/each}
								</select>
								<Separator orientation="vertical" class="mx-1 h-5" />
							{/if}
							{#each RANGES as r (r.value)}
								<Button
									size="sm"
									variant={view.range === r.value ? 'default' : 'ghost'}
									onclick={() => (view.range = r.value)}
								>
									{r.label}
								</Button>
							{/each}
							<Button size="sm" variant="ghost" onclick={() => refreshes++} title="Refresh">
								<RefreshIcon class="size-4 {loading ? 'animate-spin' : ''}" />
							</Button>
						</div>
					</div>
					<Card.Description>
						{#if loading}
							loading…
						{:else if !scalar && stepSeconds}
							{totalPoints} points · step {formatStep(stepSeconds)} · {lines.length} series
						{:else if scalar && table}
							{table.rows.length}
							{table.rows.length === 1 ? 'group' : 'groups'} · {view.aggregator} over the window
						{/if}
					</Card.Description>
				</Card.Header>

				<Card.Content class="space-y-4">
					{#if problems.length > 0}
						<ul
							class="space-y-1 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 font-mono text-xs"
						>
							{#each problems as p, i (i)}
								<li>{p}</li>
							{/each}
						</ul>
					{/if}

					{#if scalar}
						{#if table}
							<ScalarView
								{table}
								labels={request.outputs.map((o) => o.label)}
								colors={request.outputs.map((_, i) => color(i))}
								display={view.display as 'toplist' | 'value'}
							/>
						{:else if loading}
							<Skeleton class="h-[200px] w-full" />
						{/if}
					{:else if loading && lines.length === 0}
						<Skeleton class="h-[320px] w-full" />
					{:else if lines.length === 0}
						<div
							class="flex h-[320px] items-center justify-center rounded-md border border-dashed text-sm text-muted-foreground"
						>
							{request.outputs.length === 0
								? 'Nothing to draw: pick a metric, or show a query or formula.'
								: 'No points to draw. Try a wider range or fewer filters.'}
						</div>
					{:else}
						<MetricsChart
							{lines}
							window={shownWindow}
							display={view.display as 'line' | 'area' | 'bars'}
						/>
					{/if}
				</Card.Content>
			</Card.Root>

			<!-- Queries and formulas. Not clipped: the metric list drops below the card. -->
			<Card.Root class="overflow-visible">
				<Card.Content class="space-y-2 pt-6">
					{#each view.queries as query, i (query.name)}
						<QueryRow
							bind:query={view.queries[i]}
							color={color(i)}
							metrics={data.metrics}
							onremove={view.queries.length > 1 ? () => view.queries.splice(i, 1) : undefined}
						/>
					{/each}

					{#each view.formulas as formula, i (i)}
						<div
							class="flex flex-wrap items-center gap-2 rounded-md border border-dashed px-2 py-1.5 {formula.hidden
								? 'opacity-60'
								: ''}"
						>
							<span
								class="inline-flex size-6 shrink-0 items-center justify-center rounded bg-muted font-mono text-xs italic"
							>
								ƒ
							</span>
							<Input
								bind:value={formula.text}
								placeholder="a / b * 100"
								class="h-8 min-w-60 flex-1 font-mono text-xs"
								spellcheck={false}
							/>
							<FunctionChips bind:functions={formula.functions} allowModifiers={false} />
							<label class="flex items-center gap-1.5 text-xs text-muted-foreground">
								as
								<input
									bind:value={formula.alias}
									placeholder="alias"
									class="h-7 w-28 rounded-md border bg-transparent px-2 font-mono text-[11px] text-foreground outline-none placeholder:text-muted-foreground/60 focus:ring-1 focus:ring-ring"
								/>
							</label>
							<div class="ml-auto flex items-center gap-0.5">
								<button
									type="button"
									class="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
									onclick={() => (formula.hidden = !formula.hidden)}
									aria-label={formula.hidden ? 'Show formula' : 'Hide formula'}
								>
									{#if formula.hidden}
										<EyeOffIcon class="size-4" />
									{:else}
										<EyeIcon class="size-4" />
									{/if}
								</button>
								<button
									type="button"
									class="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-destructive"
									onclick={() => view.formulas.splice(i, 1)}
									aria-label="Remove formula"
								>
									<XIcon class="size-4" />
								</button>
							</div>
						</div>
					{/each}

					<div class="flex items-center gap-2 pt-1">
						<Button size="sm" variant="outline" onclick={addQuery}>
							<PlusIcon class="size-4" /> Query
						</Button>
						<Button size="sm" variant="outline" onclick={addFormula}>
							<PlusIcon class="size-4" /> Formula
						</Button>
						<span class="ml-2 text-xs text-muted-foreground">
							Formulas combine queries by name. Hide a query to use it only in formulas.
						</span>
					</div>
				</Card.Content>
			</Card.Root>
		</div>
	{/if}
</div>
