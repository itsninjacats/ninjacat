<script lang="ts">
	import type { ComponentProps } from 'svelte';
	import ChartLineIcon from '@lucide/svelte/icons/chart-line';
	import LayoutDashboardIcon from '@lucide/svelte/icons/layout-dashboard';
	import ScrollTextIcon from '@lucide/svelte/icons/scroll-text';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import * as Sidebar from '$lib/components/ui/sidebar/index.js';
	import NavMain from './nav-main.svelte';
	import NavUser from './nav-user.svelte';

	let {
		ref = $bindable(null),
		user,
		...restProps
	}: { user: { email: string; name: string | null } } & ComponentProps<
		typeof Sidebar.Root
	> = $props();

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
		<Sidebar.Menu>
			<Sidebar.MenuItem>
				<Sidebar.MenuButton size="lg">
					{#snippet child({ props })}
						<a href="/app" {...props}>
							<div
								class="flex aspect-square size-8 items-center justify-center rounded-lg bg-primary text-primary-foreground"
							>
								<span class="text-sm font-semibold">n</span>
							</div>
							<div class="grid flex-1 text-left text-sm leading-tight">
								<span class="truncate font-semibold">ninjacat</span>
								<span class="truncate text-xs text-muted-foreground">observability</span>
							</div>
						</a>
					{/snippet}
				</Sidebar.MenuButton>
			</Sidebar.MenuItem>
		</Sidebar.Menu>
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
