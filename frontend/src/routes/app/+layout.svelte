<script lang="ts">
	import { page } from '$app/state';
	import AppSidebar from '$lib/components/app/app-sidebar.svelte';
	import { Separator } from '$lib/components/ui/separator';
	import * as Sidebar from '$lib/components/ui/sidebar/index.js';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();

	// The header shows where you are, because the sidebar collapses to icons and
	// then cannot.
	const title = $derived.by(() => {
		const p = page.url.pathname;
		if (p.startsWith('/app/metrics')) return 'Metrics';
		if (p.startsWith('/app/logs')) return 'Logs';
		if (p.startsWith('/app/ustawienia/klucze')) return 'API keys';
		return 'Overview';
	});
</script>

<Sidebar.Provider>
	<AppSidebar user={data.user} />

	<Sidebar.Inset class="flex h-svh flex-col overflow-hidden">
		<header class="flex h-14 shrink-0 items-center gap-2 border-b px-4">
			<Sidebar.Trigger class="-ms-1" />
			<Separator orientation="vertical" class="mr-2 h-4" />
			<h1 class="font-heading text-sm font-semibold tracking-tight">{title}</h1>
		</header>

		<div class="flex-1 overflow-auto">
			{@render children()}
		</div>
	</Sidebar.Inset>
</Sidebar.Provider>
