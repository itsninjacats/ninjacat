<script lang="ts">
	import { enhance } from '$app/forms';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RefreshIcon from '@lucide/svelte/icons/refresh-cw';
	import XIcon from '@lucide/svelte/icons/x';
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as NativeSelect from '$lib/components/ui/native-select';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { Textarea } from '$lib/components/ui/textarea';
	import FunctionChips from '$lib/components/app/metrics/function-chips.svelte';
	import MetricsChart, {
		seriesColor,
		type ChartLine
	} from '$lib/components/app/metrics/uplot-chart.svelte';
	import QueryRow from '$lib/components/app/metrics/query-row.svelte';
	import RangeLabel from '$lib/components/app/metrics/range-label.svelte';
	import MonitorConditions from './monitor-conditions.svelte';
	import NumberInput from './number-input.svelte';
	import {
		METHODS,
		PRIORITIES,
		alertZone,
		chartRangeSeconds,
		chartRequest,
		effectiveComparator,
		groupKeysOf,
		switchMethod,
		type Method,
		type MonitorView
	} from '$lib/monitors/definition';
	import { methodLabel, thresholdRules } from '$lib/monitors/states';
	import { emptyFormula, emptyQuery, nextQueryName, type FixedRange } from '$lib/metrics/query';
	import { m } from '$lib/paraglide/messages';

	let {
		initial,
		metrics,
		tenantId,
		submitLabel,
		cancelHref,
		problems: submitted = []
	}: {
		initial: MonitorView;
		metrics: string[];
		// The tenant the page was shown for; the save is refused for another.
		tenantId: string;
		submitLabel: string;
		cancelHref: string;
		problems?: string[];
	} = $props();

	// The editor owns its copy from here on; a save that is refused keeps it.
	let view = $state(untrack(() => structuredClone(initial)));

	const methodHint: Record<Method, () => string> = {
		threshold: m.monitor_method_threshold_hint,
		change: m.monitor_method_change_hint,
		anomaly: m.monitor_method_anomaly_hint,
		outlier: m.monitor_method_outlier_hint,
		forecast: m.monitor_method_forecast_hint
	};
	// Evaluation of these lands later; they are saved and checked now.
	const notEvaluatedYet: Method[] = ['anomaly', 'outlier', 'forecast'];

	const groupKeys = $derived(groupKeysOf(view));

	function addQuery() {
		view.queries.push(emptyQuery(nextQueryName(view.queries.map((q) => q.name))));
		if (view.formulas.length === 0) {
			view.formulas.push(emptyFormula(view.queries.map((q) => q.name).join(' + ')));
		}
	}

	const RANGES = [
		{ seconds: 3600, label: '1h' },
		{ seconds: 4 * 3600, label: '4h' },
		{ seconds: 86_400, label: '1d' },
		{ seconds: 604_800, label: '1w' },
		{ seconds: 2 * 604_800, label: '2w' }
	];
	let chosenRange = $state<number | null>(null);
	const rangeSeconds = $derived(chosenRange ?? chartRangeSeconds(view));
	// Not in the URL: the queries it would redraw live in the form, not there.
	let zoom = $state<FixedRange | null>(null);

	const request = $derived(chartRequest(view));
	let lines = $state<ChartLine[]>([]);
	let shownWindow = $state({ from: 0, to: 0 });
	let loading = $state(false);
	let chartProblems = $state<string[]>([]);
	let refreshes = $state(0);

	const requestKey = $derived(JSON.stringify({ request, rangeSeconds, zoom, refreshes }));

	$effect(() => {
		const { request: asked, rangeSeconds: seconds } = JSON.parse(requestKey) as {
			request: typeof request;
			rangeSeconds: number;
		};
		const outlier = view.method === 'outlier';
		if (asked.formulas.length === 0) {
			lines = [];
			chartProblems = [];
			return;
		}
		const controller = new AbortController();
		const timer = setTimeout(() => {
			const to = zoom?.to ?? Date.now();
			const from = zoom?.from ?? to - seconds * 1000;
			loading = true;
			fetch('/app/metrics/query', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				signal: controller.signal,
				body: JSON.stringify({ kind: 'timeseries', from, to, ...asked })
			})
				.then(async (r) => {
					const body = await r.json();
					if (!r.ok) throw body?.errors ?? [`HTTP ${r.status}`];
					return body;
				})
				.then((body) => {
					chartProblems = [];
					shownWindow = { from, to };
					lines = body.series.map(
						(
							s: Omit<ChartLine, 'key' | 'label' | 'color'> & {
								output: number;
								tags: string[];
							},
							i: number
						) => ({
							...s,
							key: `s${i}`,
							label:
								(s.tags.join(', ') || m.monitor_chart_series()) +
								(outlier && s.output === 1 ? ` · ${m.monitor_chart_outlier()}` : ''),
							color: outlier
								? s.output === 1
									? 'var(--state-alert)'
									: 'var(--muted-foreground)'
								: seriesColor(i)
						})
					);
				})
				.catch((e) => {
					if (e?.name === 'AbortError') return;
					chartProblems = Array.isArray(e) ? e.map(String) : [String(e?.message ?? e)];
					lines = [];
				})
				.finally(() => (loading = false));
		}, 500);
		return () => {
			clearTimeout(timer);
			controller.abort();
		};
	});

	const rules = $derived(
		view.method === 'anomaly' || view.method === 'outlier' ? [] : thresholdRules(view)
	);
	const zone = $derived(
		rules.length === 0 ? null : alertZone(effectiveComparator(view), view.critical)
	);

	let checked = $state<{
		problems: string[];
		notActedOn: string[];
		monitor: unknown;
	} | null>(null);
	let checking = $state(false);
	const viewJson = $derived(JSON.stringify(view));

	$effect(() => {
		const body = viewJson;
		const controller = new AbortController();
		const timer = setTimeout(() => {
			checking = true;
			fetch('/app/monitors/validate', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				signal: controller.signal,
				body
			})
				.then((r) => r.json())
				.then((answer) => (checked = answer))
				.catch((e) => {
					if (e?.name !== 'AbortError') checked = null;
				})
				.finally(() => (checking = false));
		}, 600);
		return () => {
			clearTimeout(timer);
			controller.abort();
		};
	});

	let tagInput = $state('');

	function addTags() {
		for (const tag of tagInput.split(',').map((t) => t.trim())) {
			if (tag && !view.tags.includes(tag)) view.tags.push(tag);
		}
		tagInput = '';
	}

	// Enter in a field submits its form; here that would save a monitor half edited.
	function noSubmitOnEnter(form: HTMLFormElement) {
		const keep = (e: KeyboardEvent) => {
			if (e.key === 'Enter' && e.target instanceof HTMLInputElement) e.preventDefault();
		};
		form.addEventListener('keydown', keep);
		return () => form.removeEventListener('keydown', keep);
	}

	let saving = $state(false);
	// What the save was refused for and the check below does not list again.
	const refusedOnly = $derived(submitted.filter((p) => !checked?.problems.includes(p)));
	const otherOptions = $derived(Object.keys(view.otherOptions));
</script>

<form
	method="POST"
	action="?/save"
	class="space-y-6"
	{@attach noSubmitOnEnter}
	use:enhance={() => {
		saving = true;
		return async ({ update }) => {
			await update({ reset: false });
			saving = false;
		};
	}}
>
	<input type="hidden" name="tenant" value={tenantId} />
	<input type="hidden" name="view" value={viewJson} />

	<!-- 1. detection method -->
	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">1. {m.monitor_step_method()}</Card.Title>
		</Card.Header>
		<Card.Content class="@container space-y-3">
			<div class="grid grid-cols-1 gap-2 @md:grid-cols-2 @4xl:grid-cols-5" role="group">
				{#each METHODS as method (method)}
					<button
						type="button"
						aria-pressed={view.method === method}
						class="rounded-md border px-3 py-2 text-left transition-colors hover:bg-muted
						{view.method === method ? 'border-primary bg-primary/5 ring-1 ring-primary' : ''}"
						onclick={() => switchMethod(view, method)}
					>
						<span class="block text-sm font-medium">{methodLabel(method)}</span>
						<span class="mt-0.5 block text-xs text-muted-foreground">{methodHint[method]()}</span>
					</button>
				{/each}
			</div>
			{#if notEvaluatedYet.includes(view.method)}
				<p class="text-xs text-muted-foreground">{m.monitor_method_not_evaluated_yet()}</p>
			{/if}
		</Card.Content>
	</Card.Root>

	<!-- 2. the metric -->
	<Card.Root class="overflow-visible">
		<Card.Header>
			<Card.Title class="text-base">2. {m.monitor_step_metric()}</Card.Title>
			<Card.Description>
				{#if groupKeys.length > 0}
					{m.monitor_multi_alert({ keys: groupKeys.join(', ') })}
				{:else}
					{m.monitor_simple_alert()}
				{/if}
			</Card.Description>
		</Card.Header>
		<Card.Content class="space-y-4">
			<div class="flex flex-wrap items-center justify-end gap-1">
				{#if zoom}
					<RangeLabel range={zoom} />
				{/if}
				{#each RANGES as r (r.seconds)}
					<Button
						size="sm"
						variant={!zoom && rangeSeconds === r.seconds ? 'default' : 'ghost'}
						onclick={() => {
							chosenRange = r.seconds;
							zoom = null;
						}}
					>
						{r.label}
					</Button>
				{/each}
				<Button
					size="sm"
					variant="ghost"
					onclick={() => refreshes++}
					aria-label={m.monitor_chart_refresh()}
					title={m.monitor_chart_refresh()}
				>
					<RefreshIcon class="size-4 {loading ? 'animate-spin' : ''}" />
				</Button>
			</div>

			{#if chartProblems.length > 0}
				<ul
					class="space-y-1 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 font-mono text-xs wrap-anywhere"
				>
					{#each chartProblems as p, i (i)}
						<li>{p}</li>
					{/each}
				</ul>
			{/if}

			{#if loading && lines.length === 0}
				<Skeleton class="h-[320px] w-full" />
			{:else if lines.length === 0}
				<div
					class="flex h-[320px] items-center justify-center rounded-md border border-dashed px-4 text-center text-sm text-muted-foreground"
				>
					{request.formulas.length === 0 ? m.monitor_chart_pick_metric() : m.monitor_chart_empty()}
				</div>
			{:else}
				<MetricsChart
					{lines}
					window={shownWindow}
					display="line"
					{rules}
					{zone}
					onzoom={(from, to) => (zoom = { from, to })}
				/>
			{/if}

			<div class="space-y-2">
				{#each view.queries as query, i (query.name)}
					<QueryRow
						bind:query={view.queries[i]}
						color={seriesColor(i)}
						{metrics}
						onremove={view.queries.length > 1 ? () => view.queries.splice(i, 1) : undefined}
						chartControls={false}
					/>
				{/each}

				{#each view.formulas as formula, i (i)}
					<div
						class="flex flex-wrap items-center gap-2 rounded-md border border-dashed px-2 py-1.5"
					>
						<span
							class="inline-flex size-6 shrink-0 items-center justify-center rounded bg-muted font-mono text-xs italic"
						>
							ƒ
						</span>
						<Input
							bind:value={formula.text}
							placeholder="a / b * 100"
							aria-label={m.monitor_formula()}
							class="h-8 min-w-0 grow basis-60 font-mono text-xs md:text-xs"
							spellcheck={false}
						/>
						<FunctionChips bind:functions={formula.functions} allowModifiers={false} />
						<button
							type="button"
							class="ml-auto inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-destructive"
							onclick={() => view.formulas.splice(i, 1)}
							aria-label={m.monitor_formula_remove()}
						>
							<XIcon class="size-4" />
						</button>
					</div>
				{/each}

				<div class="flex flex-wrap items-center gap-2 pt-1">
					<Button size="sm" variant="outline" onclick={addQuery}>
						<PlusIcon class="size-4" />
						{m.monitor_add_query()}
					</Button>
					{#if view.formulas.length === 0}
						<Button
							size="sm"
							variant="outline"
							onclick={() => view.formulas.push(emptyFormula(view.queries[0].name))}
						>
							<PlusIcon class="size-4" />
							{m.monitor_add_formula()}
						</Button>
					{/if}
					<span class="text-xs text-muted-foreground">
						{view.formulas.length > 0 ? m.monitor_formula_hint() : m.monitor_query_hint()}
					</span>
				</div>
			</div>
		</Card.Content>
	</Card.Root>

	<!-- 3. alert conditions -->
	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">3. {m.monitor_step_conditions()}</Card.Title>
		</Card.Header>
		<Card.Content>
			<MonitorConditions bind:view multi={groupKeys.length > 0} />
		</Card.Content>
	</Card.Root>

	<!-- 4. notification -->
	<Card.Root>
		<Card.Header>
			<Card.Title class="text-base">4. {m.monitor_step_notify()}</Card.Title>
		</Card.Header>
		<Card.Content class="@container space-y-4">
			<div class="space-y-1">
				<Label for="monitor-name">{m.monitor_name()}</Label>
				<Input id="monitor-name" bind:value={view.name} maxlength={500} required />
			</div>
			<div class="space-y-1">
				<Label for="monitor-message">{m.monitor_message()}</Label>
				<Textarea
					id="monitor-message"
					bind:value={view.message}
					rows={5}
					placeholder={'{{#is_alert}}CPU is high on {{host.name}}{{/is_alert}} @team-infra'}
					class="font-mono text-xs md:text-xs"
				/>
				<p class="text-xs text-muted-foreground">{m.monitor_message_hint()}</p>
			</div>
			<div class="grid grid-cols-1 gap-4 @xl:grid-cols-[1fr_12rem]">
				<div class="space-y-1">
					<Label for="monitor-tags">{m.monitor_tags()}</Label>
					<div class="flex flex-wrap items-center gap-1">
						{#each view.tags as tag, i (i)}
							<span
								class="inline-flex items-center gap-1 rounded-md border bg-muted/40 px-1.5 py-0.5 font-mono text-[11px] wrap-anywhere"
							>
								{tag}
								<button
									type="button"
									class="opacity-50 hover:opacity-100"
									onclick={() => view.tags.splice(i, 1)}
									aria-label={m.monitor_tag_remove({ tag })}
								>
									<XIcon class="size-3" />
								</button>
							</span>
						{/each}
					</div>
					<Input
						id="monitor-tags"
						bind:value={tagInput}
						placeholder="team:infra, env:prod"
						class="h-8 font-mono text-xs md:text-xs"
						onkeydown={(e) => {
							if (e.key === 'Enter' || e.key === ',') {
								e.preventDefault();
								addTags();
							}
						}}
						onblur={addTags}
					/>
				</div>
				<div class="space-y-1">
					<Label for="monitor-priority">{m.monitor_priority()}</Label>
					<NativeSelect.Root
						id="monitor-priority"
						bind:value={
							() => String(view.priority ?? ''),
							(v: string) => (view.priority = v ? Number(v) : null)
						}
						size="sm"
						class="w-full [&>select]:text-xs"
					>
						<NativeSelect.Option value="">{m.monitor_priority_none()}</NativeSelect.Option>
						{#each PRIORITIES as p (p)}
							<NativeSelect.Option value={String(p)}>P{p}</NativeSelect.Option>
						{/each}
					</NativeSelect.Root>
				</div>
			</div>

			<div class="space-y-3 rounded-md border border-dashed p-3">
				<div>
					<h3 class="text-sm font-medium">{m.monitor_not_acted_on()}</h3>
					<p class="text-xs text-muted-foreground">{m.monitor_not_acted_on_hint()}</p>
				</div>
				<div class="grid grid-cols-1 gap-3 @xl:grid-cols-2">
					<div class="space-y-1">
						<Label for="monitor-renotify">{m.monitor_renotify_interval()}</Label>
						<NumberInput id="monitor-renotify" bind:value={view.renotifyInterval} min="1" />
					</div>
					<div class="space-y-1">
						<Label for="monitor-escalation">{m.monitor_escalation_message()}</Label>
						<Textarea
							id="monitor-escalation"
							bind:value={view.escalationMessage}
							rows={2}
							class="min-h-0 font-mono text-xs md:text-xs"
						/>
					</div>
					<div class="flex items-center gap-2">
						<Checkbox id="monitor-include-tags" bind:checked={view.includeTags} />
						<Label for="monitor-include-tags" class="text-xs font-normal">
							{m.monitor_include_tags()}
						</Label>
					</div>
					<div class="flex items-center gap-2">
						<Checkbox id="monitor-notify-audit" bind:checked={view.notifyAudit} />
						<Label for="monitor-notify-audit" class="text-xs font-normal">
							{m.monitor_notify_audit()}
						</Label>
					</div>
				</div>
				{#if otherOptions.length > 0}
					<p class="text-xs wrap-anywhere text-muted-foreground">
						{m.monitor_other_options({ options: otherOptions.join(', ') })}
					</p>
				{/if}
			</div>
		</Card.Content>
	</Card.Root>

	<!-- the check and the save -->
	<Card.Root>
		<Card.Content class="space-y-4">
			{#if submitted.length > 0}
				<div
					role="alert"
					class="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2"
				>
					<p class="text-sm font-medium text-destructive">{m.monitor_save_refused()}</p>
					{#if refusedOnly.length > 0}
						<ul class="mt-1 list-disc space-y-1 pl-5 text-xs wrap-anywhere">
							{#each refusedOnly as p, i (i)}
								<li>{p}</li>
							{/each}
						</ul>
					{/if}
				</div>
			{/if}

			{#if checked && checked.problems.length > 0}
				<div>
					<p class="text-sm font-medium">{m.monitor_check_problems()}</p>
					<ul class="mt-1 list-disc space-y-1 pl-5 text-xs wrap-anywhere text-muted-foreground">
						{#each checked.problems as p, i (i)}
							<li>{p}</li>
						{/each}
					</ul>
				</div>
			{:else if checked}
				<p class="text-sm text-muted-foreground">{m.monitor_check_ok()}</p>
			{:else if checking}
				<p class="text-sm text-muted-foreground">{m.monitor_checking()}</p>
			{/if}

			{#if checked && checked.notActedOn.length > 0}
				<p class="text-xs wrap-anywhere text-muted-foreground">
					{m.monitor_check_not_acted_on({ options: checked.notActedOn.join(', ') })}
				</p>
			{/if}

			{#if checked?.monitor}
				<details class="text-xs">
					<summary class="cursor-pointer text-muted-foreground">{m.monitor_json()}</summary>
					<pre
						class="mt-2 rounded-md bg-muted p-3 font-mono text-[11px] leading-relaxed wrap-anywhere whitespace-pre-wrap">{JSON.stringify(
							checked.monitor,
							null,
							2
						)}</pre>
				</details>
			{/if}

			<div class="flex flex-wrap items-center gap-2">
				<Button type="submit" disabled={saving}>{submitLabel}</Button>
				<Button variant="ghost" href={cancelHref}>{m.cancel()}</Button>
			</div>
		</Card.Content>
	</Card.Root>
</form>
