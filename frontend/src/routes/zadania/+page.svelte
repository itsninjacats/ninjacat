<script lang="ts">
	// enhance sprawia, ze formularz leci fetchem zamiast przeladowywac strone.
	// Bez niego wszystko dziala tak samo, tylko z pelnym przeladowaniem -
	// dlatego strona dziala nawet z wylaczonym JavaScriptem.
	import { enhance } from '$app/forms';
	import type { PageProps } from './$types';

	// data  <- to, co zwrocil load()
	// form  <- to, co zwrocila akcja (fail() albo return)
	let { data, form }: PageProps = $props();
</script>

<h1>Zadania</h1>

<p>
	{#if data.uzytkownik}Zalogowany: {data.uzytkownik}{:else}Niezalogowany{/if}
</p>

{#if form?.blad}
	<p style="color: crimson">{form.blad}</p>
{/if}

<!-- method="POST" + action="?/dodaj" wskazuje na akcje `dodaj` -->
<form method="POST" action="?/dodaj" use:enhance>
	<input name="tytul" placeholder="Co do zrobienia" value={form?.tytul ?? ''} />
	<input name="priorytet" type="number" value="1" min="1" max="5" />
	<button>Dodaj</button>
</form>

<p>
	Filtr:
	<a href="/zadania">wszystkie</a>
	{#each [1, 2, 3] as p}
		<a href="/zadania?priorytet={p}">priorytet {p}</a>
	{/each}
	{#if data.filtr}<em>(pokazuje tylko {data.filtr})</em>{/if}
</p>

<ul>
	{#each data.zadania as z (z.id)}
		<li>
			<strong>[{z.priority}]</strong>
			{z.title}
			<form method="POST" action="?/usun" use:enhance style="display:inline">
				<input type="hidden" name="id" value={z.id} />
				<button>usun</button>
			</form>
		</li>
	{:else}
		<li>Nic tu jeszcze nie ma.</li>
	{/each}
</ul>
