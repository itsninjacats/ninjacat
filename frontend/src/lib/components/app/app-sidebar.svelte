<script lang="ts">
	import type { ComponentProps } from 'svelte';
	import ChartLineIcon from '@lucide/svelte/icons/chart-line';
	import LayoutDashboardIcon from '@lucide/svelte/icons/layout-dashboard';
	import ScrollTextIcon from '@lucide/svelte/icons/scroll-text';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import * as Sidebar from '$lib/components/ui/sidebar/index.js';
	import NavMain from './nav-main.svelte';
	import NavUser from './nav-user.svelte';
	import TenantSwitcher from './tenant-switcher.svelte';
	import type { Tenant } from '$lib/tenant';

	let {
		ref = $bindable(null),
		user,
		tenant,
		tenants,
		...restProps
	}: {
		user: { email: string; name: string | null };
		tenant: Tenant | null;
		tenants: Tenant[];
	} & ComponentProps<typeof Sidebar.Root> = $props();

	// Grouped the way the work splits rather than the way the routes nest:
	// "Explore" is where you go to ask the data a question, "Settings" is where
	// you go to change how it arrives. Only routes that actually exist are
	// listed — a nav entry leading to a 404 is worse than a missing one.
	const explore = [
		{ title: 'Overview', url: '/app', icon: LayoutDashboardIcon, exact: true },
		{ title: 'Metrics', url: '/app/metrics', icon: ChartLineIcon },
		{ title: 'Logs', url: '/app/logs', icon: ScrollTextIcon }
	];

	const settings = [{ title: 'API keys', url: '/app/ustawienia/klucze', icon: KeyRoundIcon }];
</script>

<!-- collapsible="icon" is the Datadog-shaped part: the rail shrinks to icons
     rather than disappearing, so navigation stays one click away while a wide
     chart or log table takes the screen. -->
<Sidebar.Root bind:ref collapsible="icon" {...restProps}>
	<Sidebar.Header>
		<TenantSwitcher {tenant} {tenants} />
	</Sidebar.Header>

	<Sidebar.Content>
		<NavMain label="Explore" items={explore} />
		<NavMain label="Settings" items={settings} />
	</Sidebar.Content>

	<Sidebar.Footer>
		<NavUser {user} />
	</Sidebar.Footer>

	<Sidebar.Rail />
</Sidebar.Root>
