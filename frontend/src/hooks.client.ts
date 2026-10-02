import type { ClientInit } from '@sveltejs/kit';
import { env } from '$env/dynamic/public';

// Self-observability: the panel reports its own page loads, errors and console
// output to the NinjaCat it belongs to, through Datadog's browser SDKs. Off
// unless both variables are set, and the SDKs are not downloaded while it is off.
//
// `proxy` is what keeps the data at home: with it set, the SDKs build every
// intake URL from it and never from `site`.
async function reportToOwnIntake(clientToken: string, intake: string) {
	const [{ datadogRum }, { datadogLogs }] = await Promise.all([
		import('@datadog/browser-rum'),
		import('@datadog/browser-logs')
	]);

	const shared = {
		clientToken,
		proxy: ({ path, parameters }: { path: string; parameters: string }) =>
			`${intake}${path}?${parameters}`,
		service: 'ninjacat-panel',
		env: env.PUBLIC_NINJACAT_ENV || 'dev',
		// The SDK's reports about itself are meant for its authors, not for us.
		telemetrySampleRate: 0
	};

	datadogRum.init({
		...shared,
		applicationId: 'ninjacat-panel',
		sessionReplaySampleRate: 0,
		trackResources: true,
		trackLongTasks: true,
		trackUserInteractions: true
	});
	datadogLogs.init({ ...shared, forwardErrorsToLogs: true, forwardConsoleLogs: ['warn', 'error'] });
}

export const init: ClientInit = () => {
	const clientToken = env.PUBLIC_NINJACAT_RUM_CLIENT_TOKEN;
	const intake = env.PUBLIC_NINJACAT_RUM_INTAKE;

	if (clientToken && intake) {
		// Not awaited: the panel must not wait for its own monitoring to start.
		void reportToOwnIntake(clientToken, intake.replace(/\/+$/, ''));
	}
};
