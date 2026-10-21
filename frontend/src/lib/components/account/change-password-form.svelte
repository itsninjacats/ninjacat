<script lang="ts">
	import { onMount } from 'svelte';
	import { authClient } from '$lib/auth-client';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Field, FieldError, FieldGroup, FieldLabel } from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import { Spinner } from '$lib/components/ui/spinner';
	import { m } from '$lib/paraglide/messages';

	let { min, max }: { min: number; max: number } = $props();

	const id = $props.id();
	let current = $state('');
	let next = $state('');
	let again = $state('');
	let signOutOthers = $state(true);
	let error = $state<string | null>(null);
	let done = $state<string | null>(null);
	let submitting = $state(false);
	// Until the page hydrates, a submit would be the browser's own, which knows nothing of
	// change below; the button waits for the script instead.
	let hydrated = $state(false);
	onMount(() => (hydrated = true));

	function problem(): string | null {
		if (next.length < min || next.length > max) return m.password_length({ min, max });
		if (next !== again) return m.password_mismatch();
		if (next === current) return m.password_unchanged();
		return null;
	}

	// Through Better Auth's own endpoint, as sign-in is: its limiter counts the attempts, and with
	// the other sessions revoked it sets this session's new cookie itself.
	async function change(event: SubmitEvent) {
		event.preventDefault();
		done = null;
		error = problem();
		if (error) return;

		submitting = true;
		const result = await authClient.changePassword({
			currentPassword: current,
			newPassword: next,
			revokeOtherSessions: signOutOthers
		});
		submitting = false;

		if (!result.error) {
			done = signOutOthers ? m.password_changed_others_signed_out() : m.password_changed();
			current = next = again = '';
		} else if (result.error.status === 429) {
			error = m.too_many_attempts();
		} else if (result.error.code === 'INVALID_PASSWORD') {
			error = m.password_current_wrong();
		} else if (
			result.error.code === 'PASSWORD_TOO_SHORT' ||
			result.error.code === 'PASSWORD_TOO_LONG'
		) {
			error = m.password_length({ min, max });
		} else {
			error = m.something_went_wrong();
		}
	}
</script>

<!-- POST, so that even a submit the script never sees keeps the passwords out of the URL. -->
<form method="post" onsubmit={change}>
	<FieldGroup>
		<Field>
			<FieldLabel for="current-{id}">{m.password_current()}</FieldLabel>
			<Input
				id="current-{id}"
				type="password"
				bind:value={current}
				autocomplete="current-password"
				required
			/>
		</Field>

		<Field>
			<FieldLabel for="new-{id}">{m.password_new()}</FieldLabel>
			<Input
				id="new-{id}"
				type="password"
				bind:value={next}
				autocomplete="new-password"
				minlength={min}
				maxlength={max}
				required
			/>
		</Field>

		<Field>
			<FieldLabel for="again-{id}">{m.password_repeat()}</FieldLabel>
			<Input
				id="again-{id}"
				type="password"
				bind:value={again}
				autocomplete="new-password"
				required
			/>
		</Field>

		<Field orientation="horizontal">
			<Checkbox id="others-{id}" bind:checked={signOutOthers} />
			<FieldLabel for="others-{id}" class="font-normal">{m.password_sign_out_others()}</FieldLabel>
		</Field>

		{#if error}
			<FieldError>{error}</FieldError>
		{/if}
		{#if done}
			<p class="text-sm" role="status">{done}</p>
		{/if}

		<Field>
			<Button type="submit" class="w-fit" disabled={!hydrated || submitting}>
				{#if submitting}
					<Spinner />
				{/if}
				{m.password_change()}
			</Button>
		</Field>
	</FieldGroup>
</form>
