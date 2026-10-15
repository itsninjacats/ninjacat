import { error, redirect, type Handle, type RequestEvent } from '@sveltejs/kit';
import { isInstanceAdmin } from './auth-options';

// Everything is private unless listed. A layout load does not protect what is
// under it: data requests pick their loads, and endpoints, actions and remote
// functions skip it.
const publicRoutes = new Set(['/login']);

// Where a signed-in user who is a member of no tenant is sent. Nothing falls
// back to `default`: data read or written there would be another tenant's.
const noTenantRoute = '/no-tenant';

// What can follow a redirect: a page, a data request, and an enhanced form
// action, to which SvelteKit sends the redirect as an action result.
function canRedirect(event: RequestEvent): boolean {
	const headers = event.request.headers;
	return (
		event.isDataRequest ||
		headers.get('x-sveltekit-action') === 'true' ||
		(event.request.method === 'GET' && (headers.get('accept') ?? '').includes('text/html'))
	);
}

export const handleGuard: Handle = ({ event, resolve }) => {
	const routeId = event.route.id;
	const user = event.locals.user;

	if (routeId !== null && publicRoutes.has(routeId)) {
		return resolve(event);
	}

	if (!user) {
		if (canRedirect(event)) {
			redirect(302, '/login');
		}
		error(401, 'unauthorized');
	}

	// The admin pages work without a tenant: the admin is the one who creates tenants. A remote
	// call has no route, so an admin-only remote function checks isInstanceAdmin itself.
	if (routeId === '/app/admin' || routeId?.startsWith('/app/admin/')) {
		if (!isInstanceAdmin(user.role)) {
			error(403, 'forbidden');
		}
		return resolve(event);
	}

	if (event.locals.tenant || routeId === noTenantRoute) {
		return resolve(event);
	}

	if (canRedirect(event)) {
		redirect(302, noTenantRoute);
	}
	error(403, 'no tenant');
};
