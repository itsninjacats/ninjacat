import { sql } from 'drizzle-orm';
import {
	pgTable,
	text,
	uuid,
	timestamp,
	index,
	bigint,
	integer,
	smallint,
	jsonb,
	check
} from 'drizzle-orm/pg-core';
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

// A monitor's definition, in Datadog's shape (name, type, query, message,
// options, tags, priority), so a Datadog-compatible /api/v1/monitor and
// Terraform stay cheap later. The monitor service reads this table and never
// writes it; every write goes through src/lib/server/monitors.ts, which tells
// the service in the same transaction.
export const monitor = pgTable(
	'monitor',
	{
		// An integer, as Datadog's monitor ids are.
		id: bigint('id', { mode: 'number' }).primaryKey().generatedAlwaysAsIdentity(),

		// No default and restrict, as for api_key: a monitor belongs to a named
		// tenant, and its transitions in ClickHouse would lose their owner.
		tenantId: text('tenant_id')
			.notNull()
			.references(() => organization.id, { onDelete: 'restrict' }),

		name: text('name').notNull(),
		// "query alert" ("metric alert" accepted) or "service check".
		type: text('type').notNull(),
		query: text('query').notNull(),
		message: text('message').notNull().default(''),
		// Datadog's options object, verbatim.
		options: jsonb('options').notNull().default({}),
		tags: text('tags')
			.array()
			.notNull()
			.default(sql`'{}'::text[]`),
		priority: smallint('priority'),
		// The panel's own view of the query, so the editor reopens what it saved.
		editor: jsonb('editor'),

		// Bumped on every write; the service tells a stale notification by it.
		version: integer('version').notNull().default(1),

		// The actor of the transitions a change causes (definition_changed, deleted).
		createdById: text('created_by_id').references(() => user.id, { onDelete: 'set null' }),
		updatedById: text('updated_by_id').references(() => user.id, { onDelete: 'set null' }),
		deletedById: text('deleted_by_id').references(() => user.id, { onDelete: 'set null' }),

		createdAt: timestamp('created_at', { withTimezone: true }).defaultNow().notNull(),
		updatedAt: timestamp('updated_at', { withTimezone: true }).defaultNow().notNull(),
		// Soft delete: the service closes the monitor's groups when it sees this.
		deletedAt: timestamp('deleted_at', { withTimezone: true })
	},
	(table) => [
		index('monitor_tenant_idx').on(table.tenantId),
		check('monitor_priority_check', sql`${table.priority} between 1 and 5`)
	]
);

export * from './auth.schema';
