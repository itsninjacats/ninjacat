<script lang="ts">
	import * as NativeSelect from '$lib/components/ui/native-select';

	// `last_5m` is shown as 5m: the window's length is what is chosen. A value
	// outside the list (a monitor written elsewhere) stays selectable.
	let {
		value = $bindable(),
		options,
		id,
		'aria-label': ariaLabel
	}: { value: string; options: string[]; id?: string; 'aria-label'?: string } = $props();

	const label = (text: string) => text.replace(/^(last|next)_/, '').replace(/_ago$/, '');
	const all = $derived(options.includes(value) ? options : [value, ...options]);
</script>

<NativeSelect.Root
	bind:value
	{id}
	aria-label={ariaLabel}
	size="sm"
	class="w-full font-mono [&>select]:text-xs"
>
	{#each all as option (option)}
		<NativeSelect.Option value={option}>{label(option)}</NativeSelect.Option>
	{/each}
</NativeSelect.Root>
