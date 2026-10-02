import { json, redirect, type Handle } from '@sveltejs/kit';

// Everything is private unless its route is listed here.
//
// The guard stands in front of the whole app because a layout's load does not
// protect what is under it: the client chooses which loads a data request
// runs (?x-sveltekit-invalidated), and endpoints, form actions and remote
// functions never run the layout at all. Better Auth's own routes
// (/api/auth/*) are answered by its handler above and do not get here.
const publicRoutes = new Set(['/login']);

export const handleGuard: Handle = ({ event, resolve }) => {
	if (event.locals.user || (event.route.id !== null && publicRoutes.has(event.route.id))) {
		return resolve(event);
	}

	const wantsPage =
		event.isDataRequest ||
		(event.request.method === 'GET' &&
			(event.request.headers.get('accept') ?? '').includes('text/html'));

	if (wantsPage) {
		redirect(302, '/login');
	}

	return json({ error: 'unauthorized' }, { status: 401 });
};
