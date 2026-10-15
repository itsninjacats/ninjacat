<script lang="ts">
	import { resolve } from '$app/paths';
	import LoginForm from '$lib/components/login/login-form.svelte';
</script>

<svelte:head><title>Sign in — ninjacat</title></svelte:head>

<div class="grid min-h-svh lg:grid-cols-2">
	<div class="flex flex-col gap-4 p-6 md:p-10">
		<div class="flex justify-center gap-2 md:justify-start">
			<a href={resolve('/')} class="flex items-center gap-2 font-medium">
				<div class="size-6 rounded-md bg-primary"></div>
				ninjacat
			</a>
		</div>

		<div class="flex flex-1 items-center justify-center">
			<div class="w-full max-w-xs">
				<LoginForm />
			</div>
		</div>

		<p class="text-center text-xs text-muted-foreground">Your data stays with you. Always.</p>
	</div>

	<div class="relative hidden bg-muted lg:block">
		<div class="absolute inset-0 flex items-center justify-center p-16">
			<div class="max-w-sm">
				<div class="mb-6 size-10 rounded-md bg-primary"></div>
				<p class="font-heading text-2xl leading-snug font-semibold tracking-tight">
					A telemetry backend that speaks the Datadog Agent's protocol.
				</p>
				<p class="mt-4 text-sm leading-relaxed text-muted-foreground">
					Point your agents at it and the data lands in your ClickHouse. Nothing to replace on the
					machines.
				</p>
			</div>
		</div>
	</div>
</div>
