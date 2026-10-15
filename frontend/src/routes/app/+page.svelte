<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import * as Card from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import BellIcon from '@lucide/svelte/icons/bell';
	import { m } from '$lib/paraglide/messages';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const monitorsHint = $derived.by(() => {
		const s = data.monitors;
		if (!s.known) return m.overview_monitors_unknown();
		if (s.alert + s.warn + s.noData === 0) return m.overview_monitors_quiet();
		return m.overview_monitors_states({ alert: s.alert, warn: s.warn, noData: s.noData });
	});
</script>

<svelte:head><title>ninjacat</title></svelte:head>

<main class="mx-auto max-w-6xl px-6 py-10">
	<h1 class="mb-8 font-heading text-3xl font-semibold tracking-tight wrap-anywhere">
		{data.user.name ? m.overview_welcome_name({ name: data.user.name }) : m.overview_welcome()}
	</h1>

	<div class="grid gap-4 lg:grid-cols-3">
		<a href={resolve('/app/monitors')} class="rounded-xl hover:ring-1 hover:ring-ring">
			<Card.Root class="h-full">
				<Card.Content>
					<div class="flex items-start justify-between">
						<div>
							<p class="text-sm text-muted-foreground">{m.monitors()}</p>
							<p class="mt-1 font-heading text-3xl font-semibold tabular-nums">
								{data.monitors.total}
							</p>
							<p class="mt-1 text-xs text-muted-foreground">{monitorsHint}</p>
						</div>
						<BellIcon class="size-4 shrink-0 text-muted-foreground" />
					</div>
				</Card.Content>
			</Card.Root>
		</a>

		<Card.Root class="lg:col-span-2">
			<Card.Header>
				<Card.Title class="text-base">{m.overview_connect()}</Card.Title>
				<Card.Description>{m.overview_connect_hint()}</Card.Description>
			</Card.Header>
			<Card.Content>
				<pre class="overflow-x-auto rounded-md bg-muted p-3 text-xs leading-relaxed"><code
						>DD_SITE={page.url.hostname}
DD_API_KEY=&lt;key&gt;</code
					></pre>
				<Button href={resolve('/app/settings/keys')} variant="outline" size="sm" class="mt-3">
					{m.overview_connect_key()}
				</Button>
			</Card.Content>
		</Card.Root>
	</div>
</main>
