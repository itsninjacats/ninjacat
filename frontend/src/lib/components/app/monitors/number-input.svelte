<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';
	import { Input } from '$lib/components/ui/input';

	// An empty or invalid number input reads as undefined; the view holds null.
	let {
		value = $bindable(),
		class: className = '',
		...rest
	}: { value: number | null } & Omit<HTMLInputAttributes, 'value' | 'type' | 'files'> = $props();
</script>

<Input
	type="number"
	bind:value={
		() => value ?? undefined,
		(v: unknown) => (value = typeof v === 'number' && Number.isFinite(v) ? v : null)
	}
	class="h-8 font-mono text-xs md:text-xs {className}"
	{...rest}
/>
