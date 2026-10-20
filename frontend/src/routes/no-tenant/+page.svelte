<script lang="ts">
	import { resolve } from '$app/paths';
	import { signOut } from '$lib/auth-client';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { m } from '$lib/paraglide/messages';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>{m.no_tenant()} — ninjacat</title></svelte:head>

<div class="flex min-h-svh items-center justify-center p-6">
	<Card.Root class="w-full max-w-md">
		<Card.Header>
			<Card.Title>{m.no_tenant()}</Card.Title>
			<Card.Description class="wrap-anywhere">{data.email}</Card.Description>
		</Card.Header>
		<Card.Content>
			<p class="text-sm leading-relaxed text-muted-foreground">{m.no_tenant_explained()}</p>
		</Card.Content>
		<Card.Footer class="gap-2">
			{#if data.instanceAdmin}
				<Button href={resolve('/app/admin/tenants')}>{m.admin_manage_tenants()}</Button>
			{/if}
			<Button variant="outline" href={resolve('/app/settings/account')}>{m.account()}</Button>
			<Button variant="outline" onclick={signOut}>{m.sign_out()}</Button>
		</Card.Footer>
	</Card.Root>
</div>
