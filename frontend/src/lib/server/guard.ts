import { error, redirect, type Handle, type RequestEvent } from '@sveltejs/kit';
import { isInstanceAdmin } from './auth-options';

// Everything is private unless its route is listed here.
//
// The guard stands in front of the whole app because a layout's load does not
// protect what is under it: the client chooses which loads a data request
// runs (?x-sveltekit-invalidated), and endpoints, form actions and remote
// functions never run the layout at all. Better Auth's own routes
// (/api/auth/*) are answered by its handler above and do not get here.
const publicRoutes = new Set(['/login']);

// Where a signed-in user who is a member of no tenant is sent. Nothing falls
// back to `default`: data read or written there would be another tenant's.
const noTenantRoute = '/no-tenant';

// The instance admin's pages. They work without a tenant, because the admin
// is the one who creates tenants and adds their members.
//
// This covers pages, their form actions and endpoints. A remote function has
// no route (SvelteKit does not match one for a remote call), so it never
// counts as an admin route: one that only the instance admin may call checks
// isInstanceAdmin itself.
function isAdminRoute(routeId: string | null): boolean {
	return routeId === '/app/admin' || (routeId?.startsWith('/app/admin/') ?? false);
}

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

	if (isAdminRoute(routeId)) {
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
