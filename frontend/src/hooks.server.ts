import { sequence } from '@sveltejs/kit/hooks';
import { building } from '$app/environment';
import { env } from '$env/dynamic/public';
import { auth } from '$lib/server/auth';
import { svelteKitHandler } from 'better-auth/svelte-kit';
import type { Handle } from '@sveltejs/kit';
import { getTextDirection } from '$lib/paraglide/runtime';
import { paraglideMiddleware } from '$lib/paraglide/server';
import { handleGuard } from '$lib/server/guard';
import { db } from '$lib/server/db';
import { memberships, resolveTenant } from '$lib/server/tenant';

// The panel's own RUM reports go to another origin (hooks.client.ts), which the
// CSP built into the app at build time cannot know.
const rumIntake =
	env.PUBLIC_NINJACAT_RUM_CLIENT_TOKEN && env.PUBLIC_NINJACAT_RUM_INTAKE
		? URL.parse(env.PUBLIC_NINJACAT_RUM_INTAKE)?.origin
		: undefined;

const handleHeaders: Handle = async ({ event, resolve }) => {
	const response = await resolve(event);
	const headers = response.headers;
	const csp = headers.get('content-security-policy');
	if (csp && rumIntake) {
		headers.set(
			'content-security-policy',
			csp.replace("connect-src 'self'", `connect-src 'self' ${rumIntake}`)
		);
	}
	headers.set('x-content-type-options', 'nosniff');
	headers.set('referrer-policy', 'strict-origin-when-cross-origin');
	// TLS ends at the proxy; adapter-node builds event.url from ORIGIN.
	if (event.url.protocol === 'https:') {
		headers.set('strict-transport-security', 'max-age=31536000');
	}
	return response;
};

const handleParaglide: Handle = ({ event, resolve }) =>
	paraglideMiddleware(event.request, ({ request, locale }) => {
		event.request = request;

		return resolve(event, {
			transformPageChunk: ({ html }) =>
				html
					.replace('%paraglide.lang%', locale)
					.replace('%paraglide.dir%', getTextDirection(locale))
		});
	});

const handleBetterAuth: Handle = async ({ event, resolve }) => {
	const headers = event.request.headers;
	const session = await auth.api.getSession({ headers });

	if (session) {
		event.locals.session = session.session;
		event.locals.user = session.user;
	}
	const { tenants, tenant } = await resolveTenant(session?.session ?? null, {
		memberships: (userId) => memberships(db, userId),
		setActive: (organizationId) =>
			auth.api.setActiveOrganization({ headers, body: { organizationId } })
	});
	event.locals.tenants = tenants;
	event.locals.tenant = tenant;

	return svelteKitHandler({ event, resolve, auth, building });
};

export const handle: Handle = sequence(
	handleHeaders,
	handleParaglide,
	handleBetterAuth,
	handleGuard
);
