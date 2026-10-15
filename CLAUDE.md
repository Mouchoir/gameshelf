# gameshelf: guidance for coding agents

gameshelf is a self-hosted, multi-launcher, multi-account game library written in Rust. It answers "do I already
own this game, and on which account(s)?". The repository is public and licensed AGPL-3.0-or-later.

## Read first

- Constitution: `.specify/memory/constitution.md`. It overrides everything else here.
- Current feature: `specs/001-multi-account-library/` (spec, plan, research, data model, contracts, quickstart).
- Workflow: Spec Kit skills in `.claude/skills/` (specify, clarify, plan, tasks, implement, converge). On Windows,
  run the Spec Kit Python scripts with `PYTHONIOENCODING=utf-8`.

## Non-negotiables

- Never commit personal data: real account names or ids, hostnames, IP addresses, local paths, tokens, cookies,
  keys, or screenshots of a real library. Use placeholders (`example-user`, `192.0.2.10`, `/data`) and anonymized
  fixtures.
- Connectors only read from platforms. No purchase, redemption, post or account change, ever.
- Secrets stay sealed and redacted: never log, print, export or embed them.
- Ownership provenance (profile, platform, account, platform game id, acquisition) is never discarded.
- Every ownership read from the web layer goes through the visibility function in `core`.
- Code, comments, docs and commit messages in English; every UI string through the localization catalogs
  (`en`, `fr`).
- No generated-by or tool attribution footers in commits, pull requests, code or docs. Use plain hyphens rather
  than em dashes, and write out "..." rather than the ellipsis character.

## Stack

Rust (edition 2024), Leptos 0.8 SSR + hydration with Axum 0.8, SQLite through sqlx 0.9, Tauri 2 for the agent.
Workspace layout and decisions: `specs/001-multi-account-library/plan.md` and `research.md` section 5.

## Commands

```text
cargo leptos watch                                    # dev server
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo deny check
```

Tests never hit the network or real accounts.
