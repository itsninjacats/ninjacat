import { pgTable, serial, integer, text, uuid, timestamp, index } from 'drizzle-orm/pg-core';
import { user } from './auth.schema';

export const task = pgTable('task', {
	id: serial('id').primaryKey(),
	title: text('title').notNull(),
	priority: integer('priority').notNull().default(1)
});

// Klucze API dla agentow. Agent wysyla klucz w naglowku Dd-Api-Key przy
// KAZDYM zadaniu, wiec odbiornik musi go szybko odnalezc.
//
// Dwie decyzje projektowe:
//
// 1. Trzymamy SKROT, nie klucz. Klucz w postaci jawnej pokazujemy raz,
//    przy tworzeniu, i nigdy wiecej - tak jak robi to GitHub czy Stripe.
//
// 2. Skrot jest SHA-256, a nie bcrypt. Przy hasle szukasz najpierw po loginie
//    i dopiero potem weryfikujesz, wiec mozesz uzyc wolnego, solonego skrotu.
//    Tutaj szukasz PO SAMYM SKROCIE, wiec musi byc deterministyczny.
//    To bezpieczne, bo klucz ma 128 bitow entropii - nie da sie go zgadnac.
export const apiKey = pgTable(
	'api_key',
	{
		id: uuid('id').primaryKey().defaultRandom(),

		// Nazwa nadana przez czlowieka, np. "klaster produkcyjny".
		name: text('name').notNull(),

		// Do kogo naleza dane przychodzace z tym kluczem.
		//
		// Trafia do ClickHouse jako pierwsza kolumna ORDER BY, wiec zapytania
		// jednego klienta pomijaja cudze dane na poziomie granul.
		//
		// Przy jednym kliencie to zawsze 'default'. Kolumna jest tania,
		// ale dodanie jej pozniej oznaczaloby przepisanie tabel w ClickHouse —
		// dlatego jest od poczatku.
		tenantId: text('tenant_id').notNull().default('default'),

		// SHA-256 klucza, zapisany szesnastkowo. Po tym szuka odbiornik.
		keyHash: text('key_hash').notNull().unique(),

		// Pierwsze 8 znakow klucza - zeby dalo sie go rozpoznac na liscie.
		prefix: text('prefix').notNull(),

		createdAt: timestamp('created_at', { withTimezone: true }).defaultNow().notNull(),

		// Aktualizowane przez odbiornik przy uzyciu. Pozwala znalezc klucze,
		// ktore nikt juz nie uzywa.
		lastUsedAt: timestamp('last_used_at', { withTimezone: true }),

		createdById: text('created_by_id').references(() => user.id, { onDelete: 'set null' })
	},
	(table) => [
		// Odbiornik odpytuje po skrocie przy kazdym zadaniu agenta.
		index('api_key_hash_idx').on(table.keyHash)
	]
);

export * from './auth.schema';
