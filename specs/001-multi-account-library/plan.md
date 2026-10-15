# Implementation Plan: Multi-Account Game Library

**Branch**: `001-multi-account-library` | **Date**: 2026-10-15 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-multi-account-library/spec.md`

## Summary

gameshelf gathers every game a household owns, across stores and launchers and across several accounts per
platform, into one self-hosted library that answers "do I own this, and on which account?". A Rust server
(Axum + Leptos SSR with hydration, SQLite) syncs connected accounts on a schedule through isolated connectors,
groups platform products into canonical games without losing per-account provenance, and enforces per-profile
sharing through a single visibility choke point. A Tauri 2 agent for Windows and Linux covers launchers that only
keep data locally and captures browser logins. Research decisions D1 to D15 are in [research.md](./research.md).

## Technical Context

**Language/Version**: Rust stable (toolchain 1.96 at planning time), edition 2024, MSRV 1.85

**Primary Dependencies**: Leptos 0.8 + leptos_axum 0.8 + leptos_i18n 0.6, Axum 0.8, Tokio 1, sqlx 0.9
(SQLite), tower-sessions 0.15 (custom store), argon2 0.6, chacha20poly1305 0.11, secrecy 0.10, reqwest 0.13
(rustls), governor 0.10, backon 1.6, croner 4, nucleo-matcher 0.3, deunicode 1.6, image 0.25, figment 0.10,
tracing; agent: Tauri 2.12 with the autostart plugin, keyring 4, winreg 0.56 (uninstall entries);
Steam VDF files are read by a small built-in parser

**Storage**: one SQLite database in WAL mode plus a content-addressed artwork cache, both in the data volume

**Testing**: `cargo nextest` for unit, store and connector contract tests (`wiremock` 0.6, `insta` 1.x,
anonymized fixtures, no network), an end-to-end sync test with a fake connector, Playwright for user journeys,
accessibility (axe) and visibility-leak tests

**Target Platform**: server as a Linux container (linux/amd64, linux/arm64); agent on Windows 10/11 x64 and Linux
x86_64 (including Steam Deck desktop mode); web UI on current evergreen browsers, desktop and phone

**Project Type**: web application (server + hydrated UI) plus a desktop agent

**Performance Goals**: search results in under 0.2 s for 20,000 ownerships (SC-002); library page first paint in
under 1 s on a NAS-class CPU over LAN; a daily sync of 10 accounts completes in under 10 minutes, platform rate
limits aside

**Constraints**: no mandatory external service; server idle memory under 256 MB; read-only toward platforms; no
telemetry; every UI string localized (en, fr); WCAG 2.2 AA

**Scale/Scope**: one household per instance, up to 10 profiles, 50 platform accounts, 50,000 ownerships;
about 15 screens

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle | How this plan complies | Status |
|---|---|---|
| I. Generic and Private by Design | Every deployment value comes from configuration ([configuration.md](./contracts/configuration.md)); fixtures anonymized with a CI check ([connector.md](./contracts/connector.md) section 7); secret scanner in CI; no telemetry; examples use placeholders | PASS |
| II. Provenance-First Ownership | `ownership` keyed by account and platform product, games group products without merging ownerships ([data-model.md](./data-model.md)); acquisition types including subscription and free-to-play; persistent match decisions | PASS |
| III. Unattended, Transparent, User-Controlled | Persistent scheduler with per-account jobs and visible run history (D6); account health state machine; agent opt-in for autostart, schedule and every source, visible in both UIs ([agent-api.md](./contracts/agent-api.md)) | PASS |
| IV. Read-Only, Least-Privilege Credentials | Connectors are read-only by contract; credentials sealed with an operator key (D5); built-in login always on, proxy identity opt-in (D4); per-profile visibility choke point (D8); agent tokens hashed and revocable; agent forwards authorization codes only | PASS |
| V. Isolated, Resilient Connectors | Common connector contract with tiers and capabilities, error taxonomy, complete-listing rule before removals, per-platform rate limits, fixture-based contract tests | PASS |
| VI. Rust, Simple to Run | Rust everywhere, Leptos UI, single image for two architectures, SQLite only, automatic migrations, backup = copy the volume | PASS |
| VII. English-First, Localized UI | English code and docs; leptos_i18n catalogs en and fr with a CI parity check; axe checks in end-to-end tests | PASS |
| Licensing and third-party material | AGPL-3.0-or-later; GPL-3.0 sources (legendary, Maxima, nile) may be ported with attribution; artwork fetched at runtime; operator-supplied provider keys; logos used nominatively | PASS |
| Workflow and quality gates | CI runs fmt, clippy with warnings denied, nextest, cargo deny, secret scan; Spec Kit flow followed | PASS |

Post-design re-check (after Phase 1): PASS, no exception recorded. Note: the Xbox connector needs an OAuth client
registration. To respect "no shared project key", the default is an operator-registered Azure application, with a
documented setup step; see Risks.

## Project Structure

### Documentation (this feature)

```text
specs/001-multi-account-library/
├── spec.md
├── plan.md               # this file
├── research.md           # platform research and decisions D1-D15
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── connector.md
│   ├── agent-api.md
│   ├── http-api.md
│   ├── notifications.md
│   └── configuration.md
├── checklists/
│   └── requirements.md
└── tasks.md              # created by /speckit-tasks
```

### Source Code (repository root)

```text
Cargo.toml                # workspace, shared lints and dependency versions
crates/
├── core/                 # domain types, matching, normalization, visibility and sharing rules (no I/O)
├── store/                # sqlx SQLite: migrations, repositories, session store, sealed secrets
├── connectors/           # Connector contract, one module per platform
│   └── tests/fixtures/   # anonymized recorded responses
├── local/                # LocalSource readers of launcher files (Steam, Epic, GOG Galaxy, Battle.net, ...)
├── metadata/             # IGDB, SteamGridDB, Wikidata clients, artwork cache
├── notify/               # Discord, ntfy, Gotify senders, grouping and reminder ledger
├── sync/                 # job runner, scheduler, snapshot diffing, account health
├── app/                  # Leptos components, routes, server functions, i18n catalogs (locales/en, locales/fr)
├── frontend/             # hydration entry point (wasm)
├── server/               # binary: Axum router, auth, agent API, exports, health, configuration
└── agent/                # Tauri 2 app: tray, wizard, webview logins, local scans, headless mode
    └── ui/               # Leptos client-side UI of the agent
e2e/                      # Playwright tests (journeys, accessibility, visibility leaks)
docker/                   # Dockerfile (cargo-chef, multi-arch) and an example compose file with placeholders
.github/workflows/        # CI (lint, test, deny, secret scan) and release (image, agent installers)
```

**Structure Decision**: a single Cargo workspace (research D1). Pure logic in `core` keeps SC-003, SC-007 and
SC-011 testable without a database; connectors are shared by server and agent; the Leptos split (`app`,
`frontend`, `server`) follows the cargo-leptos workspace convention.

## Delivery Milestones

Order follows the spec priorities and the platform order fixed in clarification (core platforms first; the
maintainer's inventory may reorder what follows Xbox).

| # | Milestone | Delivers | Spec |
|---|---|---|---|
| M1 | Foundation | workspace, CI gates, store and migrations, setup page, sign-in, sealed secrets, i18n skeleton, container image | FR-040, FR-041, FR-045 |
| M2 | Steam multi-account MVP | Steam connector (several accounts, private profiles, family sharing awareness), matching, artwork, library grid, search, detail with playtime and achievements | US1, US2, US4 (partly) |
| M3 | Unattended sync | scheduler, run history, account health and reconnect guidance, activity feed, "Sync now", playtime snapshots | US3 |
| M4 | Epic and GOG | browser-login connectors with the paste-code flow | US1, US3 |
| M5 | Microsoft Store / Xbox and PC Game Pass | Xbox connector, Game Pass catalog, subscription badges and filters, free-to-play badge | US8 |
| M6 | Agent | Tauri agent for Windows and Linux, pairing, local sources (Steam local playtime, Epic, GOG Galaxy, Battle.net, Ubisoft Connect, Xbox packages, Riot, Rockstar, Tempo and other launchers), webview login capture | US5 |
| M7 | Profiles and sharing | multiple profiles, admin role, sharing rules and exclusions, leak tests | US6 |
| M8 | Notifications | in-app, Discord, ntfy, Gotify, grouping and reminders | US7 |
| M9 | More stores | EA app, Amazon Games, itch.io, Humble Bundle | FR-005 |
| M10 | Manual entries and enrichment | manual entries, CSV import and export, key or public-data enrichments (Steam level, Riot ranks, Guild Wars 2, OSRS hiscores, WoW public characters) | US9, FR-017, FR-044 |

M2 is the first usable release; each later milestone is independently shippable.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Unofficial endpoints change (Epic, GOG, Xbox, EA, Ubisoft, Battle.net) | connector stops | isolation, `degraded` state, contract tests from fixtures, tier shown in UI, quick patch releases |
| Ubisoft throttling and cookie-based sessions (Battle.net, Humble) | frequent reconnects | agent-first for those platforms, reconnect reminders, honest tier labels |
| Xbox needs an OAuth client id | setup friction | documented Azure registration in quickstart; revisit a project-wide public client after M5 if the constitution is amended |
| Steam private profiles | missing libraries | per-account API key flow, clear guidance in the connection page |
| Matching errors across platforms | wrong grouping | identifiers first, merge and split tools, reference test set for SC-007 |
| Credential theft if the server is compromised | account access | read-only scopes where possible, sealed secrets, no full-login enrichments in v1 (clarification), proxy and built-in auth |

## Complexity Tracking

No constitution violation requires justification. Two deliberate choices are recorded for reviewers:

| Choice | Why needed | Simpler alternative rejected because |
|---|---|---|
| The agent is a Tauri app with a webview rather than a console binary | the clarified login flow needs an embedded browser to capture authorization codes, and the wizard must be usable by non-technical players | a console wizard with the default browser cannot read the login result |
| A custom session store instead of `tower-sessions-sqlx-store` | the published store pins sqlx 0.8 and an older core crate, which would link two SQLite libraries | downgrading sqlx loses current fixes; the custom store is a few dozen lines |
