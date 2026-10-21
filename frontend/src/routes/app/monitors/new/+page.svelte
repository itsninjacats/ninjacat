<script lang="ts">
	import { resolve } from '$app/paths';
	import MonitorEditor from '$lib/components/app/monitors/monitor-editor.svelte';
	import { m } from '$lib/paraglide/messages';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>{m.monitor_new()} — ninjacat</title></svelte:head>

<div class="mx-auto max-w-6xl px-6 py-8">
	<div class="mb-6">
		<h1 class="font-heading text-2xl font-semibold tracking-tight">{m.monitor_new()}</h1>
		<p class="text-sm text-muted-foreground">{m.monitor_editor_intro()}</p>
	</div>
	{#if data.metricsError}
		<p class="mb-4 text-sm text-destructive" role="alert">
			{m.monitor_metrics_unavailable({ reason: data.metricsError })}
		</p>
	{/if}
	<MonitorEditor
		initial={data.initial}
		metrics={data.metrics}
		checks={data.checks}
		tenantId={data.tenant?.id ?? ''}
		submitLabel={m.monitor_create()}
		cancelHref={resolve('/app/monitors')}
		problems={form?.problems}
	/>
</div>
