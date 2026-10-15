-- gameshelf initial schema (specs/001-multi-account-library/data-model.md).
-- Identifiers are hyphenated UUIDv7 text, timestamps RFC 3339 UTC text, booleans 0/1.

CREATE TABLE profiles (
    id            TEXT PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE CHECK (length(username) BETWEEN 3 AND 32),
    display_name  TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 64),
    email         TEXT UNIQUE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL CHECK (role IN ('admin', 'member')),
    locale        TEXT NOT NULL DEFAULT 'auto' CHECK (locale IN ('auto', 'en', 'fr')),
    disabled_at   TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE sessions (
    id         TEXT PRIMARY KEY,
    profile_id TEXT REFERENCES profiles (id) ON DELETE CASCADE,
    data       BLOB NOT NULL,
    expires_at TEXT NOT NULL
);
CREATE INDEX sessions_expiry ON sessions (expires_at);
CREATE INDEX sessions_profile ON sessions (profile_id);

CREATE TABLE preferences (
    profile_id TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    key        TEXT NOT NULL,
    value      TEXT NOT NULL,
    PRIMARY KEY (profile_id, key)
);

CREATE TABLE platform_accounts (
    id                   TEXT PRIMARY KEY,
    profile_id           TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    platform             TEXT NOT NULL,
    external_account_id  TEXT NOT NULL,
    label                TEXT NOT NULL CHECK (length(label) BETWEEN 1 AND 64),
    display_name         TEXT,
    status               TEXT NOT NULL CHECK (status IN ('pending', 'active', 'needs_reconnect', 'degraded', 'disabled')),
    sync_enabled         INTEGER NOT NULL DEFAULT 1,
    schedule             TEXT,
    subscription_hint    TEXT,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_sync_at         TEXT,
    last_success_at      TEXT,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    UNIQUE (platform, external_account_id)
);
CREATE INDEX accounts_profile ON platform_accounts (profile_id);

CREATE TABLE credentials (
    id           TEXT PRIMARY KEY,
    account_id   TEXT NOT NULL UNIQUE REFERENCES platform_accounts (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('api_key', 'oauth', 'session_cookie', 'token')),
    sealed       BLOB NOT NULL,
    nonce        BLOB NOT NULL,
    key_id       TEXT NOT NULL,
    expires_at   TEXT,
    refreshed_at TEXT,
    created_at   TEXT NOT NULL
);

CREATE TABLE assets (
    id         TEXT PRIMARY KEY,
    sha256     TEXT NOT NULL UNIQUE,
    variant    TEXT NOT NULL CHECK (variant IN ('original', 'grid', 'thumb')),
    mime       TEXT NOT NULL,
    width      INTEGER,
    height     INTEGER,
    source     TEXT NOT NULL CHECK (source IN ('igdb', 'steamgriddb', 'platform', 'generated', 'manual')),
    source_url TEXT,
    fetched_at TEXT NOT NULL
);

CREATE TABLE games (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    sort_title      TEXT NOT NULL,
    search_key      TEXT NOT NULL,
    release_year    INTEGER,
    kind            TEXT NOT NULL CHECK (kind IN ('game', 'dlc', 'tool')),
    parent_game_id  TEXT REFERENCES games (id) ON DELETE SET NULL,
    description     TEXT,
    cover_asset_id  TEXT REFERENCES assets (id) ON DELETE SET NULL,
    hero_asset_id   TEXT REFERENCES assets (id) ON DELETE SET NULL,
    logo_asset_id   TEXT REFERENCES assets (id) ON DELETE SET NULL,
    metadata_source TEXT CHECK (metadata_source IN ('igdb', 'wikidata', 'platform', 'manual')),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE INDEX games_search_key ON games (search_key);
CREATE INDEX games_parent ON games (parent_game_id);

CREATE TABLE game_external_ids (
    game_id     TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    source      TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY (source, external_id)
);
CREATE INDEX game_external_ids_game ON game_external_ids (game_id);

CREATE TABLE platform_products (
    id                      TEXT PRIMARY KEY,
    platform                TEXT NOT NULL,
    platform_game_id        TEXT NOT NULL,
    title                   TEXT NOT NULL,
    kind                    TEXT NOT NULL CHECK (kind IN ('game', 'dlc', 'bundle', 'tool', 'unknown')),
    parent_platform_game_id TEXT,
    game_id                 TEXT REFERENCES games (id) ON DELETE SET NULL,
    edition                 TEXT,
    platform_artwork        TEXT,
    raw                     TEXT,
    updated_at              TEXT NOT NULL,
    UNIQUE (platform, platform_game_id)
);
CREATE INDEX products_game ON platform_products (game_id);

CREATE TABLE ownerships (
    id                    TEXT PRIMARY KEY,
    account_id            TEXT NOT NULL REFERENCES platform_accounts (id) ON DELETE CASCADE,
    product_id            TEXT NOT NULL REFERENCES platform_products (id) ON DELETE CASCADE,
    acquisition           TEXT NOT NULL CHECK (acquisition IN ('purchase', 'free_claim', 'subscription', 'free_to_play', 'family_shared', 'manual')),
    first_seen_at         TEXT NOT NULL,
    last_seen_at          TEXT NOT NULL,
    unavailable_since     TEXT,
    playtime_minutes      INTEGER,
    last_played_at        TEXT,
    achievements_unlocked INTEGER,
    achievements_total    INTEGER,
    source                TEXT NOT NULL CHECK (source IN ('connector', 'agent', 'manual', 'import')),
    note                  TEXT,
    UNIQUE (account_id, product_id)
);
CREATE INDEX ownerships_product ON ownerships (product_id);

CREATE TABLE playtime_snapshots (
    ownership_id     TEXT NOT NULL REFERENCES ownerships (id) ON DELETE CASCADE,
    day              TEXT NOT NULL,
    playtime_minutes INTEGER NOT NULL,
    PRIMARY KEY (ownership_id, day)
);

CREATE TABLE agents (
    id                 TEXT PRIMARY KEY,
    profile_id         TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    name               TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 64),
    os                 TEXT NOT NULL CHECK (os IN ('windows', 'linux')),
    version            TEXT NOT NULL,
    token_hash         TEXT NOT NULL UNIQUE,
    status             TEXT NOT NULL CHECK (status IN ('active', 'paused', 'revoked')),
    sources            TEXT NOT NULL DEFAULT '{}',
    schedule           TEXT NOT NULL DEFAULT '0 */6 * * *',
    send_install_paths INTEGER NOT NULL DEFAULT 0,
    autostart          INTEGER NOT NULL DEFAULT 0,
    paired_at          TEXT NOT NULL,
    last_seen_at       TEXT
);

CREATE TABLE pairing_codes (
    code_hash  TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    expires_at TEXT NOT NULL,
    used_at    TEXT
);

CREATE TABLE agent_reports (
    id           TEXT PRIMARY KEY,
    agent_id     TEXT NOT NULL REFERENCES agents (id) ON DELETE CASCADE,
    source       TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    received_at  TEXT NOT NULL,
    summary      TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX agent_reports_latest ON agent_reports (agent_id, source, received_at DESC);

CREATE TABLE installs (
    id                  TEXT PRIMARY KEY,
    agent_id            TEXT NOT NULL REFERENCES agents (id) ON DELETE CASCADE,
    platform            TEXT NOT NULL,
    platform_game_id    TEXT NOT NULL,
    account_external_id TEXT,
    install_path        TEXT,
    size_bytes          INTEGER,
    last_seen_at        TEXT NOT NULL,
    UNIQUE (agent_id, platform, platform_game_id)
);

-- Browser logins in progress, through the paste-code flow or an agent (contracts/agent-api.md).
CREATE TABLE login_requests (
    id            TEXT PRIMARY KEY,
    profile_id    TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    agent_id      TEXT REFERENCES agents (id) ON DELETE CASCADE,
    account_id    TEXT REFERENCES platform_accounts (id) ON DELETE CASCADE,
    platform      TEXT NOT NULL,
    sealed_state  BLOB NOT NULL,
    nonce         BLOB NOT NULL,
    key_id        TEXT NOT NULL,
    authorize_url TEXT NOT NULL,
    capture       TEXT NOT NULL,
    status        TEXT NOT NULL CHECK (status IN ('pending', 'completed', 'declined', 'expired')),
    created_at    TEXT NOT NULL,
    expires_at    TEXT NOT NULL
);

CREATE TABLE enrichment_values (
    id           TEXT PRIMARY KEY,
    account_id   TEXT NOT NULL REFERENCES platform_accounts (id) ON DELETE CASCADE,
    game_id      TEXT REFERENCES games (id) ON DELETE CASCADE,
    provider     TEXT NOT NULL,
    key          TEXT NOT NULL,
    value        TEXT NOT NULL,
    unit         TEXT,
    refreshed_at TEXT NOT NULL
);
CREATE UNIQUE INDEX enrichment_unique ON enrichment_values (account_id, coalesce(game_id, ''), provider, key);

CREATE TABLE subscriptions (
    id                TEXT PRIMARY KEY,
    provider          TEXT NOT NULL UNIQUE,
    name              TEXT NOT NULL,
    catalog_synced_at TEXT
);

CREATE TABLE subscription_items (
    subscription_id  TEXT NOT NULL REFERENCES subscriptions (id) ON DELETE CASCADE,
    platform         TEXT NOT NULL,
    platform_game_id TEXT NOT NULL,
    added_at         TEXT NOT NULL,
    removed_at       TEXT,
    PRIMARY KEY (subscription_id, platform, platform_game_id)
);

CREATE TABLE account_subscriptions (
    account_id      TEXT NOT NULL REFERENCES platform_accounts (id) ON DELETE CASCADE,
    subscription_id TEXT NOT NULL REFERENCES subscriptions (id) ON DELETE CASCADE,
    origin          TEXT NOT NULL CHECK (origin IN ('detected', 'declared')),
    active          INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (account_id, subscription_id)
);

CREATE TABLE match_decisions (
    id             TEXT PRIMARY KEY,
    kind           TEXT NOT NULL CHECK (kind IN ('merge', 'split')),
    product_id     TEXT NOT NULL REFERENCES platform_products (id) ON DELETE CASCADE,
    target_game_id TEXT REFERENCES games (id) ON DELETE CASCADE,
    created_by     TEXT REFERENCES profiles (id) ON DELETE SET NULL,
    created_at     TEXT NOT NULL
);

CREATE TABLE tags (
    id         TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    name       TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 40),
    UNIQUE (profile_id, name)
);

CREATE TABLE game_tags (
    profile_id TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    game_id    TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    tag_id     TEXT NOT NULL REFERENCES tags (id) ON DELETE CASCADE,
    PRIMARY KEY (profile_id, game_id, tag_id)
);

CREATE TABLE sharing_rules (
    owner_profile_id    TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    audience_profile_id TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    created_at          TEXT NOT NULL,
    PRIMARY KEY (owner_profile_id, audience_profile_id),
    CHECK (owner_profile_id <> audience_profile_id)
);

CREATE TABLE sharing_exclusions (
    id                  TEXT PRIMARY KEY,
    owner_profile_id    TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    audience_profile_id TEXT REFERENCES profiles (id) ON DELETE CASCADE,
    kind                TEXT NOT NULL CHECK (kind IN ('game', 'platform', 'account')),
    ref                 TEXT NOT NULL
);
CREATE INDEX sharing_exclusions_owner ON sharing_exclusions (owner_profile_id);

CREATE TABLE sync_runs (
    id               TEXT PRIMARY KEY,
    account_id       TEXT REFERENCES platform_accounts (id) ON DELETE CASCADE,
    platform         TEXT NOT NULL,
    capability       TEXT NOT NULL CHECK (capability IN ('library', 'playtime', 'achievements', 'enrichment', 'catalog', 'metadata')),
    trigger          TEXT NOT NULL CHECK (trigger IN ('schedule', 'manual', 'agent', 'startup')),
    status           TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'partial', 'failed', 'skipped')),
    complete_listing INTEGER NOT NULL DEFAULT 0,
    added            INTEGER NOT NULL DEFAULT 0,
    removed          INTEGER NOT NULL DEFAULT 0,
    updated          INTEGER NOT NULL DEFAULT 0,
    error_kind       TEXT,
    error_message    TEXT CHECK (error_message IS NULL OR length(error_message) <= 500),
    started_at       TEXT NOT NULL,
    finished_at      TEXT
);
CREATE INDEX sync_runs_account ON sync_runs (account_id, started_at DESC);

CREATE TABLE jobs (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    account_id   TEXT REFERENCES platform_accounts (id) ON DELETE CASCADE,
    cron         TEXT NOT NULL,
    next_run_at  TEXT NOT NULL,
    locked_until TEXT,
    enabled      INTEGER NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX jobs_unique ON jobs (kind, coalesce(account_id, ''));

CREATE TABLE activity_events (
    id         TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('game_added', 'game_unavailable', 'account_needs_reconnect', 'account_degraded', 'agent_silent', 'sync_failed')),
    game_id    TEXT REFERENCES games (id) ON DELETE SET NULL,
    account_id TEXT REFERENCES platform_accounts (id) ON DELETE SET NULL,
    payload    TEXT NOT NULL DEFAULT '{}',
    run_id     TEXT,
    created_at TEXT NOT NULL,
    read_at    TEXT
);
CREATE INDEX activity_profile ON activity_events (profile_id, created_at DESC);

CREATE TABLE notification_targets (
    id            TEXT PRIMARY KEY,
    profile_id    TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    kind          TEXT NOT NULL CHECK (kind IN ('discord', 'ntfy', 'gotify')),
    name          TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 64),
    sealed_config BLOB NOT NULL,
    nonce         BLOB NOT NULL,
    key_id        TEXT NOT NULL,
    events        TEXT NOT NULL DEFAULT '[]',
    enabled       INTEGER NOT NULL DEFAULT 1,
    last_sent_at  TEXT,
    last_error    TEXT
);

CREATE TABLE reminder_ledger (
    target_id    TEXT NOT NULL REFERENCES notification_targets (id) ON DELETE CASCADE,
    issue_key    TEXT NOT NULL,
    last_sent_on TEXT NOT NULL,
    PRIMARY KEY (target_id, issue_key)
);

CREATE TABLE instance_settings (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL,
    sealed INTEGER NOT NULL DEFAULT 0
);
