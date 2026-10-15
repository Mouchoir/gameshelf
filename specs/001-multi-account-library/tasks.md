---

description: "Task list for the multi-account game library"
---

# Tasks: Multi-Account Game Library

**Input**: Design documents from `/specs/001-multi-account-library/`

**Prerequisites**: [plan.md](./plan.md), [spec.md](./spec.md), [research.md](./research.md),
[data-model.md](./data-model.md), [contracts/](./contracts/), [quickstart.md](./quickstart.md)

**Tests**: Required. The constitution mandates connector contract tests, an end-to-end sync test with a fake
connector, and tests for new behavior; SC-011 requires automated leak tests.

**Organization**: Tasks are grouped by user story. Platform-expansion phases carry the story they extend
(`[US1]` for new library sources, `[US8]` for subscriptions).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story the task serves (US1 to US9)

## Path Conventions

Cargo workspace at the repository root; crates under `crates/`; end-to-end tests under `e2e/`; packaging under
`docker/` and `.github/workflows/` (plan.md, Project Structure).

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: workspace, toolchain and quality gates

- [X] T001 Create the workspace manifest `Cargo.toml` with members `crates/*`, edition 2024, `rust-version = "1.85"`, shared `[workspace.dependencies]` (versions from research.md section 5) and `[workspace.lints]` (`unsafe_code = "forbid"`, clippy `all` warn)
- [X] T002 [P] Add `rust-toolchain.toml` (stable, components rustfmt and clippy, target wasm32-unknown-unknown), `rustfmt.toml` and `.gitattributes` (`* text=auto eol=lf`)
- [X] T003 [P] Create empty library crates with `lib.rs` and `Cargo.toml`: `crates/core`, `crates/store`, `crates/connectors`, `crates/metadata`, `crates/notify`, `crates/sync`
- [X] T004 Create the Leptos crates `crates/app` (lib, features `ssr` and `hydrate`), `crates/frontend` (cdylib, hydrate entry) and `crates/server` (bin `gameshelf`) and the `[[workspace.metadata.leptos]]` block in `Cargo.toml` (site root `target/site`, style file `crates/app/style/main.css`, assets `crates/app/public`)
- [X] T005 [P] Add `deny.toml` allowing only AGPL-3.0-compatible licenses (MIT, Apache-2.0, BSD-2/3, ISC, Zlib, MPL-2.0, Unicode-3.0, GPL-3.0, AGPL-3.0, CC0-1.0) and denying known advisories
- [X] T006 [P] Add `.github/workflows/ci.yml`: fmt check, clippy `-D warnings`, nextest, cargo deny, gitleaks secret scan, locale key parity check, on push and pull request
- [X] T007 [P] Add `docker/Dockerfile` (cargo-chef planner and builder stages, cargo-leptos build, distroless runtime with `/data` volume and non-root user) and `docker/compose.example.yml` copied from contracts/configuration.md with placeholders only
- [X] T008 [P] Add `README.md` (what it is, status, quick start pointing to quickstart.md, license) with no personal data

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: configuration, storage, secrets, authentication, domain core, connector contract, UI shell

**CRITICAL**: no user story work starts before this phase is complete

### Core domain

- [X] T009 [P] Define ids (UUIDv7 newtypes), `PlatformKey`, `Acquisition` (`purchase`, `free_claim`, `subscription`, `free_to_play`, `family_shared`, `manual`), `ProductKind` (`game`, `dlc`, `bundle`, `tool`, `unknown`) and `AccountStatus` (`pending`, `active`, `needs_reconnect`, `degraded`, `disabled`) in `crates/core/src/model.rs`
- [X] T010 [P] Implement the platform registry (key, display name key, tier, capabilities, icon id) for every platform of spec "Platform coverage" plus `manual` in `crates/core/src/platform.rs`
- [X] T011 [P] Implement title normalization (deunicode, lowercase, punctuation removal, leading articles "the", "a", "an", "le", "la", "les", "l'" stripped for `sort_title`) with unit tests in `crates/core/src/normalize.rs`
- [X] T012 Implement the account status state machine of data-model.md (transitions and `degraded` after 3 failed runs) with unit tests in `crates/core/src/account_status.rs`

### Configuration and secrets

- [X] T013 Implement configuration loading with figment (environment variables and optional TOML from `GAMESHELF_CONFIG`) exactly as listed in contracts/configuration.md, refusing to start without a valid 32-byte secret key and printing how to generate one, in `crates/server/src/config.rs`
- [X] T014 [P] Implement sealing and unsealing with XChaCha20-Poly1305 (24-byte nonce, `key_id` stored with ciphertext, `secrecy` wrappers, no `Debug`/`Display` of plaintext) with round-trip and wrong-key tests in `crates/store/src/seal.rs`

### Storage

- [X] T015 Write the initial migration `crates/store/migrations/0001_init.sql` creating every table of data-model.md with its constraints (unique `(platform, external_account_id)` on accounts, unique `(account_id, product_id)` on ownerships, unique `(platform, platform_game_id)` on products, unique `(source, external_id)` on game external ids, foreign keys with cascade deletes as specified)
- [X] T016 Implement the database layer (sqlx SQLite pool, WAL, foreign keys on, busy timeout, embedded migrations at start-up) in `crates/store/src/db.rs`
- [X] T017 [P] Implement profile repository (create with Argon2id hash, `username` unique 3-32 chars `[a-z0-9._-]`, `display_name` 1-64 chars, at least one active admin must remain) in `crates/store/src/repo/profiles.rs`
- [X] T018 [P] Implement preference repository (`profile_id`, `key`, JSON value) in `crates/store/src/repo/prefs.rs`
- [X] T019 [P] Implement account and credential repositories (credential payload only accessible through `seal.rs`, never serialized to the web layer) in `crates/store/src/repo/accounts.rs` and `crates/store/src/repo/credentials.rs`
- [X] T020 [P] Implement catalog repositories for games, game external ids, platform products and assets in `crates/store/src/repo/catalog.rs`
- [X] T021 [P] Implement ownership repository with the snapshot diff apply of contracts/connector.md section 5 (single transaction, `unavailable_since` set only for complete listings, cleared on reappearance) and tests in `crates/store/src/repo/ownership.rs`
- [X] T022 [P] Implement sync run, job and activity event repositories (run retention 90 days) in `crates/store/src/repo/runs.rs` and `crates/store/src/repo/activity.rs`
- [X] T023 Implement the custom tower-sessions store on sqlx 0.9 (create, save, load, delete, hourly purge of expired rows) in `crates/store/src/session_store.rs`

### Visibility choke point

- [X] T024 Implement `visible_ownerships(viewer)` as the only store entry point that returns ownerships to the web layer: own accounts now, sharing rules and exclusions added in US6; expose `count`, `facets`, `page` and `by_game` variants that aggregate after filtering, in `crates/store/src/repo/visibility.rs`

### Connector contract

- [X] T025 [P] Define `ConnectorDescriptor`, `AuthMethod`, `LoginStart`, `CodeCapture`, `Tier`, `Capabilities` and `RateLimit` in `crates/connectors/src/descriptor.rs` per contracts/connector.md section 1
- [X] T026 [P] Define `LibrarySnapshot`, `LibraryItem`, `PlaytimeItem`, `AchievementItem`, `EnrichmentItem`, `AccountIdentity` and `ConnectorError` (with redaction rules) in `crates/connectors/src/model.rs` and `crates/connectors/src/error.rs`
- [X] T027 Define the `Connector` trait and the `Context` (shared reqwest client with rustls, per-platform governor limiter, backon retry with jitter for `Transient` and `RateLimited`, cancellation, redacting logger) in `crates/connectors/src/lib.rs` and `crates/connectors/src/http.rs`
- [X] T028 [P] Implement the development-only fake connector serving synthetic libraries (enabled by `GAMESHELF_DEV_FAKE_CONNECTOR`, compiled only with the `dev-fake` feature) in `crates/connectors/src/fake.rs`
- [X] T029 [P] Add the fixture hygiene test that fails on fixtures containing real-looking identifiers or secrets (SteamID64 in the real range, emails, JWT-like tokens, Windows user paths) in `crates/connectors/tests/fixture_hygiene.rs`

### Server and authentication

- [X] T030 Implement application state, Axum router, tracing setup (`GAMESHELF_LOG`, `GAMESHELF_LOG_FORMAT`), `GET /healthz` and `GET /readyz` in `crates/server/src/main.rs`, `crates/server/src/state.rs` and `crates/server/src/api/health.rs`
- [X] T031 Implement password sign-in, sign-out and session rotation (cookie `gameshelf_session`, `HttpOnly`, `SameSite=Lax`, `Secure` when `GAMESHELF_BASE_URL` is HTTPS, 30-day idle timeout) in `crates/server/src/auth/session.rs` and `crates/server/src/auth/password.rs`
- [X] T032 [P] Implement sign-in rate limiting (10 attempts per 15 minutes per address and per username) in `crates/server/src/auth/rate_limit.rs`
- [X] T033 [P] Implement the origin check for state-changing requests (Origin or Referer must match the base URL or a local address, else 403) in `crates/server/src/auth/origin.rs`
- [X] T034 Implement the first-run `/setup` flow (only while no profile exists: create the first admin) in `crates/app/src/routes/setup.rs` and its server function

### UI shell and localization

- [X] T035 Set up leptos_i18n with `crates/app/locales/en.json` (reference) and `crates/app/locales/fr.json`, locale from profile setting, browser language or manual override (FR-045), and the CI key-parity check script in `crates/app/build.rs` or a test
- [X] T036 Build the app shell (layout, navigation, sign-in page, light and dark theme tokens, focus styles, skip link) in `crates/app/src/app.rs`, `crates/app/src/routes/login.rs` and `crates/app/style/main.css`
- [X] T037 [P] Add platform logo assets from licensed sources (Simple Icons CC0 where present, Font Awesome Free brands otherwise, neutral generic glyph when neither exists) with a `crates/app/public/icons/NOTICE.md` attribution file

**Checkpoint**: an instance starts, the first admin signs in, migrations run, CI is green.

---

## Phase 3: User Story 1 - Know whether I own a game, and on which accounts (Priority: P1) MVP

**Goal**: connect several Steam accounts, import them, and answer "do I own it, on which accounts?" through
search and a game detail page.

**Independent Test**: quickstart V1 and V2.

### Tests for User Story 1

- [X] T038 [P] [US1] Record and anonymize Steam fixtures (owned games public and private, family shared apps, player summary, error and rate-limit responses) in `crates/connectors/tests/fixtures/steam/`
- [X] T039 [P] [US1] Write Steam connector contract tests (full listing, private profile with own key, family-shared items marked `family_shared`, expired key, `429` with retry, unknown fields, empty library) in `crates/connectors/tests/steam.rs`
- [X] T040 [P] [US1] Write matching unit tests (same appid on two accounts gives one game with two ownerships; Steam and Epic copies with an IGDB cross id give one game; editions grouped; remasters separate; DLC attached to parent) in `crates/core/tests/matching.rs`
- [X] T041 [P] [US1] Write search unit tests (partial, accent-insensitive, typo-tolerant, explicit no-match) in `crates/core/tests/search.rs`
- [ ] T042 [P] [US1] Write Playwright journeys V1 and V2 against the fake connector in `e2e/tests/us1-own-which-account.spec.ts`

### Implementation for User Story 1

- [X] T043 [US1] Implement the Steam connector: `validate` (GetPlayerSummaries, account identity), `library` (GetOwnedGames with `include_appinfo`, `include_played_free_games`, `include_free_sub`, `skip_unvetted_apps=false`, `include_family_licenses`, playtime and `rtime_last_played`), family-shared detection, free-to-play detection, Steam CDN artwork URLs, in `crates/connectors/src/steam/mod.rs` and `crates/connectors/src/steam/api.rs`
- [X] T044 [US1] Implement the matching engine (platform ids, then IGDB external games, then Wikidata, then normalized title plus year plus platform; edition grouping; DLC parenting; match decisions applied last) in `crates/core/src/matching.rs`
- [X] T045 [P] [US1] Implement the IGDB client (Twitch client credentials with token cache, `external_games` batch lookup, covers `t_cover_big_2x`, 4 requests/s) in `crates/metadata/src/igdb.rs`
- [ ] T046 [P] [US1] Implement the Wikidata SPARQL fallback lookup by store id in `crates/metadata/src/wikidata.rs`
- [ ] T047 [P] [US1] Implement the optional SteamGridDB client (lookup by Steam, Origin, Epic, Battle.net and Ubisoft ids, 600x900 grids without nsfw or humor) in `crates/metadata/src/steamgriddb.rs`
- [X] T048 [US1] Implement the artwork cache (download once, sha256 content addressing under `<data>/assets/`, `grid` and `thumb` variants with `image`, pinned choice per game, generated title cover when nothing is found) in `crates/metadata/src/artwork.rs`
- [X] T049 [US1] Implement the import pipeline (connector snapshot, product upsert, ownership diff, matching, metadata and artwork resolution) as a reusable function in `crates/sync/src/apply.rs`
- [X] T050 [US1] Implement the in-memory search index (normalized keys, nucleo-matcher scoring, rebuilt after imports and match decisions, filtered by `visible_ownerships`) in `crates/core/src/search.rs` and its holder in `crates/server/src/state.rs`
- [X] T051 [US1] Implement the platform picker listing every registry platform with its connection method, retrievable data and stability tier (FR-002), and the Steam connection page (API key guidance, per-account key for private profiles, validation before saving, duplicate account refusal naming the holding profile, editable label defaulting to display name) in `crates/app/src/routes/connect.rs`
- [X] T052 [US1] Implement the accounts list page with "Import now", last import result, label editing and disconnect (deletes the credential, optionally the account's ownership data, FR-004) in `crates/app/src/routes/accounts.rs`
- [X] T053 [P] [US1] Build components `GameCard`, `PlatformBadges` and `AccountMarker` (one account vs "N accounts") in `crates/app/src/components/game_card.rs`
- [X] T054 [US1] Build the library page with the cover grid and search-as-you-type (server function debounced at 120 ms, explicit "not in your library" state) in `crates/app/src/routes/library.rs` and `crates/app/src/components/search_box.rs`
- [ ] T055 [US1] Build the game detail page listing every visible ownership by platform and account with acquisition, edition, playtime, last played and "not provided by this platform" placeholders, plus merge and split actions stored as match decisions (FR-009); unknown or invisible ids return the not-found page in `crates/app/src/routes/game.rs`
- [X] T056 [US1] Add `GET /assets/{sha256}` with immutable cache headers in `crates/server/src/api/assets.rs`

**Checkpoint**: V1 and V2 pass with Steam accounts and with the fake connector.

---

## Phase 4: User Story 2 - Browse, sort and filter the whole collection (Priority: P1)

**Goal**: sorting, combined filters, grid and list views, remembered preferences.

**Independent Test**: quickstart V3.

- [ ] T057 [P] [US2] Write Playwright journey V3 in `e2e/tests/us2-browse.spec.ts` and store tests for facet counts after visibility filtering in `crates/store/tests/visibility_facets.rs`
- [ ] T058 [US2] Implement sorting by title (`sort_title`), date added, last played and total playtime, and filters by platform, account, profile, acquisition, installed state and tag, in `crates/store/src/repo/visibility.rs` (query builder) and the library server functions in `crates/app/src/server_fns/library.rs`
- [ ] T059 [US2] Build the filter panel with result count and the grid/list toggle in `crates/app/src/components/filters.rs` and `crates/app/src/routes/library.rs`
- [ ] T060 [US2] Hide subscription-only titles by default with a visible toggle (rendering only; detection comes in US8) in `crates/app/src/components/filters.rs`
- [ ] T061 [US2] Persist and restore view, sort and filters per profile through the preference repository in `crates/app/src/server_fns/prefs.rs`
- [ ] T062 [P] [US2] Implement tags (create, assign, filter) in `crates/store/src/repo/tags.rs` and `crates/app/src/components/tags.rs`

**Checkpoint**: V1 to V3 pass.

---

## Phase 5: User Story 3 - Unattended daily sync with clear health status (Priority: P2)

**Goal**: scheduled syncs, run history, account health, reconnect guidance, activity feed.

**Independent Test**: quickstart V4, V5 and V6.

- [X] T063 [P] [US3] Write the end-to-end sync test with the fake connector (added game appears with activity entry; failing account isolated; incomplete listing removes nothing) in `crates/sync/tests/e2e_sync.rs`
- [ ] T064 [P] [US3] Write Playwright journeys V4 to V6 in `e2e/tests/us3-sync.spec.ts`
- [X] T065 [US3] Implement the job runner (jobs table, `locked_until` leases, global concurrency limit, per-platform semaphore, cancellation on shutdown) in `crates/sync/src/runner.rs`
- [X] T066 [US3] Implement the scheduler (croner, instance default `GAMESHELF_SYNC_SCHEDULE` in `TZ`, per-account override, startup catch-up of missed runs) in `crates/sync/src/scheduler.rs`
- [ ] T067 [US3] Implement credential refresh before each run, persistence of rotated credentials, expiry detection within 7 days and status transitions in `crates/sync/src/health.rs`
- [X] T068 [US3] Record sync runs (scope, trigger, status, counts, redacted error of at most 500 chars, `complete_listing`) and emit activity events (`game_added`, `game_unavailable`, `account_needs_reconnect`, `account_degraded`, `sync_failed`) in `crates/sync/src/apply.rs`
- [X] T069 [US3] Write daily playtime snapshots when values change (FR-018) in `crates/sync/src/playtime.rs`
- [X] T070 [US3] Add "Sync now" for one account, one platform and everything, plus run history and health badges with platform-specific reconnect steps on `crates/app/src/routes/accounts.rs`
- [X] T071 [US3] Build the activity feed page in `crates/app/src/routes/activity.rs`
- [ ] T072 [US3] Show unavailable ownerships hidden by default with their date, and a filter to reveal them, in `crates/app/src/components/filters.rs` and `crates/app/src/routes/game.rs`

**Checkpoint**: V4 to V6 pass; library refreshes daily without action.

---

## Phase 6: User Story 4 - Game detail with per-account data (Priority: P2)

**Goal**: per-account playtime, last played, achievements, DLC and platform-level enrichment in the detail view.

**Independent Test**: quickstart V7.

- [X] T073 [P] [US4] Extend Steam fixtures and contract tests with achievements (`GetPlayerAchievements`, `GetSchemaForGame`) and Steam level (`GetSteamLevel`) in `crates/connectors/tests/steam.rs`
- [X] T074 [US4] Implement Steam `achievements` (only for games with playtime, batched, rate limited) and `enrich` (platform level) in `crates/connectors/src/steam/mod.rs`
- [X] T075 [US4] Implement the enrichment repository (unique `(account_id, game_id, provider, key)`, `refreshed_at`) in `crates/store/src/repo/enrichment.rs`
- [X] T076 [US4] Show per-account playtime, last played, achievements progress, install state, enrichment values with source and refresh date, and the DLC list in `crates/app/src/routes/game.rs`
- [ ] T077 [P] [US4] Write Playwright journey V7 in `e2e/tests/us4-detail.spec.ts`

**Checkpoint**: V7 passes.

---

## Phase 7: Platform expansion A - Epic and GOG (extends US1, plan M4)

**Goal**: two browser-login platforms with the paste-code flow.

- [ ] T078 [P] [US1] Record and anonymize Epic fixtures (token exchange and refresh, library items with cursor paging, catalog bulk items, playtime) in `crates/connectors/tests/fixtures/epic/`
- [ ] T079 [P] [US1] Record and anonymize GOG fixtures (token and refresh, galaxy-library releases, embed games, stats page) in `crates/connectors/tests/fixtures/gog/`
- [ ] T080 [P] [US1] Write Epic and GOG contract tests in `crates/connectors/tests/epic.rs` and `crates/connectors/tests/gog.rs`
- [ ] T081 [US1] Implement the Epic connector (authorization code exchange `eg1`, refresh and persistence of rotated tokens, library items with paging, catalog metadata, playtime, achievements) in `crates/connectors/src/epic/`
- [ ] T082 [US1] Implement the GOG connector (code exchange with the Galaxy client, refresh, owned releases, playtime and achievements where available) in `crates/connectors/src/gog/`
- [ ] T083 [US1] Implement the browser-login connection flow in the web app (login link, paste field for code or redirect URL, PKCE and state kept server-side, validation, reconnect reuse) in `crates/app/src/routes/connect.rs` and `crates/server/src/auth/platform_login.rs`

---

## Phase 8: Microsoft Store / Xbox and subscriptions (extends US1, delivers US8, plan M5)

**Goal**: Xbox library, PC Game Pass detection, subscription and free-to-play badges.

**Independent Test**: quickstart V12.

- [ ] T084 [P] [US8] Record and anonymize Xbox fixtures (MSA refresh, user and XSTS tokens, titlehub history with `gamePass`, userstats) and Game Pass catalog fixtures in `crates/connectors/tests/fixtures/xbox/`
- [ ] T085 [P] [US8] Write Xbox and Game Pass contract tests in `crates/connectors/tests/xbox.rs`
- [ ] T086 [US1] Implement the Xbox connector (operator client id from `GAMESHELF_XBOX_CLIENT_ID`, MSA authorization code and rolling refresh, Xbox Live and XSTS tokens, titlehub played titles with PC device, last played, achievements and gamerscore) in `crates/connectors/src/xbox/`
- [ ] T087 [US8] Implement the Game Pass catalog job (sigl lists, display catalog resolution, `subscription_item` added and removed dates) in `crates/sync/src/catalog.rs` and `crates/connectors/src/xbox/gamepass.rs`
- [ ] T088 [US8] Implement subscription rules (declared or detected subscription per account; a title counts as owned when any non-subscription ownership exists; subscription-only titles hidden by default and removed when leaving the catalog) in `crates/core/src/subscription.rs` and `crates/store/src/repo/subscriptions.rs`
- [ ] T089 [US8] Show subscription and free-to-play badges and filters in `crates/app/src/components/game_card.rs` and `crates/app/src/components/filters.rs`
- [ ] T090 [P] [US8] Write Playwright journey V12 in `e2e/tests/us8-subscriptions.spec.ts`

---

## Phase 9: User Story 5 - Agent for launchers that only keep data locally (Priority: P3)

**Goal**: a Tauri agent for Windows and Linux with pairing, transparent controls, local scans and login capture.

**Independent Test**: quickstart V8 and V9.

### Server side

- [ ] T091 [P] [US5] Write agent API tests (pairing code single use and 10-minute expiry, 5 attempts per 10 minutes, bearer auth, `426` for old versions, report `applied` vs `unchanged`, revoked agent gets `401 agent_revoked`) in `crates/server/tests/agent_api.rs`
- [ ] T092 [US5] Implement agent and pairing code repositories (token stored as SHA-256 hash, status `active`, `paused`, `revoked`) in `crates/store/src/repo/agents.rs`
- [ ] T093 [US5] Implement `/api/agent/v1` endpoints (pair, config, heartbeat, reports, login result and decline, self delete) exactly as in contracts/agent-api.md in `crates/server/src/api/agent.rs`
- [ ] T094 [US5] Apply agent reports through the import pipeline (account matching by external id or by the player's connected accounts, install records per machine, `send_install_paths` honored) in `crates/sync/src/agent_reports.rs`
- [ ] T095 [US5] Build the Agents page (create pairing code, list agents with last contact, sources, schedule, pause, revoke, login requests "Connect with agent") in `crates/app/src/routes/agents.rs`

### Local sources

- [ ] T096 [P] [US5] Implement and test the Steam local source (library folders, app manifests, per-user local config playtime and last played, login users) with anonymized fixture files in `crates/connectors/src/local/steam.rs`
- [ ] T097 [P] [US5] Implement and test the Epic local source (install manifests) in `crates/connectors/src/local/epic.rs`
- [ ] T098 [P] [US5] Implement and test the GOG Galaxy local source (read-only open of the local database: owned releases, game times) in `crates/connectors/src/local/gog_galaxy.rs`
- [ ] T099 [P] [US5] Implement and test the Battle.net local source (client config products and last played, product database install paths) in `crates/connectors/src/local/battlenet.rs`
- [ ] T100 [P] [US5] Implement and test the Ubisoft Connect local source (install registry, configuration cache) in `crates/connectors/src/local/ubisoft.rs`
- [ ] T101 [P] [US5] Implement and test the Windows packages source for Microsoft Store and Xbox games in `crates/connectors/src/local/xbox_packages.rs`
- [ ] T102 [P] [US5] Implement and test the uninstall-registry source with a signature table for publisher and single-game launchers (Riot, Rockstar, Tempo, Battlestate, HoYoPlay, Wargaming, Jagex and others from research.md section 2) in `crates/connectors/src/local/uninstall_registry.rs`
- [ ] T103 [P] [US5] Implement and test the Linux sources for Heroic (Epic, GOG, Amazon libraries) and Lutris in `crates/connectors/src/local/heroic.rs` and `crates/connectors/src/local/lutris.rs`

### Agent application

- [ ] T104 [US5] Scaffold the Tauri 2 agent (tray icon, single instance, autostart plugin off by default, Windows NSIS and MSI plus Linux AppImage and deb bundles) in `crates/agent/` and its Leptos UI in `crates/agent/ui/`
- [ ] T105 [US5] Implement the first-run wizard (detect launchers, show what will be sent with a preview, ask at most three questions: server URL, pairing code, autostart and schedule) in `crates/agent/src/wizard.rs` and `crates/agent/ui/src/wizard.rs`
- [ ] T106 [US5] Implement the API client, keychain token storage, config pull, heartbeat, scans per schedule, gzip reports with content hash, pause and per-source toggles, revocation handling in `crates/agent/src/client.rs` and `crates/agent/src/scan.rs`
- [ ] T107 [US5] Implement webview login capture (prompt before opening, `RedirectQuery`, `JsonPage` and `LocalCallback` capture, send only the code) in `crates/agent/src/login.rs`
- [ ] T108 [US5] Implement headless mode (`--headless`, configuration file, no UI) in `crates/agent/src/headless.rs`
- [ ] T109 [P] [US5] Add agent installer builds for Windows and Linux to `.github/workflows/release.yml`

**Checkpoint**: V8 and V9 pass on Windows and Linux.

---

## Phase 10: User Story 6 - Family profiles and controlled sharing (Priority: P3)

**Goal**: several profiles, admin role, sharing with exclusions and zero leaks.

**Independent Test**: quickstart V10.

- [ ] T110 [P] [US6] Write visibility unit and store tests covering every exclusion kind (game, platform, account; global or per audience) and aggregation after filtering in `crates/store/tests/visibility_sharing.rs`
- [ ] T111 [P] [US6] Write the Playwright leak suite for V10 (search, filters, counts, detail, direct links, exports) in `e2e/tests/us6-sharing-leaks.spec.ts`
- [ ] T112 [US6] Implement sharing rule and exclusion repositories (sharing off by default) in `crates/store/src/repo/sharing.rs`
- [ ] T113 [US6] Extend `visible_ownerships` with sharing rules and exclusions applied before aggregation in `crates/store/src/repo/visibility.rs`
- [ ] T114 [US6] Build the admin page for profiles (create, disable, delete with cascade, reset sign-in; admins never read unshared libraries or credentials) in `crates/app/src/routes/admin.rs`
- [ ] T115 [US6] Build the Sharing page (audiences, exclusions by game, platform or account) in `crates/app/src/routes/sharing.rs`
- [ ] T116 [US6] Attribute shared ownerships ("owned by <profile>") in cards, search and detail, own accounts listed first, in `crates/app/src/components/game_card.rs` and `crates/app/src/routes/game.rs`
- [ ] T117 [US6] Implement trusted-proxy sign-in (`GAMESHELF_TRUSTED_PROXIES`, `GAMESHELF_PROXY_AUTH_HEADER`, mapping to existing profiles only) in `crates/server/src/auth/proxy.rs`

**Checkpoint**: V10 passes with zero leaks.

---

## Phase 11: User Story 7 - Notifications (Priority: P4)

**Goal**: in-app, Discord, ntfy and Gotify notifications, grouped, with daily reminders at most.

**Independent Test**: quickstart V11.

- [ ] T118 [P] [US7] Write sender tests against mock servers (Discord embed and `429` handling, ntfy headers and bearer token, Gotify key header) in `crates/notify/tests/senders.rs`
- [ ] T119 [P] [US7] Implement the Discord, ntfy and Gotify senders per contracts/notifications.md in `crates/notify/src/discord.rs`, `crates/notify/src/ntfy.rs` and `crates/notify/src/gotify.rs`
- [ ] T120 [US7] Implement grouping per sync run (up to 10 games then "and N more"), localized rendering in the owner's language, and the reminder ledger (one reminder per issue per target per day) in `crates/notify/src/grouping.rs` and `crates/notify/src/ledger.rs`
- [ ] T121 [US7] Implement notification target repository with sealed configuration in `crates/store/src/repo/notify.rs`
- [ ] T122 [US7] Build the Notifications page (targets, event selection, "Send test", last error) and in-app unread badge in `crates/app/src/routes/notifications.rs`
- [ ] T123 [US7] Emit `agent_silent` events when an agent misses twice its schedule in `crates/sync/src/health.rs`

---

## Phase 12: Platform expansion B - EA app, Amazon, itch.io, Humble (extends US1, plan M9)

- [ ] T124 [P] [US1] Record fixtures and write contract tests for EA, Amazon, itch.io and Humble in `crates/connectors/tests/fixtures/` and `crates/connectors/tests/{ea,amazon,itch,humble}.rs`
- [ ] T125 [P] [US1] Implement the EA app connector (PKCE login, refresh, owned games, playtime, achievements) in `crates/connectors/src/ea/`
- [ ] T126 [P] [US1] Implement the Amazon Games connector (device registration, refresh, entitlements) in `crates/connectors/src/amazon/`
- [ ] T127 [P] [US1] Implement the itch.io connector (API key, owned keys paging) in `crates/connectors/src/itch/`
- [ ] T128 [P] [US1] Implement the Humble Bundle connector (session cookie, orders and keys, unredeemed store keys exposed as enrichment) in `crates/connectors/src/humble/`

---

## Phase 13: User Story 9 - Manual entries, import and export; key-based enrichment (Priority: P5)

**Goal**: manual ownerships, CSV import and export, enrichments needing only a key or public data.

**Independent Test**: quickstart V13.

- [ ] T129 [P] [US9] Write CSV import and export tests (header rules of contracts/http-api.md section 4, re-import creates no duplicates, unmatched rows reported, exports exclude shared ownerships, credentials and install paths) in `crates/server/tests/csv.rs`
- [ ] T130 [US9] Implement manual accounts and manual ownership add, edit and delete in `crates/store/src/repo/manual.rs` and `crates/app/src/routes/settings.rs`
- [ ] T131 [US9] Implement `GET /api/v1/export.csv`, `GET /api/v1/export.json` and `POST /api/v1/import/csv` in `crates/server/src/api/export.rs` and `crates/server/src/api/import.rs`
- [ ] T132 [P] [US4] Implement key or public-data enrichment providers (Riot account and league for LoL and TFT, Guild Wars 2 account, OSRS hiscores, WoW public character profiles) with fixtures and tests in `crates/connectors/src/enrich/`

---

## Phase 14: Polish and Cross-Cutting Concerns

- [ ] T133 [P] Run the 20,000-ownership synthetic benchmark for SC-002 and record results in `crates/core/benches/search.rs`
- [ ] T134 [P] Add axe accessibility checks to every Playwright journey in `e2e/tests/a11y.spec.ts`
- [ ] T135 [P] Complete French translations and the locale parity check in `crates/app/locales/fr.json`
- [ ] T136 Add multi-architecture image publishing (linux/amd64, linux/arm64) to `.github/workflows/release.yml`
- [ ] T137 Security review: redaction of every error path, session and origin rules, sealed secrets, visibility choke point, agent token handling; record findings in `docs/security-review.md`
- [ ] T138 Run every quickstart.md scenario (V1 to V14) and update `README.md` with operator setup notes
- [ ] T139 [P] Build the cross-platform matching reference set (500 titles with public store ids only, no personal data) and a test asserting SC-007 (at least 95% grouped without manual merge) in `crates/core/tests/matching_reference.rs`

---

## Dependencies and Execution Order

### Phase dependencies

- Setup (Phase 1) has no dependency.
- Foundational (Phase 2) depends on Setup and blocks every story.
- US1 (Phase 3) depends on Phase 2. It is the MVP.
- US2 (Phase 4) depends on US1 (library page and visibility queries).
- US3 (Phase 5) depends on US1 (import pipeline).
- US4 (Phase 6) depends on US1; it is independent of US2 and US3.
- Platform expansion A (Phase 7) depends on US1; it benefits from US3 for scheduled refresh.
- Phase 8 (Xbox, US8) depends on US1 and US2 (filters).
- US5 (Phase 9) depends on US1 and US3; the login capture part depends on Phase 7.
- US6 (Phase 10) depends on US1 and US2 (visibility queries and filters).
- US7 (Phase 11) depends on US3 (activity events).
- Platform expansion B (Phase 12) depends on Phase 7 (browser-login flow).
- US9 (Phase 13) depends on US1; T132 depends on US4.
- Polish (Phase 14) depends on the stories it covers.

### Within each phase

- Tests and fixtures are written before the implementation they cover and must fail first.
- Repositories before services, services before server functions, server functions before pages.

### Parallel opportunities

- Phase 1: T002, T003, T005, T006, T007, T008 in parallel after T001.
- Phase 2: T009 to T011, T014, T017 to T022, T025, T026, T028, T029, T032, T033, T037 in parallel once T015 and
  T016 exist where they need the database.
- US1: T038 to T042 together; T045 to T047 together; T053 alongside T051 and T052.
- Local sources T096 to T103 are fully parallel.
- Phase 12 connectors T125 to T128 are fully parallel.

### Parallel example: User Story 1

```text
Task: "T038 Record and anonymize Steam fixtures in crates/connectors/tests/fixtures/steam/"
Task: "T040 Write matching unit tests in crates/core/tests/matching.rs"
Task: "T041 Write search unit tests in crates/core/tests/search.rs"
Task: "T045 Implement the IGDB client in crates/metadata/src/igdb.rs"
Task: "T047 Implement the optional SteamGridDB client in crates/metadata/src/steamgriddb.rs"
```

## Implementation Strategy

### MVP first (User Story 1)

1. Phase 1 and Phase 2.
2. Phase 3 (US1) with Steam: several accounts, search, detail.
3. Stop and validate V1 and V2, then deploy to the NAS.

### Incremental delivery

1. US2 (browse) completes the P1 scope.
2. US3 (unattended sync) and US4 (detail data).
3. Epic and GOG, then Xbox with Game Pass.
4. Agent (US5), sharing (US6), notifications (US7).
5. More stores, manual entries and key-based enrichment.

Each step is shippable on its own and keeps earlier scenarios green.
