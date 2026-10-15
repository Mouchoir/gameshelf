# Quickstart and Validation Guide: Multi-Account Game Library

**Related**: [plan.md](./plan.md), [contracts/configuration.md](./contracts/configuration.md),
[data-model.md](./data-model.md)

This guide describes how to run gameshelf during development and the scenarios that prove each user story
works. Commands become valid as the milestones in the plan land.

## Prerequisites

- Rust stable with the `wasm32-unknown-unknown` target, and `cargo-leptos` 0.3
- `cargo-nextest` and `cargo-deny`
- Node.js LTS, only for the Playwright end-to-end tests
- Docker with Buildx, for the container image
- For agent work: WebView2 runtime (Windows, preinstalled on 10/11) or WebKitGTK 4.1 (Linux), and the Tauri CLI 2

## Run the server locally

1. Generate a secret key and keep it outside the repository:
   `openssl rand -base64 32 > ~/.config/gameshelf/dev-key.txt`
2. Start the dev server with hot reload:
   `GAMESHELF_SECRET_KEY_FILE=~/.config/gameshelf/dev-key.txt GAMESHELF_DATA_DIR=./.data cargo leptos watch`
3. Open `http://localhost:3000/setup`, create the first admin profile, then sign in.

`.data/` and any key file are ignored by git. Never commit real fixtures or screenshots of a real library
(Constitution Principle I).

## Run the quality gates

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo deny check
npx playwright test --project=chromium   # from e2e/
```

## Run the container

Use the Compose example in [configuration.md](./contracts/configuration.md) with your own placeholders replaced.
`GET /readyz` must answer `200` once migrations ran.

## Validation scenarios

Each scenario uses test accounts or the fake connector (`GAMESHELF_DEV_FAKE_CONNECTOR=1`, development builds only)
that serves synthetic libraries.

| # | Story | Steps | Expected |
|---|---|---|---|
| V1 | US1 | Connect two Steam test accounts that share a game, plus an Epic test account with that game; search its title | one result, Steam and Epic badges, marker "3 accounts"; detail lists three ownerships |
| V2 | US1 | Search a title not owned | explicit "not in your library" message |
| V3 | US2 | Sort by title; filter one platform and one account; reload the page | order ignores leading articles; count matches; view, sort and filters restored |
| V4 | US3 | Add a game to a fake account, run the scheduled job (or "Sync now") | game appears with an "added" activity entry; run recorded on the account |
| V5 | US3 | Make the fake connector fail for one account during a run | other accounts complete; failed account keeps data and shows the error |
| V6 | US3 | Expire a credential | account shows "reconnect" with platform-specific steps; one notification per day at most |
| V7 | US4 | Open a game owned on two accounts with different playtime | each account row has its own playtime and last played; DLC listed under the game, not in the grid |
| V8 | US5 | Install the agent, pair with a code, accept defaults with two launchers present | at most three questions; installed games of both launchers appear for that machine |
| V9 | US5 | Pause the agent, then revoke it | no reports while paused; after revoke, calls return `agent_revoked` and the agent explains how to pair again |
| V10 | US6 | Profile B shares with A, excludes one platform and one game; sign in as A | B's shared games show "owned by B"; excluded items absent from search, filters, counts, detail, direct links and exports |
| V11 | US7 | Configure ntfy and Discord test targets; press "Send test"; then trigger several new games in one run | test messages arrive; one grouped message per target for the run |
| V12 | US8 | Enable PC Game Pass on an account | subscription titles hidden by default, visible with a badge when revealed; owned copies still count as owned |
| V13 | US9 | Import a CSV twice | first import creates rows and lists unmatched ones; second import creates no duplicates |
| V14 | SC-002 | Load the synthetic 20,000-ownership data set and type a query | results update in under 0.2 s |

## Connect real platforms (operator notes)

- **Steam**: create a Web API key on each account whose profile game details are private; public accounts can use
  any one key.
- **Epic, GOG, EA**: connect through a paired agent (embedded login window) or paste the code shown after
  logging in from the link in the web app.
- **Xbox**: register a personal Azure application (personal Microsoft accounts, redirect to the instance or to
  the agent) and enter its client id in the admin page, then sign in.
- **Battle.net, Ubisoft Connect, publisher launchers**: install the agent on the machines where these launchers
  run.
