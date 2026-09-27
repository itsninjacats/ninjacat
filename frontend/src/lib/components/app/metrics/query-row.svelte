<script lang="ts">
	import CodeIcon from '@lucide/svelte/icons/code';
	import EyeIcon from '@lucide/svelte/icons/eye';
	import EyeOffIcon from '@lucide/svelte/icons/eye-off';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import SlidersIcon from '@lucide/svelte/icons/sliders-horizontal';
	import XIcon from '@lucide/svelte/icons/x';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
	import * as Popover from '$lib/components/ui/popover/index.js';
	import { Input } from '$lib/components/ui/input';
	import FunctionChips from './function-chips.svelte';
	import { SPACE_AGGREGATORS, builderText, type ExplorerQuery } from '$lib/metrics/query';

	let {
		query = $bindable(),
		color,
		metrics,
		onremove
	}: {
		query: ExplorerQuery;
		color: string;
		/** The first page of metric names, shown before anything is typed. */
		metrics: string[];
		onremove?: () => void;
	} = $props();

	const TEXT_PLACEHOLDER = 'avg:system.cpu.user{env:prod} by {host}';

	const b = $derived(query.builder);

	// ---- metric picker: type to search, pick from the dropdown ----

	let metricSearch = $state('');
	let metricOptions = $state<string[]>([]);
	let metricOpen = $state(false);

	$effect(() => {
		if (!metricOpen) return;
		const search = metricSearch;
		const timer = setTimeout(
			() => {
				fetch(`/app/metrics/meta?what=names&search=${encodeURIComponent(search)}`)
					.then((r) => r.json())
					.then((body) => (metricOptions = body.names ?? []))
					.catch(() => {});
			},
			search ? 250 : 0
		);
		return () => clearTimeout(timer);
	});

	function pickMetric(m: string) {
		query.builder.metric = m;
		metricSearch = '';
		metricOpen = false;
	}

	// ---- tag keys and values of the chosen metric ----

	let tagKeys = $state<string[]>([]);

	$effect(() => {
		const metric = b.metric;
		if (!metric) {
			tagKeys = [];
			return;
		}
		const controller = new AbortController();
		fetch(`/app/metrics/meta?what=tags&metric=${encodeURIComponent(metric)}`, {
			signal: controller.signal
		})
			.then((r) => r.json())
			.then((body) => (tagKeys = body.keys ?? []))
			.catch(() => {});
		return () => controller.abort();
	});

	let filterOpen = $state(false);
	let filterKey = $state('');
	let valueSearch = $state('');
	let valueOptions = $state<string[]>([]);

	$effect(() => {
		if (!filterOpen || !b.metric || !filterKey) {
			valueOptions = [];
			return;
		}
		const q = new URLSearchParams({
			what: 'values',
			metric: b.metric,
			key: filterKey,
			search: valueSearch
		});
		const timer = setTimeout(
			() => {
				fetch(`/app/metrics/meta?${q}`)
					.then((r) => r.json())
					.then((body) => (valueOptions = body.values ?? []))
					.catch(() => {});
			},
			valueSearch ? 250 : 0
		);
		return () => clearTimeout(timer);
	});

	function addFilter(value: string) {
		if (!filterKey || !value) return;
		if (!b.filters.some((t) => t.key === filterKey && t.value === value)) {
			query.builder.filters = [...b.filters, { key: filterKey, value }];
		}
		valueSearch = '';
		filterOpen = false;
	}

	function removeFilter(index: number) {
		query.builder.filters = b.filters.filter((_, i) => i !== index);
	}

	function addGroup(key: string) {
		if (key && !b.groupBy.includes(key)) query.builder.groupBy = [...b.groupBy, key];
	}

	function removeGroup(key: string) {
		query.builder.groupBy = b.groupBy.filter((k) => k !== key);
	}

	// ---- builder ⇄ text ----

	function toText() {
		// The text starts as what the builder shows; with no metric picked yet,
		// whatever was typed before stays.
		const built = builderText(b);
		if (built) query.text = built;
		query.mode = 'text';
	}

	function toBuilder() {
		query.mode = 'builder';
	}
</script>

<div
	class="flex items-start gap-2 rounded-md border bg-card px-2 py-1.5 {query.hidden
		? 'opacity-60'
		: ''}"
>
	<div class="flex min-w-0 flex-1 flex-wrap items-center gap-2">
		<span
			class="inline-flex size-6 shrink-0 items-center justify-center rounded font-mono text-xs font-semibold text-white"
			style="background: {color}"
		>
			{query.name}
		</span>

		{#if query.mode === 'text'}
			<Input
				bind:value={query.text}
				placeholder={TEXT_PLACEHOLDER}
				class="h-8 min-w-72 flex-1 font-mono text-xs"
				spellcheck={false}
			/>
		{:else}
			<!-- metric -->
			<div class="relative w-72">
				<Input
					value={metricOpen ? metricSearch : b.metric}
					placeholder="metric…"
					class="h-8 font-mono text-xs"
					onfocus={() => {
						metricSearch = '';
						metricOpen = true;
					}}
					onblur={() => setTimeout(() => (metricOpen = false), 150)}
					oninput={(e) => (metricSearch = e.currentTarget.value)}
				/>
				{#if metricOpen}
					<div
						class="absolute top-full z-30 mt-1 max-h-72 w-full overflow-y-auto rounded-md border bg-popover p-1 shadow-md"
					>
						{#each metricOptions.length > 0 || metricSearch ? metricOptions : metrics as m (m)}
							<button
								type="button"
								class="w-full truncate rounded-sm px-2 py-1.5 text-left font-mono text-xs transition-colors
								{m === b.metric ? 'bg-primary text-primary-foreground' : 'hover:bg-muted'}"
								onmousedown={() => pickMetric(m)}
								title={m}
							>
								{m}
							</button>
						{:else}
							<p class="px-2 py-3 text-xs text-muted-foreground">No matching metrics.</p>
						{/each}
					</div>
				{/if}
			</div>

			<!-- from -->
			<span class="text-xs text-muted-foreground">from</span>
			<div class="flex flex-wrap items-center gap-1">
				{#each b.filters as f, i (`${f.key}:${f.value}`)}
					<span
						class="inline-flex items-center gap-1 rounded-md border bg-muted/40 px-1.5 py-0.5 font-mono text-[11px]"
					>
						{f.key}:{f.value}
						<button
							type="button"
							class="opacity-50 hover:opacity-100"
							onclick={() => removeFilter(i)}
							aria-label={`Remove filter ${f.key}:${f.value}`}
						>
							<XIcon class="size-3" />
						</button>
					</span>
				{:else}
					<span class="font-mono text-xs text-muted-foreground">everywhere</span>
				{/each}

				<Popover.Root bind:open={filterOpen}>
					<Popover.Trigger
						class="inline-flex size-6 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-40"
						disabled={tagKeys.length === 0}
						aria-label="Add tag filter"
						title="Add tag filter"
					>
						<PlusIcon class="size-3.5" />
					</Popover.Trigger>
					<Popover.Content align="start" class="w-72 space-y-2 p-2">
						<select
							bind:value={filterKey}
							class="h-8 w-full rounded-md border bg-transparent px-2 font-mono text-xs"
						>
							<option value="">tag key…</option>
							{#each tagKeys as k (k)}
								<option value={k}>{k}</option>
							{/each}
						</select>
						<Input
							bind:value={valueSearch}
							placeholder={filterKey
								? `${filterKey} value… (wildcards: web-*)`
								: 'pick a key first'}
							disabled={!filterKey}
							class="h-8 font-mono text-xs"
							onkeydown={(e) => {
								if (e.key === 'Enter' && valueSearch) addFilter(valueSearch);
							}}
						/>
						{#if valueOptions.length > 0}
							<div class="max-h-56 overflow-y-auto">
								{#each valueOptions as v (v)}
									<button
										type="button"
										class="w-full truncate rounded-sm px-2 py-1 text-left font-mono text-xs hover:bg-muted"
										onclick={() => addFilter(v)}
										title={v}
									>
										{v}
									</button>
								{/each}
							</div>
						{/if}
					</Popover.Content>
				</Popover.Root>
			</div>

			<!-- aggregation and grouping -->
			<select
				bind:value={query.builder.agg}
				class="h-8 rounded-md border bg-transparent px-2 font-mono text-xs"
				aria-label="Space aggregation"
			>
				{#each SPACE_AGGREGATORS as a (a)}
					<option value={a}>{a}</option>
				{/each}
			</select>
			<span class="text-xs text-muted-foreground">by</span>
			<div class="flex flex-wrap items-center gap-1">
				{#if b.groupBy.length === 0}
					<span class="font-mono text-xs text-muted-foreground">everything</span>
				{/if}
				{#each b.groupBy as key (key)}
					<span
						class="inline-flex items-center gap-1 rounded-md border bg-muted/40 px-1.5 py-0.5 font-mono text-[11px]"
					>
						{key}
						<button
							type="button"
							class="opacity-50 hover:opacity-100"
							onclick={() => removeGroup(key)}
							aria-label={`Stop grouping by ${key}`}
						>
							<XIcon class="size-3" />
						</button>
					</span>
				{/each}
				<DropdownMenu.Root>
					<DropdownMenu.Trigger
						class="inline-flex size-6 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-40"
						disabled={tagKeys.length === 0}
						aria-label="Group by"
						title="Group by"
					>
						<PlusIcon class="size-3.5" />
					</DropdownMenu.Trigger>
					<DropdownMenu.Content align="start" class="max-h-72 w-56 overflow-y-auto">
						{#each tagKeys.filter((k) => !b.groupBy.includes(k)) as k (k)}
							<DropdownMenu.Item class="font-mono text-xs" onSelect={() => addGroup(k)}>
								{k}
							</DropdownMenu.Item>
						{:else}
							<p class="px-2 py-1.5 text-xs text-muted-foreground">Already grouped by every key.</p>
						{/each}
					</DropdownMenu.Content>
				</DropdownMenu.Root>
			</div>
		{/if}

		<FunctionChips bind:functions={query.functions} allowModifiers />
		<label class="flex items-center gap-1.5 text-xs text-muted-foreground">
			as
			<input
				bind:value={query.alias}
				placeholder="alias"
				class="h-7 w-28 rounded-md border bg-transparent px-2 font-mono text-[11px] text-foreground outline-none placeholder:text-muted-foreground/60 focus:ring-1 focus:ring-ring"
			/>
		</label>
	</div>

	<div class="flex shrink-0 items-center gap-0.5 pt-0.5">
		<button
			type="button"
			class="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
			onclick={() => (query.mode === 'text' ? toBuilder() : toText())}
			title={query.mode === 'text'
				? 'Back to the builder (text edits are dropped)'
				: 'Edit as text'}
			aria-label={query.mode === 'text' ? 'Back to the builder' : 'Edit as text'}
		>
			{#if query.mode === 'text'}
				<SlidersIcon class="size-4" />
			{:else}
				<CodeIcon class="size-4" />
			{/if}
		</button>
		<button
			type="button"
			class="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
			onclick={() => (query.hidden = !query.hidden)}
			title={query.hidden ? 'Show on the chart' : 'Hide from the chart (formulas can still use it)'}
			aria-label={query.hidden ? 'Show query' : 'Hide query'}
		>
			{#if query.hidden}
				<EyeOffIcon class="size-4" />
			{:else}
				<EyeIcon class="size-4" />
			{/if}
		</button>
		{#if onremove}
			<button
				type="button"
				class="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-destructive"
				onclick={onremove}
				aria-label={`Remove query ${query.name}`}
			>
				<XIcon class="size-4" />
			</button>
		{/if}
	</div>
</div>
