<script lang="ts">
	import { stateClass, stateLabel } from '$lib/monitors/states';

	let { state }: { state: string | null } = $props();
</script>

<span
	class="inline-flex items-center gap-1.5 rounded-md border px-1.5 py-0.5 text-xs font-medium whitespace-nowrap"
>
	<span class="size-2 shrink-0 rounded-full {stateClass(state)}"></span>
	{stateLabel(state)}
</span>
