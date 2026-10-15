# Contract: Notifications

**Applies to**: crate `notify`. **Related**: research D12; spec US7, FR-034 to FR-036.

## Events

| Event | Trigger | Grouping |
|---|---|---|
| `game_added` | new ownership in a sync run or agent report | one message per run per target, listing up to 10 games then "and N more" |
| `game_unavailable` | ownership marked unavailable | same as above |
| `account_needs_reconnect` | account enters `needs_reconnect`, or its credential expires within 7 days | first occurrence, then at most one reminder per day |
| `account_degraded` | three failed runs in a row | first occurrence, then at most one reminder per day |
| `agent_silent` | no heartbeat for twice its schedule | first occurrence, then at most one reminder per day |
| `test` | "Send test" button | immediate |

Messages are rendered in the target owner's language and never include credentials, tokens, install paths or
other profiles' data.

## Discord

`POST <webhook URL>` with JSON:

```json
{
  "username": "gameshelf",
  "embeds": [{
    "title": "3 new games",
    "description": "Hades II (Steam, account \"Main\")\nBalatro (Steam, account \"Second\")\nAlan Wake 2 (Epic, account \"Main\")",
    "url": "https://gameshelf.example/activity",
    "timestamp": "2026-10-15T04:02:11Z"
  }]
}
```

Handles `429` by waiting `retry_after` once, then records the error on the target.

## ntfy

`POST <server>/<topic>` with the message as body and headers:

- `Title: 3 new games`
- `Tags: video_game`
- `Priority: default` (`high` for `account_needs_reconnect`)
- `Click: <public base URL>/activity`
- `Authorization: Bearer <access token>` when a token is configured

Works with self-hosted servers and the public ntfy.sh service.

## Gotify

`POST <server>/message` with header `X-Gotify-Key: <application token>` and JSON
`{"title": "...", "message": "...", "priority": 5, "extras": {"client::notification": {"click": {"url": "..."}}}}`.

## Delivery rules

- Each target is sent independently; a failure on one does not block the others.
- Failures are retried twice with backoff, then recorded in `last_error` and shown on the Notifications page.
- The reminder ledger enforces at most one reminder per issue per target per day (FR-035).
