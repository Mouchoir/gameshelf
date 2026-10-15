# Contract: Agent API (`/api/agent/v1`)

**Applies to**: the `server` (provider) and the `agent` (consumer). **Related**: research D9, D10;
[connector.md](./connector.md) section 6; spec US5, FR-026 to FR-030.

## Conventions

- JSON over HTTPS. Plain HTTP is accepted only from private network ranges, and the agent warns when it is used.
- Every call except pairing sends `Authorization: Bearer <agent token>` and `X-Gameshelf-Agent-Version`.
- Bodies may be gzip-encoded. Maximum body size: 5 MiB.
- Version policy: within `v1`, changes are additive only (new optional fields, new endpoints). A server that no
  longer supports an agent version answers `426 Upgrade Required` with
  `{"error": {"code": "upgrade_required", ...}, "min_version": "0.1.0"}`.
- Errors use `{"error": {"code": "<snake_case>", "message": "<english text>"}}`.
- Dates are RFC 3339 strings in UTC.
- The server never connects to the agent; the agent polls.

## Pairing

### `POST /api/agent/v1/pair`

Request:

```json
{ "code": "K7Q2-M9XD", "name": "Living-room PC", "os": "windows", "version": "0.1.0" }
```

Response `201`:

```json
{
  "agent_id": "0193f5c2-...",
  "token": "<256-bit random, base64url>",
  "config": { "...": "same shape as GET /config" }
}
```

Errors: `400 invalid_code` (unknown, expired or already used; the response does not say which), `429 too_many_attempts`
(5 attempts per 10 minutes per source address).

The code is created in the web app (Agents page), is valid 10 minutes, single use, and is stored hashed. The agent
stores the token in the OS keychain.

## Configuration

### `GET /api/agent/v1/config`

Response `200`:

```json
{
  "status": "active",
  "scan_hours": 6,
  "send_install_paths": false,
  "sources": {
    "steam-local": { "enabled": true, "paths": [] },
    "battlenet-local": { "enabled": false, "paths": [] }
  }
}
```

`sources` always lists every source the server knows (`steam-local`, `epic-local`, `gog-galaxy-local`,
`battlenet-local`, `ubisoft-local`, `launchers-local`); `paths` are extra folders the player added. `scan_hours`
is between 1 and 168. When logins through the agent ship, a `login_requests` array is added (see below).

`status` may be `paused` (the agent keeps running but sends nothing) or `revoked` (see below). Settings changed in
the web app reach the agent at its next pull; settings changed on the agent are reported by heartbeat and shown in
the web app (Principle III).

## Heartbeat

### `POST /api/agent/v1/heartbeat`

```json
{
  "version": "0.1.0",
  "autostart": true,
  "paused": false,
  "sources": { "steam-local": { "enabled": true, "detected": true, "last_scan_at": "2026-10-15T06:00:00Z" } }
}
```

Response `204`. Sent after each scan and at least every 6 hours.

## Reports

### `POST /api/agent/v1/reports`

One report per source and platform per scan, always a full snapshot of what the source sees. Single-platform
sources send one report; `launchers-local` sends one per platform it recognized.

```json
{
  "source": "steam-local",
  "platform": "steam",
  "generated_at": "2026-10-15T06:00:02Z",
  "content_hash": "sha256:...",
  "accounts": [
    {
      "external_account_id": "76561190000000001",
      "display_name": "example-user",
      "items": [
        { "platform_game_id": "730", "title": "Counter-Strike 2", "kind": "game", "installed": true,
          "size_bytes": 38000000000, "playtime_minutes": 9420, "last_played_at": "2026-10-14T21:12:00Z" }
      ]
    }
  ],
  "machine_items": []
}
```

- `accounts[].external_account_id` is set when the launcher stores the account id locally. The server attaches
  those items to the player's account with that id, creates an agent-fed account for the player when none
  exists, and ignores accounts another profile holds. Items in `machine_items` go to the player's only account
  on that platform, or to an account named after the agent when there are none or several.
- `title` may be `null` when the launcher's files do not hold it; the server names the product from the
  platform's public data (Steam store) or leaves it out.
- Agent reports never remove games, never change how a game was acquired and never lower playtime: a launcher
  only sees one machine.
- `install_path` is included per item only when `send_install_paths` is true; the server drops it otherwise.
- Response `202` with `{"result": "applied"}` (accepted, applied in the background) or `{"result": "unchanged"}`
  when `content_hash` matches the last report for that source and platform. Errors: `400 unknown_source`,
  `409 source_disabled`, `409 agent_paused`, `413 too_large`.

A report never contains credentials, file contents, or data from disabled sources (FR-030).

## Browser logins through the agent

Not offered yet: the endpoints below answer `404 unknown_request` until the agent application can capture logins.

1. In the web app, the player chooses "Connect with agent <name>" for a platform. The server creates a login
   request (PKCE verifier kept server-side) that appears in the agent's next config pull.
2. The agent notifies the player on its machine and does nothing until the player accepts.
3. The agent opens `authorize_url` in an embedded webview, captures the code as described by `capture`, closes
   the window, and sends:

### `POST /api/agent/v1/logins/{id}/result`

```json
{ "code": "<authorization code or redirect URL>" }
```

Response `204`. The server exchanges the code, validates the account and stores the credential. Errors:
`404 unknown_request`, `410 expired`, `409 already_completed`. The player can also decline:
`POST /api/agent/v1/logins/{id}/decline` → `204`.

## Revocation and removal

- When an agent is revoked in the web app, every call returns `401 agent_revoked`; the agent stops, deletes its
  token and explains how to pair again (US5 scenario 5).
- `DELETE /api/agent/v1/self` lets the agent unpair itself during uninstall; the server marks it revoked and keeps
  its install records until the player deletes the agent.
