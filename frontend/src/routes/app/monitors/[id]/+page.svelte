<script lang="ts">
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { onMount } from 'svelte';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import * as Table from '$lib/components/ui/table';
	import LocalTime from '$lib/components/app/local-time.svelte';
	import MetricsChart, {
		seriesColor,
		type ChartLine
	} from '$lib/components/app/metrics/uplot-chart.svelte';
	import StateBadge from '$lib/components/app/monitors/state-badge.svelte';
	import { alertZone } from '$lib/monitors/definition';
	import {
		STATES,
		methodLabel,
		reasonLabel,
		severity,
		stateClass,
		stateLabel,
		thresholdRules
	} from '$lib/monitors/states';
	import { timelines } from '$lib/monitors/timeline';
	import { m } from '$lib/paraglide/messages';
	import { getLocale } from '$lib/paraglide/runtime';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	// Rows beyond these are counted, not drawn.
	const MAX_TIMELINES = 50;
	const MAX_TRANSITIONS = 200;

	const groups = $derived(
		(data.groups ?? []).toSorted(
			(a, b) => severity(a.state) - severity(b.state) || a.group.localeCompare(b.group)
		)
	);
	// The monitor's state is its most severe group's.
	const overall = $derived(data.groups && data.groups.length > 0 ? groups[0].state : null);

	const from = $derived(Date.parse(data.window.from));
	const to = $derived(Date.parse(data.window.to));

	const number = $derived(new Intl.NumberFormat(getLocale(), { maximumFractionDigits: 4 }));

	// Each group's evaluated values; the monitor-wide rows (no group) carry none.
	const evaluated = $derived.by(() => {
		const byGroup: Record<string, { t: number; v: number }[]> = {};
		for (const e of data.evaluations?.evaluations ?? []) {
			if (!e.group || e.value === null) continue;
			(byGroup[e.group] ??= []).push({ t: Date.parse(e.at), v: e.value });
		}
		return byGroup;
	});
	const evaluatedGroups = $derived(Object.keys(evaluated));
	// The server fetches the values of the drawn groups only.
	const shownOf = $derived(Math.max(evaluatedGroups.length, groups.length));

	const lines = $derived.by<ChartLine[]>(() => {
		const rank = Object.fromEntries(groups.map((g, i) => [g.group, i]));
		const keys = evaluatedGroups.toSorted(
			(a, b) => (rank[a] ?? Infinity) - (rank[b] ?? Infinity) || a.localeCompare(b)
		);
		return keys.slice(0, data.maxLines).map((group, i) => ({
			key: `g${i}`,
			label: group,
			color: seriesColor(i),
			points: evaluated[group],
			band: [],
			forecast: []
		}));
	});

	const rules = $derived(data.monitor.method === 'outlier' ? [] : thresholdRules(data.thresholds));
	const zone = $derived(
		rules.length === 0 || !data.thresholds.comparator
			? null
			: alertZone(data.thresholds.comparator, data.thresholds.critical)
	);

	const rows = $derived(
		data.transitions
			? timelines(
					data.transitions.initial,
					data.transitions.transitions,
					from,
					to,
					data.transitions.truncated
				)
			: []
	);

	const transitions = $derived((data.transitions?.transitions ?? []).toReversed());

	// The browser's zone, which the server cannot know.
	let zoneName = $state('');
	onMount(() => (zoneName = Intl.DateTimeFormat().resolvedOptions().timeZone));

	const groupLabel = (group: string) => (group === '*' ? m.monitor_group_all() : group);
</script>

<svelte:head><title>{data.monitor.name} — ninjacat</title></svelte:head>

<div class="mx-auto max-w-6xl space-y-6 px-6 py-8">
	<div class="flex flex-wrap items-start justify-between gap-4">
		<div class="min-w-0 space-y-2">
			<div class="flex flex-wrap items-center gap-2">
				{#if data.groups}
					<StateBadge state={overall} />
				{/if}
				<span class="text-sm text-muted-foreground">
					{methodLabel(data.monitor.method)}
					{#if data.monitor.priority}· P{data.monitor.priority}{/if}
				</span>
			</div>
			<h1 class="font-heading text-2xl font-semibold tracking-tight wrap-anywhere">
				{data.monitor.name}
			</h1>
			{#if data.monitor.tags.length > 0}
				<div class="flex flex-wrap gap-1">
					{#each data.monitor.tags as tag, i (i)}
						<span class="rounded border px-1 font-mono text-[11px] wrap-anywhere">{tag}</span>
					{/each}
				</div>
			{/if}
		</div>
		<div class="flex flex-wrap gap-2">
			<Button
				variant="outline"
				href={resolve('/app/monitors/[id]/edit', { id: String(data.monitor.id) })}
			>
				<PencilIcon class="size-4" />
				{m.monitor_edit()}
			</Button>
			<AlertDialog.Root>
				<AlertDialog.Trigger>
					{#snippet child({ props })}
						<Button {...props} variant="outline">
							<Trash2Icon class="size-4" />
							{m.monitor_delete()}
						</Button>
					{/snippet}
				</AlertDialog.Trigger>
				<AlertDialog.Content>
					<AlertDialog.Header>
						<AlertDialog.Title class="wrap-anywhere">
							{m.monitor_delete_confirm({ name: data.monitor.name })}
						</AlertDialog.Title>
						<AlertDialog.Description>{m.monitor_delete_explained()}</AlertDialog.Description>
					</AlertDialog.Header>
					<AlertDialog.Footer>
						<AlertDialog.Cancel>{m.cancel()}</AlertDialog.Cancel>
						<form method="POST" action="?/delete" use:enhance>
							<input type="hidden" name="tenant" value={data.tenant?.id} />
							<AlertDialog.Action type="submit" class="w-full sm:w-auto">
								{m.monitor_delete()}
							</AlertDialog.Action>
						</form>
					</AlertDialog.Footer>
				</AlertDialog.Content>
			</AlertDialog.Root>
		</div>
	</div>

	{#if form?.deleteError}
		<p class="text-sm text-destructive" role="alert">{form.deleteError}</p>
	{/if}
	{#if data.readError}
		<p class="text-sm text-destructive" role="alert">
			{m.monitor_read_failed({ reason: data.readError })}
		</p>
	{/if}

	<div class="flex flex-wrap items-center justify-between gap-2">
		<p class="text-xs text-muted-foreground">
			{zoneName ? m.monitor_times_local({ zone: zoneName }) : m.monitor_times_utc()}
		</p>
		<div class="flex flex-wrap gap-1">
			{#each ['-1h', '-4h', '-1d', '-1w'] as r (r)}
				<Button
					size="sm"
					variant={data.range === r ? 'default' : 'ghost'}
					href={`?range=${r}`}
					data-sveltekit-noscroll
				>
					{r.slice(1)}
				</Button>
			{/each}
		</div>
	</div>

	<!-- evaluated values -->
	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">{m.monitor_evaluated_values()}</Card.Title>
			<Card.Description>
				{#if shownOf > data.maxLines}
					{m.monitor_lines_shown({ shown: data.maxLines, total: shownOf })}
				{:else}
					{m.monitor_evaluated_values_hint()}
				{/if}
				{#if data.evaluations?.truncated}
					{m.monitor_evaluations_truncated()}
				{/if}
			</Card.Description>
		</Card.Header>
		<Card.Content>
			{#if lines.length === 0}
				<div
					class="flex h-[200px] items-center justify-center rounded-md border border-dashed px-4 text-center text-sm text-muted-foreground"
				>
					{m.monitor_no_evaluations()}
				</div>
			{:else}
				<MetricsChart {lines} window={{ from, to }} display="line" {rules} {zone} />
			{/if}
		</Card.Content>
	</Card.Root>

	<!-- state timeline -->
	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">{m.monitor_timeline()}</Card.Title>
			{#if rows.length > MAX_TIMELINES}
				<Card.Description>
					{m.monitor_timelines_shown({ shown: MAX_TIMELINES, total: rows.length })}
				</Card.Description>
			{/if}
		</Card.Header>
		<Card.Content class="@container space-y-3">
			{#if rows.length === 0}
				<p class="text-sm text-muted-foreground">{m.monitor_no_transitions()}</p>
			{:else}
				<div class="space-y-2">
					{#each rows.slice(0, MAX_TIMELINES) as row (row.group)}
						<div class="grid grid-cols-1 items-center gap-x-4 gap-y-1 @3xl:grid-cols-[2fr_3fr]">
							<p class="font-mono text-[11px] wrap-anywhere text-muted-foreground">
								{groupLabel(row.group)}
							</p>
							<div class="flex h-3 w-full overflow-hidden rounded-sm bg-muted">
								{#each row.segments as segment (segment.from)}
									<div
										class="h-full {segment.state ? stateClass(segment.state) : ''}"
										style="width: {((segment.to - segment.from) / (to - from)) * 100}%"
										title={segment.state ? stateLabel(segment.state) : undefined}
									></div>
								{/each}
							</div>
						</div>
					{/each}
				</div>
				<div
					class="grid grid-cols-1 gap-x-4 gap-y-2 text-[11px] text-muted-foreground @3xl:grid-cols-[2fr_3fr]"
				>
					<div class="flex flex-wrap gap-3">
						{#each STATES as s (s)}
							<span class="flex items-center gap-1">
								<span class="size-2 rounded-full {stateClass(s)}"></span>
								{stateLabel(s)}
							</span>
						{/each}
					</div>
					<div class="flex flex-wrap justify-between gap-x-4">
						<LocalTime at={data.window.from} />
						<LocalTime at={data.window.to} />
					</div>
				</div>
			{/if}
		</Card.Content>
	</Card.Root>

	<!-- groups -->
	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">{m.monitor_groups({ count: groups.length })}</Card.Title>
		</Card.Header>
		<Card.Content>
			{#if groups.length === 0}
				<p class="text-sm text-muted-foreground">{m.monitor_no_groups()}</p>
			{:else}
				<div class="@container">
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head class="w-28">{m.monitor_status()}</Table.Head>
								<Table.Head>{m.monitor_group()}</Table.Head>
								<Table.Head class="hidden @2xl:table-cell">{m.monitor_value()}</Table.Head>
								<Table.Head class="hidden @2xl:table-cell">{m.monitor_since()}</Table.Head>
								<Table.Head class="hidden @2xl:table-cell">
									{m.monitor_last_triggered()}
								</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each groups as g (g.group)}
								<Table.Row>
									<Table.Cell class="align-top"><StateBadge state={g.state} /></Table.Cell>
									<Table.Cell class="align-top font-mono text-xs wrap-anywhere whitespace-normal">
										{groupLabel(g.group)}
										<div class="mt-1 font-sans text-muted-foreground @2xl:hidden">
											{g.value === null ? '—' : number.format(g.value)} ·
											{m.monitor_since()}
											<LocalTime at={g.since} /> · {reasonLabel(g.reason)}
										</div>
									</Table.Cell>
									<Table.Cell class="hidden align-top font-mono text-xs @2xl:table-cell">
										{g.value === null ? '—' : number.format(g.value)}
									</Table.Cell>
									<Table.Cell class="hidden align-top text-xs whitespace-normal @2xl:table-cell">
										<LocalTime at={g.since} />
										<span class="block text-muted-foreground">{reasonLabel(g.reason)}</span>
									</Table.Cell>
									<Table.Cell class="hidden align-top text-xs @2xl:table-cell">
										{#if g.lastTriggeredAt}
											<LocalTime at={g.lastTriggeredAt} />
										{:else}
											<span class="text-muted-foreground">{m.monitor_never_triggered()}</span>
										{/if}
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</div>
			{/if}
		</Card.Content>
	</Card.Root>

	<!-- transitions -->
	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">{m.monitor_transitions()}</Card.Title>
			{#if transitions.length > MAX_TRANSITIONS || data.transitions?.truncated}
				<Card.Description
					>{m.monitor_transitions_truncated({ shown: MAX_TRANSITIONS })}</Card.Description
				>
			{/if}
		</Card.Header>
		<Card.Content>
			{#if transitions.length === 0}
				<p class="text-sm text-muted-foreground">{m.monitor_no_transitions()}</p>
			{:else}
				<ul class="divide-y">
					{#each transitions.slice(0, MAX_TRANSITIONS) as t, i (i)}
						<li class="flex flex-wrap items-start gap-x-3 gap-y-1 py-2 text-xs">
							<span class="w-40 shrink-0 text-muted-foreground"
								><LocalTime at={t.at} seconds /></span
							>
							<span class="flex shrink-0 items-center gap-1">
								{#if t.fromState}<StateBadge state={t.fromState} /> →{/if}
								<StateBadge state={t.toState} />
							</span>
							<span class="min-w-0 grow basis-48 space-y-0.5">
								<span class="block font-mono wrap-anywhere">{groupLabel(t.group)}</span>
								<span class="block text-muted-foreground">
									{reasonLabel(t.reason)}
									{#if t.value !== null}· {m.monitor_value()} {number.format(t.value)}{/if}
									{#if t.crossed && t.crossed !== 'none'}· {t.crossed}{/if}
								</span>
							</span>
						</li>
					{/each}
				</ul>
			{/if}
		</Card.Content>
	</Card.Root>

	<!-- definition -->
	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">{m.monitor_definition()}</Card.Title>
			<Card.Description>
				<span>{m.monitor_created()} <LocalTime at={data.monitor.createdAt} /></span> ·
				<span>{m.monitor_updated()} <LocalTime at={data.monitor.updatedAt} /></span>
			</Card.Description>
		</Card.Header>
		<Card.Content class="space-y-4 text-xs">
			<div class="space-y-1">
				<p class="font-medium">{m.monitor_query()}</p>
				<pre
					class="rounded-md bg-muted p-3 font-mono text-[11px] wrap-anywhere whitespace-pre-wrap">{data
						.monitor.query}</pre>
			</div>
			{#if data.monitor.message}
				<div class="space-y-1">
					<p class="font-medium">{m.monitor_message()}</p>
					<pre
						class="rounded-md bg-muted p-3 font-mono text-[11px] wrap-anywhere whitespace-pre-wrap">{data
							.monitor.message}</pre>
				</div>
			{/if}
			<div class="space-y-1">
				<p class="font-medium">{m.monitor_options()}</p>
				<pre
					class="rounded-md bg-muted p-3 font-mono text-[11px] wrap-anywhere whitespace-pre-wrap">{JSON.stringify(
						data.monitor.options,
						null,
						2
					)}</pre>
			</div>
		</Card.Content>
	</Card.Root>
</div>
