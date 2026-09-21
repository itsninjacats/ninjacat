<script lang="ts">
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
	type Series = { metric: string; host: string; points: Point[] };

	const RANGES = [
		{ value: '-15m', label: '15 min' },
		{ value: '-1h', label: '1 godz' },
		{ value: '-6h', label: '6 godz' },
		{ value: '-24h', label: '24 godz' },
		{ value: '-7d', label: '7 dni' }
	];
	const AGGS = [
		{ value: 'avg', label: 'średnia' },
		{ value: 'max', label: 'maks' },
		{ value: 'min', label: 'min' },
		{ value: 'sum', label: 'suma' }
	];

	let filter = $state('');
	let metric = $state('');
	let range = $state('-1h');
	let agg = $state('avg');
	let hostFilter = $state<string[]>([]);

	let series = $state<Series[]>([]);
	let hosts = $state<string[]>([]);
	let step = $state(0);
	let loading = $state(false);
	let error = $state<string | null>(null);

	// Pick the first metric once the list arrives, and again if a navigation
	// replaces it. Reading data.metrics[0] straight into $state would capture
	// only the value present at first render.
	$effect(() => {
		if (!metric && data.metrics.length > 0) metric = data.metrics[0];
	});

	const visible = $derived(
		filter ? data.metrics.filter((m) => m.toLowerCase().includes(filter.toLowerCase())) : data.metrics
	);

	// Points are keyed by timestamp so several hosts share one x axis.
	// LayerChart wants one row per x value with a column per series.
	const rows = $derived.by(() => {
		const byTime = new Map<number, Record<string, unknown>>();
		for (const s of series) {
			for (const p of s.points) {
				const ms = new Date(p.t).getTime();
				let row = byTime.get(ms);
				if (!row) {
					row = { czas: new Date(ms) };
					byTime.set(ms, row);
				}
				row[s.host] = p.v;
			}
		}
		return [...byTime.entries()].sort((a, b) => a[0] - b[0]).map(([, r]) => r);
	});

	const chartSeries = $derived(
		series.map((s, i) => ({
			key: s.host,
			label: s.host,
			color: `var(--chart-${(i % 5) + 1})`
		}))
	);

	const config = $derived(
		Object.fromEntries(
			series.map((s, i) => [s.host, { label: s.host, color: `var(--chart-${(i % 5) + 1})` }])
		) as Chart.ChartConfig
	);

	const totalPoints = $derived(series.reduce((n, s) => n + s.points.length, 0));

	// Re-fetches whenever a control changes. $effect tracks metric, range, agg
	// and hostFilter because they are read below; nothing else has to be wired.
	$effect(() => {
		if (!metric) return;
		const q = new URLSearchParams({ metric, from: range, agg });
		for (const h of hostFilter) q.append('host', h);

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
				hosts = body.hosts ?? [];
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

	function toggleHost(h: string) {
		hostFilter = hostFilter.includes(h) ? hostFilter.filter((x) => x !== h) : [...hostFilter, h];
	}

	function formatStep(s: number) {
		if (s >= 3600) return `${Math.round(s / 3600)}h`;
		if (s >= 60) return `${Math.round(s / 60)}m`;
		return `${s}s`;
	}
</script>

<svelte:head><title>Metryki — ninjacat</title></svelte:head>

<div class="mx-auto max-w-6xl px-6 py-8">
	<div class="mb-6">
		<h1 class="font-heading text-2xl font-semibold tracking-tight">Metryki</h1>
		<p class="text-sm text-muted-foreground">
			Podgląd szeregów czasowych prosto z ClickHouse'a.
		</p>
	</div>

	{#if data.error}
		<Card.Root class="border-destructive/40">
			<Card.Header>
				<Card.Title class="text-base">Brak połączenia z serwerem</Card.Title>
				<Card.Description>{data.error}</Card.Description>
			</Card.Header>
			<Card.Content class="text-sm text-muted-foreground">
				Sprawdź, czy proces <code class="font-mono">ninjacat</code> działa i czy odpowiada
				na porcie 8081.
			</Card.Content>
		</Card.Root>
	{:else}
		<div class="grid gap-6 lg:grid-cols-[260px_1fr]">
			<!-- Lista metryk -->
			<Card.Root class="h-fit">
				<Card.Header class="pb-3">
					<Card.Title class="text-sm">Metryki</Card.Title>
					<Card.Description>{data.metrics.length} dostępnych</Card.Description>
				</Card.Header>
				<Card.Content class="space-y-3">
					<Input bind:value={filter} placeholder="Szukaj…" class="h-8 text-sm" />
					<div class="max-h-[460px] space-y-0.5 overflow-y-auto">
						{#each visible as m (m)}
							<button
								type="button"
								onclick={() => { metric = m; hostFilter = []; }}
								class="w-full truncate rounded-md px-2 py-1.5 text-left font-mono text-xs transition-colors
									{m === metric ? 'bg-primary text-primary-foreground' : 'hover:bg-muted'}"
								title={m}
							>
								{m}
							</button>
						{:else}
							<p class="px-2 py-4 text-xs text-muted-foreground">Nic nie pasuje.</p>
						{/each}
					</div>
				</Card.Content>
			</Card.Root>

			<!-- Wykres -->
			<Card.Root>
				<Card.Header class="pb-4">
					<div class="flex flex-wrap items-start justify-between gap-3">
						<div class="min-w-0">
							<Card.Title class="truncate font-mono text-base">
								{metric || 'Wybierz metrykę'}
							</Card.Title>
							<Card.Description>
								{#if loading}
									wczytuję…
								{:else if step}
									{totalPoints} punktów · krok {formatStep(step)} · {series.length}
									{series.length === 1 ? 'seria' : 'serii'}
								{:else}
									brak danych w tym zakresie
								{/if}
							</Card.Description>
						</div>

						<div class="flex flex-wrap items-center gap-1">
							{#each AGGS as a (a.value)}
								<Button
									size="sm"
									variant={agg === a.value ? 'default' : 'ghost'}
									onclick={() => (agg = a.value)}
								>
									{a.label}
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
				</Card.Header>

				<Card.Content class="space-y-4">
					{#if error}
						<div class="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm">
							{error}
						</div>
					{/if}

					{#if loading && rows.length === 0}
						<Skeleton class="h-[280px] w-full" />
					{:else if rows.length === 0}
						<div
							class="flex h-[280px] items-center justify-center rounded-md border border-dashed text-sm text-muted-foreground"
						>
							Brak punktów do narysowania.
						</div>
					{:else}
						<Chart.Container {config} class="h-[280px] w-full">
							<LineChart
								data={rows}
								x="czas"
								series={chartSeries}
								props={{ spline: { curve: curveMonotoneX, 'stroke-width': 2 } }}
							>
								{#snippet tooltip()}
									<Chart.Tooltip />
								{/snippet}
							</LineChart>
						</Chart.Container>
					{/if}

					{#if hosts.length > 1}
						<div class="flex flex-wrap items-center gap-1.5 border-t pt-4">
							<span class="mr-1 text-xs text-muted-foreground">Hosty:</span>
							{#each hosts as h (h)}
								<button type="button" onclick={() => toggleHost(h)}>
									<Badge
										variant={hostFilter.length === 0 || hostFilter.includes(h)
											? 'default'
											: 'outline'}
										class="cursor-pointer font-mono text-[11px]"
									>
										{h}
									</Badge>
								</button>
							{/each}
							{#if hostFilter.length > 0}
								<Button size="sm" variant="ghost" onclick={() => (hostFilter = [])}>
									pokaż wszystkie
								</Button>
							{/if}
						</div>
					{/if}
				</Card.Content>
			</Card.Root>
		</div>
	{/if}
</div>
