import { randomBytes } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { drizzle } from 'drizzle-orm/postgres-js';
import { migrate } from 'drizzle-orm/postgres-js/migrator';
import postgres from 'postgres';
import type { Database } from '../auth-options';
import * as schema from './schema';

// For the *.postgres.spec.ts files: a scratch database built from the Drizzle
// migrations, so the rules that live in SQL are what is tested. The dev
// stack's Postgres unless DATABASE_URL names another.
const url = process.env.DATABASE_URL ?? 'postgres://root:mysecretpassword@localhost:5433/local';

export const migrations = fileURLToPath(new URL('../../../../drizzle', import.meta.url));

export async function reachable(): Promise<boolean> {
	const probe = postgres(url, { max: 1, connect_timeout: 2, onnotice: () => {} });
	try {
		await probe`select 1`;
		return true;
	} catch {
		return false;
	} finally {
		await probe.end();
	}
}

// Nothing connects until `create`; `drop` cleans up after a failed one too.
// A test of a migration passes a folder holding only the ones before it.
export function scratchDatabase(migrationsFolder = migrations) {
	const name = `ninjacat_test_${randomBytes(6).toString('hex')}`;
	const admin = postgres(url, { max: 1, onnotice: () => {} });
	const scratch = new URL(url);
	scratch.pathname = `/${name}`;
	const sql = postgres(scratch.toString(), { onnotice: () => {} });
	const db: Database = drizzle(sql, { schema });

	return {
		sql,
		db,
		async create() {
			await admin.unsafe(`create database ${name}`);
			await migrate(db, { migrationsFolder });
		},
		async drop() {
			await sql.end();
			await admin.unsafe(`drop database if exists ${name} with (force)`);
			await admin.end();
		}
	};
}
