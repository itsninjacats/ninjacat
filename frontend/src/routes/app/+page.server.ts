import { redirect } from '@sveltejs/kit';
import { auth } from '$lib/server/auth';
import type { Actions } from './$types';

// Straznik i dane uzytkownika sa w +layout.server.ts — obejmuja cale /app.
// Tutaj zostaje tylko akcja wylogowania, bo akcje moga zyc wylacznie
// w +page.server.ts. Formularz w layoucie celuje w nia przez /app?/wyloguj.
export const actions: Actions = {
	wyloguj: async ({ request }) => {
		await auth.api.signOut({ headers: request.headers });
		redirect(302, '/login');
	}
};
