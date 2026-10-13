import { randomBytes, createHash } from 'node:crypto';
import { sql } from 'drizzle-orm';
import { db } from './db';

// Datadog's shape, 32 hex characters (128 bits), so a key drops into an
// existing agent config unchanged.
export function newKey(): string {
	return randomBytes(16).toString('hex');
}

// SHA-256 rather than bcrypt: the intake looks a key up by its hash on every
// request, and a salted hash differs each time. 128 random bits cannot be
// guessed or found in a dictionary, so the hash needs no salt.
export function keyHash(key: string): string {
	return createHash('sha256').update(key).digest('hex');
}

// Enough of the key to recognise it in the list without revealing it.
export function keyPrefix(key: string): string {
	return key.slice(0, 8);
}

/**
 * Tells the intake that the keys changed, so it re-reads them now instead of
 * at its next periodic refresh. The message travels through Postgres (NOTIFY),
 * which the panel and the intake already share; the channel name is the one
 * the intake listens on (api/src/Server/ApiKeysKeeper.fs).
 *
 * NEVER throws: the key is already stored, and the intake picks it up within
 * 30 seconds anyway. It reports whether it worked instead.
 */
export async function refreshApiKeys(): Promise<{ ok: boolean; reason?: string }> {
	try {
		await db.execute(sql`NOTIFY ninjacat_api_keys`);
		return { ok: true };
	} catch (e) {
		return { ok: false, reason: e instanceof Error ? e.message : 'unknown error' };
	}
}
