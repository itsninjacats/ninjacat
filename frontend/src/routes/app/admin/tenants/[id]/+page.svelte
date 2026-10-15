<script lang="ts">
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import { Button } from '$lib/components/ui/button';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as Card from '$lib/components/ui/card';
	import * as Table from '$lib/components/ui/table';
	import * as NativeSelect from '$lib/components/ui/native-select';
	import { Field, FieldLabel } from '$lib/components/ui/field';
	import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { m } from '$lib/paraglide/messages';
	import { roleName } from '$lib/role-name';
	import { tenantRoles } from '$lib/tenant';
	import type { SubmitFunction } from '@sveltejs/kit';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	// A refused change puts the select back on the role that is stored. By id:
	// enhance keeps the function it was first given, and the member object
	// in it is stale after the next load.
	function roleChange(memberId: string): SubmitFunction {
		return () =>
			async ({ formElement, result, update }) => {
				await update({ reset: false });
				const stored = data.members.find((row) => row.id === memberId)?.role;
				if (result.type === 'failure' && stored) {
					(formElement.elements.namedItem('role') as HTMLSelectElement).value = stored;
				}
			};
	}

	let adding = $state(false);
	const add: SubmitFunction = () => {
		adding = true;
		return async ({ update }) => {
			await update();
			adding = false;
		};
	};

	// The dialog is closed before the result lands, so that a refusal shown at
	// the top of the page is not hidden behind it.
	let removing = $state<string | null>(null);
	const remove: SubmitFunction = () => {
		removing = null;
		return ({ update }) => update();
	};
</script>

<svelte:head><title>{data.tenant.name} — ninjacat</title></svelte:head>

{#snippet roleForm(member: (typeof data.members)[number])}
	<form method="POST" action="?/role" use:enhance={roleChange(member.id)}>
		<input type="hidden" name="member" value={member.id} />
		<NativeSelect.Root
			name="role"
			size="sm"
			class="w-36"
			value={member.role}
			aria-label={m.admin_member_role()}
			onchange={(e) => e.currentTarget.form?.requestSubmit()}
		>
			{#each tenantRoles as role (role)}
				<NativeSelect.Option value={role}>{roleName(role)}</NativeSelect.Option>
			{/each}
		</NativeSelect.Root>
	</form>
{/snippet}

<div class="mx-auto max-w-4xl px-6 py-10">
	<a
		href={resolve('/app/admin/tenants')}
		class="mb-4 inline-flex items-center gap-1 text-sm text-muted-foreground hover:underline"
	>
		<ArrowLeftIcon class="size-4" />
		{m.admin_all_tenants()}
	</a>
	<div class="mb-8">
		<h2 class="font-heading text-2xl font-semibold tracking-tight wrap-anywhere">
			{data.tenant.name}
		</h2>
		<p class="mt-1 text-sm wrap-anywhere text-muted-foreground">
			<code>{data.tenant.slug}</code> · <code>{data.tenant.id}</code>
		</p>
	</div>

	{#if form?.error}
		<p class="mb-6 text-sm text-destructive" role="alert">{form.error}</p>
	{/if}

	<Card.Root class="mb-8">
		<Card.Header>
			<Card.Title class="text-base">{m.admin_member_add()}</Card.Title>
			<Card.Description>{m.admin_member_add_hint()}</Card.Description>
		</Card.Header>
		<Card.Content>
			{#if data.candidates.length === 0}
				<p class="text-sm text-muted-foreground">{m.admin_member_no_candidates()}</p>
			{:else}
				<form method="POST" action="?/add" use:enhance={add} class="flex flex-wrap items-end gap-3">
					<Field class="min-w-56 flex-1">
						<FieldLabel for="user">{m.admin_member_user()}</FieldLabel>
						<NativeSelect.Root id="user" name="user" class="w-full" required>
							{#each data.candidates as user (user.id)}
								<NativeSelect.Option value={user.id}>{user.email}</NativeSelect.Option>
							{/each}
						</NativeSelect.Root>
					</Field>
					<Field class="w-40">
						<FieldLabel for="role">{m.admin_member_role()}</FieldLabel>
						<NativeSelect.Root id="role" name="role" class="w-full" value="member">
							{#each tenantRoles as role (role)}
								<NativeSelect.Option value={role}>{roleName(role)}</NativeSelect.Option>
							{/each}
						</NativeSelect.Root>
					</Field>
					<Button type="submit" disabled={adding}>{m.admin_member_add_button()}</Button>
				</form>
			{/if}
		</Card.Content>
	</Card.Root>

	<h3 class="mb-3 text-sm font-semibold">{m.admin_members()}</h3>
	{#if data.members.length === 0}
		<p class="text-sm text-muted-foreground">{m.admin_members_empty()}</p>
	{:else}
		<!-- Below the width a role column needs beside an e-mail, the role goes
		     under the e-mail. -->
		<div class="@container">
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>{m.admin_member_user()}</Table.Head>
						<Table.Head class="hidden w-40 @md:table-cell">{m.admin_member_role()}</Table.Head>
						<Table.Head class="w-10"></Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each data.members as member (member.id)}
						<Table.Row>
							<Table.Cell class="wrap-anywhere whitespace-normal">
								<div class="font-medium">{member.email}</div>
								{#if member.name !== member.email}
									<div class="text-xs text-muted-foreground">{member.name}</div>
								{/if}
								<div class="mt-2 @md:hidden">{@render roleForm(member)}</div>
							</Table.Cell>
							<Table.Cell class="hidden @md:table-cell">{@render roleForm(member)}</Table.Cell>
							<Table.Cell>
								<AlertDialog.Root
									bind:open={
										() => removing === member.id, (open) => (removing = open ? member.id : null)
									}
								>
									<AlertDialog.Trigger>
										{#snippet child({ props })}
											<Button
												{...props}
												variant="ghost"
												size="icon"
												aria-label={m.admin_member_remove()}
											>
												<Trash2Icon class="size-4" />
											</Button>
										{/snippet}
									</AlertDialog.Trigger>
									<AlertDialog.Content>
										<AlertDialog.Header>
											<AlertDialog.Title class="wrap-anywhere">
												{m.admin_member_remove_confirm({
													email: member.email,
													tenant: data.tenant.name
												})}
											</AlertDialog.Title>
											<AlertDialog.Description>
												{m.admin_member_remove_explained()}
											</AlertDialog.Description>
										</AlertDialog.Header>
										<AlertDialog.Footer>
											<AlertDialog.Cancel>{m.cancel()}</AlertDialog.Cancel>
											<form method="POST" action="?/remove" use:enhance={remove}>
												<input type="hidden" name="member" value={member.id} />
												<AlertDialog.Action type="submit" class="w-full sm:w-auto"
													>{m.admin_member_remove()}</AlertDialog.Action
												>
											</form>
										</AlertDialog.Footer>
									</AlertDialog.Content>
								</AlertDialog.Root>
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</div>
	{/if}
</div>
