<!--
Sync Impact Report
- Version change: (template) -> 1.0.0
- Principles defined: I. Generic and Private by Design; II. Provenance-First Ownership;
  III. Unattended, Transparent, User-Controlled; IV. Read-Only, Least-Privilege Credentials;
  V. Isolated, Resilient Connectors; VI. Rust, Simple to Run; VII. English-First, Localized UI
- Added sections: Licensing and Third-Party Material; Development Workflow and Quality Gates
- Removed sections: none
- Templates: plan-template.md "Constitution Check" reads this file at runtime; no template edits
- Deferred TODOs: none
-->

# gameshelf Constitution

## Core Principles

### I. Generic and Private by Design (NON-NEGOTIABLE)

gameshelf is a public, self-hosted project. Nothing in the repository may identify or describe a
specific user, household or deployment.

- The repository MUST NOT contain real account names, account or player IDs, hostnames, IP
  addresses, local paths, tokens, cookies, API keys, screenshots of real libraries or any other
  personal data. Examples use obvious placeholders (`example-user`, `192.0.2.10`, `/data`).
- Test fixtures MUST be synthetic or fully anonymized before they are committed.
- Every deployment-specific value (URLs, schedules, credentials, paths on the agent host) MUST be
  supplied at runtime through configuration or the UI, never compiled in.
- The software MUST NOT phone home: no telemetry, no analytics, no update pings unless the
  operator explicitly enables them. Outbound traffic goes only to the platform and metadata
  services the operator has enabled.

Rationale: the code is shared with everyone, the data belongs to each operator.

### II. Provenance-First Ownership

The product exists to answer "do I already own this game, and on which account(s)?".

- Every ownership record MUST carry its full provenance: profile, platform, platform account and
  the platform's own game identifier, plus how it was obtained (purchase, free claim,
  subscription, free-to-play, family sharing, manual entry).
- Merging records into one canonical game MUST NEVER discard provenance. The same game owned on
  two platforms, or on five accounts of one platform, stays visible as that many ownerships.
- Subscription access and free-to-play play MUST be distinguishable from owned copies everywhere
  they appear (badges, filters, counts).
- Automatic matching across platforms MUST be overridable by the user (merge and split), and
  overrides MUST survive later syncs.

Rationale: an aggregator that loses "which account" fails the core question.

### III. Unattended, Transparent, User-Controlled

- Library synchronization MUST run on a schedule without user action once an account is
  connected. Manual steps are allowed only when a platform makes them unavoidable, and the UI
  MUST then say exactly what to do.
- Nothing runs hidden. Every background job, connector, agent feature and schedule MUST be
  listed in the UI with its state, last run, next run and last result, and MUST be individually
  switchable on and off.
- Failures and credentials nearing expiry MUST be surfaced proactively in the app and through
  the notifiers the user configured.
- The agent MUST ask before enabling autostart, scheduled scans or any data source, MUST show
  what it collects before sending it, and MUST be pausable and removable without leftovers.

Rationale: the user wants zero chores and full visibility at the same time.

### IV. Read-Only, Least-Privilege Credentials

- gameshelf MUST only read from platforms. It MUST NEVER purchase, redeem, post, message,
  change account settings or perform any write on a user's platform account.
- Platform credentials MUST be encrypted at rest with a key supplied by the operator (environment
  variable or secret file), MUST NEVER be logged, and MUST be redacted from errors and exports.
- Built-in authentication MUST always be active: passwords hashed with a memory-hard algorithm,
  sessions bound to secure cookies. Trusting a reverse-proxy identity header is an explicit
  opt-in restricted to configured proxy addresses.
- Authorization is per profile: a profile sees another profile's games only through that
  profile's sharing settings, and sharing exclusions MUST be enforced server-side.
- Agents pair with the server through a short-lived one-time code and then use a per-agent,
  revocable token. Agent traffic MUST use TLS whenever it leaves the local network.

Rationale: the server concentrates long-lived access to every account a household owns.

### V. Isolated, Resilient Connectors

- Each platform is a connector behind a common interface that declares its capabilities
  (library, playtime, achievements, enrichment) and its stability tier (official API,
  unofficial API, local data via agent, manual).
- A connector failure MUST NOT block, delay or corrupt other connectors or existing data. A
  failed sync keeps the last known good state and records the error.
- Unofficial APIs are allowed. Their tier MUST be visible in the UI, and connectors MUST respect
  rate limits with backoff and jitter. Connectors MUST NOT bypass CAPTCHAs or anti-bot
  protections, and MUST NOT ask for passwords where a token or browser-based flow exists.
- Parsers MUST tolerate unknown fields and missing optional data.
- Every connector MUST ship contract tests that run against recorded, anonymized fixtures. Tests
  MUST NOT require network access or real accounts.

Rationale: platforms change without notice; one broken store must not break the shelf.

### VI. Rust, Simple to Run

- Server, web UI and agent are written in stable Rust. The web UI uses Leptos.
- The server ships as one multi-architecture container image; the agent ships as one native
  binary per supported OS with a guided first-run setup.
- The default storage is a single embedded database file. No external service (database,
  queue, cache) may be mandatory.
- Schema migrations run automatically and are forward-only. Backing up an instance means copying
  its data volume.
- Defaults MUST make a fresh instance usable with only an encryption key and an admin password.

Rationale: self-hosters adopt what is easy to run and easy to back up.

### VII. English-First, Localized UI

- Code, identifiers, comments, documentation and commit messages are written in English.
- Every user-visible string goes through the localization catalog. English is the reference
  locale; French ships at first release. Hard-coded UI strings are defects.
- The UI MUST meet WCAG 2.2 AA, work with keyboard alone, and support light and dark themes.

Rationale: an international audience, and a French-speaking first user.

## Licensing and Third-Party Material

- The project is licensed under AGPL-3.0-or-later.
- Third-party code may be vendored or ported only under an AGPL-compatible license (for example
  MIT, Apache-2.0, BSD, GPL-3.0), with attribution recorded in a third-party notices file.
- Platform names and logos are used nominatively, only to identify the platform a game comes
  from, and are sourced from properly licensed icon sets or official brand kits.
- Game artwork and metadata are fetched at runtime from providers under their terms and cached
  per instance. They are never committed to the repository.
- No shared project API key is embedded in the source or binaries. Each operator supplies their
  own keys for metadata providers that require one.

## Development Workflow and Quality Gates

- Features follow the Spec Kit flow: specify, clarify, plan, tasks, implement. Each plan's
  Constitution Check MUST pass or record a justified exception.
- Continuous integration MUST pass before merge: `cargo fmt --check`, `cargo clippy` with
  warnings denied, the full test suite, `cargo deny` (licenses and advisories) and a secret
  scanner that enforces Principle I.
- New behavior lands with tests: unit tests for pure logic, connector contract tests for every
  connector, and an end-to-end sync test that uses a fake connector.
- Commit messages and documentation carry no generated-by or tool attribution footers.

## Governance

This constitution supersedes other practices in this repository. Amendments are made through a
pull request that states the change and its rationale and updates the version below:

- MAJOR for removing or redefining a principle,
- MINOR for a new principle or section, or materially expanded guidance,
- PATCH for clarifications and wording.

Every plan and every pull request review checks compliance with these principles. Runtime
development guidance for coding agents lives in the agent context file (`CLAUDE.md`), which
must stay consistent with this document.

**Version**: 1.0.0 | **Ratified**: 2026-10-15 | **Last Amended**: 2026-10-15
