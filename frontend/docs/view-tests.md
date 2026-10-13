# View tests

`tests/e2e/` is a Playwright suite that opens every page of the panel, and the states that
only show after a click (menus, dialogs, refused forms), and fails when something overflows
where it should not. It also checks that one tenant's member sees nothing of another tenant.

```bash
bun run db:start                    # or the whole dev stack: docker compose up
bunx playwright install chromium    # once
bun run test:e2e                    # every page, at every width
bun run test:e2e --project=phone-light views.spec.ts   # one width, one file
bunx playwright show-report         # the last run, with traces of what failed
```

## What it needs

- **Postgres** from `DATABASE_URL` in `.env`. The seed (`tests/e2e/seed.setup.ts`) adds two
  tenants, six people and four keys to it. Every row it writes is prefixed `e2e-`, and it
  touches no other row: it inserts what is missing and brings its own rows back in line with
  `tests/e2e/people.ts`, so it is safe against a development database. It deletes nothing;
  to remove its rows, delete the `e2e-` ones by hand.
- **The query process** on `NINJACAT_INTERNAL_URL`, which the metrics and logs pages read
  from; without it they do not show what they would. The e2e tenants have no telemetry, so
  those pages are tested empty: charts, facets and log rows are not covered yet.
- **A panel on :5174.** Playwright starts `bun run dev --port 5174` itself, with
  `ORIGIN=http://localhost:5174`, or reuses one already running there (started with that
  `ORIGIN`). It is not :5173, so the suite can run beside a panel you are working in.

## The widths

Each page runs at four widths: `phone` (360), `tablet` (768), `desktop` (1280) and `wide`
(1920). Phone and tablet are touch screens, where a `title` cannot be read, so text cut with an
ellipsis passes there only inside a control that opens a popup. The isolation tests run at
desktop width only. Dark mode is not tested, since nothing in the panel turns it on; add
`'dark'` to `schemes` in `playwright.config.ts` once something does.

## What counts as overflow

`tests/e2e/overflow.ts` measures the page in the browser. Its rules are at the top of the
file: the page scrolling sideways, content spilling out of its box or cut off by it, text cut
with an ellipsis that cannot be read whole, a table that hides columns behind a sideways
scroll, text squeezed to a word a line, text touching a border, and an item off the centre of
its row. What is allowed to stick out is listed there too, each with its reason.

A test that passes only on a retry fails the run (`failOnFlakyTests`), so an overflow that
comes and goes is not let through.

## Adding a page

Add it to `tests/e2e/routes.ts`: a name, a path, and the person who sees the most of it. A
long name, tag or query in the path is worth more than a short one. Screenshots of every page
at every width land in `test-results/shots/<name>/` (`E2E_SHOTS_DIR` to put them elsewhere);
they are for looking through, nothing compares them.
