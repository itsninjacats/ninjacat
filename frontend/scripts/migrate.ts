/**
 * Standalone migration entrypoint — the equivalent of `manage.py migrate`.
 *
 * This exists so that applying migrations does NOT require drizzle-kit, and
 * therefore does not require the dev toolchain. drizzle-kit is a development
 * tool: it *generates* migrations by diffing the schema. Applying the SQL it
 * already generated needs only drizzle-orm's migrator, which reads the same
 * ./drizzle folder and tracks what ran in __drizzle_migrations.
 *
 * It is bundled into the runtime image (see Dockerfile), so the same small
 * image both serves traffic and migrates — which is what lets Kubernetes run
 * it as a pre-upgrade hook or an init container.
 *
 * Deliberately not importing anything from $lib or $env: those are SvelteKit
 * aliases, and this has to run as a plain Node process outside the app.
 */
import { drizzle } from 'drizzle-orm/postgres-js';
import { migrate } from 'drizzle-orm/postgres-js/migrator';
import postgres from 'postgres';

const url = process.env.DATABASE_URL;
if (!url) {
	console.error('migrate: DATABASE_URL is not set');
	process.exit(1);
}

// The migrations folder ships next to this script in the image. Override with
// MIGRATIONS_DIR if the layout differs.
const migrationsFolder = process.env.MIGRATIONS_DIR ?? './drizzle';

// max: 1 is what drizzle documents for migrations — a single connection, so
// statements cannot interleave across the pool.
const sql = postgres(url, { max: 1, onnotice: () => {} });

try {
	const started = Date.now();
	await migrate(drizzle(sql), { migrationsFolder });
	console.log(`migrate: schema up to date (${Date.now() - started}ms)`);
} catch (e) {
	console.error('migrate: failed —', e instanceof Error ? e.message : e);
	process.exit(1);
} finally {
	await sql.end();
}
