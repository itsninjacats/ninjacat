<script lang="ts">
	import { resolve } from '$app/paths';
	import { Button } from '$lib/components/ui/button';
	import MonitorEditor from '$lib/components/app/monitors/monitor-editor.svelte';
	import { m } from '$lib/paraglide/messages';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const statusHref = $derived(resolve('/app/monitors/[id]', { id: String(data.id) }));
</script>

<svelte:head><title>{m.monitor_edit()} — ninjacat</title></svelte:head>

<div class="mx-auto max-w-6xl px-6 py-8">
	<div class="mb-6 min-w-0">
		<h1 class="font-heading text-2xl font-semibold tracking-tight wrap-anywhere">
			{m.monitor_edit_title({ name: data.name })}
		</h1>
		{#if data.initial}
			<p class="text-sm text-muted-foreground">{m.monitor_editor_intro()}</p>
		{/if}
	</div>
	{#if !data.initial}
		<div class="space-y-3 rounded-md border border-dashed p-4 text-sm">
			<p>{m.monitor_not_from_editor()}</p>
			<pre
				class="rounded-md bg-muted p-3 font-mono text-[11px] wrap-anywhere whitespace-pre-wrap">{data.query}</pre>
			<Button variant="outline" href={statusHref}>{m.monitor_back_to_status()}</Button>
		</div>
	{:else}
		{#if data.drifted}
			<div class="mb-4 space-y-2 rounded-md border border-state-warn/50 p-3 text-sm" role="note">
				<p>{m.monitor_drifted()}</p>
				<pre
					class="rounded-md bg-muted p-3 font-mono text-[11px] wrap-anywhere whitespace-pre-wrap">{data.query}</pre>
			</div>
		{/if}
		{#if data.metricsError}
			<p class="mb-4 text-sm text-destructive" role="alert">
				{m.monitor_metrics_unavailable({ reason: data.metricsError })}
			</p>
		{/if}
		<MonitorEditor
			initial={data.initial}
			metrics={data.metrics}
			tenantId={data.tenant?.id ?? ''}
			submitLabel={m.monitor_save()}
			cancelHref={statusHref}
			problems={form?.problems}
		/>
	{/if}
</div>
