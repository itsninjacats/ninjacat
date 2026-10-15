<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { Badge } from '$lib/components/ui/badge';
	import { Separator } from '$lib/components/ui/separator';
	import Chart from '$lib/components/app/metrics/uplot-chart.svelte';

	// Sample data, until this page reads the intake's own numbers.
	const now = Date.now();
	const sampleWindow = { from: now - 47 * 30 * 60 * 1000, to: now };
	const sample = [
		{
			key: 'accepted',
			label: 'Punkty / min',
			color: 'var(--chart-1)',
			points: Array.from({ length: 48 }, (_, i) => ({
				t: now - (47 - i) * 30 * 60 * 1000,
				v: 40 + Math.round(25 * Math.sin(i / 5) + (i % 7) * 2)
			})),
			band: [],
			forecast: []
		}
	];

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
				<Button href="/app" variant="ghost" size="sm">Panel</Button>
			</nav>
		</div>
	</header>

	<!-- Hero -->
	<section class="mx-auto max-w-5xl px-6 py-20">
		<Badge variant="secondary" class="mb-4">Agent 7.83.2 — protokół rozpoznany</Badge>
		<h1 class="max-w-2xl font-heading text-5xl leading-[1.05] font-semibold tracking-tight">
			Twój Datadog.<br />Twoje dane.
		</h1>
		<p class="mt-5 max-w-xl text-base leading-relaxed text-muted-foreground">
			Odbiornik mówiący protokołem Datadog Agenta. Przekierowujesz agenty, dane zostają u Ciebie w
			ClickHouse. Bez wymiany czegokolwiek na maszynach.
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
				<Chart lines={sample} display="area" window={sampleWindow} />
			</Card.Content>
		</Card.Root>
	</section>

	<!-- Endpointy -->
	<section class="mx-auto max-w-5xl px-6 pb-20">
		<h2 class="mb-1 font-heading text-2xl font-semibold tracking-tight">Obsługiwane endpointy</h2>
		<p class="mb-6 text-sm text-muted-foreground">
			Ustalone przez podsłuchanie agenta — Datadog tego nie dokumentuje.
		</p>

		<div class="grid gap-3 sm:grid-cols-2">
			{#each endpointy as e (e.sciezka)}
				<Card.Root class="p-4">
					<div class="flex items-start justify-between gap-3">
						<div class="min-w-0">
							<code class="text-sm font-medium">{e.sciezka}</code>
							<p class="mt-1 text-xs text-muted-foreground">{e.opis}</p>
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
		<div class="mx-auto max-w-5xl px-6 py-8 text-xs text-muted-foreground">
			ninjacat — odbiornik telemetrii. Dane w ClickHouse, agenty bez zmian.
		</div>
	</footer>
</div>
