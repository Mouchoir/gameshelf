# Research: Platform Sources, Enrichment and Identity

**Feature**: [spec.md](./spec.md) | **Date**: 2026-10-15 | **Status**: pre-plan input, extended during `/speckit-plan`

This document records how each platform's library can be read, what can be enriched, and how games are matched
across platforms. Claims that could not be confirmed against documentation, source code or a live call are marked
*(unverified)* and MUST be re-checked when the connector is built.

Tiers used below, matching Constitution Principle V:

- **official**: public, documented API.
- **unofficial**: API used by the platform's own client or by maintained open-source clients.
- **agent**: no usable remote source; the agent reads the launcher's local data.
- **via other**: ownership is visible on another platform (redeemed keys, linked stores).
- **manual**: no automatic source.

## 1. PC stores and general launchers

| Platform | Owned-games source | Auth and headless viability | Playtime / achievements | Reference implementations |
|---|---|---|---|---|
| Steam (official) | `IPlayerService/GetOwnedGames/v1` (`include_appinfo`, `include_played_free_games`, `include_free_sub`, `skip_unvetted_apps=false`, `include_family_licenses`) | Static Web API key, no expiry; requires an account that has spent at least 5 USD. Private profiles: the key must belong to the queried account, so store one account id + key pair per private account (Playnite does this). Terms: 100,000 calls/day, key kept confidential. | Per account: `playtime_forever`, per-OS playtime, `rtime_last_played`. Achievements: `ISteamUserStats/GetPlayerAchievements`, `GetSchemaForGame`. Level: `IPlayerService/GetSteamLevel`, `GetBadges`. | Playnite SteamLibrary (`SteamServiceAggregator.cs` merges accounts by appid and loses provenance, which gameshelf must not do); SteamDatabase/SteamTracking |
| Steam family sharing | `IFamilyGroupsService/GetSharedLibraryApps` (undocumented; owner field *(unverified)*) | Web access token (about 1 day) from a refresh token (about 200 days) *(unverified)* | n/a | Playnite FamilyGroupsService |
| Epic Games Store (unofficial) | `library-service.live.use1a.on.epicgames.com/library/api/public/items?includeMetadata=true` (cursor paging); catalog `bulk/items` for metadata | OAuth `eg1` with the launcher client; one interactive login per account (authorization code), then refresh. Refresh lifetime returned in `refresh_expires`, about 23 days for the launcher client *(unverified)*; a daily job keeps it alive. Persist the new refresh token after each refresh. | Playtime: `/library/api/public/playtime/account/{id}/all`. Achievements: launcher GraphQL. | legendary (GPL-3.0), egs-api-rs (Rust, MIT), Heroic, Playnite EpicLibrary |
| GOG (unofficial) | `galaxy-library.gog.com/users/{uid}/releases`; `embed.gog.com/user/data/games` | Login once in a browser (CAPTCHA possible), exchange the code at `auth.gog.com/token` with the public Galaxy client; access token 1 h; refresh lifetime *(unverified)*. | Playtime via `gog.com/u/{user}/games/stats` (cookie session confirmed, bearer *(unverified)*). Achievements: `gameplay.gog.com`. | heroic-gogdl, comet (Rust), gogapidocs, Playnite GogLibrary |
| Microsoft Store / Xbox app (unofficial) | Xbox titlehub `titlehistory` (played titles only, with `gamePass` flag); unplayed games come from locally installed packages via the agent. Store entitlements are partner-only. | Own Azure app registration (personal accounts), MSA refresh token 90 days, replaced on each use; daily sync keeps it alive. Revoked on password change. | Last played from titlehub; `MinutesPlayed` via userstats (often empty); achievements and gamerscore in titlehub. | xbox-webapi-python (MIT), Playnite XboxLibrary |
| Ubisoft Connect (agent; web fragile) | Local cache `cache/configuration/configurations` + registry installs (last logged-in account only). Web GraphQL `public-ubiservices.ubi.com/v1/profiles/me/uplay/graphql` exists. | Ticket + remember-me ticket captured from a browser login; lifetime *(unverified)*; heavy per-IP throttling; password login reported broken in 2026. Do not plan on headless months. | Playtime via `statscard` (heuristic, partial). | galaxy-integration-uplay, Lutris `ubisoft.py`, Playnite UplayLibrary |
| EA app (unofficial, periodic reconnection) | GraphQL `service-aggregation-layer.juno.ea.com/graphql` (`getPreloadedOwnedGames`, legacy entitlements) | OAuth code + PKCE (JUNO client) with refresh, per-user token storage; lifetime *(unverified)*. Origin closed in April 2025. | `GetGamePlayTimes` returns total seconds and last session; achievements query exists. | Maxima (Rust, GPL-3.0, pre-alpha), Lutris `ea_app.py` |
| Battle.net (agent + web session) | `account.battle.net/api/games-and-subs`, `/api/classic-games` (session cookies); local `product.db` and `Battle.net.config` (installs, last played per product code) | Cookie session with silent refresh, no documented lifetime. Blizzard user OAuth tokens last about 24 h without refresh token, so account-wide official data is not headless-viable. | No playtime endpoint; last played from local config. WoW public character profiles via client-credentials tokens. | Playnite BattleNetLibrary, galaxy-integration-battlenet |
| Amazon Games / Prime Gaming (unofficial) | `gaming.amazon.com/api/distribution/entitlements` (`GetEntitlements`, paged) | Device registration with PKCE, refresh at `api.amazon.com/auth/token`; long-lived *(unverified)*. | none | nile (GPL-3.0), Playnite AmazonGamesLibrary |
| itch.io (official) | `api.itch.io/profile/owned-keys?page=N` | User API key, static, one per account. Fully headless. | none | Lutris `itchio.py` |
| Humble Bundle (unofficial, periodic reconnection) | `/api/v1/user/order` then `/api/v1/orders?all_tpkds=true&gamekeys=` (keys and DRM-free) | Session cookie `_simpleauth_sess` copied from a browser; lifetime *(unverified)*. Humble restricts third-party clients. | none; reveals unredeemed store keys | humble-cli (Go, MIT), Lutris, Playnite HumbleLibrary |
| IndieGala, Game Jolt, ZOOM Platform | Undocumented user-library endpoints | Session cookies or tokens, fragile | none | FreeCarnival, Lutris |
| Rockstar Games Launcher, Meta Quest Link, Viveport, Legacy Games | No ownership API | Installed games only, through the agent | none | Playnite RockstarLibrary, community scanners |
| Key resellers (Instant Gaming, Fanatical, Green Man Gaming, Gamesplanet, DLGamer) | No library API | Redeemed keys appear on the target platform | n/a | n/a |

Retired, for reference: Bethesda.net Launcher (2022), Twitch app (2022), Kartridge (2023), 2K Launcher (2024),
Origin (April 2025), Robot Cache (closure announced April 2026).

## 2. Publisher and single-game launchers

Most of these serve free-to-play games, where "ownership" means an account exists or has played. Many of these
games also exist on Steam or Epic, sometimes with a separate account. The agent detects installs; enrichment comes
from game APIs where they exist.

| Launcher | Games | Detection | Enrichment and auth | Headless |
|---|---|---|---|---|
| Riot Client | LoL, Valorant, TFT, 2XKO | `RiotClientInstalls.json`, product settings | Official API: account-v1, LoL/TFT league and match (personal key). Valorant: no personal keys, production key required; unofficial HenrikDev API (rate-limited). | LoL/TFT yes; Valorant fragile |
| Battle.net games | WoW, Diablo IV, Overwatch, Hearthstone | see above | WoW profile API (public characters). No official Diablo IV or Overwatch API. | public lookups yes |
| Minecraft Launcher | Java, Bedrock | local launcher files | MS OAuth -> Xbox Live -> XSTS -> Minecraft token -> `/entitlements/mcstore` | yes with MS refresh token |
| HoYoPlay | Genshin, Star Rail, ZZZ (ZZZ also on Steam since June 2026) | install folders | HoYoLAB game records via cookie (months, *unverified*); Enka.Network public showcases | until cookie expires |
| Level Infinite / Garena | Delta Force, Arena Breakout: Infinite | Steam manifests or launcher install | no stats API | n/a |
| Wargaming Game Center | WoT, WoWS | install folders | Official public API by nickname (application id); private data via OpenID (2 weeks, extendable) | yes |
| Jagex Launcher | OSRS, RS3 | launcher files | Public hiscores by display name, no auth | yes |
| ArenaNet | Guild Wars 2 | install folder | Official API v2 with account key: `/v2/account` (age, expansions), progression | yes |
| Grinding Gear Games | Path of Exile 1 and 2 (also Steam, Epic) | install folder | Official OAuth API, but new applications are not being accepted | only with an existing app |
| CCP | EVE Online | install folder | ESI via SSO, long-lived refresh token | yes |
| Frontier | Elite Dangerous | local journals (same folder for every store) | CAPI OAuth (rotating refresh) | fragile |
| Bungie | Destiny 2, Marathon (Steam) | Steam | Bungie.net API key + OAuth | yes |
| Krafton | PUBG (Steam) | Steam | Official API key, 10 requests/minute | yes |
| Nexon | MapleStory, The First Descendant | launcher | Region-specific Open API key | yes |
| Square Enix | FFXIV | install folder | Lodestone scraping (unofficial) | fragile |
| Tempo | The Bazaar (also Steam since August 2025) | launcher install | none | n/a |
| Battlestate Games | Escape from Tarkov, Arena (Tarkov also on Steam since 1.0) | launcher install | none | n/a |
| Others with no API found | Rockstar, Kuro (Wuthering Waves), Gryphline (Arknights: Endfield), Gaijin (War Thunder), RSI (Star Citizen), Hytale, Pearl Abyss, Albion, Warframe, Ankama (Dofus, Wakfu), Gameforge, NCSoft PURPLE, Smilegate STOVE, Plarium Play, Arc, Embark, Paradox Launcher | agent | none, or undocumented public profile endpoints | n/a |

Full-login enrichments, out of scope for the first release (spec FR-017): a game's own coordinator data such as a
shooter's profile level, XP and ranks (requires logging each account in with a refresh token that grants a full
client session).

## 3. Subscriptions, keys and sharing

| Item | Detectable | How |
|---|---|---|
| PC Game Pass / Ultimate | yes (unofficial) | Public catalog lists `catalog.gamepass.com/sigls/v2?id=<list id>` resolved through `displaycatalog.mp.microsoft.com/v7.0/products?bigIds=`, joined to titlehub by title id or package family name; titlehub `gamePass.isGamePass` as a second signal. |
| EA Play | partly | Included in the Game Pass Ultimate catalog lists. |
| Ubisoft+ | no | manual |
| Prime Gaming / Luna | via other | Claimed games land on Amazon, Epic or GOG. |
| Humble Choice and unredeemed keys | yes (unofficial) | Humble order API. |
| Epic weekly free games | catalog only | `freeGamesPromotions` endpoint (no claim status). |
| Steam family sharing | partly | See section 1. |

Consoles (out of scope for the first release): PlayStation via the unofficial psn-api flow (NPSSO, refresh token
about 2 months, played games with playtime, purchased PS4/PS5 games, trophies); Xbox console through the same
Microsoft account as PC; Nintendo has no API (manual or CSV).

## 4. Metadata, artwork and identity

| Source | Use | Notes |
|---|---|---|
| IGDB | Canonical game id; `external_games` maps store ids (Steam, GOG, Microsoft, Epic, itch, Xbox, PlayStation and more); covers `t_cover_big_2x` | Twitch client credentials, 4 requests/s; operator-supplied keys. |
| SteamGridDB | Optional 600x900 grids, heroes, logos, icons; lookups by Steam, Origin, Epic, Battle.net, Ubisoft ids, search for the rest | Operator-supplied key; user-uploaded images, filter nsfw/humor. |
| Wikidata | Fallback cross-ids (Steam P1733, GOG P2725, Epic P6278, IGDB P5794, Microsoft Store P5885, PlayStation P12332, SteamGridDB P12561) | CC0; GOG/Epic/IGDB values are slugs; notable games only. |
| IsThereAnyDeal | Optional shop-id cross-check | API key, 1000 requests / 5 min. |
| PCGamingWiki | Optional store-id links | Since August 2026 Cargo queries require a bot login. |
| HowLongToBeat | Optional, failure-tolerant | No official API; scrapers break often. |

**Matching strategy**: platform identifiers first, resolved through IGDB `external_games` in cached batches; then
Wikidata; then normalized title + year + platform. Keep a persistent merge/split override table (spec FR-009).
Group editions under one game with per-ownership edition; keep remasters separate by default.

**Artwork strategy**: IGDB cover by default, SteamGridDB grid when available and allowed, platform artwork last
(Steam CDN `library_600x900`, Xbox titlehub images). Cache locally and pin the chosen image per game.

**Platform logos**: Simple Icons (CC0 files, trademarks remain with owners; Xbox and Nintendo are absent), Font
Awesome Free brands, or official brand kits (Steam brand guidelines; Xbox via Microsoft's brand team). Nominative
use only (constitution, Licensing section).

## Sources

- Steam: https://partner.steamgames.com/doc/webapi/IPlayerService, https://steamcommunity.com/dev/apiterms,
  https://raw.githubusercontent.com/SteamDatabase/SteamTracking/master/API/IPlayerService.json,
  https://github.com/JosefNemec/PlayniteExtensions/tree/master/source/Libraries,
  https://steamapi.xpaw.me/IFamilyGroupsService
- Epic: https://raw.githubusercontent.com/derrod/legendary/master/legendary/api/egs.py,
  https://github.com/AchetaGames/egs-api-rs
- GOG: https://gogapidocs.readthedocs.io/en/latest/, https://github.com/Heroic-Games-Launcher/heroic-gogdl,
  https://github.com/imLinguin/comet
- Ubisoft: https://github.com/melcom-creations/galaxy-integration-uplay, https://webservices.openplanet.dev/auth/ubi
- EA: https://github.com/ArmchairDevelopers/Maxima, https://raw.githubusercontent.com/lutris/lutris/master/lutris/services/ea_app.py
- Battle.net: https://github.com/melcom-creations/galaxy-integration-battlenet,
  https://us.forums.blizzard.com/en/blizzard/t/oauth-api-refresh-token/559
- Xbox: https://github.com/OpenXbox/xbox-webapi-python, https://learn.microsoft.com/en-us/entra/identity-platform/refresh-tokens,
  https://xbl.io/pricing
- Amazon: https://github.com/imLinguin/nile
- itch.io, Humble: https://itch.io/docs/api/serverside, https://github.com/smbl64/humble-cli
- Riot: https://developer.riotgames.com/docs/lol, https://developer.riotgames.com/docs/valorant,
  https://github.com/Henrik-3/unofficial-valorant-api
- Game APIs: https://wiki.guildwars2.com/wiki/API:2/account, https://www.pathofexile.com/developer/docs,
  https://developers.eveonline.com/docs/services/sso/, https://developers.wargaming.net/,
  https://runescape.wiki/w/Application_programming_interface, https://documentation.pubg.com/en/rate-limits.html,
  https://hosting.zaonce.net/docs/oauth2/instructions.html,
  https://minecraft.wiki/w/Minecraft_Wiki:Projects/wiki.vg_merge/Microsoft_Authentication_Scheme
- Coordinator data (deferred): https://github.com/DoctorMcKay/node-globaloffensive,
  https://github.com/SteamDatabase/Protobufs/blob/master/csgo/cstrike15_gcmessages.proto
- Subscriptions: https://catalog.gamepass.com/sigls/v2?id=b8900d09-a491-44cc-916e-32b5acae621b&language=en-US&market=US,
  https://store-site-backend-static.ak.epicgames.com/freeGamesPromotions?locale=en-US&country=US&allowCountries=US
- Consoles: https://github.com/achievements-app/psn-api, https://github.com/samuelthomas2774/nxapi
- Metadata: https://api-docs.igdb.com/, https://www.steamgriddb.com/static/openapi.yml, https://docs.isthereanydeal.com/,
  https://www.pcgamingwiki.com/wiki/PCGamingWiki:API, https://query.wikidata.org/
- Logos: https://github.com/simple-icons/simple-icons, https://partner.steamgames.com/doc/marketing/branding
