# Production image for the panel.
#
# Three stages. The toolchain is needed to compile and to nothing else.
#
#   deps       every dependency, dev included
#   builder    compiles the app -> build/, and the migrator -> migrate.js
#   prod-deps  runtime dependencies only
#   runner     serves traffic AND applies migrations; carries no toolchain
#
# The runner has two entrypoints on purpose:
#
#   node build             serve
#   node migrate.js        apply migrations, then exit
#   node create-user.js    create an account, then exit
#
# One image doing both is what makes a Kubernetes pre-upgrade hook (or an
# init container) possible without shipping drizzle-kit to production.

# ---------------------------------------------------------------------------
FROM oven/bun:1 AS deps

WORKDIR /app

# Debian-based rather than Alpine on purpose: the pinned lightningcss binary
# is the glibc build (lightningcss-linux-x64-gnu).
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile

# ---------------------------------------------------------------------------
FROM deps AS builder

WORKDIR /app
COPY . .

# SvelteKit evaluates the server bundle while building, so
# src/lib/server/auth.ts and db/index.ts actually run — and both throw when
# their configuration is missing. Hence the placeholders.
#
# They are NOT baked into the output: the app reads $env/dynamic/private,
# resolved from the real environment at startup. Nothing here reaches the
# running container.
RUN DATABASE_URL="postgres://build:build@localhost:5432/build" \
    BETTER_AUTH_SECRET="build-time-placeholder-not-used-at-runtime" \
    bun run build

# The migration entrypoint, bundled to a single dependency-free file.
# --target=node leaves Node's built-ins external and inlines everything else,
# so the runner needs neither drizzle-orm nor postgres in node_modules.
RUN bun build scripts/migrate.ts --target=node --outfile=migrate.js

# The account-creation entrypoint, bundled the same way.
#
# Registration is closed (src/lib/server/auth.ts), so without this a fresh
# deployment has no way to make its first account: the runtime image carries
# node and nothing else — no bun, no TypeScript, no scripts directory. That
# was a real gap, found by trying to log into a freshly deployed lab.
RUN bun build scripts/create-user.ts --target=node --outfile=create-user.js

# ---------------------------------------------------------------------------
# Resolved separately from `deps` so it can run in parallel with the build.
FROM oven/bun:1 AS prod-deps

WORKDIR /app
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile --production

# ---------------------------------------------------------------------------
FROM node:24-slim AS runner

WORKDIR /app
ENV NODE_ENV=production
ENV PORT=3000

COPY --from=builder   /app/build        ./build
COPY --from=prod-deps /app/node_modules ./node_modules
COPY --from=builder   /app/package.json ./

# The migrator and the SQL it applies. Small enough that carrying it in the
# serving image costs nothing and saves maintaining a second one.
COPY --from=builder   /app/migrate.js     ./migrate.js
COPY --from=builder   /app/create-user.js ./create-user.js
COPY --from=builder   /app/drizzle      ./drizzle

# The node image ships a non-root user for exactly this.
USER node

EXPOSE 3000

# ORIGIN must be set at runtime — SvelteKit rejects form actions without it,
# and better-auth uses it as its base URL.
CMD ["node", "build"]
