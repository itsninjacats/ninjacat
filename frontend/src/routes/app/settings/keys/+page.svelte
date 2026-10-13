<script lang="ts">
	import { enhance } from '$app/forms';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import * as Table from '$lib/components/ui/table';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Input } from '$lib/components/ui/input';
	import { Field, FieldLabel, FieldError } from '$lib/components/ui/field';
	import { Badge } from '$lib/components/ui/badge';
	import { Separator } from '$lib/components/ui/separator';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import CheckIcon from '@lucide/svelte/icons/check';
	import KeyIcon from '@lucide/svelte/icons/key-round';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { m } from '$lib/paraglide/messages';
	import { getLocale } from '$lib/paraglide/runtime';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	let copied = $state(false);

	async function copy(text: string) {
		await navigator.clipboard.writeText(text);
		copied = true;
		setTimeout(() => (copied = false), 2000);
	}

	function date(d: Date | string) {
		return new Date(d).toLocaleString(getLocale(), { dateStyle: 'medium', timeStyle: 'short' });
	}
</script>

<svelte:head><title>{m.api_keys()} — ninjacat</title></svelte:head>

<div class="mx-auto max-w-4xl px-6 py-10">
	<p class="mb-8 text-sm text-muted-foreground">{m.api_keys_intro()}</p>

	<!-- The key in the clear is shown once, right after it is made; the server
	     keeps only its hash. -->
	{#if form?.created}
		<Card.Root class="mb-6 border-primary">
			<Card.Header>
				<Card.Title class="text-base wrap-anywhere"
					>{m.api_key_created({ name: form.name })}</Card.Title
				>
				<Card.Description>{m.api_key_created_explained()}</Card.Description>
			</Card.Header>
			<Card.Content>
				{#if form.activeNow === false}
					<p class="mb-3 text-xs text-muted-foreground">{m.api_key_not_active_yet()}</p>
				{/if}
				<div class="flex items-center gap-2">
					<code class="min-w-0 flex-1 rounded-md bg-muted px-3 py-2 font-mono text-sm break-all">
						{form.created}
					</code>
					<Button
						variant="outline"
						size="icon"
						aria-label={m.api_key_copy()}
						onclick={() => copy(form.created)}
					>
						{#if copied}
							<CheckIcon class="size-4" />
						{:else}
							<CopyIcon class="size-4" />
						{/if}
					</Button>
				</div>
			</Card.Content>
		</Card.Root>
	{/if}

	{#if form?.revokedNow === false}
		<p class="mb-6 text-sm text-muted-foreground">{m.api_key_not_revoked_yet()}</p>
	{/if}
	{#if form?.deleteError}
		<p class="mb-6 text-sm text-destructive" role="alert">{form.deleteError}</p>
	{/if}

	<Card.Root class="mb-8">
		<Card.Header>
			<Card.Title class="text-base">{m.api_key_new()}</Card.Title>
			<Card.Description>{m.api_key_new_hint()}</Card.Description>
		</Card.Header>
		<Card.Content>
			<form method="POST" action="?/create" use:enhance>
				<input type="hidden" name="tenant" value={data.tenant?.id} />
				<Field>
					<FieldLabel for="name">{m.api_key_name()}</FieldLabel>
					<div class="flex gap-3">
						<Input
							id="name"
							name="name"
							placeholder={m.api_key_name_placeholder()}
							value={form?.created ? '' : (form?.name ?? '')}
							maxlength={100}
							required
						/>
						<Button type="submit">{m.api_key_create()}</Button>
					</div>
					{#if form?.error}
						<FieldError>{form.error}</FieldError>
					{/if}
				</Field>
			</form>
		</Card.Content>
	</Card.Root>

	<Separator class="mb-8" />

	{#if data.keys.length === 0}
		<Card.Root class="p-10">
			<div class="flex flex-col items-center gap-3 text-center">
				<KeyIcon class="size-8 text-muted-foreground" />
				<div>
					<p class="text-sm font-medium">{m.api_keys_empty()}</p>
					<p class="mt-1 text-sm text-muted-foreground">{m.api_keys_empty_hint()}</p>
				</div>
			</div>
		</Card.Root>
	{:else}
		<!-- Below the width the columns need, a key is its name, with what those
		     columns hold underneath. -->
		<div class="@container">
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>{m.api_key_name()}</Table.Head>
						<Table.Head class="hidden @2xl:table-cell">{m.api_key_key()}</Table.Head>
						<Table.Head class="hidden @2xl:table-cell">{m.api_key_created_at()}</Table.Head>
						<Table.Head class="hidden @2xl:table-cell">{m.api_key_last_used()}</Table.Head>
						<Table.Head class="w-10"></Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each data.keys as k (k.id)}
						<Table.Row>
							<Table.Cell class="font-medium wrap-anywhere whitespace-normal">
								{k.name}
								<div class="mt-1 text-xs font-normal text-muted-foreground @2xl:hidden">
									<code>{k.prefix}…</code> · {m.api_key_created_at()}
									{date(k.createdAt)} ·
									{k.lastUsedAt
										? `${m.api_key_last_used()} ${date(k.lastUsedAt)}`
										: m.api_key_unused()}
								</div>
							</Table.Cell>
							<Table.Cell class="hidden @2xl:table-cell">
								<code class="text-xs text-muted-foreground">{k.prefix}…</code>
							</Table.Cell>
							<Table.Cell class="hidden text-sm text-muted-foreground @2xl:table-cell"
								>{date(k.createdAt)}</Table.Cell
							>
							<Table.Cell class="hidden text-sm @2xl:table-cell">
								{#if k.lastUsedAt}
									<span class="text-muted-foreground">{date(k.lastUsedAt)}</span>
								{:else}
									<Badge variant="outline">{m.api_key_unused()}</Badge>
								{/if}
							</Table.Cell>
							<Table.Cell>
								<AlertDialog.Root>
									<AlertDialog.Trigger>
										{#snippet child({ props })}
											<Button
												{...props}
												variant="ghost"
												size="icon"
												aria-label={m.api_key_delete()}
											>
												<Trash2Icon class="size-4" />
											</Button>
										{/snippet}
									</AlertDialog.Trigger>
									<AlertDialog.Content>
										<AlertDialog.Header>
											<AlertDialog.Title class="wrap-anywhere"
												>{m.api_key_delete_confirm({ name: k.name })}</AlertDialog.Title
											>
											<AlertDialog.Description
												>{m.api_key_delete_explained()}</AlertDialog.Description
											>
										</AlertDialog.Header>
										<AlertDialog.Footer>
											<AlertDialog.Cancel>{m.cancel()}</AlertDialog.Cancel>
											<form method="POST" action="?/delete" use:enhance>
												<input type="hidden" name="tenant" value={data.tenant?.id} />
												<input type="hidden" name="id" value={k.id} />
												<!-- Stretched like Cancel while the footer stacks; the form would not stretch it. -->
												<AlertDialog.Action type="submit" class="w-full sm:w-auto"
													>{m.api_key_delete()}</AlertDialog.Action
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
