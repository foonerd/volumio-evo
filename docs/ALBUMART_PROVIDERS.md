# Album art: online providers for reliability

Evo uses **multiple online providers** in a fallback chain so album art stays reliable when one service is down, rate-limited, or has no result. Implementation: `crates/core/src/albumart.rs` (sync local/personal + async online), wired in GET /albumart, /albumartd, /tinyart/*.

## Recommended provider order (album art)

Try in order; use first successful result and cache to `albumart_root/web/<artist>/<album>/` as today.

| Order | Provider | API | Key? | Notes |
|-------|----------|-----|------|--------|
| 1 | **Cover Art Archive** (MusicBrainz) | 1) `GET https://musicbrainz.org/ws/2/release/?query=artist:"…" AND release:"…"&fmt=json` (User-Agent required). 2) Take first release `id` (MBID), then `GET https://coverartarchive.org/release/{mbid}/front` (307 → image URL). | No | High quality, free, good coverage. Two requests (search then front). |
| 2 | **Last.fm** | `GET https://ws.audioscrobbler.com/2.0/?format=json&api_key=…&method=album.getinfo&artist=…&album=…`; use `album.image[].#text` by size (small/medium/large/extralarge/mega). | Yes (env e.g. `LASTFM_API_KEY`) | Good coverage; Volumio uses this today. |
| 3 | **iTunes Search** | `GET https://itunes.apple.com/search?term=…&entity=album&limit=5` (artist+album in `term`); use `results[0].artworkUrl600` or `artworkUrl100`. | No | No key; ~20 req/min fair use. Good fallback. |
| 4 | **Volumio meta** (optional) | Artist: `GET https://meta.volumio.org/metas/v1/getDatas?mode=artistArt&artist=…&variant=…`. Album: if they add it, use similarly. | No | Keep for artist art; use as last resort for album if others fail. |

## Artist art only (no album)

- **Volumio meta** (artistArt) — current Volumio behaviour.
- **Last.fm** `artist.getinfo` + `image[]` — optional extra.
- **MusicBrainz** artist search + Cover Art Archive is release-focused; for “artist image” Volumio meta or Last.fm artist is the main source.

## Implementation notes

- **Caching:** All online results should be written under `albumart_root/web/<artist>/<album>/` (and optionally `albumart_root/web/<artist>/` for artist art) with a stable filename (e.g. by resolution), and `info.json` can store which URL/filename per size, as in current Volumio.
- **Rate limits:** MusicBrainz/Cover Art Archive and iTunes ask for a descriptive User-Agent; Last.fm uses an API key (per-app). Respect 503/429 and back off; fallback to next provider.
- **Config:** API keys live in `volumio-evo.toml` under `[albumart_providers]`: `lastfm_api_key`, `musicbrainz_user_agent`. Env overrides: `VOLUMIO_EVO_LASTFM_API_KEY`, `VOLUMIO_EVO_MUSICBRAINZ_USER_AGENT` (so keys can be set in systemd without editing the config file). `VOLUMIO_ALBUMART_VARIANT` for meta.volumio.org if used.

## Browse UI: album story, artist bio, credits (metavolumio)

The stock UI loads extra HTML via **`POST /api/v1/pluginEndpoint`** with **`endpoint: "metavolumio"`** (not the album-art pipeline above). Evo resolves that in **`crates/core/src/metavolumio.rs`** using the **same** `[albumart_providers]` keys where applicable:

- **`lastfm_api_key` / `VOLUMIO_EVO_LASTFM_API_KEY`** — strongly recommended (album/artist wiki, or tags/listeners when wiki is empty).
- **`musicbrainz_user_agent` / `VOLUMIO_EVO_MUSICBRAINZ_USER_AGENT`** — used for MusicBrainz release credits and entity lookups; a default User-Agent is used if unset.
- Outbound **HTTPS** to Last.fm, MusicBrainz, and Wikimedia (Wikipedia/Wikidata) must be allowed on the device for online text.
- **Order rationale:** Cover Art Archive first (no key, high quality); then Last.fm (good coverage, key required); then iTunes (no key, rate limited); then Volumio meta as fallback.

## References

- [Cover Art Archive API](https://musicbrainz.org/doc/Cover_Art_Archive/API)
- [MusicBrainz API search](https://musicbrainz.org/doc/MusicBrainz_API) (release search, User-Agent required)
- [Last.fm API](https://www.last.fm/api) (album.getinfo, artist.getinfo)
- [iTunes Search API](https://developer.apple.com/library/archive/documentation/AudioVideo/Conceptual/iTuneSearchAPI/) (no key)
- Volumio: `volumio3-backend/app/plugins/miscellanea/albumart/albumart.js` (`retrieveAlbumart`)
