<script lang="ts">
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import * as Table from '$lib/components/ui/table';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { Field, FieldDescription, FieldError, FieldLabel } from '$lib/components/ui/field';
	import { m } from '$lib/paraglide/messages';
	import { getLocale } from '$lib/paraglide/runtime';
	import { roleName } from '$lib/role-name';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const date = (d: Date) => new Date(d).toLocaleDateString(getLocale(), { dateStyle: 'medium' });
</script>

<svelte:head><title>{m.admin_users()} — ninjacat</title></svelte:head>

{#snippet tenantsOf(tenants: (typeof data.users)[number]['tenants'])}
	{#each tenants as tenant (tenant.id)}
		<div>
			<a href={resolve('/app/admin/tenants/[id]', { id: tenant.id })} class="hover:underline"
				>{tenant.name}</a
			>
			<span class="text-muted-foreground">· {roleName(tenant.role)}</span>
		</div>
	{:else}
		<span class="text-muted-foreground">{m.tenant_none()}</span>
	{/each}
{/snippet}

<div class="mx-auto max-w-4xl px-6 py-10">
	<p class="mb-8 text-sm text-muted-foreground">{m.admin_users_intro()}</p>

	{#if form?.created}
		<p class="mb-6 rounded-md border border-primary px-4 py-3 text-sm wrap-anywhere" role="status">
			{m.admin_user_created({ email: form.created })}
		</p>
	{/if}

	<Card.Root class="mb-8">
		<Card.Header>
			<Card.Title class="text-base">{m.admin_user_new()}</Card.Title>
		</Card.Header>
		<Card.Content class="@container">
			<form method="POST" action="?/create" use:enhance class="grid gap-4 @2xl:grid-cols-3">
				<Field>
					<FieldLabel for="email">{m.admin_user_email()}</FieldLabel>
					<Input id="email" name="email" type="email" value={form?.email ?? ''} required />
				</Field>
				<Field>
					<FieldLabel for="name">{m.admin_user_name()}</FieldLabel>
					<Input id="name" name="name" value={form?.name ?? ''} />
				</Field>
				<Field>
					<FieldLabel for="password">{m.admin_user_password()}</FieldLabel>
					<Input
						id="password"
						name="password"
						type="password"
						autocomplete="new-password"
						required
					/>
				</Field>
				<Field orientation="horizontal" class="@2xl:col-span-3">
					<Checkbox id="admin" name="admin" checked={form?.admin ?? false} />
					<div>
						<FieldLabel for="admin">{m.admin_user_instance_admin()}</FieldLabel>
						<FieldDescription>{m.admin_user_instance_admin_hint()}</FieldDescription>
					</div>
				</Field>
				{#if form?.error}
					<FieldError class="@2xl:col-span-3">{form.error}</FieldError>
				{/if}
				<div class="@2xl:col-span-3">
					<Button type="submit">{m.admin_user_create()}</Button>
				</div>
			</form>
		</Card.Content>
	</Card.Root>

	<!-- Below the width the columns need, a user's tenants go under the user. -->
	<div class="@container">
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head>{m.admin_member_user()}</Table.Head>
					<Table.Head class="hidden @2xl:table-cell">{m.admin_user_tenants()}</Table.Head>
					<Table.Head class="hidden @2xl:table-cell">{m.admin_created()}</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each data.users as user (user.id)}
					<Table.Row>
						<Table.Cell class="align-top wrap-anywhere whitespace-normal">
							<div class="flex flex-wrap items-center gap-x-2 gap-y-1 font-medium">
								{user.email}
								{#if user.instanceAdmin}
									<Badge variant="outline">{m.admin_user_instance_admin()}</Badge>
								{/if}
							</div>
							{#if user.name !== user.email}
								<div class="text-xs text-muted-foreground">{user.name}</div>
							{/if}
							<div class="mt-2 text-sm @2xl:hidden">
								{@render tenantsOf(user.tenants)}
								<div class="mt-1 text-xs text-muted-foreground">
									{m.admin_created()}
									{date(user.createdAt)}
								</div>
							</div>
						</Table.Cell>
						<Table.Cell
							class="hidden align-top text-sm wrap-anywhere whitespace-normal @2xl:table-cell"
						>
							{@render tenantsOf(user.tenants)}
						</Table.Cell>
						<Table.Cell class="hidden align-top text-sm text-muted-foreground @2xl:table-cell"
							>{date(user.createdAt)}</Table.Cell
						>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</div>
</div>
