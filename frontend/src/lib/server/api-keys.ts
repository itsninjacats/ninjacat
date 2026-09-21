import { randomBytes, createHash } from 'node:crypto';

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
