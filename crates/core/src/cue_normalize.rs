//! External `.cue` preprocessing and lightweight parsing for Evo.
//!
//! **Normalization** strips BOMs, fixes line endings, forwards slashes on `FILE`, and pads `INDEX`
//! timestamps so brittle parsers behave like the legacy Node `cue-parser`.
//!
//! **Browse** expands the first `FILE … TRACK …` block only (parity with legacy `files[0]` behaviour).
//!
//! ## Future work (safe to defer)
//!
//! - **MPD-facing sidecar**: write `*.volumio.cue` when `load` fails but normalized text parses;
//!   never overwrite the user’s original without opt-in (see repo `docs/CUE_SHEETS.md`).
//! - **Multi-`FILE` cues**: expose all images in browse and queue semantics (needs UI agreement).
//! - **Embedded FLAC cues**: separate from external `.cue`; depends on MPD decoder build.
//! - **Richer cue grammar**: pre-gap, malformed `INDEX` sequences — add regression fixtures per ripper.

use regex::Regex;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct CueBrowseTrack {
    pub track_no: u32,
    pub title: Option<String>,
    pub performer: Option<String>,
}

/// Normalize cue text for parsers (does not touch files on disk).
pub fn normalize_cue_text(raw: &str) -> String {
    let unified = raw.replace("\r\n", "\n").replace('\r', "\n");
    unified
        .lines()
        .map(normalize_cue_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_cue_line(line: &str) -> String {
    let trimmed = line.trim_start();
    let ul = trimmed.to_ascii_uppercase();
    if ul.starts_with("FILE ") {
        return line.replace('\\', "/");
    }
    if ul.starts_with("INDEX ") {
        return pad_index_line(line);
    }
    line.to_string()
}

fn pad_index_line(line: &str) -> String {
    let re = index_line_regex();
    let Some(caps) = re.captures(line) else {
        return line.to_string();
    };
    let time_tok = caps.get(2).map(|m| m.as_str()).unwrap_or("");
    let parts: Vec<&str> = time_tok.split(':').collect();
    if parts.len() != 3 {
        return line.to_string();
    }
    let mm_raw = parts[0];
    let mm = if mm_raw.len() < 2 {
        format!("{:02}", mm_raw.parse::<u32>().unwrap_or(0))
    } else {
        mm_raw.to_string()
    };
    let ss = format!("{:02}", parts[1].parse::<u32>().unwrap_or(0));
    let ff = format!("{:02}", parts[2].parse::<u32>().unwrap_or(0));
    let padded = format!("{}:{}:{}", mm, ss, ff);
    format!(
        "{}{}{}",
        caps.get(1).map(|m| m.as_str()).unwrap_or(""),
        padded,
        caps.get(3).map(|m| m.as_str()).unwrap_or("")
    )
}

fn index_line_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^(\s*INDEX\s+\d+\s+)(\S+)(\s*)$").expect("regex"))
}

/// Read cue file bytes; strip UTF-8 / UTF-16 BOM; return UTF-8 string (lossy only as last resort).
pub fn read_cue_file_utf8(abs: &Path) -> Result<String, std::io::Error> {
    let buf = fs::read(abs)?;
    Ok(decode_cue_bytes_lossy(&buf))
}

fn decode_cue_bytes_lossy(buf: &[u8]) -> String {
    if buf.len() >= 3 && buf[0] == 0xEF && buf[1] == 0xBB && buf[2] == 0xBF {
        return String::from_utf8_lossy(&buf[3..]).into_owned();
    }
    if buf.len() >= 2 && buf[0] == 0xFF && buf[1] == 0xFE {
        let b = &buf[2..];
        return utf16le_to_string(b);
    }
    if buf.len() >= 2 && buf[0] == 0xFE && buf[1] == 0xFF {
        let mut b = buf[2..].to_vec();
        for chunk in b.chunks_exact_mut(2) {
            chunk.swap(0, 1);
        }
        return utf16le_to_string(&b);
    }
    String::from_utf8_lossy(buf).into_owned()
}

fn utf16le_to_string(b: &[u8]) -> String {
    if b.len() % 2 != 0 {
        return String::from_utf8_lossy(b).into_owned();
    }
    let u: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&u)
}

/// Read file from disk, normalize text, parse **first** `FILE` block only.
pub fn read_and_parse_cue(abs: &Path) -> Result<Vec<CueBrowseTrack>, std::io::Error> {
    let raw = read_cue_file_utf8(abs)?;
    let normalized = normalize_cue_text(&raw);
    Ok(parse_cue_first_file_only(&normalized))
}

/// Minimal parse: tracks from the first `FILE` section (legacy `cue-parser` `files[0]` parity).
pub fn parse_cue_first_file_only(text: &str) -> Vec<CueBrowseTrack> {
    let mut tracks: Vec<CueBrowseTrack> = Vec::new();
    let mut cur: Option<CueBrowseTrack> = None;
    let mut file_blocks = 0usize;
    let mut in_first_file = false;

    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let ul = t.to_ascii_uppercase();
        if ul.starts_with("FILE ") {
            file_blocks += 1;
            if file_blocks == 1 {
                in_first_file = true;
            } else {
                break;
            }
            continue;
        }
        if !in_first_file {
            continue;
        }
        if ul.starts_with("TRACK ") {
            if let Some(done) = cur.take() {
                tracks.push(done);
            }
            let rest = strip_prefix_ci(t, "TRACK ").unwrap_or("");
            let mut parts = rest.split_whitespace();
            let num_str = parts.next().unwrap_or("");
            let track_no = num_str.parse::<u32>().unwrap_or(tracks.len() as u32 + 1);
            cur = Some(CueBrowseTrack {
                track_no,
                title: None,
                performer: None,
            });
            continue;
        }
        let Some(ref mut c) = cur else {
            continue;
        };
        if ul.starts_with("TITLE ") {
            c.title = Some(parse_cue_string_field(t, "TITLE"));
        } else if ul.starts_with("PERFORMER ") {
            c.performer = Some(parse_cue_string_field(t, "PERFORMER"));
        }
    }
    if let Some(done) = cur {
        tracks.push(done);
    }
    tracks
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let t = s.trim_start();
    let n = prefix.len();
    if t.len() >= n && t[..n].eq_ignore_ascii_case(prefix) {
        Some(t[n..].trim_start())
    } else {
        None
    }
}

fn parse_cue_string_field(line: &str, keyword: &str) -> String {
    let Some(rest) = strip_prefix_ci(line.trim(), keyword) else {
        return line.to_string();
    };
    let rest = rest.trim_start();
    if rest.starts_with('"') {
        let mut out = String::new();
        let mut esc = false;
        for ch in rest[1..].chars() {
            if esc {
                out.push(ch);
                esc = false;
                continue;
            }
            match ch {
                '\\' => esc = true,
                '"' => break,
                _ => out.push(ch),
            }
        }
        return out;
    }
    rest.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_index_minutes() {
        let raw = "    INDEX 01 0:02:35\n";
        let out = normalize_cue_text(raw);
        assert!(out.contains("00:02:35"));
    }

    #[test]
    fn file_backslashes() {
        let raw = r#"FILE "album\disc.flac" WAVE"#;
        let out = normalize_cue_text(raw);
        assert!(out.contains("album/disc.flac"));
    }

    #[test]
    fn parse_two_tracks_first_file_only() {
        let text = r#"FILE "a.flac" WAVE
  TRACK 01 AUDIO
    TITLE "One"
  TRACK 02 AUDIO
    TITLE "Two"
FILE "b.flac" WAVE
  TRACK 03 AUDIO
    TITLE "Three"
"#;
        let tr = parse_cue_first_file_only(text);
        assert_eq!(tr.len(), 2);
        assert_eq!(tr[0].title.as_deref(), Some("One"));
        assert_eq!(tr[1].title.as_deref(), Some("Two"));
    }
}
