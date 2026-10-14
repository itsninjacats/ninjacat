<script lang="ts">
	import { onMount } from 'svelte';
	import { getLocale } from '$lib/paraglide/runtime';

	// Times are stored and evaluated in UTC and shown in the browser's zone,
	// which the server does not know: it renders UTC, and the browser swaps
	// in its own once the page runs.
	let { at, seconds = false }: { at: string; seconds?: boolean } = $props();

	let local = $state(false);
	onMount(() => (local = true));

	const text = $derived.by(() => {
		const options: Intl.DateTimeFormatOptions = {
			dateStyle: 'medium',
			timeStyle: seconds ? 'medium' : 'short'
		};
		const date = new Date(at);
		return local
			? date.toLocaleString(getLocale(), options)
			: `${date.toLocaleString(getLocale(), { ...options, timeZone: 'UTC' })} UTC`;
	});
</script>

<time datetime={at}>{text}</time>
