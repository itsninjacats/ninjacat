<script lang="ts">
	import { resolve } from '$app/paths';
	import * as Card from '$lib/components/ui/card';
	import { Separator } from '$lib/components/ui/separator';
	import ActivityIcon from '@lucide/svelte/icons/activity';
	import ServerIcon from '@lucide/svelte/icons/server';
	import BellIcon from '@lucide/svelte/icons/bell';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import { m } from '$lib/paraglide/messages';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	// Hosts, metrics and logs are placeholders until the server counts them.
	const placeholders = $derived([
		{ label: m.overview_hosts(), value: '0', hint: m.overview_hosts_hint(), icon: ServerIcon },
		{
			label: m.overview_metrics(),
			value: '0',
			hint: m.overview_metrics_hint(),
			icon: ActivityIcon
		},
		{ label: m.overview_logs(), value: '0', hint: m.overview_logs_hint(), icon: FileTextIcon }
	]);

	const monitorsHint = $derived.by(() => {
		const s = data.monitors;
		if (!s.known) return m.overview_monitors_unknown();
		if (s.alert + s.warn + s.noData === 0) return m.overview_monitors_quiet();
		return m.overview_monitors_states({ alert: s.alert, warn: s.warn, noData: s.noData });
	});

	const steps = $derived([
		{ title: m.overview_step_agent(), hint: m.overview_step_agent_hint() },
		{ title: m.overview_step_data(), hint: m.overview_step_data_hint() },
		{ title: m.overview_step_monitor(), hint: m.overview_step_monitor_hint() }
	]);
</script>

<svelte:head><title>ninjacat</title></svelte:head>

<main class="mx-auto max-w-6xl px-6 py-10">
	<div class="mb-8">
		<h1 class="font-heading text-3xl font-semibold tracking-tight wrap-anywhere">
			{data.user.name ? m.overview_welcome_name({ name: data.user.name }) : m.overview_welcome()}
		</h1>
		<p class="mt-2 text-sm text-muted-foreground">{m.overview_intro()}</p>
	</div>

	<div class="mb-10 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
		{#each placeholders as tile (tile.label)}
			<Card.Root>
				<Card.Content>
					<div class="flex items-start justify-between">
						<div>
							<p class="text-sm text-muted-foreground">{tile.label}</p>
							<p class="mt-1 font-heading text-3xl font-semibold tabular-nums">{tile.value}</p>
							<p class="mt-1 text-xs text-muted-foreground">{tile.hint}</p>
						</div>
						<tile.icon class="size-4 shrink-0 text-muted-foreground" />
					</div>
				</Card.Content>
			</Card.Root>
		{/each}
		<a href={resolve('/app/monitors')} class="rounded-xl hover:ring-1 hover:ring-ring">
			<Card.Root class="h-full">
				<Card.Content>
					<div class="flex items-start justify-between">
						<div>
							<p class="text-sm text-muted-foreground">{m.monitors()}</p>
							<p class="mt-1 font-heading text-3xl font-semibold tabular-nums">
								{data.monitors.total}
							</p>
							<p class="mt-1 text-xs text-muted-foreground">{monitorsHint}</p>
						</div>
						<BellIcon class="size-4 shrink-0 text-muted-foreground" />
					</div>
				</Card.Content>
			</Card.Root>
		</a>
	</div>

	<Separator class="mb-10" />

	<div class="grid gap-6 lg:grid-cols-3">
		<div class="lg:col-span-2">
			<h2 class="mb-1 font-heading text-xl font-semibold tracking-tight">
				{m.overview_first_steps()}
			</h2>
			<p class="mb-4 text-sm text-muted-foreground">{m.overview_first_steps_hint()}</p>

			<div class="space-y-3">
				{#each steps as step, i (step.title)}
					<Card.Root class="p-4">
						<div class="flex gap-4">
							<div
								class="flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-sm font-medium text-muted-foreground tabular-nums"
							>
								{i + 1}
							</div>
							<div class="min-w-0 flex-1">
								<p class="text-sm font-medium">{step.title}</p>
								<p class="mt-1 text-sm text-muted-foreground">{step.hint}</p>
							</div>
						</div>
					</Card.Root>
				{/each}
			</div>
		</div>

		<Card.Root>
			<Card.Header>
				<Card.Title class="text-base">{m.overview_connect()}</Card.Title>
				<Card.Description>{m.overview_connect_hint()}</Card.Description>
			</Card.Header>
			<Card.Content>
				<pre class="overflow-x-auto rounded-md bg-muted p-3 text-xs leading-relaxed"><code
						>DD_DD_URL=http://ninjacat:8080</code
					></pre>
				<p class="mt-3 text-xs text-muted-foreground">
					{m.overview_connect_more()}
					<code class="whitespace-nowrap">docs/datadog-agent.md</code>.
				</p>
			</Card.Content>
		</Card.Root>
	</div>
</main>
