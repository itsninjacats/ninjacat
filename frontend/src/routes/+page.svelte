<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { Badge } from '$lib/components/ui/badge';
	import { Separator } from '$lib/components/ui/separator';
	import * as Chart from '$lib/components/ui/chart';
	import { AreaChart } from 'layerchart';
	import { curveMonotoneX } from 'd3-shape';

	// Dane przykladowe — docelowo pojda z ClickHouse przez /api.
	const teraz = Date.now();
	const dane = Array.from({ length: 48 }, (_, i) => ({
		czas: new Date(teraz - (47 - i) * 30 * 60 * 1000),
		przyjete: 40 + Math.round(25 * Math.sin(i / 5) + (i % 7) * 2)
	}));

	const konfiguracja = {
		przyjete: { label: 'Punkty / min', color: 'var(--chart-1)' }
	} satisfies Chart.ChartConfig;

	const endpointy = [
		{ sciezka: '/api/v2/series', opis: 'Metryki jako protobuf', stan: 'gotowe' },
		{ sciezka: '/api/v1/series', opis: 'Metryki jako JSON', stan: 'gotowe' },
		{ sciezka: '/intake/', opis: 'Metadane hosta i gohai', stan: 'gotowe' },
		{ sciezka: '/api/v1/check_run', opis: 'Statusy checków', stan: 'gotowe' },
		{ sciezka: '/api/beta/sketches', opis: 'DDSketch — percentyle', stan: 'gotowe' },
		{ sciezka: '/api/v1/collector', opis: 'Process-agent', stan: 'gotowe' },
		{ sciezka: '/api/v2/logs', opis: 'Logi', stan: 'gotowe' },
		{ sciezka: '/v1/traces', opis: 'Ślady OTLP', stan: 'planowane' }
	];
</script>

<svelte:head>
	<title>NinjaCat — przechwytywacz protokołu Datadoga</title>
</svelte:head>

<div class="min-h-screen bg-background">
	<!-- Nagłówek -->
	<header class="border-b">
		<div class="mx-auto flex h-14 max-w-5xl items-center justify-between px-6">
			<div class="flex items-center gap-2">
				<div class="size-6 bg-primary"></div>
				<span class="font-heading text-sm font-semibold tracking-tight">ninjacat</span>
			</div>
			<nav class="flex items-center gap-1">
				<Button href="/zadania" variant="ghost" size="sm">Zadania</Button>
				<Button href="/demo/better-auth" variant="ghost" size="sm">Konto</Button>
			</nav>
		</div>
	</header>

	<!-- Hero -->
	<section class="mx-auto max-w-5xl px-6 py-20">
		<Badge variant="secondary" class="mb-4">Agent 7.83.2 — protokół rozpoznany</Badge>
		<h1 class="font-heading max-w-2xl text-5xl leading-[1.05] font-semibold tracking-tight">
			Twój Datadog.<br />Twoje dane.
		</h1>
		<p class="text-muted-foreground mt-5 max-w-xl text-base leading-relaxed">
			Odbiornik mówiący protokołem Datadog Agenta. Przekierowujesz agenty, dane zostają
			u Ciebie w ClickHouse. Bez wymiany czegokolwiek na maszynach.
		</p>
		<div class="mt-8 flex gap-3">
			<Button>Zacznij zbierać</Button>
			<Button variant="outline">Dokumentacja</Button>
		</div>
	</section>

	<Separator />

	<!-- Wykres -->
	<section class="mx-auto max-w-5xl px-6 py-14">
		<Card.Root>
			<Card.Header>
				<Card.Title>Ruch przychodzący</Card.Title>
				<Card.Description>Punkty pomiarowe przyjęte w ostatniej dobie</Card.Description>
			</Card.Header>
			<Card.Content>
				<Chart.Container config={konfiguracja} class="h-[240px] w-full">
					<AreaChart
						data={dane}
						x="czas"
						series={[{ key: 'przyjete', label: 'Punkty / min', color: 'var(--chart-1)' }]}
						props={{ area: { curve: curveMonotoneX, 'fill-opacity': 0.25 } }}
					>
						{#snippet tooltip()}
							<Chart.Tooltip />
						{/snippet}
					</AreaChart>
				</Chart.Container>
			</Card.Content>
		</Card.Root>
	</section>

	<!-- Endpointy -->
	<section class="mx-auto max-w-5xl px-6 pb-20">
		<h2 class="font-heading mb-1 text-2xl font-semibold tracking-tight">Obsługiwane endpointy</h2>
		<p class="text-muted-foreground mb-6 text-sm">
			Ustalone przez podsłuchanie agenta — Datadog tego nie dokumentuje.
		</p>

		<div class="grid gap-3 sm:grid-cols-2">
			{#each endpointy as e (e.sciezka)}
				<Card.Root class="p-4">
					<div class="flex items-start justify-between gap-3">
						<div class="min-w-0">
							<code class="text-sm font-medium">{e.sciezka}</code>
							<p class="text-muted-foreground mt-1 text-xs">{e.opis}</p>
						</div>
						<Badge variant={e.stan === 'gotowe' ? 'default' : 'outline'} class="shrink-0">
							{e.stan}
						</Badge>
					</div>
				</Card.Root>
			{/each}
		</div>
	</section>

	<footer class="border-t">
		<div class="text-muted-foreground mx-auto max-w-5xl px-6 py-8 text-xs">
			ninjacat — odbiornik telemetrii. Dane w ClickHouse, agenty bez zmian.
		</div>
	</footer>
</div>
