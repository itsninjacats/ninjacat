<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import CheckIcon from '@lucide/svelte/icons/check';
	import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
	import { authClient } from '$lib/auth-client';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
	import * as Sidebar from '$lib/components/ui/sidebar/index.js';
	import { m } from '$lib/paraglide/messages';
	import { toast } from 'svelte-sonner';
	import { roleName } from '$lib/role-name';
	import type { Tenant } from '$lib/tenant';

	let { tenant, tenants }: { tenant: Tenant | null; tenants: Tenant[] } = $props();

	const sidebar = Sidebar.useSidebar();

	// The session remembers the choice; every load then runs again for the new
	// tenant.
	async function switchTo(id: string) {
		if (id === tenant?.id) return;
		const { error } = await authClient.organization.setActive({ organizationId: id });
		if (error) {
			toast.error(m.tenant_switch_failed());
		}
		// After a refusal too: the tenant shown may be one the user has left, and
		// the reload shows the one they are in now.
		await invalidateAll();
	}
</script>

<Sidebar.Menu>
	<Sidebar.MenuItem>
		<DropdownMenu.Root>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<Sidebar.MenuButton
						{...props}
						size="lg"
						class="data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground"
					>
						<div
							class="flex aspect-square size-8 items-center justify-center rounded-lg bg-primary text-primary-foreground"
						>
							<span class="text-sm font-semibold">n</span>
						</div>
						<div class="grid flex-1 text-left text-sm leading-tight">
							<span class="truncate font-semibold">{tenant?.name ?? m.tenant_none()}</span>
							<span class="truncate text-xs text-muted-foreground">
								{tenant ? roleName(tenant.role) : 'ninjacat'}
							</span>
						</div>
						<ChevronsUpDownIcon class="ml-auto size-4" />
					</Sidebar.MenuButton>
				{/snippet}
			</DropdownMenu.Trigger>

			<DropdownMenu.Content
				class="w-(--bits-dropdown-menu-anchor-width) min-w-56 rounded-lg"
				side={sidebar.isMobile ? 'bottom' : 'right'}
				align="start"
				sideOffset={4}
			>
				<DropdownMenu.Label class="text-xs text-muted-foreground">{m.tenants()}</DropdownMenu.Label>
				{#each tenants as option (option.id)}
					<DropdownMenu.Item onSelect={() => switchTo(option.id)} class="gap-2 p-2">
						<div class="grid flex-1 leading-tight">
							<span class="truncate">{option.name}</span>
							<span class="truncate text-xs text-muted-foreground">{roleName(option.role)}</span>
						</div>
						{#if option.id === tenant?.id}
							<CheckIcon class="size-4" />
						{/if}
					</DropdownMenu.Item>
				{:else}
					<DropdownMenu.Item disabled class="p-2">{m.tenant_none()}</DropdownMenu.Item>
				{/each}
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	</Sidebar.MenuItem>
</Sidebar.Menu>
