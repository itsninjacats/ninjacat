<script lang="ts" module>
	export type ScalarTable = {
		rows: { tags: string[]; values: (number | null)[] }[];
	};
</script>

<script lang="ts">
	let {
		table,
		labels,
		colors,
		display
	}: {
		table: ScalarTable;
		/** One per value column: what the column is called on screen. */
		labels: string[];
		colors: string[];
		display: 'toplist' | 'value';
	} = $props();

	const format = (v: number) =>
		Math.abs(v) >= 1000 || Number.isInteger(v)
			? v.toLocaleString(undefined, { maximumFractionDigits: 0 })
			: v.toLocaleString(undefined, { maximumFractionDigits: 3 });

	// The top list ranks the first column; the others ride along in the row.
	const ranked = $derived(
		[...table.rows]
			.filter((r) => r.values[0] !== null)
			.sort((a, b) => (b.values[0] as number) - (a.values[0] as number))
	);

	const maxAbs = $derived(Math.max(...ranked.map((r) => Math.abs(r.values[0] as number)), 0));

	const groupName = (tags: string[]) => (tags.length > 0 ? tags.join(', ') : '(all)');
</script>

{#if table.rows.length === 0}
	<div
		class="flex h-[200px] items-center justify-center rounded-md border border-dashed text-sm text-muted-foreground"
	>
		No values in this range.
	</div>
{:else if display === 'value'}
	<div class="grid gap-4 {labels.length > 1 ? 'sm:grid-cols-2' : ''}">
		{#each labels as label, c (c)}
			<div class="rounded-md border p-6 text-center">
				<div class="font-mono text-xs text-muted-foreground">{label}</div>
				{#if table.rows.length === 1 && table.rows[0].values[c] !== null}
					<div class="mt-2 text-5xl font-semibold tabular-nums" style="color: {colors[c]}">
						{format(table.rows[0].values[c] as number)}
					</div>
				{:else}
					<!-- A query value shows one number; a grouped query has one per group. -->
					<div class="mt-3 space-y-1 text-left">
						{#each table.rows as row, r (r)}
							{#if row.values[c] !== null}
								<div class="flex justify-between gap-4 font-mono text-sm">
									<span class="truncate text-muted-foreground">{groupName(row.tags)}</span>
									<span class="tabular-nums">{format(row.values[c] as number)}</span>
								</div>
							{/if}
						{/each}
					</div>
				{/if}
			</div>
		{/each}
	</div>
{:else}
	<div class="space-y-1.5">
		<div class="font-mono text-xs text-muted-foreground">{labels[0]}</div>
		{#each ranked as row, r (r)}
			{@const v = row.values[0] as number}
			<div class="grid grid-cols-[minmax(0,14rem)_1fr_auto] items-center gap-3 font-mono text-xs">
				<span class="truncate" title={groupName(row.tags)}>{groupName(row.tags)}</span>
				<div class="h-5 rounded-sm bg-muted/40">
					<div
						class="h-full rounded-sm"
						style="width: {maxAbs > 0 ? (Math.abs(v) / maxAbs) * 100 : 0}%; background: {colors[0]}"
					></div>
				</div>
				<span class="tabular-nums">{format(v)}</span>
			</div>
		{/each}
	</div>
{/if}
