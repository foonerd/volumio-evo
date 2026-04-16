//! Stock UI calls `POST /api/v1/pluginEndpoint` with `endpoint: "metavolumio"` for album stories,
//! artist bios, and credits. Implemented in Rust (Last.fm + MusicBrainz + Wikipedia fallback).

use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::Config;

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .expect("reqwest client")
    })
}

const DEFAULT_MB_UA: &str = "VolumioEvo/1.0 (https://volumio.org)";

/// Wikimedia APIs reject or throttle clients without a descriptive User-Agent.
/// https://meta.wikimedia.org/wiki/User-Agent_policy
const WIKIMEDIA_UA: &str = "VolumioEvo/1.0 (+https://volumio.org/)";

fn musicbrainz_ua(config: &Config) -> String {
    config
        .albumart_providers
        .musicbrainz_user_agent
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_MB_UA.to_string())
}

fn lastfm_key(config: &Config) -> Option<&str> {
    config
        .albumart_providers
        .lastfm_api_key
        .as_deref()
        .filter(|s| !s.is_empty())
}

#[derive(Debug, Deserialize)]
pub struct PluginEndpointBody {
    pub endpoint: String,
    #[serde(default, alias = "Data")]
    pub data: MetavolumioData,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MetavolumioData {
    #[serde(default, alias = "Mode")]
    pub mode: Option<String>,
    #[serde(default, alias = "Artist")]
    pub artist: Option<String>,
    #[serde(default, alias = "Album")]
    pub album: Option<String>,
    #[serde(default, alias = "Mbid")]
    pub mbid: Option<String>,
}

/// JSON body the Angular UI expects: `response.data.success` && `response.data.data.value` (HTML).
pub async fn metavolumio_response(config: &Config, body: &PluginEndpointBody) -> Value {
    if body.endpoint != "metavolumio" {
        return json!({
            "success": false,
            "error": "unsupported endpoint"
        });
    }

    let mode_raw = body.data.mode.as_deref().unwrap_or("").trim();
    let mode_lc = mode_raw.to_ascii_lowercase();
    let client = http_client();

    let html = match mode_lc.as_str() {
        "storyalbum" => {
            let artist = body.data.artist.as_deref().unwrap_or("").trim();
            let album = body.data.album.as_deref().unwrap_or("").trim();
            if artist.is_empty() || album.is_empty() {
                tracing::warn!(
                    artist_empty = artist.is_empty(),
                    album_empty = album.is_empty(),
                    "{} metavolumio storyAlbum: missing artist or album in request body",
                    crate::log_tags::EVO_META
                );
                None
            } else {
                story_album(config, client, artist, album).await
            }
        }
        "storyartist" => story_artist(config, client, &body.data).await,
        "creditsalbum" => {
            let artist = body.data.artist.as_deref().unwrap_or("").trim();
            let album = body.data.album.as_deref().unwrap_or("").trim();
            if artist.is_empty() || album.is_empty() {
                None
            } else {
                credits_album(config, client, artist, album).await
            }
        }
        "storylabel" | "storyplace" => {
            let mbid = body.data.mbid.as_deref().unwrap_or("").trim();
            if mbid.is_empty() {
                None
            } else if mode_lc == "storylabel" {
                story_entity_wikipedia(client, "label", mbid, &musicbrainz_ua(config)).await
            } else {
                story_entity_wikipedia(client, "place", mbid, &musicbrainz_ua(config)).await
            }
        }
        _ => {
            if !mode_raw.is_empty() {
                tracing::warn!(%mode_raw, "{} metavolumio: unknown mode", crate::log_tags::EVO_META);
            }
            None
        }
    };

    let out = match html {
        Some(value) if !value.trim().is_empty() => json!({
            "success": true,
            "data": { "value": value }
        }),
        _ => json!({
            "success": false,
            "error": "no metadata"
        }),
    };
    if matches!(out.get("success"), Some(v) if v.as_bool() == Some(false)) {
        tracing::warn!(
            mode = %mode_raw,
            artist = ?body.data.artist,
            album = ?body.data.album,
            has_mbid = body.data.mbid.is_some(),
            "{} metavolumio: no text — set VOLUMIO_EVO_LASTFM_API_KEY for best results; device needs HTTPS to Last.fm, MusicBrainz, and Wikipedia",
            crate::log_tags::EVO_META
        );
    }
    out
}

async fn story_album(
    config: &Config,
    client: &reqwest::Client,
    artist: &str,
    album: &str,
) -> Option<String> {
    if let Some(key) = lastfm_key(config) {
        if let Some(s) = lastfm_album_story_or_tags_fallback(client, key, artist, album).await {
            return Some(s);
        }
    }
    wikipedia_album_with_variants(client, artist, album).await
}

async fn story_artist(
    config: &Config,
    client: &reqwest::Client,
    data: &MetavolumioData,
) -> Option<String> {
    if let Some(m) = data.mbid.as_deref() {
        let id = m.trim();
        if !id.is_empty() {
            if let Some(s) = story_entity_wikipedia(client, "artist", id, &musicbrainz_ua(config)).await {
                return Some(s);
            }
        }
    }
    let artist = data.artist.as_deref()?.trim();
    if artist.is_empty() {
        return None;
    }
    if let Some(key) = lastfm_key(config) {
        if let Some(s) = lastfm_artist_bio(client, key, artist).await {
            return Some(s);
        }
    }
    wikipedia_summary_artist(client, artist).await
}

/// Last.fm `album.getinfo`: prefer wiki HTML; many releases have no wiki — use tags + stats + link.
async fn lastfm_album_story_or_tags_fallback(
    client: &reqwest::Client,
    api_key: &str,
    artist: &str,
    album: &str,
) -> Option<String> {
    let url = format!(
        "https://ws.audioscrobbler.com/2.0/?format=json&api_key={}&method=album.getinfo&artist={}&album={}",
        api_key,
        urlencoding::encode(artist),
        urlencoding::encode(album)
    );
    let text = client
        .get(&url)
        .header("User-Agent", WIKIMEDIA_UA)
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    if v.get("error").is_some() {
        return None;
    }
    let album_val = v.get("album")?;
    if let Some(wiki) = album_val.get("wiki") {
        let raw = wiki
            .get("content")
            .and_then(|c| c.as_str())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| wiki.get("summary").and_then(|c| c.as_str()));
        if let Some(r) = raw {
            return Some(lastfm_strip_footer(r));
        }
    }
    lastfm_album_tags_fallback_html(album_val)
}

/// Minimal HTML when Last.fm has no wiki (still useful for the browse panel).
fn lastfm_album_tags_fallback_html(album: &Value) -> Option<String> {
    let name = album.get("name").and_then(|x| x.as_str()).unwrap_or("");
    let url = album.get("url").and_then(|x| x.as_str()).unwrap_or("");
    let listeners = album
        .get("listeners")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .or_else(|| album.get("listeners").and_then(|x| x.as_u64()).map(|n| n.to_string()));
    let playcount = album
        .get("playcount")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .or_else(|| album.get("playcount").and_then(|x| x.as_u64()).map(|n| n.to_string()));

    let mut tag_names: Vec<&str> = Vec::new();
    if let Some(tags) = album.get("tags") {
        if let Some(arr) = tags.get("tag").and_then(|t| t.as_array()) {
            for t in arr {
                if let Some(n) = t.get("name").and_then(|x| x.as_str()) {
                    tag_names.push(n);
                }
            }
        }
    }

    if name.is_empty() && url.is_empty() && tag_names.is_empty() {
        return None;
    }

    let mut parts: Vec<String> = Vec::new();
    if !url.is_empty() && !name.is_empty() {
        parts.push(format!(
            "<p><a href=\"{}\" rel=\"noopener\">{}</a> on Last.fm</p>",
            escape_html(url),
            escape_html(name)
        ));
    } else if !name.is_empty() {
        parts.push(format!("<p><strong>{}</strong></p>", escape_html(name)));
    }
    let mut stats: Vec<String> = Vec::new();
    if let Some(ref l) = listeners {
        stats.push(format!("listeners: {}", escape_html(l)));
    }
    if let Some(ref p) = playcount {
        stats.push(format!("scrobbles: {}", escape_html(p)));
    }
    if !stats.is_empty() {
        parts.push(format!("<p class=\"text-muted\">{}</p>", stats.join(" · ")));
    }
    if !tag_names.is_empty() {
        let joined = tag_names
            .into_iter()
            .map(escape_html)
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("<p><strong>Tags:</strong> {}</p>", joined));
    }
    Some(parts.join("\n"))
}

fn lastfm_strip_footer(html: &str) -> String {
    let s = html.trim();
    // Last.fm often appends "Read more on Last.fm" link in summary.
    let cut = s.find("<a href=\"https://www.last.fm").or_else(|| s.find("<a href='https://www.last.fm"));
    match cut {
        Some(i) if i > 0 => s[..i].trim().to_string(),
        _ => s.to_string(),
    }
}

async fn lastfm_artist_bio(client: &reqwest::Client, api_key: &str, artist: &str) -> Option<String> {
    let url = format!(
        "https://ws.audioscrobbler.com/2.0/?format=json&api_key={}&method=artist.getinfo&artist={}",
        api_key,
        urlencoding::encode(artist)
    );
    let text = client
        .get(&url)
        .header("User-Agent", WIKIMEDIA_UA)
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    if v.get("error").is_some() {
        return None;
    }
    let bio = v.get("artist")?.get("bio")?;
    let raw = bio
        .get("content")
        .and_then(|c| c.as_str())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| bio.get("summary").and_then(|c| c.as_str()))?;
    Some(lastfm_strip_footer(raw))
}

fn escape_lucene(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

async fn sleep_mb_courtesy() {
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
}

async fn mb_release_search_first_mbid(
    client: &reqwest::Client,
    artist: &str,
    album: &str,
    ua: &str,
) -> Option<String> {
    let query = format!(
        "artist:\"{}\" AND release:\"{}\"",
        escape_lucene(artist),
        escape_lucene(album)
    );
    let url = format!(
        "https://musicbrainz.org/ws/2/release/?query={}&fmt=json&limit=1",
        urlencoding::encode(&query)
    );
    let resp = client.get(&url).header("User-Agent", ua).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    v.get("releases")?
        .as_array()?
        .first()?
        .get("id")?
        .as_str()
        .map(|s| s.to_string())
}

/// Album credits: release metadata + track list from MusicBrainz.
async fn credits_album(
    config: &Config,
    client: &reqwest::Client,
    artist: &str,
    album: &str,
) -> Option<String> {
    let ua = musicbrainz_ua(config);
    let mbid = mb_release_search_first_mbid(client, artist, album, &ua).await?;
    sleep_mb_courtesy().await;
    let url = format!(
        "https://musicbrainz.org/ws/2/release/{}?inc=artist-credits+labels+media+recordings&fmt=json",
        mbid
    );
    let resp = client.get(&url).header("User-Agent", &ua).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let rel: Value = resp.json().await.ok()?;
    let mut parts: Vec<String> = Vec::new();

    if let Some(title) = rel.get("title").and_then(|t| t.as_str()) {
        parts.push(format!("<h3>{}</h3>", escape_html(title)));
    }
    let root_date = rel
        .get("date")
        .and_then(|t| t.as_str())
        .filter(|d| !d.is_empty());
    if let Some(date) = root_date {
        parts.push(format!("<p><strong>Released:</strong> {}</p>", escape_html(date)));
    } else if let Some(ev) = rel.get("release-events").and_then(|e| e.as_array()) {
        for e in ev {
            if let Some(d) = e.get("date").and_then(|t| t.as_str()) {
                if !d.is_empty() {
                    parts.push(format!("<p><strong>Released:</strong> {}</p>", escape_html(d)));
                    break;
                }
            }
        }
    }

    if let Some(ac) = rel.get("artist-credit") {
        if let Some(names) = collect_artist_credit_names(ac) {
            parts.push(format!(
                "<p><strong>Artist credit:</strong> {}</p>",
                escape_html(&names)
            ));
        }
    }

    if let Some(labels) = rel.get("label-info") {
        if let Some(lbl_html) = format_label_info(labels) {
            parts.push(lbl_html);
        }
    }

    let mut track_lines: Vec<String> = Vec::new();
    if let Some(media) = rel.get("media").and_then(|m| m.as_array()) {
        for m in media {
            let tracks = m
                .get("tracks")
                .and_then(|t| t.as_array())
                .or_else(|| m.get("track").and_then(|t| t.as_array()));
            if let Some(tlist) = tracks {
                for tr in tlist {
                    let pos = tr
                        .get("position")
                        .and_then(|p| p.as_u64())
                        .or_else(|| tr.get("number").and_then(|p| p.as_u64()))
                        .unwrap_or(0);
                    let title = tr
                        .get("title")
                        .and_then(|t| t.as_str())
                        .or_else(|| tr.pointer("/recording/title").and_then(|t| t.as_str()))
                        .unwrap_or("?");
                    let line = if let Some(ac) = tr
                        .get("artist-credit")
                        .or_else(|| tr.pointer("/recording/artist-credit"))
                    {
                        if let Some(names) = collect_artist_credit_names(ac) {
                            format!(
                                "<li value=\"{}\">{} — <span class=\"text-muted\">{}</span></li>",
                                pos,
                                escape_html(title),
                                escape_html(&names)
                            )
                        } else {
                            format!("<li value=\"{}\">{}</li>", pos, escape_html(title))
                        }
                    } else {
                        format!("<li value=\"{}\">{}</li>", pos, escape_html(title))
                    };
                    track_lines.push(line);
                }
            }
        }
    }
    if !track_lines.is_empty() {
        parts.push("<h4>Tracks</h4>".to_string());
        parts.push("<ol>".to_string());
        parts.extend(track_lines);
        parts.push("</ol>".to_string());
    }

    if parts.is_empty() {
        return None;
    }
    Some(parts.join("\n"))
}

fn collect_artist_credit_names(ac: &Value) -> Option<String> {
    let arr = ac.as_array()?;
    let mut s = String::new();
    for entry in arr {
        if let Some(n) = entry.get("name").and_then(|x| x.as_str()) {
            s.push_str(n);
        } else if let Some(a) = entry.get("artist") {
            if let Some(n) = a.get("name").and_then(|x| x.as_str()) {
                s.push_str(n);
            }
        }
        if let Some(jp) = entry.get("joinphrase").and_then(|x| x.as_str()) {
            s.push_str(jp);
        }
    }
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn format_label_info(labels: &Value) -> Option<String> {
    let arr = labels.as_array()?;
    let mut lbls: Vec<String> = Vec::new();
    for li in arr {
        if let Some(l) = li.get("label") {
            if let Some(n) = l.get("name").and_then(|x| x.as_str()) {
                let cat = li.get("catalog-number").and_then(|c| c.as_str()).unwrap_or("");
                if cat.is_empty() {
                    lbls.push(n.to_string());
                } else {
                    lbls.push(format!("{} ({})", n, cat));
                }
            }
        }
    }
    if lbls.is_empty() {
        None
    } else {
        Some(format!(
            "<p><strong>Label:</strong> {}</p>",
            escape_html(&lbls.join(", "))
        ))
    }
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn paragraph_from_plain(s: &str) -> String {
    format!("<p>{}</p>", escape_html(s.trim()))
}

/// Try several Wikipedia search strings (opensearch + MediaWiki search API).
async fn wikipedia_album_with_variants(
    client: &reqwest::Client,
    artist: &str,
    album: &str,
) -> Option<String> {
    let mut queries: Vec<String> = vec![format!("{} {}", artist, album), album.to_string()];
    if !artist.eq_ignore_ascii_case("various artists") {
        queries.push(format!("{} (album)", album));
    }
    for q in queries {
        if let Some(s) = wikipedia_summarize_any(client, &q).await {
            return Some(s);
        }
    }
    None
}

async fn wikipedia_summarize_any(client: &reqwest::Client, query: &str) -> Option<String> {
    if let Some(s) = wikipedia_summary_search(client, query).await {
        return Some(s);
    }
    wikipedia_query_search_then_summary(client, query).await
}

async fn wikipedia_summary_artist(client: &reqwest::Client, artist: &str) -> Option<String> {
    wikipedia_summary_search(client, artist).await
}

async fn wikipedia_summary_search(client: &reqwest::Client, query: &str) -> Option<String> {
    let search_url = format!(
        "https://en.wikipedia.org/w/api.php?action=opensearch&search={}&limit=1&namespace=0&format=json",
        urlencoding::encode(query)
    );
    let text = client
        .get(&search_url)
        .header("User-Agent", WIKIMEDIA_UA)
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    let arr: Vec<Value> = serde_json::from_str(&text).ok()?;
    let title = arr.get(1)?.as_array()?.first()?.as_str()?;
    if title.is_empty() {
        return None;
    }
    wikipedia_summary_by_title(client, title).await
}

/// `action=query&list=search` often finds pages when opensearch returns nothing.
async fn wikipedia_query_search_then_summary(
    client: &reqwest::Client,
    query: &str,
) -> Option<String> {
    let search_url = format!(
        "https://en.wikipedia.org/w/api.php?action=query&list=search&format=json&srlimit=3&srsearch={}",
        urlencoding::encode(query)
    );
    let text = client
        .get(&search_url)
        .header("User-Agent", WIKIMEDIA_UA)
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let title = v
        .pointer("/query/search/0/title")?
        .as_str()?
        .trim();
    if title.is_empty() {
        return None;
    }
    wikipedia_summary_by_title(client, title).await
}

/// Artist / label / place by MBID: follow Wikipedia URL relation, then REST summary.
async fn story_entity_wikipedia(
    client: &reqwest::Client,
    entity: &str,
    mbid: &str,
    ua: &str,
) -> Option<String> {
    let url = format!(
        "https://musicbrainz.org/ws/2/{}/{}?inc=url-rels&fmt=json",
        entity, mbid
    );
    let resp = client.get(&url).header("User-Agent", ua).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    let relations = v.get("relations")?.as_array()?;

    for rel in relations {
        let url = rel.get("url")?.get("resource")?.as_str()?;
        let lower = url.to_ascii_lowercase();
        if lower.contains("wikipedia.org/wiki/") {
            if let Some(t) = wikipedia_title_from_wiki_url(url) {
                return wikipedia_summary_by_title(client, &t).await;
            }
        }
    }
    for rel in relations {
        if rel.get("type").and_then(|t| t.as_str()) != Some("wikidata") {
            continue;
        }
        let wurl = rel.get("url")?.get("resource")?.as_str()?;
        if let Some(qid) = wikidata_id_from_url(wurl) {
            if let Some(title) = enwiki_title_from_wikidata(client, &qid).await {
                return wikipedia_summary_by_title(client, &title).await;
            }
        }
    }
    None
}

fn wikidata_id_from_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let rest = if let Some(i) = lower.rfind("/wiki/") {
        &url[i + 6..]
    } else if let Some(i) = lower.rfind("entity/") {
        &url[i + 7..]
    } else {
        return None;
    };
    let id = rest.trim_matches('/').split('?').next()?.trim();
    if id.starts_with('Q') && id[1..].chars().all(|c| c.is_ascii_digit()) {
        Some(id.to_string())
    } else {
        None
    }
}

async fn enwiki_title_from_wikidata(client: &reqwest::Client, qid: &str) -> Option<String> {
    let url = format!(
        "https://www.wikidata.org/wiki/Special:EntityData/{}.json",
        qid
    );
    let resp = client
        .get(&url)
        .header("User-Agent", WIKIMEDIA_UA)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    let entities = v.get("entities")?.as_object()?;
    let entity = entities.values().next()?;
    let title = entity
        .pointer("/sitelinks/enwiki/title")?
        .as_str()?
        .to_string();
    Some(title.replace(' ', "_"))
}

fn wikipedia_title_from_wiki_url(url: &str) -> Option<String> {
    let idx = url.rfind("/wiki/")?;
    let tail = &url[idx + 6..];
    let t = urlencoding::decode(tail).ok()?;
    Some(t.into_owned())
}

async fn wikipedia_summary_by_title(client: &reqwest::Client, title: &str) -> Option<String> {
    let title_enc = title.replace(' ', "_");
    let enc: String = urlencoding::encode(&title_enc).into_owned();
    let summary_url = format!("https://en.wikipedia.org/api/rest_v1/page/summary/{}", enc);
    let sum = client
        .get(summary_url)
        .header("User-Agent", WIKIMEDIA_UA)
        .send()
        .await
        .ok()?;
    if !sum.status().is_success() {
        tracing::debug!(status = %sum.status(), "{} wikipedia summary by title HTTP error", crate::log_tags::EVO_META);
        return None;
    }
    let v: Value = sum.json().await.ok()?;
    let extract = v.get("extract").and_then(|e| e.as_str())?;
    if extract.trim().is_empty() {
        return None;
    }
    Some(paragraph_from_plain(extract))
}
