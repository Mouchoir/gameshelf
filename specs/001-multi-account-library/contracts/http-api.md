# Contract: Web Routes, Stable HTTP Endpoints and CSV Format

**Applies to**: the `server` and `app` crates. **Related**: research D2, D4, D8; spec FR-011 to FR-016,
FR-039 to FR-045.

## 1. Pages (Leptos routes)

| Route | Purpose | Story |
|---|---|---|
| `/setup` | First run only: create the first admin profile, check the secret key | - |
| `/login`, `/logout` | Sign in and out | - |
| `/` and `/library` | Grid or list of games with search, sort and filters | US1, US2 |
| `/games/{game_id}` | Game detail: ownerships by platform and account, playtime, achievements, enrichment, DLC | US4 |
| `/accounts` | Connected accounts with health, last and next sync, "Sync now" | US3 |
| `/accounts/connect/{platform}` | Guided connection for one platform | US1, US3 |
| `/agents` | Paired agents, pairing codes, sources, schedules, revoke | US5 |
| `/sharing` | Who this profile shares with, and exclusions | US6 |
| `/notifications` | Targets, events, "Send test" | US7 |
| `/activity` | Activity feed | US3 |
| `/settings` | Profile preferences, language, export, CSV import | US9 |
| `/admin` | Profiles, instance schedule, metadata provider keys, trusted proxy | - |

Server functions back these pages under `/api/fn/*`. They are internal and may change between releases.

A direct link to a game the viewer cannot see returns the same "not found" page as a game that does not exist
(SC-011).

## 2. Stable endpoints

| Method and path | Auth | Response |
|---|---|---|
| `GET /healthz` | none | `200 ok` when the process runs |
| `GET /readyz` | none | `200` when the database is reachable and migrations applied, else `503` |
| `GET /api/v1/export.csv` | session | the viewer's own library as CSV (section 4) |
| `GET /api/v1/export.json` | session | the same data as JSON, with `schema_version` |
| `POST /api/v1/import/csv` | session | `200` with `{ imported, updated, unmatched: [rows] }` |
| `/api/agent/v1/*` | agent token | see [agent-api.md](./agent-api.md) |
| `GET /assets/{sha256}` | session | cached artwork, `Cache-Control: public, max-age=31536000, immutable` |
| `GET /metrics` | none, bound to a separate address | only when metrics are enabled |

Exports contain only the viewer's own accounts, never shared ownerships of other profiles, never credentials or
install paths (FR-044).

## 3. Authentication and request safety

- Sessions: cookie `gameshelf_session`, `HttpOnly`, `SameSite=Lax`, `Secure` on requests addressed to the HTTPS
  public base URL (not on LAN addresses, where the server is reached over HTTP);
  idle timeout 30 days, rotated at sign-in and on privilege change.
- State-changing requests require an `Origin` or `Referer` header matching the configured public base URL or a
  local address; others are rejected with `403`.
- Sign-in is rate limited per source address and per username (10 attempts per 15 minutes).
- Trusted proxy mode (opt-in): when the request comes from an address in `GAMESHELF_TRUSTED_PROXIES` and carries
  the configured identity header, the profile with the matching username or email is signed in. Unknown identities
  are refused, not created.

## 4. CSV format (import and export)

UTF-8, comma separated, header row required, RFC 4180 quoting.

| Column | Required on import | Notes |
|---|---|---|
| `title` | yes | |
| `platform` | yes | registry key (`switch`, `steam`, `manual`...) |
| `account` | yes | account label; created as a manual account if no account with that label exists on that platform |
| `acquisition` | no | `purchase` (default), `free_claim`, `subscription`, `free_to_play`, `manual` |
| `edition` | no | |
| `playtime_minutes` | no | integer |
| `last_played_at` | no | RFC 3339 or `YYYY-MM-DD` |
| `note` | no | |
| `platform_game_id` | export only | |
| `game_id` | export only | gameshelf id, accepted on import to force a match |

Re-importing the same file creates no duplicates: a row is identified by (`platform`, `account`,
`platform_game_id` or normalized `title`). Rows that match no known game are created as manual games and listed
in `unmatched` for review.
