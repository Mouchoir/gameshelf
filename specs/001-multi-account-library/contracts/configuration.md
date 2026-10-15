# Contract: Server Configuration

**Applies to**: the `server` binary and its container image. **Related**: research D5, D11, D14; Constitution
Principles I and VI.

Configuration comes from environment variables, optionally from a TOML file (`GAMESHELF_CONFIG`), with
environment variables taking precedence. Values marked "UI" can also be changed in the admin page; the
environment wins when both are set and the UI shows the setting as locked.

| Variable | Default | Purpose |
|---|---|---|
| `GAMESHELF_SECRET_KEY` | none (required unless `_FILE` is set) | 32-byte key, base64, sealing credentials and notification secrets |
| `GAMESHELF_SECRET_KEY_FILE` | none | path to a file holding the key (Docker secret) |
| `GAMESHELF_DATA_DIR` | `/data` | database, artwork cache and logs |
| `GAMESHELF_BIND` | `0.0.0.0:8080` | listen address |
| `GAMESHELF_BASE_URL` | none | public URL, used for links in notifications, cookie security and origin checks |
| `GAMESHELF_TRUSTED_PROXIES` | empty | CIDR list allowed to forward client addresses and identity headers |
| `GAMESHELF_PROXY_AUTH_HEADER` | empty (disabled) | identity header to trust from trusted proxies (UI) |
| `GAMESHELF_SYNC_SCHEDULE` | `0 4 * * *` | default daily sync, cron in instance time zone (UI) |
| `TZ` | `UTC` | instance time zone |
| `GAMESHELF_IGDB_CLIENT_ID`, `GAMESHELF_IGDB_CLIENT_SECRET` | empty | IGDB access through Twitch credentials (UI) |
| `GAMESHELF_STEAMGRIDDB_API_KEY` | empty | optional artwork provider (UI) |
| `GAMESHELF_XBOX_CLIENT_ID` | empty | operator-registered Azure application for the Xbox connector (UI) |
| `GAMESHELF_DEV_FAKE_CONNECTOR` | unset | development builds only: enables a connector serving synthetic libraries |
| `GAMESHELF_LOG` | `info` | log filter, `tracing` syntax |
| `GAMESHELF_LOG_FORMAT` | `json` | `json` or `pretty` |
| `GAMESHELF_METRICS_BIND` | empty (disabled) | address for `/metrics` |

Rules:

- The server refuses to start without a valid secret key and says how to generate one.
- Losing the key makes stored credentials unreadable: accounts move to `needs_reconnect`, library data stays.
- No variable has a default that points to a real host, account or key (Principle I).

Example Compose service (placeholders only):

```yaml
services:
  gameshelf:
    image: ghcr.io/<owner>/gameshelf:<version>
    restart: unless-stopped
    ports:
      - "8080:8080"
    environment:
      GAMESHELF_BASE_URL: "https://gameshelf.example.com"
      GAMESHELF_SECRET_KEY_FILE: /run/secrets/gameshelf_key
      TZ: "Europe/Paris"
    secrets:
      - gameshelf_key
    volumes:
      - gameshelf-data:/data
secrets:
  gameshelf_key:
    file: ./gameshelf_key.txt
volumes:
  gameshelf-data:
```
