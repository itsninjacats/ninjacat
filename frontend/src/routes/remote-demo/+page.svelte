<script lang="ts">
	// Importujesz funkcje serwerowa jak zwykly modul...
	import { pobierzZadania, dodajZadanie, usunZadanie } from './zadania.remote';

	let tytul = $state('');

	async function dodaj() {
		// ...i wolasz ja jak zwykla funkcje. Pod spodem leci HTTP.
		await dodajZadanie(tytul);
		tytul = '';
	}
</script>

<h1>Remote functions</h1>

<input bind:value={tytul} placeholder="Nowe zadanie" />
<button onclick={dodaj}>Dodaj</button>

{#await pobierzZadania()}
	<p>ladowanie...</p>
{:then zadania}
	<ul>
		{#each zadania as z (z.id)}
			<li>
				[{z.priority}] {z.title}
				<button onclick={() => usunZadanie(z.id)}>usun</button>
			</li>
		{/each}
	</ul>
{:catch blad}
	<p style="color:crimson">{blad.message}</p>
{/await}
