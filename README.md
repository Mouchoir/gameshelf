# gameshelf

A self-hosted game library for people with many launchers and several accounts per platform.
Search a title and gameshelf tells you whether you already own it, and on which account(s).

- One library for every store and launcher, each game shown once with the logos of the platforms that have it
  and a marker when it sits on several accounts.
- Daily automatic sync, with clear health status for every connected account.
- Per-account details: playtime, last session, achievements, platform level.
- Family profiles with controlled sharing (planned), an agent for launchers that only keep data locally
  (planned), notifications to the app, Discord, ntfy and Gotify (planned).

gameshelf only reads from platforms. It never buys, posts or changes anything on your accounts, and it sends
nothing anywhere except the platforms and providers you enable.

## Status

Early development. The first usable milestone is Steam with several accounts: connect accounts, import,
search, browse and open a game's detail. See the [specification](specs/001-multi-account-library/spec.md),
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
