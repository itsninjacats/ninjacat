<script lang="ts">
	import { enhance } from '$app/forms';
	import { Button } from '$lib/components/ui/button';
	import { FieldGroup, Field, FieldLabel, FieldError } from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import { Spinner } from '$lib/components/ui/spinner';
	import { cn, type WithElementRef } from '$lib/utils.js';
	import type { HTMLFormAttributes } from 'svelte/elements';

	let {
		ref = $bindable(null),
		class: className,
		blad = null,
		email = '',
		...restProps
	}: WithElementRef<HTMLFormAttributes> & { blad?: string | null; email?: string } = $props();

	const id = $props.id();
	let wysylanie = $state(false);
</script>

<form
	method="POST"
	action="?/zaloguj"
	class={cn('flex flex-col gap-6', className)}
	bind:this={ref}
	use:enhance={() => {
		wysylanie = true;
		return async ({ update }) => {
			await update();
			wysylanie = false;
		};
	}}
	{...restProps}
>
	<FieldGroup>
		<div class="flex flex-col items-center gap-1 text-center">
			<h1 class="font-heading text-2xl font-semibold tracking-tight">Zaloguj się</h1>
			<p class="text-muted-foreground text-sm text-balance">
				Podaj dane dostępowe do swojej instancji ninjacata
			</p>
		</div>

		<Field>
			<FieldLabel for="email-{id}">Adres e-mail</FieldLabel>
			<Input
				id="email-{id}"
				name="email"
				type="email"
				placeholder="ty@firma.pl"
				value={email}
				autocomplete="username"
				required
			/>
		</Field>

		<Field>
			<FieldLabel for="haslo-{id}">Hasło</FieldLabel>
			<Input
				id="haslo-{id}"
				name="haslo"
				type="password"
				autocomplete="current-password"
				required
			/>
			{#if blad}
				<FieldError>{blad}</FieldError>
			{/if}
		</Field>

		<Field>
			<Button type="submit" disabled={wysylanie}>
				{#if wysylanie}
					<Spinner />
				{/if}
				Zaloguj
			</Button>
		</Field>
	</FieldGroup>
</form>
