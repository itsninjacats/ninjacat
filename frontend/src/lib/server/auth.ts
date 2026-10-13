import { env } from '$env/dynamic/private';
import { betterAuth } from 'better-auth/minimal';
import { sveltekitCookies } from 'better-auth/svelte-kit';
import { getRequestEvent } from '$app/server';
import { db } from '$lib/server/db';
import { authOptions } from './auth-options';

const options = authOptions(db, env);

export const auth = betterAuth({
	...options,
	plugins: [
		...options.plugins,
		sveltekitCookies(getRequestEvent) // make sure this is the last plugin in the array
	]
});
