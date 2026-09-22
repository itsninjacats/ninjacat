<script lang="ts">
	import * as Card from '$lib/components/ui/card';
	import { Badge } from '$lib/components/ui/badge';
	import { Separator } from '$lib/components/ui/separator';
	import ActivityIcon from '@lucide/svelte/icons/activity';
	import ServerIcon from '@lucide/svelte/icons/server';
	import BellIcon from '@lucide/svelte/icons/bell';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	// Zaslepka. Docelowo z ClickHouse przez API ninjacata.
	const kafelki = [
		{
			etykieta: 'Hosty',
			wartosc: '0',
			opis: 'raportujących w ostatniej godzinie',
			ikona: ServerIcon
		},
		{ etykieta: 'Metryki', wartosc: '0', opis: 'serii czasowych', ikona: ActivityIcon },
		{ etykieta: 'Monitory', wartosc: '0', opis: 'aktywnych', ikona: BellIcon },
		{ etykieta: 'Logi', wartosc: '0', opis: 'wpisów dzisiaj', ikona: FileTextIcon }
	];

	const kroki = [
		{
			tytul: 'Podłącz pierwszego agenta',
			opis: 'Ustaw DD_DD_URL na adres tej instancji i zrestartuj agenta.',
			gotowe: false
		},
		{
			tytul: 'Sprawdź, czy dane przychodzą',
			opis: 'Pierwsze metryki powinny pojawić się w ciągu ~20 sekund.',
			gotowe: false
		},
		{
			tytul: 'Zdefiniuj pierwszy monitor',
			opis: 'Próg na metryce, która Cię obchodzi.',
			gotowe: false
		}
	];
</script>

<svelte:head><title>ninjacat</title></svelte:head>

<main class="mx-auto max-w-6xl px-6 py-10">
	<div class="mb-8">
		<h1 class="font-heading text-3xl font-semibold tracking-tight">
			Witaj{data.user.name ? `, ${data.user.name}` : ''}
		</h1>
		<p class="mt-2 text-sm text-muted-foreground">
			Instancja stoi i czeka na dane. Nic jeszcze nie przyszło.
		</p>
	</div>

	<div class="mb-10 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
		{#each kafelki as k (k.etykieta)}
			{@const Ikona = k.ikona}
			<Card.Root>
				<Card.Content class="pt-6">
					<div class="flex items-start justify-between">
						<div>
							<p class="text-sm text-muted-foreground">{k.etykieta}</p>
							<p class="mt-1 font-heading text-3xl font-semibold tabular-nums">{k.wartosc}</p>
							<p class="mt-1 text-xs text-muted-foreground">{k.opis}</p>
						</div>
						<Ikona class="size-4 shrink-0 text-muted-foreground" />
					</div>
				</Card.Content>
			</Card.Root>
		{/each}
	</div>

	<Separator class="mb-10" />

	<div class="grid gap-6 lg:grid-cols-3">
		<div class="lg:col-span-2">
			<h2 class="mb-1 font-heading text-xl font-semibold tracking-tight">Pierwsze kroki</h2>
			<p class="mb-4 text-sm text-muted-foreground">Trzy rzeczy, żeby zacząć widzieć cokolwiek.</p>

			<div class="space-y-3">
				{#each kroki as krok, i (krok.tytul)}
					<Card.Root class="p-4">
						<div class="flex gap-4">
							<div
								class="flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-sm font-medium text-muted-foreground tabular-nums"
							>
								{i + 1}
							</div>
							<div class="min-w-0 flex-1">
								<div class="flex items-center gap-2">
									<p class="text-sm font-medium">{krok.tytul}</p>
									{#if krok.gotowe}
										<Badge variant="secondary">gotowe</Badge>
									{/if}
								</div>
								<p class="mt-1 text-sm text-muted-foreground">{krok.opis}</p>
							</div>
						</div>
					</Card.Root>
				{/each}
			</div>
		</div>

		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">Podłączenie agenta</Card.Title>
				<Card.Description>Jedna zmienna środowiskowa.</Card.Description>
			</Card.Header>
			<Card.Content>
				<pre class="overflow-x-auto rounded-md bg-muted p-3 text-xs leading-relaxed"><code
						>DD_DD_URL=http://ninjacat:8080</code
					></pre>
				<p class="mt-3 text-xs text-muted-foreground">
					Reszta endpointów — logi, ślady, process-agent — ma osobne zmienne. Opisane w <code
						>docs/datadog-agent.md</code
					>.
				</p>
			</Card.Content>
		</Card.Root>
	</div>
</main>
