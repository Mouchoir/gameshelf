# Feature Specification: Multi-Account Game Library

**Feature Branch**: `001-multi-account-library`

**Created**: 2026-10-15

**Status**: Draft

**Input**: User description: "A self-hosted tool that lists every possible game launcher and gathers my whole game
library into one dashboard. I often have several accounts on the same platform. Goal: when I want to play a game,
I search whether I already own it, and if so it tells me on which account(s). Clean design, library view with
cover art and, under each game, the logo of the platform(s) that have it (a game can be on two platforms) and a
way to tell whether it is on one account or several. Clicking a game opens its detail with the accounts, playtime
and so on. I also want to browse the full collection and sort it. Automatic daily sync of every account to catch
new games, nothing manual. Enriched per-account data where possible (for example the profile level of a
competitive shooter on each account). Several profiles per instance, each able to share its library with the
rest of the family while hiding chosen games or platforms. An installable agent for machines whose launchers only
keep data locally, with a guided setup and full user control. Notifications in the app, on Discord and through
ntfy or Gotify."

## Clarifications

### Session 2026-10-15

- Q: Should the first release include enrichments that need a full login session of each account (for example a
  shooter's profile level and rank)? → A: No. The first release covers platform-level data plus enrichments that
  need only an API key or public data; full-login enrichments become a separate feature with dedicated safeguards.
- Q: In which order are platforms delivered? → A: Core first (Steam with several accounts, then Epic and GOG, then
  Microsoft Store / Xbox with PC Game Pass, then the agent with Battle.net, Ubisoft Connect and publisher
  launchers, then EA app, Amazon, itch.io and Humble); the maintainer's inventory may reorder what follows step 3.
- Q: How does a headless server obtain credentials from platforms that require a browser login? → A: Through a
  paired agent when one is available (it opens the login on the player's machine and forwards the credential),
  otherwise through a link plus a field where the player pastes the returned code.
- Q: Which operating systems must the agent support in the first release? → A: Windows and Linux; macOS later.
- Q: Can an administrator see other profiles' libraries? → A: No. Administrators manage profiles but see other
  libraries only through sharing rules, like any profile, and never see their credentials.
- Q: Can one platform account belong to several profiles? → A: No. Each platform account belongs to exactly one
  profile; other profiles see its games only through sharing.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Know whether I own a game, and on which accounts (Priority: P1)

A player connects their platform accounts, including several accounts on the same platform. gameshelf imports
every library into one collection where each game appears once. The player types part of a title and immediately
sees whether they own it. Each result shows the cover art, the badge of every platform that has it, and a marker
that tells one account from several. Opening the game lists every ownership: platform, account label, and how it
was obtained.

**Why this priority**: This is the reason the product exists. Without it nothing else matters.

**Independent Test**: Connect two accounts of the same platform that share some games, plus one account of a
second platform that has one of those games. Search for the shared title: one result appears, with two platform
badges and a "3 accounts" marker, and its detail lists the three ownerships.

**Acceptance Scenarios**:

1. **Given** two connected accounts on the same platform that both own a game, **When** the player searches its
   title, **Then** exactly one result appears, marked as owned on 2 accounts, and its detail names both accounts.
2. **Given** a game owned on two different platforms, **When** the player views it in the library, **Then** both
   platform badges appear under the cover.
3. **Given** a title the player does not own, **When** they search it, **Then** the app clearly says it is not in
   the library rather than showing an empty page.
4. **Given** a search that matches several games, **When** the player keeps typing, **Then** results narrow as
   they type without pressing a button.

---

### User Story 2 - Browse, sort and filter the whole collection (Priority: P1)

The player opens the library and sees every game as a grid of covers, each with its platform badges and account
marker. They sort by title, recently added, last played or total playtime, and filter by platform, account,
profile, installed state, acquisition type (owned, free claim, subscription, free-to-play) and tags.

**Why this priority**: Searching answers a single question; browsing is how the player rediscovers what they
already have. Both form the minimum useful product.

**Independent Test**: With an imported library, switch sorts and combine two filters (one platform, one account);
the grid and its count update and every visible game matches both filters.

**Acceptance Scenarios**:

1. **Given** an imported library, **When** the player sorts by title, **Then** games appear in alphabetical order
   using the display title, ignoring leading articles such as "The".
2. **Given** filters for one platform and one account, **When** both are active, **Then** only games owned by that
   account on that platform are shown and the result count says how many.
3. **Given** subscription titles exist, **When** the player opens the library with default filters, **Then**
   subscription titles are hidden and a visible toggle reveals them with a "subscription" badge.
4. **Given** the player switches between grid and list views, **When** they return later, **Then** the app
   remembers their last view, sort and filters.

---

### User Story 3 - Unattended daily sync with clear health status (Priority: P2)

Once accounts are connected, gameshelf refreshes every library on a schedule without any action from the player.
New games, removed games and changed playtimes are recorded in an activity feed. Each connected account shows its
health: last successful sync, next sync, and whether its credentials need attention soon.

**Why this priority**: The player explicitly wants nothing manual. Without scheduled sync the library goes stale.

**Independent Test**: Add a game to a connected test account, wait for the scheduled run (or trigger it), and see
the game appear with an "added" entry in the activity feed. Expire a credential and see the account flagged.

**Acceptance Scenarios**:

1. **Given** a connected account, **When** the daily sync runs, **Then** games added since the previous run appear
   in the library and in the activity feed with their date.
2. **Given** a game that disappeared from an account (refund, subscription rotation, family removal), **When** the
   sync runs, **Then** the ownership is marked as no longer available with its date, not silently deleted.
3. **Given** one platform fails during a run, **When** the run finishes, **Then** every other platform still
   synced, the failed account keeps its last known data, and the error is shown on that account.
4. **Given** a credential that will expire within 7 days or has expired, **When** the player opens the app,
   **Then** the account shows a "reconnect" call to action with the exact steps for that platform.
5. **Given** any connected account, **When** the player presses "Sync now" on it, **Then** that account syncs
   immediately and the result is reported.

---

### User Story 4 - Game detail with per-account data (Priority: P2)

Opening a game shows its artwork, description and every ownership grouped by platform and account. For each
ownership the detail shows what the platform exposes: total playtime, last played, achievements progress, install
state, edition, acquisition type and date first seen. Where a game or platform exposes per-account progression
(profile level, rank, characters), the detail shows it per account with the time it was last refreshed. DLC owned
for the game is listed in its detail.

**Why this priority**: It turns "I own it on 5 accounts" into "this is the account to play on".

**Independent Test**: Open a game owned on two accounts with different playtimes; the detail lists both with their
own playtime and last-played date, and the totals add up.

**Acceptance Scenarios**:

1. **Given** a game owned on several accounts, **When** the player opens it, **Then** each account has its own row
   with playtime and last played when the platform provides them, and "not provided by this platform" otherwise.
2. **Given** per-account progression data is available for that game, **When** the detail opens, **Then** each
   account shows its values (for example a profile level) with a "refreshed on" date.
3. **Given** DLC owned for a game, **When** the detail opens, **Then** the DLC is listed under the game and DLC
   never appears as separate tiles in the library grid.

---

### User Story 5 - Agent for launchers that only keep data locally (Priority: P3)

For platforms with no usable remote source, the player installs a small agent on any of their machines. On first
run the agent finds the launchers installed in their default locations, shows what it found and what it would
send, and asks for confirmation. It pairs with the server through a short code displayed in the web app. The
player decides whether it starts with the system, how often it scans, and which launchers it reads. Everything the
agent does is visible and switchable, on the agent and in the web app.

**Why this priority**: It extends coverage to many launchers but the core is useful without it.

**Independent Test**: Install the agent on a machine with two supported launchers, pair it, accept the defaults;
the installed games of both launchers appear in the web app attributed to that machine and those accounts.

**Acceptance Scenarios**:

1. **Given** launchers installed in default locations, **When** the agent runs for the first time, **Then** it
   lists the launchers it found and asks no more than three questions before the first scan.
2. **Given** a launcher in a non-default location, **When** detection misses it, **Then** the player can point the
   agent to its folder and the agent remembers it.
3. **Given** the agent is paired, **When** the player disables one launcher or pauses the agent, **Then** no data
   for that launcher is sent until it is re-enabled, and the web app shows the agent as paused.
4. **Given** an agent that has not reported for longer than its schedule, **When** the player opens the app,
   **Then** the agent shows its last contact time and its data stays visible.
5. **Given** the player revokes an agent in the web app, **When** that agent next connects, **Then** it is refused
   and tells the player how to pair again.

---

### User Story 6 - Family profiles and controlled sharing (Priority: P3)

An instance hosts several profiles, for example the members of a household. Each profile connects its own
accounts. A profile can share its library with chosen other profiles, and can exclude specific games, platforms or
accounts from what it shares. When profile A searches a game, results also show matching games from libraries
shared with A, clearly attributed to their owner ("owned by B").

**Why this priority**: Valuable for households, but a single-person instance works without it.

**Independent Test**: Profile B shares its library with A but excludes one platform and one game. Signed in as A,
search for B's titles: shared ones show "owned by B", excluded ones never appear, including through filters,
counts, detail pages or direct links.

**Acceptance Scenarios**:

1. **Given** a newly created profile, **When** nothing has been configured, **Then** its library is shared with
   nobody.
2. **Given** B shares with A and excludes a platform, **When** A views a game B owns only on that platform,
   **Then** A cannot see that B owns it.
3. **Given** a game B owns on an excluded and a non-excluded platform, **When** A views it, **Then** A sees only the
   non-excluded ownership.
4. **Given** A and B both own a game, **When** A opens it, **Then** A's own accounts are listed first and B's
   shared ownerships follow under B's name.

---

### User Story 7 - Notifications (Priority: P4)

The player chooses which events notify them and where: in the app, on a Discord channel, or through an ntfy or
Gotify server. Events include new games found, games no longer available, accounts needing reconnection, agents
gone silent and repeated sync failures.

**Why this priority**: Sync works without notifications; they mostly save the player from checking the app.

**Independent Test**: Configure a test notification target, trigger a new-game event, and receive one message
that names the game, the platform and the account.

**Acceptance Scenarios**:

1. **Given** a configured target, **When** the player presses "Send test", **Then** a test message arrives or a
   clear error explains why it failed.
2. **Given** a daily sync finds several new games, **When** it finishes, **Then** one grouped message is sent per
   target rather than one message per game.
3. **Given** an account needs reconnection, **When** days pass without action, **Then** the player is reminded at
   most once a day, and never repeatedly within the same day.

---

### User Story 8 - Subscriptions and free-to-play (Priority: P4)

Games available only through a subscription the player holds are marked as such and hidden by default.
Free-to-play games that an account has played or added to its library appear like other games with a
"free-to-play" badge and can be filtered out.

**Why this priority**: Refines answers to "do I have it?" but is not required for the core question.

**Independent Test**: With a subscription enabled for an account, a subscription-only title is hidden by default,
appears with its badge when revealed, and disappears from the library when it leaves the subscription catalog.

**Acceptance Scenarios**:

1. **Given** a title both owned and available through a subscription, **When** shown, **Then** it counts as owned
   and the subscription is mentioned only as extra information in the detail.
2. **Given** a free-to-play game with playtime on an account, **When** the player filters out free-to-play,
   **Then** it disappears from the grid and the count.

---

### User Story 9 - Manual entries and import (Priority: P5)

For platforms with no automatic source, the player adds a game by hand, choosing its platform and account, or
imports a CSV file. Manual entries behave like any other ownership and are marked as manual.

**Why this priority**: Covers the long tail; the product is complete without it for most players.

**Independent Test**: Add a game manually to a platform with no connector; it appears in search with that
platform's badge and a "manual" marker, and survives every sync.

**Acceptance Scenarios**:

1. **Given** a CSV with title, platform and account columns, **When** imported, **Then** each row becomes an
   ownership, rows that cannot be matched are listed for review, and nothing is imported twice on re-import.

---

### Edge Cases

- The same game is owned on two platforms and three accounts: one tile, two badges, a "3 accounts" marker.
- Editions (Standard, Deluxe, Game of the Year) of one game are grouped under that game, each ownership keeping its
  edition. Remasters and re-releases that the platforms sell as separate products stay separate games unless the
  player merges them.
- Automatic matching joins two different games, or splits one game in two: the player merges or splits them and
  the decision survives later syncs.
- A game borrowed through a platform's family sharing is shown as "available through sharing", not as owned.
- The same platform account is connected twice (by one profile, or by two profiles): the app refuses the
  duplicate and says which profile already holds it.
- A platform returns a partial result or hits a rate limit: the run keeps the previous state for what it could not
  read, resumes at the next run and never marks unread games as removed.
- A platform changes its data format: the affected connector reports an error; others are unaffected.
- The agent reports the same launcher account from two machines: ownerships are deduplicated per account and the
  install state is tracked per machine.
- A game has no metadata match: it shows the platform's own title and artwork, or a generated cover with its
  title.
- Very large libraries (tens of thousands of free claims): browsing, search and sync remain usable (see SC-002).
- Titles differ by language across platforms: matching relies on platform identifiers first, then titles.
- A profile is deleted: its accounts, credentials and ownerships are deleted, and the profiles it shared with lose
  access immediately.
- Credentials are revoked on the platform side: the account is flagged at the next sync; data is kept read-only.

## Requirements *(mandatory)*

### Functional Requirements

**Accounts and platforms**

- **FR-001**: The system MUST let a profile connect any number of accounts per platform, each with a label the
  player can edit (defaulting to the account's display name on the platform). Each platform account belongs to
  exactly one profile; other profiles see its games only through sharing (FR-032).
- **FR-002**: The system MUST present, for every supported platform, the connection method, the data it can
  retrieve, and its stability tier (official source, unofficial source, local data through the agent, manual).
- **FR-003**: The system MUST guide each connection step by step, including the browser-based login or the key
  creation the platform requires, and MUST validate the connection before saving it. For platforms that require a
  browser login, a paired agent completes the login on the player's machine and forwards the resulting
  credential when one is available; otherwise the web app offers a login link and a field to paste the returned
  code. The same flow serves reconnections.
- **FR-004**: The system MUST let the player disconnect an account, which deletes its stored credentials and
  optionally its ownership data.
- **FR-005**: The first release MUST support the platforms listed in "Platform coverage" below.

**Library and matching**

- **FR-006**: The system MUST store every ownership with its profile, platform, account, platform game identifier,
  acquisition type (purchase, free claim, subscription, free-to-play, family sharing, manual), edition when known,
  and first-seen date.
- **FR-007**: The system MUST group ownerships of the same game into one game entry, across accounts and
  platforms, without losing any ownership.
- **FR-008**: Matching MUST rely on platform identifiers and cross-reference databases first, and on normalized
  titles only as a fallback.
- **FR-009**: The player MUST be able to merge two game entries or split ownerships out of one, and these decisions
  MUST persist across syncs.
- **FR-010**: DLC and add-ons MUST be attached to their base game and MUST NOT appear as library tiles.

**Search and browsing**

- **FR-011**: The library MUST display games as a grid of covers, each showing the badges of the platforms that
  have it and a marker distinguishing one account from several (with the account count when more than one).
- **FR-012**: Search MUST match partial titles, ignore case, accents and punctuation, and tolerate small typos.
- **FR-013**: Search MUST state explicitly when no owned game matches.
- **FR-014**: The library MUST support sorting by title, date added, last played and total playtime, and
  filtering by platform, account, profile, acquisition type, installed state and tags, in combination.
- **FR-015**: The library MUST remember each player's last view, sort and filters.

**Game detail**

- **FR-016**: The detail view MUST list every ownership visible to the player, grouped by platform and account,
  with playtime, last played, achievements progress, install state, edition and acquisition type when available,
  and an explicit "not provided by this platform" otherwise.
- **FR-017**: The detail view MUST show per-account progression data when an enrichment source exists for that
  game or platform, each value with its source and refresh date. The first release covers platform-level data
  (playtime, last played, achievements, platform profile level) and game-specific enrichments that need only an
  API key or public data (for example ranks from an official game API, public hiscores, public character
  profiles). Enrichments that require a full login session of an account (for example a shooter's profile level
  and rank read from the game's own servers) are out of scope for the first release.
- **FR-018**: The system MUST keep daily playtime snapshots per ownership so that playtime evolution can be shown.

**Sync and scheduling**

- **FR-019**: The system MUST sync every connected account automatically on a configurable schedule, daily by
  default.
- **FR-020**: The system MUST let the player trigger a sync for one account, one platform or everything.
- **FR-021**: A failure in one connector or account MUST NOT stop or roll back the others; the failed account keeps
  its last known data.
- **FR-022**: The system MUST record every sync run (scope, start, end, outcome, counts, error) and show it per
  account.
- **FR-023**: The system MUST detect credentials that have expired or will expire within 7 days and show the exact
  reconnection steps.
- **FR-024**: The system MUST record an activity feed of added games, removed games and account status changes.
- **FR-025**: An ownership that disappears from a platform MUST be marked unavailable with a date, and hidden from
  the default library view, never deleted silently.

**Agent**

- **FR-026**: The agent MUST be installable on Windows and Linux machines in the first release (macOS later) and
  pair with the server through a one-time code shown in the web app.
- **FR-027**: On first run the agent MUST auto-detect supported launchers in their default locations, show what it
  found and what it will send, and require confirmation before sending anything.
- **FR-028**: The agent MUST offer, as opt-in choices, starting with the system and a scan frequency, and MUST let
  the player enable or disable each launcher and pause all activity.
- **FR-029**: The agent's state (paired machines, enabled launchers, last contact, schedule) MUST be visible and
  controllable from the web app, including revoking an agent.
- **FR-030**: The agent MUST only send library, install and playtime data, plus the platform credential produced
  by a login the player explicitly started through it (FR-003). It never reads or sends other credentials or
  unrelated files.

**Profiles and sharing**

- **FR-031**: An instance MUST support several profiles, each with its own login, accounts and library, and one or
  more administrators who manage profiles (create, delete, reset sign-in). Administrators see other profiles'
  libraries only through those profiles' sharing rules, like any other profile, and never see their credentials.
- **FR-032**: Each profile MUST be able to share its library with chosen profiles, and exclude specific games,
  platforms or accounts from that sharing. Sharing is off by default.
- **FR-033**: Shared ownerships MUST appear in search, browsing and detail attributed to their owning profile, and
  excluded items MUST NOT be exposed anywhere (results, counts, filters, detail, direct links, exports).

**Notifications**

- **FR-034**: The system MUST notify in the app and, when configured, through Discord webhooks and ntfy or Gotify
  servers, with per-target choice of events.
- **FR-035**: Notifications MUST be grouped per sync run and reminders for the same issue MUST be sent at most once
  a day.
- **FR-036**: Each notification target MUST offer a "Send test" action.

**Subscriptions and free-to-play**

- **FR-037**: The system MUST mark titles available through a subscription the account holds, hide them by default,
  and update them when the subscription catalog changes.
- **FR-038**: Free-to-play games played or added on an account MUST appear with a free-to-play badge and be
  filterable.

**Manual entries**

- **FR-039**: The player MUST be able to add, edit and delete manual ownerships, and import them from CSV with a
  review of unmatched rows and no duplicates on re-import.

**Security, privacy and administration**

- **FR-040**: Access MUST always require signing in with the instance's own accounts; trusting an identity
  forwarded by a reverse proxy MUST be an explicit administrator choice limited to configured proxy addresses.
- **FR-041**: Platform credentials MUST be stored encrypted with a key provided by the operator, and MUST never be
  displayed back, logged or exported.
- **FR-042**: The system MUST only read from platforms and MUST never perform purchases, redemptions, posts or
  account changes.
- **FR-043**: The system MUST NOT send any data to parties other than the platforms, metadata providers and
  notification targets the operator enabled.
- **FR-044**: The player MUST be able to export their library (titles, platforms, accounts, playtime) as CSV and
  JSON, excluding credentials.
- **FR-045**: Every user-visible text MUST be available in English and French, and the interface MUST follow the
  player's language with a manual override.

### Platform coverage

The first release covers these platforms, each with the tier it can reach. Delivery order: (1) Steam with several
accounts, (2) Epic and GOG, (3) Microsoft Store / Xbox with PC Game Pass, (4) the agent with Battle.net, Ubisoft
Connect and publisher launchers, (5) EA app, Amazon, itch.io and Humble. The maintainer's inventory may reorder
the platforms after step 3.

| Platform | Expected tier |
|---|---|
| Steam (several accounts, family sharing aware) | Official source |
| Epic Games Store | Unofficial source |
| GOG | Unofficial source |
| Microsoft Store / Xbox app, with PC Game Pass | Unofficial source |
| Battle.net | Local data through the agent, plus web session |
| Ubisoft Connect | Local data through the agent |
| EA app | Unofficial source, periodic reconnection |
| Amazon Games / Prime Gaming | Unofficial source |
| itch.io | Official source |
| Humble Bundle | Unofficial source, periodic reconnection |
| Publisher and single-game launchers (Riot, Rockstar, HoYoPlay, Tempo, Battlestate and others) | Local data through the agent |

Consoles (PlayStation, Xbox console, Nintendo) are out of scope for the first release, but nothing in the data
model may prevent adding them later.

### Key Entities

- **Profile**: a person using the instance; has a login, a language, sharing settings, and owns platform accounts.
- **Platform**: a store or launcher (Steam, Epic, a publisher launcher); has a connection method and a stability
  tier.
- **Platform account**: one account on one platform, belonging to one profile; has a label, the platform's account
  identifier, a connection status, credential health and sync history.
- **Credential**: the secret that lets the system read one platform account (key, token, cookie); encrypted, with
  an expiry when known.
- **Game**: the canonical game a player recognizes; has titles, artwork, identifiers on each platform and in
  cross-reference databases, and DLC.
- **Ownership**: the fact that one platform account has access to one game; has acquisition type, edition, first
  seen, availability, playtime, last played, achievements progress, install state per machine.
- **Enrichment value**: a per-account data point for a game or platform (level, rank, characters); has source and
  refresh date.
- **Subscription**: a catalog an account can access (for example PC Game Pass), with the titles it currently
  contains.
- **Sync run**: one execution for one account or connector; has scope, timing, outcome, counts and error.
- **Agent**: a paired machine; has a name, the launchers it reads, its schedule, its last contact and its state.
- **Sharing rule**: what a profile shares and with whom, plus its exclusions by game, platform or account.
- **Notification target**: a destination (in-app, Discord, ntfy, Gotify) and the events it receives.
- **Activity event**: a dated record of a game added or removed, or an account or agent status change.
- **Match decision**: a player's merge or split of game entries, preserved across syncs.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A player can answer "do I own this game, and on which account?" in under 10 seconds from opening the
  app.
- **SC-002**: Search results update while typing, in under 0.2 seconds, for collections of up to 20,000
  ownerships.
- **SC-003**: For every game owned on N accounts, the detail lists exactly N ownerships (verified on a reference
  data set with no exceptions).
- **SC-004**: A game added to any automatically synced account appears in the library within 24 hours without any
  action, or within 5 minutes of a manual sync.
- **SC-005**: Over 30 days, connected platforms need no player action except reconnections the platform forces,
  and each forced reconnection is announced at least 24 hours before data stops updating whenever the platform
  exposes an expiry.
- **SC-006**: When one platform fails during a run, 100% of the other accounts still complete their sync in that
  run.
- **SC-007**: At least 95% of games owned on two platforms appear as a single entry without manual merging, on a
  reference set of 500 cross-platform titles.
- **SC-008**: At least 95% of games display cover art.
- **SC-009**: A new operator reaches a first imported library in under 15 minutes with three platforms to connect.
- **SC-010**: With launchers in default locations, the agent's first run asks at most three questions before its
  first scan.
- **SC-011**: Zero items excluded from sharing are visible to other profiles, verified by automated tests covering
  search, filters, counts, detail, direct links and exports.

## Assumptions

- One instance serves one household and runs on an always-on server with internet access. Remote access, TLS and
  any identity proxy in front of it are the operator's responsibility; the app also works on a plain local
  network.
- The default sync runs once a day at night, local time, and can be changed per instance.
- Unofficial platform sources are acceptable to the operator. Each is labeled with its stability tier, and the
  product never bypasses platform protections.
- Cover art and metadata come from third-party metadata providers using keys the operator supplies; without them,
  the platforms' own artwork is used.
- Editions are grouped under one game; remasters sold as separate products stay separate games by default.
- Ownership history is kept indefinitely; sync run logs are kept for 90 days.
- New profiles are private until their owner shares them.
- Purchase prices, wishlists, deals and launching games from the dashboard are out of scope for the first release.
- Enrichments that need a full login session of an account are out of scope for the first release and will be
  specified as a separate feature.
- English and French ship first; other languages can be added through the localization catalog.
