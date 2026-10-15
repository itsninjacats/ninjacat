<script lang="ts">
	import SigmaIcon from '@lucide/svelte/icons/sigma';
	import XIcon from '@lucide/svelte/icons/x';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
	import {
		FUNCTION_FAMILIES,
		addFunction,
		functionSpec,
		isModifier,
		type FunctionCall,
		type FunctionSpec
	} from '$lib/metrics/functions';

	let {
		functions = $bindable(),
		allowModifiers
	}: {
		functions: FunctionCall[];
		/** Modifiers (.rollup, .fill…) belong to a query, so formula rows leave them out. */
		allowModifiers: boolean;
	} = $props();

	const families = $derived(
		FUNCTION_FAMILIES.map((f) => ({
			family: f.family,
			functions: f.functions.filter((s) => allowModifiers || s.kind !== 'modifier')
		})).filter((f) => f.functions.length > 0)
	);

	function add(spec: FunctionSpec) {
		functions = addFunction(functions, spec);
	}

	function remove(index: number) {
		functions = functions.filter((_, i) => i !== index);
	}
</script>

<div class="flex flex-wrap items-center gap-1">
	{#each functions as fn, i (i)}
		<span
			class="inline-flex items-center gap-0.5 rounded-md border bg-muted/40 py-0.5 pr-1 pl-1.5 font-mono text-[11px]"
		>
			<span class="cursor-help text-primary" title={functionSpec(fn.name)?.description}>
				{isModifier(fn) ? `.${fn.name}` : fn.name}
			</span>
			{#if fn.args.length > 0}
				<span class="text-muted-foreground">(</span>
				{#each fn.args, a}
					{#if a > 0}<span class="text-muted-foreground">,</span>{/if}
					<input
						bind:value={fn.args[a]}
						class="field-sizing-content h-4 min-w-3 rounded-sm border-0 bg-transparent px-0.5 font-mono text-[11px] outline-none hover:bg-background focus:bg-background focus:ring-1 focus:ring-ring"
						title={functionSpec(fn.name)?.argNames?.[a]}
						aria-label={functionSpec(fn.name)?.argNames?.[a] ?? `${fn.name} argument ${a + 1}`}
					/>
				{/each}
				<span class="text-muted-foreground">)</span>
			{/if}
			<button
				type="button"
				class="ml-0.5 opacity-50 hover:opacity-100"
				onclick={() => remove(i)}
				aria-label={`Remove ${fn.name}`}
			>
				<XIcon class="size-3" />
			</button>
		</span>
	{/each}

	<DropdownMenu.Root>
		<DropdownMenu.Trigger
			class="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
			aria-label="Add function"
			title="Add function"
		>
			<SigmaIcon class="size-4" />
		</DropdownMenu.Trigger>
		<DropdownMenu.Content align="start" class="w-44">
			{#each families as family (family.family)}
				<DropdownMenu.Sub>
					<DropdownMenu.SubTrigger>{family.family}</DropdownMenu.SubTrigger>
					<!-- Portalled: inside the menu, its overflow clipping hides the submenu. -->
					<DropdownMenu.Portal>
						<DropdownMenu.SubContent class="max-h-96 w-80 overflow-y-auto">
							{#each family.functions as spec (spec.name)}
								<DropdownMenu.Item class="flex-col items-start gap-0.5" onSelect={() => add(spec)}>
									<span class="font-mono text-xs">
										{spec.kind === 'modifier' ? `.${spec.name}` : spec.name}({(
											spec.argNames ?? []
										).join(', ')})
									</span>
									<span class="text-[11px] leading-snug text-muted-foreground">
										{spec.description}
									</span>
								</DropdownMenu.Item>
							{/each}
						</DropdownMenu.SubContent>
					</DropdownMenu.Portal>
				</DropdownMenu.Sub>
			{/each}
		</DropdownMenu.Content>
	</DropdownMenu.Root>
</div>
