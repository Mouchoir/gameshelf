# Contract: Connectors and Local Sources

**Applies to**: crate `connectors`, used by the server (remote connectors) and the agent (local sources).
**Related**: [data-model.md](../data-model.md), [research.md](../research.md) sections 1-3 and D6.

A connector reads one platform for one account. A local source reads one launcher's files on the agent's machine.
Both produce the same normalized items, so the sync engine treats them alike. Signatures below are indicative;
the semantics are the contract.

## 1. Descriptor

Every connector exposes a static descriptor, shown in the UI before connection (FR-002).

```rust
pub struct ConnectorDescriptor {
    pub platform: &'static str,          // registry key, e.g. "steam"
    pub display_name: &'static str,      // localization key, not a literal
    pub tier: Tier,                      // Official | Unofficial | Agent | ViaOther | Manual
    pub capabilities: Capabilities,      // library, playtime, last_played, achievements, enrichment, family
    pub auth: &'static [AuthMethod],     // accepted connection methods, in order of preference
    pub default_rate: RateLimit,         // requests per period, burst
    pub multi_account: bool,             // always true unless the platform forbids it
}

pub enum AuthMethod {
    ApiKey { fields: &'static [KeyField], help_url: &'static str },
    BrowserLogin { start: LoginStart, capture: CodeCapture },   // see section 3
    SessionCookie { cookie_names: &'static [&'static str], help_url: &'static str },
    AgentOnly,                                                  // no remote source
}
```

## 2. Operations

```rust
#[trait_variant::make(Send)]
pub trait Connector: Sync {
    fn descriptor(&self) -> &'static ConnectorDescriptor;

    /// Checks a new credential and returns the account it belongs to. Must not write anything anywhere.
    async fn validate(&self, cx: &Context, cred: &Secret<CredentialPayload>) -> Result<AccountIdentity, ConnectorError>;

    /// Refreshes a rolling credential. Returns the new payload; the runtime persists it before any other call.
    async fn refresh(&self, cx: &Context, cred: &Secret<CredentialPayload>) -> Result<Refreshed, ConnectorError>;

    /// Lists the account's library. Paging is internal; the result says whether the listing is complete.
    async fn library(&self, cx: &Context, acct: &AccountRef, cred: &Secret<CredentialPayload>) -> Result<LibrarySnapshot, ConnectorError>;

    /// Optional capabilities. Default implementations return `Unsupported`.
    async fn playtime(&self, cx: &Context, acct: &AccountRef, cred: &Secret<CredentialPayload>) -> Result<Vec<PlaytimeItem>, ConnectorError>;
    async fn achievements(&self, cx: &Context, acct: &AccountRef, cred: &Secret<CredentialPayload>, ids: &[PlatformGameId]) -> Result<Vec<AchievementItem>, ConnectorError>;
    async fn enrich(&self, cx: &Context, acct: &AccountRef, cred: &Secret<CredentialPayload>) -> Result<Vec<EnrichmentItem>, ConnectorError>;
}
```

`Context` carries the shared HTTP client (with the platform's rate limiter and retry policy already applied), the
clock, a cancellation token and a structured logger that redacts secrets.

### Normalized items

```rust
pub struct LibrarySnapshot {
    pub complete: bool,                 // true only if every page was read without error
    pub items: Vec<LibraryItem>,
}

pub struct LibraryItem {
    pub platform_game_id: String,       // stable id on that platform
    pub title: String,
    pub kind: ProductKind,              // Game | Dlc | Bundle | Tool | Unknown
    pub parent_platform_game_id: Option<String>,
    pub edition: Option<String>,
    pub acquisition: Acquisition,       // Purchase | FreeClaim | Subscription | FreeToPlay | FamilyShared
    pub playtime_minutes: Option<u32>,  // None = not provided (never 0 as a placeholder)
    pub last_played_at: Option<OffsetDateTime>,
    pub cross_ids: Vec<(IdSource, String)>, // ids on other databases when the platform gives them
    pub artwork: Artwork,               // URLs offered by the platform, optional
    pub raw: serde_json::Value,         // trimmed payload kept for re-matching; no personal data
}
```

## 3. Browser logins

For `BrowserLogin`, the connector describes how to start and how to capture the result; it never runs a
browser itself.

- `LoginStart` builds the authorization URL (client id, scopes, PKCE challenge, state).
- `CodeCapture` says where the result appears: `RedirectQuery { param }` (code in the redirect URL),
  `JsonPage { field }` (a page that prints JSON), or `LocalCallback { port }` (a loopback redirect).
- With an agent (clarification 2026-10-15), the agent opens the URL in an embedded webview, captures the code
  according to `CodeCapture`, and sends only the authorization code to the server (see
  [agent-api.md](./agent-api.md)). Without an agent, the web app shows the URL and a field where the player pastes
  the redirect URL or code. In both cases the server performs the code exchange, so long-lived tokens never live
  on the agent.

## 4. Errors

```rust
pub enum ConnectorError {
    AuthExpired,                        // refresh failed or token rejected: account -> needs_reconnect
    AuthInvalid,                        // wrong key or account mismatch during validate
    RateLimited { retry_after: Option<Duration> },
    Transient(String),                  // network, 5xx: retried with backoff inside the run
    Permanent(String),                  // 4xx other than auth, unsupported region...
    Parse { context: String },          // unexpected payload shape
    Unsupported,                        // capability not implemented
}
```

Rules:

- Messages MUST NOT contain secrets, cookies, tokens or full URLs with query strings.
- `Transient` and `RateLimited` are retried inside the run up to the policy limit; after that the run ends
  `partial` with `complete = false`.
- `Parse` on one item skips that item and marks the listing incomplete; it never aborts other items.

## 5. Sync semantics

- The engine applies a snapshot as a diff in one transaction: new items create products and ownerships, known items
  update `last_seen_at` and playtime, and only a `complete = true` snapshot may set `unavailable_since` on
  ownerships it no longer contains (FR-025, spec edge case on partial results).
- Connectors are read-only (Principle IV): no request may change platform state. Requests that exist only to
  keep a session alive are allowed when the platform's own client does the same.
- A connector failure never touches other accounts' data (FR-021).

## 6. Local sources (agent)

```rust
pub trait LocalSource {
    fn key(&self) -> &'static str;                         // "steam-local", "battlenet-local", ...
    fn platform(&self) -> &'static str;
    fn detect(&self, env: &HostEnv) -> Vec<DetectedLocation>; // default locations per OS
    fn read(&self, loc: &DetectedLocation) -> Result<LocalReport, LocalError>;
}
```

`LocalReport` lists accounts seen on the machine (when the launcher stores them) and, per account or per machine,
`LibraryItem`s with `installed = true` plus playtime and last played when the files hold them. First sources:
Steam (library folders, app manifests, per-user local config with playtime), Epic (install manifests), GOG Galaxy
(local database), Battle.net (config file and product database), Ubisoft Connect (install registry and
configuration cache), Xbox and Microsoft Store packages (Windows), Riot Client, Rockstar, Tempo and other
publisher launchers (install registry and known folders), Heroic and Lutris on Linux.

Local sources MUST only open the files they declare, never send file contents verbatim, and never read browser
profiles, credential stores or save games.

## 7. Contract tests

Every connector and local source ships tests that:

1. Replay recorded responses or files from `tests/fixtures/<platform>/` through a mock server or a temporary
   directory; no network access.
2. Snapshot the normalized output with `insta`.
3. Cover: a full listing, a multi-page listing, an expired credential, a rate limit with `Retry-After`, an
   unknown field, a malformed item, and an empty library.

Fixtures MUST be anonymized before commit (Principle I): account ids, names, emails, avatars, paths and tokens are
replaced with consistent fake values, and a CI check rejects fixtures containing patterns that look like real
identifiers or secrets.
