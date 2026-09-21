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
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	let skopiowano = $state(false);

	async function kopiuj(tekst: string) {
		await navigator.clipboard.writeText(tekst);
		skopiowano = true;
		setTimeout(() => (skopiowano = false), 2000);
	}

	function data_(d: Date | string | null) {
		if (!d) return '—';
		return new Date(d).toLocaleString('pl-PL', { dateStyle: 'medium', timeStyle: 'short' });
	}
</script>

<svelte:head><title>Klucze API — ninjacat</title></svelte:head>

<div class="mx-auto max-w-4xl px-6 py-10">
	<div class="mb-8">
		<h1 class="font-heading text-2xl font-semibold tracking-tight">Klucze API</h1>
		<p class="mt-2 text-sm text-muted-foreground">
			Agent wysyła klucz w nagłówku <code>Dd-Api-Key</code> przy każdym żądaniu. Bez poprawnego klucza
			odbiornik odrzuca dane.
		</p>
	</div>

	<!-- Klucz w postaci jawnej pokazujemy TYLKO raz, zaraz po utworzeniu. -->
	{#if form?.utworzony}
		<Card.Root class="mb-6 border-primary">
			<Card.Header>
				<Card.Title class="text-base">Klucz „{form.nazwa}" utworzony</Card.Title>
				<Card.Description>
					Skopiuj go teraz — nie pokażemy go ponownie. W bazie zostaje tylko skrót.
				</Card.Description>
			</Card.Header>
			<Card.Content>
				{#if form.aktywnyOdRazu === false}
					<p class="text-muted-foreground mb-3 text-xs">
						Odbiornik nie odpowiedział — klucz zacznie działać w ciągu 30 sekund,
						przy najbliższym odświeżeniu.
					</p>
				{/if}
				<div class="flex items-center gap-2">
					<code class="flex-1 overflow-x-auto rounded-md bg-muted px-3 py-2 font-mono text-sm">
						{form.utworzony}
					</code>
					<Button variant="outline" size="icon" onclick={() => kopiuj(form.utworzony)}>
						{#if skopiowano}
							<CheckIcon class="size-4" />
						{:else}
							<CopyIcon class="size-4" />
						{/if}
					</Button>
				</div>
			</Card.Content>
		</Card.Root>
	{/if}

	<!-- Tworzenie -->
	<Card.Root class="mb-8">
		<Card.Header>
			<Card.Title class="text-base">Nowy klucz</Card.Title>
			<Card.Description>Nazwij go tak, żebyś wiedział, która maszyna go używa.</Card.Description>
		</Card.Header>
		<Card.Content>
			<form method="POST" action="?/utworz" use:enhance class="flex items-end gap-3">
				<Field class="flex-1">
					<FieldLabel for="nazwa">Nazwa</FieldLabel>
					<Input
						id="nazwa"
						name="nazwa"
						placeholder="klaster produkcyjny"
						value={form?.nazwa ?? ''}
						required
					/>
					{#if form?.blad}
						<FieldError>{form.blad}</FieldError>
					{/if}
				</Field>
				<Button type="submit">Utwórz</Button>
			</form>
		</Card.Content>
	</Card.Root>

	<Separator class="mb-8" />

	<!-- Lista -->
	{#if data.klucze.length === 0}
		<Card.Root class="p-10">
			<div class="flex flex-col items-center gap-3 text-center">
				<KeyIcon class="size-8 text-muted-foreground" />
				<div>
					<p class="text-sm font-medium">Nie masz jeszcze żadnego klucza</p>
					<p class="mt-1 text-sm text-muted-foreground">Utwórz pierwszy, żeby podłączyć agenta.</p>
				</div>
			</div>
		</Card.Root>
	{:else}
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head>Nazwa</Table.Head>
					<Table.Head>Klucz</Table.Head>
					<Table.Head>Utworzony</Table.Head>
					<Table.Head>Ostatnio użyty</Table.Head>
					<Table.Head class="w-10"></Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each data.klucze as k (k.id)}
					<Table.Row>
						<Table.Cell class="font-medium">{k.name}</Table.Cell>
						<Table.Cell>
							<code class="text-xs text-muted-foreground">{k.prefix}…</code>
						</Table.Cell>
						<Table.Cell class="text-sm text-muted-foreground">{data_(k.createdAt)}</Table.Cell>
						<Table.Cell class="text-sm">
							{#if k.lastUsedAt}
								<span class="text-muted-foreground">{data_(k.lastUsedAt)}</span>
							{:else}
								<Badge variant="outline">nieużywany</Badge>
							{/if}
						</Table.Cell>
						<Table.Cell>
							<AlertDialog.Root>
								<AlertDialog.Trigger>
									{#snippet child({ props })}
										<Button {...props} variant="ghost" size="icon">
											<Trash2Icon class="size-4" />
										</Button>
									{/snippet}
								</AlertDialog.Trigger>
								<AlertDialog.Content>
									<AlertDialog.Header>
										<AlertDialog.Title>Usunąć klucz „{k.name}"?</AlertDialog.Title>
										<AlertDialog.Description>
											Agenty używające tego klucza natychmiast przestaną być przyjmowane. Tej
											operacji nie da się cofnąć.
										</AlertDialog.Description>
									</AlertDialog.Header>
									<AlertDialog.Footer>
										<AlertDialog.Cancel>Anuluj</AlertDialog.Cancel>
										<form method="POST" action="?/usun" use:enhance>
											<input type="hidden" name="id" value={k.id} />
											<AlertDialog.Action type="submit">Usuń</AlertDialog.Action>
										</form>
									</AlertDialog.Footer>
								</AlertDialog.Content>
							</AlertDialog.Root>
						</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	{/if}
</div>
