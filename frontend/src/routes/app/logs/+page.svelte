<script lang="ts">
	import { goto } from '$app/navigation';
	import { navigating, page } from '$app/state';
	import * as Card from '$lib/components/ui/card';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Separator } from '$lib/components/ui/separator';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const RANGES = [
		{ value: '-15m', label: '15m' },
		{ value: '-1h', label: '1h' },
		{ value: '-6h', label: '6h' },
		{ value: '-24h', label: '24h' },
		{ value: '-7d', label: '7d' }
	];

	// ---- URL is the single source of truth; controls navigate to a new one ----

	function apply(patch: Record<string, string | string[] | null>) {
		const q = new URLSearchParams(page.url.searchParams);
		for (const [key, value] of Object.entries(patch)) {
			q.delete(key);
			if (value === null || value === '') continue;
			if (Array.isArray(value)) {
				for (const v of value) q.append(key, v);
			} else {
				q.set(key, value);
			}
		}
		// keepFocus so typing in the search box survives the reload of data.
		goto(`?${q}`, { keepFocus: true, noScroll: true, replaceState: true });
	}

	// The search box edits local state and navigates after a pause, so every
	// keystroke does not fire a query. Seeded from the URL once; afterwards
	// the box itself is the source of what the user is typing.
	const initialQuery = page.url.searchParams.get('q') ?? '';
	let qInput = $state(initialQuery);
	let qTimer: ReturnType<typeof setTimeout> | undefined;
	function onSearchInput() {
		clearTimeout(qTimer);
		qTimer = setTimeout(() => apply({ q: qInput }), 350);
	}

	let tagInput = $state('');
	function addTag() {
		const raw = tagInput.trim();
		if (!raw.includes(':') || raw.startsWith(':')) return;
		if (!data.params.tags.includes(raw)) {
			apply({ tag: [...data.params.tags, raw] });
		}
		tagInput = '';
	}
	function removeTag(raw: string) {
		apply({ tag: data.params.tags.filter((t) => t !== raw) });
	}

	function toggleFacet(name: 'service' | 'host' | 'status', value: string) {
		apply({ [name]: data.params[name] === value ? null : value });
	}

	// ---- presentation ----

	// Newest first, whatever order the backend answered in.
	const logs = $derived(
		[...data.logs].sort((a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime())
	);

	// Row expansion is per-view; a new result set collapses everything.
	let expanded = $state<Set<number>>(new Set());
	$effect(() => {
		void data.logs;
		expanded = new Set();
	});
	function toggleRow(i: number) {
		const next = new Set(expanded);
		if (next.has(i)) next.delete(i);
		else next.add(i);
		expanded = next;
	}

	// Same grouped rendering as the metrics page: chips sharing a key are OR'd.
	const tagGroups = $derived.by(() => {
		const groups = new Map<string, string[]>();
		for (const raw of data.params.tags) {
			const i = raw.indexOf(':');
			if (i <= 0) continue;
			const key = raw.slice(0, i);
			const list = groups.get(key);
			if (list) list.push(raw);
			else groups.set(key, [raw]);
		}
		return [...groups.entries()].map(([key, raws]) => ({ key, raws }));
	});

	function statusClass(status: string): string {
		switch (status.toLowerCase()) {
			case 'error':
			case 'err':
			case 'critical':
			case 'fatal':
			case 'emergency':
				return 'bg-red-500/15 text-red-700 dark:text-red-400';
			case 'warn':
			case 'warning':
				return 'bg-amber-500/15 text-amber-700 dark:text-amber-400';
			case 'info':
			case 'notice':
				return 'bg-blue-500/15 text-blue-700 dark:text-blue-400';
			case 'debug':
			case 'trace':
				return 'bg-muted text-muted-foreground';
			default:
				return 'bg-muted text-muted-foreground';
		}
	}

	function formatTime(t: string): string {
		const d = new Date(t);
		if (Number.isNaN(d.getTime())) return t;
		return d.toLocaleString(undefined, {
			month: 'short',
			day: '2-digit',
			hour: '2-digit',
			minute: '2-digit',
			second: '2-digit',
			hour12: false
		});
	}

	const facetSections = $derived([
		{ name: 'service' as const, label: 'Service', items: data.facets.services },
		{ name: 'host' as const, label: 'Host', items: data.facets.hosts },
		{ name: 'status' as const, label: 'Status', items: data.facets.statuses }
	]);
</script>

<svelte:head><title>Logs — ninjacat</title></svelte:head>

<div class="mx-auto max-w-6xl px-6 py-8">
	<div class="mb-6">
		<h1 class="font-heading text-2xl font-semibold tracking-tight">Logs</h1>
		<p class="text-sm text-muted-foreground">
			Search and filter log events straight from ClickHouse.
		</p>
	</div>

	<!-- Search bar + time range -->
	<div class="mb-4 flex flex-wrap items-center gap-3">
		<Input
			bind:value={qInput}
			oninput={onSearchInput}
			onkeydown={(e) => {
				if (e.key === 'Enter') {
					clearTimeout(qTimer);
					apply({ q: qInput });
				}
			}}
			placeholder="Search log messages…"
			class="h-9 min-w-64 flex-1 text-sm"
		/>
		<div class="flex items-center gap-1">
			{#each RANGES as r (r.value)}
				<Button
					size="sm"
					variant={data.params.from === r.value ? 'default' : 'ghost'}
					onclick={() => apply({ from: r.value })}
				>
					{r.label}
				</Button>
			{/each}
		</div>
	</div>

	<!-- Tag filter chips -->
	<div class="mb-4 flex flex-wrap items-center gap-2">
		<Input
			bind:value={tagInput}
			onkeydown={(e) => {
				if (e.key === 'Enter') addTag();
			}}
			placeholder="Add tag filter (key:value)…"
			class="h-8 w-56 font-mono text-xs"
		/>
		{#if tagGroups.length > 0}
			<Separator orientation="vertical" class="h-5" />
			{#each tagGroups as g, gi (g.key)}
				{#if gi > 0}
					<span class="text-[10px] font-semibold text-muted-foreground uppercase">and</span>
				{/if}
				<div class="flex items-center gap-1 rounded-md border bg-muted/40 px-1.5 py-1">
					<span class="font-mono text-[11px] text-muted-foreground">{g.key}:</span>
					{#each g.raws as raw, ri (raw)}
						{#if ri > 0}
							<span class="text-[10px] font-semibold text-muted-foreground uppercase">or</span>
						{/if}
						<Badge variant="secondary" class="gap-1 font-mono text-[11px]">
							{raw.slice(g.key.length + 1)}
							<button
								type="button"
								class="ml-0.5 opacity-60 hover:opacity-100"
								onclick={() => removeTag(raw)}
								aria-label={`Remove filter ${raw}`}
							>
								×
							</button>
						</Badge>
					{/each}
				</div>
			{/each}
			<Button size="sm" variant="ghost" onclick={() => apply({ tag: null })}>clear</Button>
		{/if}
	</div>

	<div class="grid gap-6 lg:grid-cols-[220px_1fr]">
		<!-- Facet sidebar -->
		<div class="space-y-4">
			{#if data.facetsError}
				<Card.Root class="border-destructive/40">
					<Card.Content class="px-4 py-3 text-xs text-muted-foreground">
						Facets unavailable: {data.facetsError}
					</Card.Content>
				</Card.Root>
			{/if}
			{#each facetSections as section (section.name)}
				<Card.Root class="h-fit">
					<Card.Header class="pb-2">
						<Card.Title class="text-xs tracking-wide text-muted-foreground uppercase">
							{section.label}
						</Card.Title>
					</Card.Header>
					<Card.Content class="space-y-0.5 pb-3">
						{#each section.items as item (item.value)}
							{@const active = data.params[section.name] === item.value}
							<button
								type="button"
								onclick={() => toggleFacet(section.name, item.value)}
								class="flex w-full items-center justify-between gap-2 rounded-md px-2 py-1 text-left text-xs transition-colors
									{active ? 'bg-primary text-primary-foreground' : 'hover:bg-muted'}"
								title={item.value}
							>
								<span class="truncate font-mono">{item.value}</span>
								<span class={active ? 'opacity-80' : 'text-muted-foreground'}>
									{item.count}
								</span>
							</button>
						{:else}
							<p class="px-2 py-1 text-xs text-muted-foreground">Nothing in this range.</p>
						{/each}
					</Card.Content>
				</Card.Root>
			{/each}
		</div>

		<!-- Result list -->
		<Card.Root class="h-fit">
			<Card.Header class="pb-3">
				<div class="flex items-baseline justify-between gap-3">
					<Card.Title class="text-sm">Events</Card.Title>
					<Card.Description>
						{#if navigating.to}
							loading…
						{:else if data.searchError}
							&nbsp;
						{:else}
							{data.count} matching{data.count > logs.length ? ` · showing ${logs.length}` : ''}
						{/if}
					</Card.Description>
				</div>
			</Card.Header>
			<Card.Content class="px-0 pb-0">
				{#if data.searchError}
					<div
						class="mx-6 mb-6 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm"
					>
						{data.searchError}
					</div>
				{:else if logs.length === 0}
					<div
						class="mx-6 mb-6 flex h-40 items-center justify-center rounded-md border border-dashed text-sm text-muted-foreground"
					>
						No log events match. Try a wider range or fewer filters.
					</div>
				{:else}
					<div class="border-t {navigating.to ? 'opacity-50 transition-opacity' : ''}">
						{#each logs as log, i (i)}
							{@const open = expanded.has(i)}
							<div class="border-b last:border-b-0">
								<button
									type="button"
									onclick={() => toggleRow(i)}
									class="grid w-full grid-cols-[130px_64px_minmax(0,120px)_minmax(0,120px)_minmax(0,1fr)] items-center gap-x-3 px-4 py-1.5 text-left font-mono text-xs transition-colors hover:bg-muted/50
										{open ? 'bg-muted/40' : ''}"
								>
									<span class="whitespace-nowrap text-muted-foreground">
										{formatTime(log.timestamp)}
									</span>
									<span
										class="inline-flex justify-center rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase {statusClass(
											log.status
										)}"
									>
										{log.status || '—'}
									</span>
									<span class="truncate" title={log.service}>{log.service}</span>
									<span class="truncate text-muted-foreground" title={log.host}>{log.host}</span>
									<span class="truncate">{log.message}</span>
								</button>

								{#if open}
									<div class="space-y-3 border-t bg-muted/30 px-4 py-3">
										<pre
											class="max-h-64 overflow-y-auto font-mono text-xs break-all whitespace-pre-wrap">{log.message}</pre>
										<div
											class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 font-mono text-xs text-muted-foreground"
										>
											<span>timestamp</span><span class="text-foreground">{log.timestamp}</span>
											<span>service</span><span class="text-foreground">{log.service}</span>
											<span>host</span><span class="text-foreground">{log.host}</span>
											<span>source</span><span class="text-foreground">{log.source}</span>
											<span>status</span><span class="text-foreground">{log.status}</span>
											{#each Object.entries(log.tags ?? {}) as [key, values] (key)}
												<span>{key}</span>
												<span class="flex flex-wrap gap-1">
													{#each values as v (v)}
														<Badge variant="outline" class="font-mono text-[10px]">{v}</Badge>
													{/each}
												</span>
											{/each}
										</div>
									</div>
								{/if}
							</div>
						{/each}
					</div>
				{/if}
			</Card.Content>
		</Card.Root>
	</div>
</div>
