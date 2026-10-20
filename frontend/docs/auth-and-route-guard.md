# Sessions, tenants and the route guard

How a request to the panel learns who sent it and which tenant it is for, and what stands
in front of each route.

## Who is asking

The server remembers nothing between requests. A request is anonymous until the browser
sends Better Auth's session cookie: `better-auth.session_token`, with a `__Secure-` prefix
when `ORIGIN` is `https://`, `httpOnly`, `SameSite=Lax`, seven days by default. The cookie
holds a token and nothing else; who it belongs to and until when is the `session` row in
Postgres. Signing out deletes that row, so a copied cookie stops working too.

`src/hooks.server.ts` runs three handles in order:

```
paraglide      the locale, from the URL
better auth    the session → locals.user, locals.session
               the memberships → locals.tenants, locals.tenant
               /api/auth/* is answered here and goes no further
guard          src/lib/server/guard.ts
```

Better Auth's own routes (sign-in, sign-out, `organization/set-active`, …) are answered by
its handler inside the second handle, so they never reach the guard. They check what they
need themselves.

## Which tenant

A tenant is a Better Auth organization; its `organization.id` is the `tenant_id` every
ClickHouse row carries. A user can be a member of several, with one of the plugin's roles
(owner, admin, member), and the session's `activeOrganizationId` names the one in use.

The hook does not trust that field alone. `resolveTenant` (`src/lib/server/tenant.ts`) reads
the user's memberships on every request, oldest first, and picks:

1. the session's active organization, while the user is still a member of it;
2. otherwise the first membership;
3. otherwise none — `locals.tenant` is `null`. Nothing falls back to `default`.

Removing a member does not clear the active organization of their other sessions, which is
why the membership is read each time. When the request lands on a different tenant than the
session names (2 or 3), the session is moved there with `auth.api.setActiveOrganization`, so
Better Auth's own calls that default to the active organization act on the tenant the panel
shows. A new session starts in the first membership (`databaseHooks.session.create.before`
in `src/lib/server/auth-options.ts`, shared by the panel's `auth.ts` and
`scripts/create-user.ts`).

The tenant switcher in the sidebar calls `organization.setActive` and then `invalidateAll()`,
so every load runs again for the new tenant.

## The guard

Everything is private unless its route is in `publicRoutes` in `src/lib/server/guard.ts`;
only `/login` is. For the rest:

| Who                           | Admin pages (`/app/admin/**`) | `/no-tenant` | `/app/settings/account` | Anything else  |
| ----------------------------- | ----------------------------- | ------------ | ----------------------- | -------------- |
| no session                    | → `/login`                    | → `/login`   | → `/login`              | → `/login`     |
| instance admin, with a tenant | yes                           | → `/app`     | yes                     | yes            |
| instance admin, no tenant     | yes                           | yes          | yes                     | → `/no-tenant` |
| anyone else, with a tenant    | 403                           | → `/app`     | yes                     | yes            |
| anyone else, no tenant        | 403                           | yes          | yes                     | → `/no-tenant` |

The account page is the user's own, not a tenant's, so its password can be changed without
one. The instance admin is a user whose `user.role` includes `admin` (`isInstanceAdmin`). The
redirect from `/no-tenant` to `/app` is that page's own load; the rest is the guard.

"→" is a redirect where the request can follow one — a page load (`GET` that accepts
HTML), a data request, an enhanced form action (`x-sveltekit-action: true`, answered as a
redirect result) — and otherwise an error: 401 without a session, 403 without a tenant. A
`+server.ts` endpoint or a plain form post without JavaScript gets the error.

### A layout's load is not a guard

It protects nothing:

- a data request (`__data.json?x-sveltekit-invalidated=…`) names the loads it wants run,
  so the client can skip the layout's;
- a form action runs before any load;
- a `+server.ts` endpoint never runs a layout at all.

The layout still checks `locals.user`, but only to narrow its type for TypeScript; the
guard has already answered by then. Form actions do the same, and every action on a tenant's
data also checks the tenant itself: the keys page's forms carry the tenant they were shown
for, and an action answers 409 if the session has moved to another tenant in the meantime,
because a key made for the wrong tenant sends an agent's data there.

## Signing in and out

The login form posts to Better Auth's endpoint (`/api/auth/sign-in/email`) through
`src/lib/auth-client.ts`, not through a form action: its rate limiter (five attempts a
minute per client address) and its cookies work only on its own path. Sign-out is
`authClient.signOut()`, then a navigation to `/login` that reloads every load. Sign-up is
closed (`disableSignUp`); accounts come from `scripts/create-user.ts` and the admin pages.

## The query port

Each call to the query port (`src/lib/server/ninjacat.ts`) carries a 60-second token minted
from `locals.user` and `locals.tenant`; without a tenant nothing is sent.

Tests: `src/lib/server/guard.spec.ts`, `tenant.spec.ts`, `ninjacat.spec.ts`,
`src/routes/app/settings/keys/keys.spec.ts`, `src/routes/app/admin/admin.spec.ts`.

## Worth knowing before scaling out

- **Session cookie cache** is off: every request reads the session from Postgres. Turning
  on `session.cookieCache` saves that query at the price of a sign-out or revoked session
  lingering for its `maxAge`. The membership is read on every request either way.
- **More than one panel pod.** Nothing in the panel's own memory has to be shared, so it
  scales without sticky sessions, but `BETTER_AUTH_SECRET` and
  `NINJACAT_QUERY_TOKEN_SECRET` must be the same on every pod, and the sign-in rate limiter
  counts in the process's memory: with several pods, set `rateLimit.storage` to
  `"database"` (it needs a `rateLimit` table).
