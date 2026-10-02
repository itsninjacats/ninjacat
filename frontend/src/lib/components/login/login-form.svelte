<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { authClient } from '$lib/auth-client';
	import { Button } from '$lib/components/ui/button';
	import { FieldGroup, Field, FieldLabel, FieldError } from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import { Spinner } from '$lib/components/ui/spinner';
	import { cn, type WithElementRef } from '$lib/utils.js';
	import type { HTMLFormAttributes } from 'svelte/elements';

	let {
		ref = $bindable(null),
		class: className,
		...restProps
	}: WithElementRef<HTMLFormAttributes> = $props();

	const id = $props.id();
	let email = $state('');
	let password = $state('');
	let error = $state<string | null>(null);
	let submitting = $state(false);

	async function signIn(event: SubmitEvent) {
		event.preventDefault();
		submitting = true;
		error = null;

		const result = await authClient.signIn.email({ email: email.trim(), password });
		submitting = false;

		if (!result.error) {
			await goto(resolve('/app'), { invalidateAll: true });
		} else if (result.error.status === 429) {
			error = 'Too many attempts. Wait a minute and try again.';
		} else if (result.error.status >= 500) {
			error = 'Something went wrong. Try again.';
		} else {
			error = 'Wrong e-mail or password';
		}
	}
</script>

<form class={cn('flex flex-col gap-6', className)} bind:this={ref} onsubmit={signIn} {...restProps}>
	<FieldGroup>
		<div class="flex flex-col items-center gap-1 text-center">
			<h1 class="font-heading text-2xl font-semibold tracking-tight">Sign in</h1>
			<p class="text-sm text-balance text-muted-foreground">
				Enter the credentials for your ninjacat instance
			</p>
		</div>

		<Field>
			<FieldLabel for="email-{id}">E-mail address</FieldLabel>
			<Input
				id="email-{id}"
				name="email"
				type="email"
				placeholder="you@company.com"
				bind:value={email}
				autocomplete="username"
				required
			/>
		</Field>

		<Field>
			<FieldLabel for="password-{id}">Password</FieldLabel>
			<Input
				id="password-{id}"
				name="password"
				type="password"
				bind:value={password}
				autocomplete="current-password"
				required
			/>
			{#if error}
				<FieldError>{error}</FieldError>
			{/if}
		</Field>

		<Field>
			<Button type="submit" disabled={submitting}>
				{#if submitting}
					<Spinner />
				{/if}
				Sign in
			</Button>
		</Field>
	</FieldGroup>
</form>
