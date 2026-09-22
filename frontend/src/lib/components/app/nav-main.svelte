<script lang="ts">
	import type { Component } from 'svelte';
	import { page } from '$app/state';
	import * as Sidebar from '$lib/components/ui/sidebar/index.js';

	let {
		label,
		items
	}: {
		label: string;
		items: { title: string; url: string; icon: Component; exact?: boolean }[];
	} = $props();

	// /app must match exactly or it would light up on every child route;
	// everything else matches by prefix so /app/metrics/data keeps Metrics lit.
	function isActive(url: string, exact = false) {
		return exact ? page.url.pathname === url : page.url.pathname.startsWith(url);
	}
</script>

<Sidebar.Group>
	<Sidebar.GroupLabel>{label}</Sidebar.GroupLabel>
	<Sidebar.Menu>
		{#each items as item (item.url)}
			<Sidebar.MenuItem>
				<!-- tooltipContent is what makes the collapsed icon rail usable:
				     with the sidebar shrunk to icons, this is the only label left. -->
				<Sidebar.MenuButton isActive={isActive(item.url, item.exact)} tooltipContent={item.title}>
					{#snippet child({ props })}
						<a href={item.url} {...props}>
							<item.icon />
							<span>{item.title}</span>
						</a>
					{/snippet}
				</Sidebar.MenuButton>
			</Sidebar.MenuItem>
		{/each}
	</Sidebar.Menu>
</Sidebar.Group>
