import { fail, redirect } from '@sveltejs/kit';
import { APIError } from 'better-auth/api';
import { auth } from '$lib/server/auth';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => {
	// Zalogowanego odsylamy od razu do aplikacji.
	if (locals.user) {
		redirect(302, '/app');
	}
	return {};
};

export const actions: Actions = {
	// Tylko logowanie. Rejestracji swiadomie nie ma - konta zaklada sie inaczej.
	zaloguj: async ({ request }) => {
		const dane = await request.formData();
		const email = dane.get('email')?.toString().trim() ?? '';
		const haslo = dane.get('haslo')?.toString() ?? '';

		if (!email || !haslo) {
			return fail(400, { blad: 'Podaj e-mail i hasło', email });
		}

		try {
			await auth.api.signInEmail({ body: { email, password: haslo } });
		} catch (error) {
			if (error instanceof APIError) {
				return fail(400, { blad: 'Nieprawidłowy e-mail lub hasło', email });
			}
			return fail(500, { blad: 'Coś poszło nie tak. Spróbuj ponownie.', email });
		}

		redirect(302, '/app');
	}
};
