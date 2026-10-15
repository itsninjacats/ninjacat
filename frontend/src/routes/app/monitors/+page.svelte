<script lang="ts">
	import { afterNavigate, replaceState } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import BellIcon from '@lucide/svelte/icons/bell';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import SearchIcon from '@lucide/svelte/icons/search';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { Input } from '$lib/components/ui/input';
	import * as Table from '$lib/components/ui/table';
	import LocalTime from '$lib/components/app/local-time.svelte';
	import StateBadge from '$lib/components/app/monitors/state-badge.svelte';
	import { STATES, methodLabel, stateLabel } from '$lib/monitors/states';
	import { m } from '$lib/paraglide/messages';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const TAGS_SHOWN = 4;

	// "none" is a monitor with no open group.
	const FILTERS = ['all', ...STATES, 'none'] as const;
	type Filter = (typeof FILTERS)[number];

	const params = page.url.searchParams;
	let search = $state(params.get('q') ?? '');
	let status = $state<Filter>(
		FILTERS.includes(params.get('status') as Filter) ? (params.get('status') as Filter) : 'all'
	);

	// The filters live in the address, so a filtered list can be linked to.
	let routerReady = $state(false);
	afterNavigate(() => (routerReady = true));
	$effect(() => {
		const q = new URLSearchParams([
			...(search.trim() ? [['q', search.trim()]] : []),
			...(status !== 'all' ? [['status', status]] : [])
		]).toString();
		const target = q ? `?${q}` : page.url.pathname;
		if (!routerReady) return;
		const current = page.url.search || page.url.pathname;
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		if (current !== target) replaceState(target, {});
	});

	// A monitor whose state the query process did not give counts under "all" only.
	const stateOf = (monitor: (typeof data.monitors)[number]): Filter | 'unknown' =>
		monitor.known ? (monitor.state ?? 'none') : 'unknown';

	const matching = $derived.by(() => {
		const words = search.toLowerCase().split(/\s+/).filter(Boolean);
		return data.monitors.filter((monitor) => {
			const haystack = [monitor.name, monitor.query, ...monitor.tags].join(' ').toLowerCase();
			return words.every((w) => haystack.includes(w));
		});
	});

	const counts = $derived(
		Object.fromEntries(
			FILTERS.map((f) => [
				f,
				f === 'all' ? matching.length : matching.filter((x) => stateOf(x) === f).length
			])
		) as Record<Filter, number>
	);

	const shown = $derived(
		status === 'all' ? matching : matching.filter((x) => stateOf(x) === status)
	);

	const filterLabel = (f: Filter) =>
		f === 'all' ? m.monitors_filter_all() : stateLabel(f === 'none' ? null : f);
</script>

<svelte:head><title>{m.monitors()} — ninjacat</title></svelte:head>

<!-- The first few tags; the monitor's page shows them all. -->
{#snippet tags(all: string[])}
	<div class="flex flex-wrap gap-1">
		{#each all.slice(0, TAGS_SHOWN) as tag, i (i)}
			<span class="rounded border px-1 font-mono text-[11px] wrap-anywhere">{tag}</span>
		{/each}
		{#if all.length > TAGS_SHOWN}
			<span class="px-1 text-[11px] text-muted-foreground">
				{m.monitors_more_tags({ count: all.length - TAGS_SHOWN })}
			</span>
		{/if}
	</div>
{/snippet}

<div class="mx-auto max-w-6xl px-6 py-8">
	<div class="mb-6 flex flex-wrap items-end justify-between gap-4">
		<div class="min-w-0">
			<h1 class="font-heading text-2xl font-semibold tracking-tight">{m.monitors()}</h1>
			<p class="text-sm text-muted-foreground">{m.monitors_intro()}</p>
		</div>
		<Button href={resolve('/app/monitors/new')}>
			<PlusIcon class="size-4" />
			{m.monitor_new()}
		</Button>
	</div>

	{#if data.statusError}
		<p class="mb-4 text-sm text-destructive" role="alert">
			{m.monitors_status_unavailable({ reason: data.statusError })}
		</p>
	{/if}

	{#if data.monitors.length === 0}
		<Card.Root class="p-10">
			<div class="flex flex-col items-center gap-3 text-center">
				<BellIcon class="size-8 text-muted-foreground" />
				<div>
					<p class="text-sm font-medium">{m.monitors_empty()}</p>
					<p class="mt-1 text-sm text-muted-foreground">{m.monitors_empty_hint()}</p>
				</div>
			</div>
		</Card.Root>
	{:else}
		<div class="mb-4 flex flex-wrap items-center gap-3">
			<div class="relative min-w-0 grow basis-64">
				<SearchIcon
					class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground"
				/>
				<Input
					bind:value={search}
					placeholder={m.monitors_search()}
					aria-label={m.monitors_search()}
					class="pl-8"
				/>
			</div>
			<div class="flex flex-wrap gap-1" role="group" aria-label={m.monitors_filter_status()}>
				{#each FILTERS as f (f)}
					<Button
						size="sm"
						variant={status === f ? 'default' : 'outline'}
						aria-pressed={status === f}
						onclick={() => (status = f)}
					>
						{filterLabel(f)}
						<span class="tabular-nums opacity-70">{counts[f]}</span>
					</Button>
				{/each}
			</div>
		</div>

		{#if shown.length === 0}
			<p class="py-10 text-center text-sm text-muted-foreground">{m.monitors_none_match()}</p>
		{:else}
			<!-- Below the width the columns need, a monitor is its name, with what
			     those columns hold underneath. -->
			<div class="@container rounded-md border">
				<Table.Root>
					<Table.Header>
						<Table.Row>
							<Table.Head class="w-28">{m.monitor_status()}</Table.Head>
							<Table.Head>{m.monitor_name()}</Table.Head>
							<Table.Head class="hidden @3xl:table-cell">{m.monitor_type()}</Table.Head>
							<Table.Head class="hidden @3xl:table-cell">{m.monitor_tags()}</Table.Head>
							<Table.Head class="hidden @3xl:table-cell">{m.monitor_last_triggered()}</Table.Head>
						</Table.Row>
					</Table.Header>
					<Table.Body>
						{#each shown as monitor (monitor.id)}
							<Table.Row>
								<Table.Cell class="align-top">
									{#if monitor.known}
										<StateBadge state={monitor.state} />
									{:else}
										<span class="text-xs text-muted-foreground">{m.monitor_state_unknown()}</span>
									{/if}
								</Table.Cell>
								<Table.Cell class="align-top whitespace-normal">
									<a
										href={resolve('/app/monitors/[id]', { id: String(monitor.id) })}
										class="font-medium wrap-anywhere hover:underline"
									>
										{monitor.name}
									</a>
									<div class="mt-1 space-y-1 text-xs text-muted-foreground @3xl:hidden">
										<p>
											{methodLabel(monitor.method)}
											{#if monitor.priority}· P{monitor.priority}{/if}
											·
											{#if monitor.lastTriggeredAt}
												{m.monitor_last_triggered()} <LocalTime at={monitor.lastTriggeredAt} />
											{:else}
												{m.monitor_never_triggered()}
											{/if}
										</p>
										{#if monitor.tags.length > 0}
											{@render tags(monitor.tags)}
										{/if}
									</div>
								</Table.Cell>
								<Table.Cell class="hidden align-top text-sm @3xl:table-cell">
									{methodLabel(monitor.method)}
									{#if monitor.priority}
										<span class="text-muted-foreground">· P{monitor.priority}</span>
									{/if}
								</Table.Cell>
								<Table.Cell class="hidden align-top whitespace-normal @3xl:table-cell">
									<div class="max-w-xs">{@render tags(monitor.tags)}</div>
								</Table.Cell>
								<Table.Cell class="hidden align-top text-sm text-muted-foreground @3xl:table-cell">
									{#if monitor.lastTriggeredAt}
										<LocalTime at={monitor.lastTriggeredAt} />
									{:else}
										{m.monitor_never_triggered()}
									{/if}
								</Table.Cell>
							</Table.Row>
						{/each}
					</Table.Body>
				</Table.Root>
			</div>
		{/if}
	{/if}
</div>
