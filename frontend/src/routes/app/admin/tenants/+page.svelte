<script lang="ts">
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import * as Table from '$lib/components/ui/table';
	import * as NativeSelect from '$lib/components/ui/native-select';
	import { Input } from '$lib/components/ui/input';
	import { Field, FieldDescription, FieldError, FieldLabel } from '$lib/components/ui/field';
	import { m } from '$lib/paraglide/messages';
	import { getLocale } from '$lib/paraglide/runtime';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const date = (d: Date) => new Date(d).toLocaleDateString(getLocale(), { dateStyle: 'medium' });
</script>

<svelte:head><title>{m.tenants()} — ninjacat</title></svelte:head>

<div class="mx-auto max-w-4xl px-6 py-10">
	<p class="mb-8 text-sm text-muted-foreground">{m.admin_tenants_intro()}</p>

	<Card.Root class="mb-8">
		<Card.Header>
			<Card.Title class="text-base">{m.admin_tenant_new()}</Card.Title>
		</Card.Header>
		<Card.Content>
			<form method="POST" action="?/create" use:enhance class="grid gap-4 sm:grid-cols-3">
				<Field>
					<FieldLabel for="name">{m.admin_tenant_name()}</FieldLabel>
					<Input id="name" name="name" value={form?.name ?? ''} maxlength={100} required />
				</Field>
				<Field>
					<FieldLabel for="slug">{m.admin_tenant_slug()}</FieldLabel>
					<Input id="slug" name="slug" value={form?.slug ?? ''} maxlength={63} required />
					<FieldDescription>{m.admin_tenant_slug_hint()}</FieldDescription>
				</Field>
				<Field>
					<FieldLabel for="owner">{m.admin_tenant_owner()}</FieldLabel>
					<NativeSelect.Root id="owner" name="owner" class="w-full" required>
						{#each data.users as user (user.id)}
							<NativeSelect.Option
								value={user.id}
								selected={user.id === (form?.owner ?? data.adminId)}
							>
								{user.email}
							</NativeSelect.Option>
						{/each}
					</NativeSelect.Root>
				</Field>
				{#if form?.error}
					<FieldError class="sm:col-span-3">{form.error}</FieldError>
				{/if}
				<div class="sm:col-span-3">
					<Button type="submit">{m.admin_tenant_create()}</Button>
				</div>
			</form>
		</Card.Content>
	</Card.Root>

	{#if data.tenants.length === 0}
		<p class="text-sm text-muted-foreground">{m.admin_tenants_empty()}</p>
	{:else}
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head>{m.admin_tenant_name()}</Table.Head>
					<Table.Head>{m.admin_tenant_slug()}</Table.Head>
					<Table.Head>{m.admin_members()}</Table.Head>
					<Table.Head>{m.admin_created()}</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each data.tenants as tenant (tenant.id)}
					<Table.Row>
						<Table.Cell class="font-medium">
							<a
								href={resolve('/app/admin/tenants/[id]', { id: tenant.id })}
								class="hover:underline">{tenant.name}</a
							>
						</Table.Cell>
						<Table.Cell><code class="text-xs">{tenant.slug}</code></Table.Cell>
						<Table.Cell>{tenant.members}</Table.Cell>
						<Table.Cell class="text-sm text-muted-foreground">{date(tenant.createdAt)}</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	{/if}
</div>
