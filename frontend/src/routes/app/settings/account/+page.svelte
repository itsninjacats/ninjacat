<script lang="ts">
	import ChangePasswordForm from '$lib/components/account/change-password-form.svelte';
	import * as Card from '$lib/components/ui/card';
	import { m } from '$lib/paraglide/messages';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>{m.account()} — ninjacat</title></svelte:head>

<div class="mx-auto max-w-4xl px-6 py-10">
	<p class="mb-8 text-sm text-muted-foreground">{m.account_intro()}</p>

	<Card.Root class="max-w-xl">
		<Card.Header>
			<Card.Title class="text-base">{m.password_change()}</Card.Title>
			<Card.Description>{m.password_change_hint({ min: data.minPasswordLength })}</Card.Description>
		</Card.Header>
		<Card.Content>
			<ChangePasswordForm min={data.minPasswordLength} max={data.maxPasswordLength} />
		</Card.Content>
	</Card.Root>
</div>
