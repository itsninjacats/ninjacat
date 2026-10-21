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

// NOTIFY on the channel the intake programs listen on (backend/intake/src/keys.rs).
// Never throws: the key is stored, and the intake re-reads every 30s anyway.
export async function refreshApiKeys(): Promise<boolean> {
	try {
		await db.execute(sql`NOTIFY ninjacat_api_keys`);
		return true;
	} catch {
		return false;
	}
}
