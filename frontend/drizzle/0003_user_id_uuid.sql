-- Datadog's API addresses a user by UUID (/api/v2/users/{user_id}), and Better
-- Auth's ids were 32 random characters. Every user whose id is not a UUID yet
-- gets a new v4 one, and every column holding it follows. The foreign keys
-- cannot follow on their own (ON UPDATE no action), so they are dropped and
-- added back as 0000 and 0002 wrote them.
--
-- Left with the old id: monitor_transitions.actor in ClickHouse (history, not
-- rewritten), and a user that a pod of the previous release creates while this
-- one rolls out, since a migration never runs twice.
--
-- The Job retries, so it waits briefly for the locks rather than queueing every
-- sign-in behind it.
SET LOCAL lock_timeout = '10s';--> statement-breakpoint
CREATE TEMP TABLE "user_id_map" ON COMMIT DROP AS
	SELECT "id" AS "old_id", gen_random_uuid()::text AS "new_id"
	FROM "user"
	WHERE "id" !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$';--> statement-breakpoint
ALTER TABLE "api_key" DROP CONSTRAINT "api_key_created_by_id_user_id_fk";--> statement-breakpoint
ALTER TABLE "account" DROP CONSTRAINT "account_user_id_user_id_fk";--> statement-breakpoint
ALTER TABLE "invitation" DROP CONSTRAINT "invitation_inviter_id_user_id_fk";--> statement-breakpoint
ALTER TABLE "member" DROP CONSTRAINT "member_user_id_user_id_fk";--> statement-breakpoint
ALTER TABLE "session" DROP CONSTRAINT "session_user_id_user_id_fk";--> statement-breakpoint
ALTER TABLE "monitor" DROP CONSTRAINT "monitor_created_by_id_user_id_fk";--> statement-breakpoint
ALTER TABLE "monitor" DROP CONSTRAINT "monitor_updated_by_id_user_id_fk";--> statement-breakpoint
ALTER TABLE "monitor" DROP CONSTRAINT "monitor_deleted_by_id_user_id_fk";--> statement-breakpoint
UPDATE "user" SET "id" = m."new_id" FROM "user_id_map" m WHERE "user"."id" = m."old_id";--> statement-breakpoint
UPDATE "api_key" SET "created_by_id" = m."new_id" FROM "user_id_map" m WHERE "api_key"."created_by_id" = m."old_id";--> statement-breakpoint
-- A credential account's account_id is its user's id.
UPDATE "account" SET
	"user_id" = m."new_id",
	"account_id" = CASE
		WHEN "account"."provider_id" = 'credential' AND "account"."account_id" = m."old_id" THEN m."new_id"
		ELSE "account"."account_id"
	END
FROM "user_id_map" m WHERE "account"."user_id" = m."old_id";--> statement-breakpoint
UPDATE "invitation" SET "inviter_id" = m."new_id" FROM "user_id_map" m WHERE "invitation"."inviter_id" = m."old_id";--> statement-breakpoint
UPDATE "member" SET "user_id" = m."new_id" FROM "user_id_map" m WHERE "member"."user_id" = m."old_id";--> statement-breakpoint
UPDATE "session" SET "user_id" = m."new_id" FROM "user_id_map" m WHERE "session"."user_id" = m."old_id";--> statement-breakpoint
UPDATE "session" SET "impersonated_by" = m."new_id" FROM "user_id_map" m WHERE "session"."impersonated_by" = m."old_id";--> statement-breakpoint
UPDATE "monitor" SET "created_by_id" = m."new_id" FROM "user_id_map" m WHERE "monitor"."created_by_id" = m."old_id";--> statement-breakpoint
UPDATE "monitor" SET "updated_by_id" = m."new_id" FROM "user_id_map" m WHERE "monitor"."updated_by_id" = m."old_id";--> statement-breakpoint
UPDATE "monitor" SET "deleted_by_id" = m."new_id" FROM "user_id_map" m WHERE "monitor"."deleted_by_id" = m."old_id";--> statement-breakpoint
ALTER TABLE "api_key" ADD CONSTRAINT "api_key_created_by_id_user_id_fk" FOREIGN KEY ("created_by_id") REFERENCES "public"."user"("id") ON DELETE set null ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "account" ADD CONSTRAINT "account_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."user"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "invitation" ADD CONSTRAINT "invitation_inviter_id_user_id_fk" FOREIGN KEY ("inviter_id") REFERENCES "public"."user"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "member" ADD CONSTRAINT "member_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."user"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "session" ADD CONSTRAINT "session_user_id_user_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."user"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "monitor" ADD CONSTRAINT "monitor_created_by_id_user_id_fk" FOREIGN KEY ("created_by_id") REFERENCES "public"."user"("id") ON DELETE set null ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "monitor" ADD CONSTRAINT "monitor_updated_by_id_user_id_fk" FOREIGN KEY ("updated_by_id") REFERENCES "public"."user"("id") ON DELETE set null ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "monitor" ADD CONSTRAINT "monitor_deleted_by_id_user_id_fk" FOREIGN KEY ("deleted_by_id") REFERENCES "public"."user"("id") ON DELETE set null ON UPDATE no action;
