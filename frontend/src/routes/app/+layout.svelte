<script lang="ts">
	import { enhance } from '$app/forms';
	import { page } from '$app/state';
	import { Button } from '$lib/components/ui/button';
	import { Separator } from '$lib/components/ui/separator';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();

	const pozycje = [
		{ href: '/app', etykieta: 'Przegląd' },
		{ href: '/app/metrics', etykieta: 'Metryki' },
		{ href: '/app/logs', etykieta: 'Logs' },
		{ href: '/app/ustawienia/klucze', etykieta: 'Klucze API' }
	];

	// Dokladne dopasowanie dla /app, prefiksowe dla podstron.
	function aktywna(href: string) {
		return href === '/app' ? page.url.pathname === '/app' : page.url.pathname.startsWith(href);
	}
</script>

<div class="min-h-svh bg-background">
	<header class="border-b">
		<div class="mx-auto flex h-14 max-w-6xl items-center justify-between px-6">
			<div class="flex items-center gap-6">
				<a href="/app" class="flex items-center gap-2">
					<div class="size-6 rounded-md bg-primary"></div>
					<span class="font-heading text-sm font-semibold tracking-tight">ninjacat</span>
				</a>

				<Separator orientation="vertical" class="h-5" />

				<nav class="flex items-center gap-1">
					{#each pozycje as p (p.href)}
						<Button
							href={p.href}
							variant="ghost"
							size="sm"
							class={aktywna(p.href) ? 'bg-accent text-accent-foreground' : ''}
						>
							{p.etykieta}
						</Button>
					{/each}
				</nav>
			</div>

			<div class="flex items-center gap-3">
				<span class="hidden text-sm text-muted-foreground sm:inline">{data.uzytkownik.email}</span>
				<!-- Akcje zyja tylko w +page.server.ts, wiec celujemy w akcje strony /app -->
				<form method="POST" action="/app?/wyloguj" use:enhance>
					<Button type="submit" variant="ghost" size="sm">Wyloguj</Button>
				</form>
			</div>
		</div>
	</header>

	{@render children()}
</div>
