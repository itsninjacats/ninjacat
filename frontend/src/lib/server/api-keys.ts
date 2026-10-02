import { randomBytes, createHash } from 'node:crypto';
import { sql } from 'drizzle-orm';
import { db } from './db';

// Klucz w formacie datadogowym: 32 znaki szesnastkowe (128 bitow entropii).
// Dzieki temu wchodzi do istniejacych konfiguracji agenta bez zmian.
export function nowyKlucz(): string {
	return randomBytes(16).toString('hex');
}

// SHA-256, bo odbiornik szuka PO SKROCIE przy kazdym zadaniu agenta.
// Bcrypt by tu nie zadzialal - jest solony, wiec ten sam klucz dawalby
// inny skrot za kazdym razem i nie dalo by sie go odnalezc.
// To bezpieczne, bo klucza o 128 bitach entropii nie da sie zgadnac
// ani rozbic slownikiem.
export function skrotKlucza(klucz: string): string {
	return createHash('sha256').update(klucz).digest('hex');
}

// Pierwsze znaki klucza - do rozpoznania go na liscie bez ujawniania calosci.
export function prefiks(klucz: string): string {
	return klucz.slice(0, 8);
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
