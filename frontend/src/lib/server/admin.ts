import { error } from '@sveltejs/kit';
import { isAPIError } from 'better-auth/api';
import { and, DrizzleQueryError, eq } from 'drizzle-orm';
import postgres from 'postgres';
import type { TenantRole } from '$lib/tenant';
import { isInstanceAdmin, type Database } from './auth-options';
import { member } from './db/schema';

// The guard keeps everyone but the instance admin off /app/admin, by route.
// Every admin load and action asks again, so that one moved out from under
// that route does not quietly open to every user.
export function requireInstanceAdmin(locals: App.Locals) {
	const user = locals.user;
	if (!user || !isInstanceAdmin(user.role)) {
		error(403, 'forbidden');
	}
	return user;
}

export function formText(form: FormData, name: string): string {
	return form.get(name)?.toString().trim() ?? '';
}

// Better Auth reports what it refused as a code in the error's body.
export function authErrorCode(e: unknown): string | undefined {
	return isAPIError(e) ? e.body?.code : undefined;
}

// Better Auth checks a slug or a membership is free and then inserts; two
// requests at once both pass the check, and the unique index refuses the
// second.
export function isUniqueViolation(e: unknown): boolean {
	return (
		e instanceof DrizzleQueryError &&
		e.cause instanceof postgres.PostgresError &&
		e.cause.code === '23505'
	);
}

// The plugin keeps several roles in one comma-separated column.
const isOwner = (role: string) => role.split(',').includes('owner');

// Better Auth refuses to let the last owner go on its own routes; the admin
// pages keep the same rule. A tenant that has no owner already is not held to
// it, or none of its members could be removed.
export function leavesNoOwner(
	members: { id: string; role: string }[],
	memberId: string,
	newRole: string | null
): boolean {
	const target = members.find((m) => m.id === memberId);
	if (!target || !isOwner(target.role) || (newRole !== null && isOwner(newRole))) {
		return false;
	}
	return !members.some((m) => m.id !== memberId && isOwner(m.role));
}

// A new role for a member of the tenant, or null to remove them. Through
// Drizzle: the plugin's own calls for this act only for a member of the
// tenant, and the instance admin need not be one.
export function changeMember(
	db: Database,
	tenantId: string,
	memberId: string,
	role: TenantRole | null
): Promise<'done' | 'not-found' | 'last-owner'> {
	return db.transaction(async (tx) => {
		// Locked, so two admins demoting the last two owners at once cannot
		// both pass the check.
		const members = await tx
			.select({ id: member.id, role: member.role })
			.from(member)
			.where(eq(member.organizationId, tenantId))
			.for('update');
		if (!members.some((m) => m.id === memberId)) {
			return 'not-found';
		}
		if (leavesNoOwner(members, memberId, role)) {
			return 'last-owner';
		}

		const target = and(eq(member.id, memberId), eq(member.organizationId, tenantId));
		if (role === null) {
			await tx.delete(member).where(target);
		} else {
			await tx.update(member).set({ role }).where(target);
		}
		return 'done';
	});
}
