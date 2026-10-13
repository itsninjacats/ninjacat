import { pgTable, text, uuid, timestamp, index } from 'drizzle-orm/pg-core';
import { organization, user } from './auth.schema';

// The agents' API keys. An agent sends its key in Dd-Api-Key with EVERY
// request, so the intake must find it fast.
//
// Two design decisions:
//
// 1. We keep a HASH, not the key. The key in the clear is shown once, when it
//    is created, and never again, as GitHub and Stripe do.
//
// 2. The hash is SHA-256, not bcrypt. A password is looked up by login first
//    and verified afterwards, so a slow salted hash works there. A key is
//    looked up BY ITS HASH ALONE, so the hash must be deterministic. That is
//    safe because the key has 128 bits of entropy and cannot be guessed.
export const apiKey = pgTable(
	'api_key',
	{
		id: uuid('id').primaryKey().defaultRandom(),

		// A name given by a person, e.g. "production cluster".
		name: text('name').notNull(),

		// The tenant the data arriving with this key belongs to. It becomes the
		// first ORDER BY column in ClickHouse, so one tenant's queries skip the
		// others' data granule by granule.
		//
		// No default: a key is always created for a named tenant. Restrict, because
		// deleting a tenant would leave its ClickHouse rows without an owner.
		tenantId: text('tenant_id')
			.notNull()
			.references(() => organization.id, { onDelete: 'restrict' }),

		// The key's SHA-256, in hex. What the intake looks up.
		keyHash: text('key_hash').notNull().unique(),

		// The key's first 8 characters, so it can be recognised in the list.
		prefix: text('prefix').notNull(),

		createdAt: timestamp('created_at', { withTimezone: true }).defaultNow().notNull(),

		// For finding the keys nobody uses any more. Nothing writes it yet: the
		// intake does not record a key's use.
		lastUsedAt: timestamp('last_used_at', { withTimezone: true }),

		createdById: text('created_by_id').references(() => user.id, { onDelete: 'set null' })
	},
	(table) => [
		// The intake looks a key up by its hash on every agent request.
		index('api_key_hash_idx').on(table.keyHash)
	]
);

export * from './auth.schema';
