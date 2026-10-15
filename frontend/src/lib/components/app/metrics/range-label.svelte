<script lang="ts">
	import { onMount } from 'svelte';
	import type { FixedRange } from '$lib/metrics/query';
	import { getLocale } from '$lib/paraglide/runtime';

	let { range }: { range: FixedRange } = $props();

	// As in LocalTime: the server renders UTC, the browser swaps in its own zone.
	let local = $state(false);
	onMount(() => (local = true));

	// formatRange prints what both ends share once: "Oct 15, 6:28 – 6:46 PM".
	const text = $derived.by(() => {
		const thisYear = new Date(range.from).getFullYear() === new Date().getFullYear();
		const format = new Intl.DateTimeFormat(getLocale(), {
			year: thisYear ? undefined : 'numeric',
			month: 'short',
			day: 'numeric',
			hour: 'numeric',
			minute: '2-digit',
			second: range.to - range.from < 3_600_000 ? '2-digit' : undefined,
			timeZone: local ? undefined : 'UTC'
		});
		const text = format.formatRange(range.from, range.to);
		return local ? text : `${text} UTC`;
	});
</script>

<span class="px-1 text-xs text-muted-foreground">{text}</span>
