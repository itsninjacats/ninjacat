<script lang="ts">
	import { replaceState } from '$app/navigation';
	import { page } from '$app/state';
	import * as Card from '$lib/components/ui/card';
	import * as Chart from '$lib/components/ui/chart';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Separator } from '$lib/components/ui/separator';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { LineChart } from 'layerchart';
	import { curveMonotoneX } from 'd3-shape';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	type Point = { t: string; v: number };
	type Series = { name: string; tags: Record<string, string>; points: Point[] };
	type TagFilter = { key: string; value: string };

	const RANGES = [
		{ value: '-15m', label: '15m' },
		{ value: '-1h', label: '1h' },
		{ value: '-6h', label: '6h' },
		{ value: '-24h', label: '24h' },
		{ value: '-7d', label: '7d' }
	];
	const AGGS = ['avg', 'min', 'max', 'sum', 'count'];
	const MAX_GROUP_BY = 4;

	// ---- state, seeded from the URL so a pasted address reproduces the view ----

	function parseTagParam(raw: string): TagFilter | null {
		const i = raw.indexOf(':');
		if (i <= 0) return null;
		return { key: raw.slice(0, i), value: raw.slice(i + 1) };
	}

	const initial = page.url.searchParams;

	let metric = $state(initial.get('metric') ?? '');
	let range = $state(initial.get('from') ?? '-1h');
	let agg = $state(AGGS.includes(initial.get('agg') ?? '') ? initial.get('agg')! : 'avg');
	let tagFilters = $state<TagFilter[]>(
		initial
			.getAll('tag')
			.map(parseTagParam)
			.filter((t): t is TagFilter => t !== null)
	);
	let groupBy = $state<string[]>(initial.getAll('by').slice(0, MAX_GROUP_BY));

	// Metric picker (combobox: type to search, pick from the dropdown).
	let metricSearch = $state('');
	let metricOptions = $state<string[]>([]);
	let metricOpen = $state(false);

	// Tag filter builder: pick a key, then search for a value.
	let tagKeys = $state<string[]>([]);
	let filterKey = $state('');
	let valueSearch = $state('');
	let valueOptions = $state<string[]>([]);
	let valueOpen = $state(false);

	// Query results.
	let series = $state<Series[]>([]);
	let step = $state(0);
	let loading = $state(false);
	let error = $state<string | null>(null);

	// Pick the first metric once the list arrives, unless the URL named one.
	$effect(() => {
		if (!metric && data.metrics.length > 0) metric = data.metrics[0];
	});

	// ---- keep the URL in sync so the current view can be pasted to someone ----

	const queryString = $derived.by(() => {
		const q = new URLSearchParams();
		if (metric) q.set('metric', metric);
		q.set('from', range);
		q.set('agg', agg);
		for (const t of tagFilters) q.append('tag', `${t.key}:${t.value}`);
		for (const b of groupBy) q.append('by', b);
		return q.toString();
	});

	$effect(() => {
		const target = `?${queryString}`;
		// replaceState rather than goto: the address should follow the controls
		// without piling an entry onto history for every click.
		if (page.url.search !== target) replaceState(target, {});
	});

	// ---- lookups through the meta/ proxy (debounced where the user types) ----

	$effect(() => {
		if (!metricOpen) return;
		const search = metricSearch;
		const timer = setTimeout(
			() => {
				fetch(`/app/metrics/meta?what=names&search=${encodeURIComponent(search)}`)
					.then((r) => r.json())
					.then((body) => {
						metricOptions = body.names ?? [];
					})
					.catch(() => {});
			},
			search ? 250 : 0
		);
		return () => clearTimeout(timer);
	});

	// Tag keys belong to the chosen metric, so refetch when it changes.
	$effect(() => {
		if (!metric) {
			tagKeys = [];
			return;
		}
		const controller = new AbortController();
		fetch(`/app/metrics/meta?what=tags&metric=${encodeURIComponent(metric)}`, {
			signal: controller.signal
		})
			.then((r) => r.json())
			.then((body) => {
				tagKeys = body.keys ?? [];
			})
			.catch(() => {});
		return () => controller.abort();
	});

	$effect(() => {
		if (!metric || !filterKey) {
			valueOptions = [];
			return;
		}
		const q = new URLSearchParams({
			what: 'values',
			metric,
			key: filterKey,
			search: valueSearch
		});
		const timer = setTimeout(
			() => {
				fetch(`/app/metrics/meta?${q}`)
					.then((r) => r.json())
					.then((body) => {
						valueOptions = body.values ?? [];
					})
					.catch(() => {});
			},
			valueSearch ? 250 : 0
		);
		return () => clearTimeout(timer);
	});

	// ---- the query itself ----

	// Re-fetches whenever a control changes. $effect tracks metric, range, agg,
	// tagFilters and groupBy because they are read below; nothing else has to
	// be wired. With no explicit grouping the chart groups by host, as the
	// original single-metric page did.
	$effect(() => {
		if (!metric) return;
		const q = new URLSearchParams({ metric, from: range, agg });
		for (const t of tagFilters) q.append('tag', `${t.key}:${t.value}`);
		for (const b of groupBy.length > 0 ? groupBy : ['host']) q.append('by', b);

		loading = true;
		const controller = new AbortController();

		fetch(`/app/metrics/data?${q}`, { signal: controller.signal })
			.then(async (r) => {
				const body = await r.json();
				if (!r.ok) throw new Error(body?.error ?? `HTTP ${r.status}`);
				return body;
			})
			.then((body) => {
				series = body.series ?? [];
				step = body.step ?? 0;
				error = null;
			})
			.catch((e) => {
				if (e.name === 'AbortError') return;
				error = e.message;
				series = [];
			})
			.finally(() => {
				loading = false;
			});

		// Abort the in-flight request when the controls change again, so a
		// slow answer cannot overwrite a newer one.
		return () => controller.abort();
	});

	// ---- chart plumbing: LayerChart wants one row per x value ----

	const rows = $derived.by(() => {
		const byTime = new Map<number, Record<string, unknown>>();
		for (const s of series) {
			for (const p of s.points) {
				const ms = new Date(p.t).getTime();
				let row = byTime.get(ms);
				if (!row) {
					row = { time: new Date(ms) };
					byTime.set(ms, row);
				}
				row[s.name] = p.v;
			}
		}
		return [...byTime.entries()].sort((a, b) => a[0] - b[0]).map(([, r]) => r);
	});

	const chartSeries = $derived(
		series.map((s, i) => ({
			key: s.name,
			label: s.name,
			color: `var(--chart-${(i % 5) + 1})`
		}))
	);

	const config = $derived(
		Object.fromEntries(
			series.map((s, i) => [s.name, { label: s.name, color: `var(--chart-${(i % 5) + 1})` }])
		) as Chart.ChartConfig
	);

	const totalPoints = $derived(series.reduce((n, s) => n + s.points.length, 0));

	// Chips with the same key are OR'd by the backend; grouping them makes
	// that readable: [env: prod OR staging] AND [service: api].
	const filterGroups = $derived.by(() => {
		const groups = new Map<string, string[]>();
		for (const t of tagFilters) {
			const list = groups.get(t.key);
			if (list) list.push(t.value);
			else groups.set(t.key, [t.value]);
		}
		return [...groups.entries()].map(([key, values]) => ({ key, values }));
	});

	// ---- handlers ----

	function pickMetric(m: string) {
		metric = m;
		metricSearch = '';
		metricOpen = false;
		filterKey = '';
		valueSearch = '';
	}

	function addFilter(value: string) {
		if (!filterKey || !value) return;
		if (!tagFilters.some((t) => t.key === filterKey && t.value === value)) {
			tagFilters = [...tagFilters, { key: filterKey, value }];
		}
		valueSearch = '';
		valueOpen = false;
	}

	function removeFilter(key: string, value: string) {
		tagFilters = tagFilters.filter((t) => !(t.key === key && t.value === value));
	}

	function toggleGroupBy(key: string) {
		if (groupBy.includes(key)) {
			groupBy = groupBy.filter((k) => k !== key);
		} else if (groupBy.length < MAX_GROUP_BY) {
			groupBy = [...groupBy, key];
		}
	}

	function formatStep(s: number) {
		if (s >= 3600) return `${Math.round(s / 3600)}h`;
		if (s >= 60) return `${Math.round(s / 60)}m`;
		return `${s}s`;
	}
</script>

<svelte:head><title>Metrics — ninjacat</title></svelte:head>

<div class="mx-auto max-w-6xl px-6 py-8">
	<div class="mb-6">
		<h1 class="font-heading text-2xl font-semibold tracking-tight">Metrics</h1>
		<p class="text-sm text-muted-foreground">
			Explore time series straight from ClickHouse: filter by tags, group, compare.
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
			<!-- Query builder -->
			<Card.Root>
				<Card.Content class="space-y-3 pt-6">
					<!-- Metric + aggregation + time range -->
					<div class="flex flex-wrap items-center gap-3">
						<div class="relative min-w-64 flex-1">
							<Input
								value={metricOpen ? metricSearch : metric}
								placeholder="Search metrics…"
								class="h-9 font-mono text-sm"
								onfocus={() => {
									metricSearch = '';
									metricOpen = true;
								}}
								onblur={() => setTimeout(() => (metricOpen = false), 150)}
								oninput={(e) => (metricSearch = e.currentTarget.value)}
							/>
							{#if metricOpen}
								<div
									class="absolute top-full z-20 mt-1 max-h-72 w-full overflow-y-auto rounded-md border bg-popover p-1 shadow-md"
								>
									{#each metricOptions.length > 0 || metricSearch ? metricOptions : data.metrics as m (m)}
										<button
											type="button"
											class="w-full truncate rounded-sm px-2 py-1.5 text-left font-mono text-xs transition-colors
												{m === metric ? 'bg-primary text-primary-foreground' : 'hover:bg-muted'}"
											onmousedown={() => pickMetric(m)}
											title={m}
										>
											{m}
										</button>
									{:else}
										<p class="px-2 py-3 text-xs text-muted-foreground">No matching metrics.</p>
									{/each}
								</div>
							{/if}
						</div>

						<div class="flex items-center gap-1">
							{#each AGGS as a (a)}
								<Button
									size="sm"
									variant={agg === a ? 'default' : 'ghost'}
									onclick={() => (agg = a)}
								>
									{a}
								</Button>
							{/each}
							<Separator orientation="vertical" class="mx-1 h-5" />
							{#each RANGES as r (r.value)}
								<Button
									size="sm"
									variant={range === r.value ? 'default' : 'ghost'}
									onclick={() => (range = r.value)}
								>
									{r.label}
								</Button>
							{/each}
						</div>
					</div>

					<Separator />

					<!-- Tag filters -->
					<div class="flex flex-wrap items-center gap-2">
						<span class="text-xs font-medium text-muted-foreground">Filter</span>

						<select
							bind:value={filterKey}
							class="h-8 rounded-md border bg-transparent px-2 font-mono text-xs"
							disabled={tagKeys.length === 0}
						>
							<option value="">tag key…</option>
							{#each tagKeys as k (k)}
								<option value={k}>{k}</option>
							{/each}
						</select>

						<div class="relative">
							<Input
								bind:value={valueSearch}
								placeholder={filterKey ? `${filterKey} value…` : 'pick a key first'}
								disabled={!filterKey}
								class="h-8 w-48 font-mono text-xs"
								onfocus={() => (valueOpen = true)}
								onblur={() => setTimeout(() => (valueOpen = false), 150)}
								onkeydown={(e) => {
									if (e.key === 'Enter' && valueSearch) addFilter(valueSearch);
								}}
							/>
							{#if valueOpen && filterKey && valueOptions.length > 0}
								<div
									class="absolute top-full z-20 mt-1 max-h-60 w-full overflow-y-auto rounded-md border bg-popover p-1 shadow-md"
								>
									{#each valueOptions as v (v)}
										<button
											type="button"
											class="w-full truncate rounded-sm px-2 py-1 text-left font-mono text-xs hover:bg-muted"
											onmousedown={() => addFilter(v)}
											title={v}
										>
											{v}
										</button>
									{/each}
								</div>
							{/if}
						</div>

						{#if filterGroups.length > 0}
							<Separator orientation="vertical" class="h-5" />
							{#each filterGroups as g, gi (g.key)}
								{#if gi > 0}
									<span class="text-[10px] font-semibold text-muted-foreground uppercase">and</span>
								{/if}
								<div class="flex items-center gap-1 rounded-md border bg-muted/40 px-1.5 py-1">
									<span class="font-mono text-[11px] text-muted-foreground">{g.key}:</span>
									{#each g.values as v, vi (v)}
										{#if vi > 0}
											<span class="text-[10px] font-semibold text-muted-foreground uppercase">
												or
											</span>
										{/if}
										<Badge variant="secondary" class="gap-1 font-mono text-[11px]">
											{v}
											<button
												type="button"
												class="ml-0.5 opacity-60 hover:opacity-100"
												onclick={() => removeFilter(g.key, v)}
												aria-label={`Remove filter ${g.key}:${v}`}
											>
												×
											</button>
										</Badge>
									{/each}
								</div>
							{/each}
							<Button size="sm" variant="ghost" onclick={() => (tagFilters = [])}>clear</Button>
						{/if}
					</div>

					<!-- Group by -->
					<div class="flex flex-wrap items-center gap-1.5">
						<span class="text-xs font-medium text-muted-foreground">
							Group by
							{#if groupBy.length === 0}
								<span class="font-normal">(host by default)</span>
							{/if}
						</span>
						{#each tagKeys as k (k)}
							<button type="button" onclick={() => toggleGroupBy(k)}>
								<Badge
									variant={groupBy.includes(k) ? 'default' : 'outline'}
									class="cursor-pointer font-mono text-[11px]
										{!groupBy.includes(k) && groupBy.length >= MAX_GROUP_BY ? 'opacity-40' : ''}"
								>
									{k}
								</Badge>
							</button>
						{:else}
							<span class="text-xs text-muted-foreground">
								{metric ? 'no tag keys reported for this metric' : 'pick a metric first'}
							</span>
						{/each}
						{#if groupBy.length >= MAX_GROUP_BY}
							<span class="text-[10px] text-muted-foreground">max {MAX_GROUP_BY} keys</span>
						{/if}
					</div>
				</Card.Content>
			</Card.Root>

			<!-- Chart -->
			<Card.Root>
				<Card.Header class="pb-4">
					<div class="min-w-0">
						<Card.Title class="truncate font-mono text-base">
							{metric || 'Pick a metric'}
						</Card.Title>
						<Card.Description>
							{#if loading}
								loading…
							{:else if step}
								{totalPoints} points · step {formatStep(step)} · {series.length} series
							{:else}
								no data in this range
							{/if}
						</Card.Description>
					</div>
				</Card.Header>

				<Card.Content class="space-y-4">
					{#if error}
						<div class="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm">
							{error}
						</div>
					{/if}

					{#if loading && rows.length === 0}
						<Skeleton class="h-[300px] w-full" />
					{:else if rows.length === 0}
						<div
							class="flex h-[300px] items-center justify-center rounded-md border border-dashed text-sm text-muted-foreground"
						>
							No points to draw. Try a wider range or fewer filters.
						</div>
					{:else}
						<Chart.Container {config} class="h-[300px] w-full">
							<LineChart
								data={rows}
								x="time"
								series={chartSeries}
								props={{ spline: { curve: curveMonotoneX, 'stroke-width': 2 } }}
							>
								{#snippet tooltip()}
									<Chart.Tooltip />
								{/snippet}
							</LineChart>
						</Chart.Container>

						<!-- Legend: one entry per returned series -->
						<div class="flex flex-wrap gap-x-4 gap-y-1.5 border-t pt-3">
							{#each chartSeries as s (s.key)}
								<span class="flex items-center gap-1.5 font-mono text-xs">
									<span
										class="inline-block size-2 shrink-0 rounded-full"
										style="background: {s.color}"
									></span>
									{s.label}
								</span>
							{/each}
						</div>
					{/if}
				</Card.Content>
			</Card.Root>
		</div>
	{/if}
</div>
