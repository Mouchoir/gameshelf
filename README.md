# gameshelf

A self-hosted game library for people with many launchers and several accounts per platform.
Search a title and gameshelf tells you whether you already own it, and on which account(s).

- One library for every store and launcher, each game shown once with the logos of the platforms that have it
  and a marker when it sits on several accounts.
- Daily automatic sync, with clear health status for every connected account.
- Per-account details: playtime, last session, achievements, platform level.
- Family profiles with controlled sharing, an agent for launchers that only keep data locally, notifications
  to the app, Discord, ntfy and Gotify.

gameshelf only reads from platforms. It never buys, posts or changes anything on your accounts, and it sends
nothing anywhere except the platforms and providers you enable.

## Status

Early development. Steam (Web API keys), Epic Games and GOG connect from the web app; the agent adds what
launchers keep on each computer (Steam playtime per local account, Epic, GOG Galaxy, Battle.net, Ubisoft Connect
and single-game launchers). See the [specification](specs/001-multi-account-library/spec.md),
the [plan](specs/001-multi-account-library/plan.md) and the [tasks](specs/001-multi-account-library/tasks.md).

## Run it

Container image and compose example: [docker/](docker/). Configuration reference:
[contracts/configuration.md](specs/001-multi-account-library/contracts/configuration.md).

For development (Rust stable, `cargo-leptos`, the `wasm32-unknown-unknown` target):

```text
scripts/dev.sh        # or scripts/dev.ps1 on Windows
```

This starts the server on http://localhost:3000 with a local `.data/` folder and a fake connector that serves
invented libraries. Open `/setup` to create the first profile.

## The agent

`gameshelf-agent` runs on Windows and Linux. It reads the launchers installed on the computer and sends their
game lists to your server; it never reads passwords, login names or e-mail addresses, and sends nothing from a
source you turned off.

```text
gameshelf-agent setup            # shows what would be sent, then asks: server, pairing code, autostart
gameshelf-agent scan             # what would be sent, without sending
gameshelf-agent status           # pairing, schedule, sources found on this computer
gameshelf-agent pause | resume
gameshelf-agent source steam-local off
gameshelf-agent autostart enable | disable
gameshelf-agent unpair
```

Pairing codes come from the Agents page of the web app, where you also choose the scan interval, the sources
and whether install folders are sent (off by default). Build it with `cargo build --release -p gameshelf-agent`.

## Quality gates

```text
cargo fmt --all --check
cargo clippy --workspace --exclude gameshelf-frontend --all-targets -- -D warnings
cargo nextest run --workspace --exclude gameshelf-frontend
cargo deny check
```

## License

GNU Affero General Public License v3.0 or later. See [LICENSE](LICENSE).
Platform names and logos belong to their owners and are used only to identify where a game comes from; see
[crates/app/public/icons/NOTICE.md](crates/app/public/icons/NOTICE.md).
