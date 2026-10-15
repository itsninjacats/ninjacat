<script lang="ts" generics="T extends string">
	import * as NativeSelect from '$lib/components/ui/native-select';

	// `last_5m` shows as 5m. A value outside the list (a monitor written elsewhere) stays selectable.
	let {
		value = $bindable(),
		options,
		id
	}: { value: T; options: readonly T[]; id: string } = $props();

	const label = (text: string) => text.replace(/^(last|next)_/, '').replace(/_ago$/, '');
	const all = $derived(options.includes(value) ? options : [value, ...options]);
</script>

<NativeSelect.Root bind:value {id} size="sm" class="w-full font-mono [&>select]:text-xs">
	{#each all as option (option)}
		<NativeSelect.Option value={option}>{label(option)}</NativeSelect.Option>
	{/each}
</NativeSelect.Root>
