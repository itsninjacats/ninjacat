<script lang="ts">
	import LoginForm from '$lib/components/login/login-form.svelte';
	import type { PageProps } from './$types';

	let { form }: PageProps = $props();
</script>

<svelte:head><title>Logowanie — ninjacat</title></svelte:head>

<div class="grid min-h-svh lg:grid-cols-2">
	<div class="flex flex-col gap-4 p-6 md:p-10">
		<div class="flex justify-center gap-2 md:justify-start">
			<a href="/" class="flex items-center gap-2 font-medium">
				<div class="bg-primary size-6 rounded-md"></div>
				ninjacat
			</a>
		</div>

		<div class="flex flex-1 items-center justify-center">
			<div class="w-full max-w-xs">
				<LoginForm blad={form?.blad ?? null} email={form?.email ?? ''} />
			</div>
		</div>

		<p class="text-muted-foreground text-center text-xs">
			Dane zostają u Ciebie. Zawsze.
		</p>
	</div>

	<!-- Panel dekoracyjny. Docelowo moze tu byc podglad metryk na zywo. -->
	<div class="bg-muted relative hidden lg:block">
		<div class="absolute inset-0 flex items-center justify-center p-16">
			<div class="max-w-sm">
				<div class="bg-primary mb-6 size-10 rounded-md"></div>
				<p class="font-heading text-2xl leading-snug font-semibold tracking-tight">
					Odbiornik telemetrii mówiący protokołem Datadog Agenta.
				</p>
				<p class="text-muted-foreground mt-4 text-sm leading-relaxed">
					Przekierowujesz agenty, dane lądują w Twoim ClickHouse.
					Bez wymiany czegokolwiek na maszynach.
				</p>
			</div>
		</div>
	</div>
</div>
