//! Artist string normalization for browse tiles and `/albumart` `web=` lookups.
//! All rules live in code — no external phrase lists or config files.

/// Remove trailing ` | …` segments when the rest looks like a URL or promo site (e.g. `| www.…`).
fn strip_pipe_promotional_tail(s: &str) -> &str {
    let t = s.trim();
    let Some(pipe_pos) = t.find(" | ") else {
        return t;
    };
    let tail = t[pipe_pos + 3..].trim_start();
    let is_junk = tail.starts_with("http://")
        || tail.starts_with("https://")
        || tail.starts_with("www.")
        || tail.contains(".com/")
        || tail.ends_with(".com")
        || tail.contains(".net");
    if is_junk {
        t[..pipe_pos].trim_end()
    } else {
        t
    }
}

/// Join phrases that separate **lead** credit from the rest (case-insensitive). Checked in order of first occurrence in string.
fn cut_before_first_join_phrase(s: &str) -> &str {
    let lower = s.to_lowercase();
    // Space-delimited phrases; longest first to avoid matching " ft " inside " feat "
    const PHRASES: &[&str] = &[
        " featuring ",
        " feat. ",
        " feat ",
        " ft. ",
        " ft ",
        " with ",
        " vs. ",
        " vs ",
    ];
    let mut end = s.len();
    for p in PHRASES {
        if let Some(i) = lower.find(p) {
            if i < end {
                end = i;
            }
        }
    }
    if end < s.len() {
        s[..end].trim_end()
    } else {
        s
    }
}

/// Strip disambiguation / featuring-in-parens: `Paul McCartney (Off The Ground)`, `Name (ft …)`.
fn cut_before_space_open_paren(s: &str) -> &str {
    match s.find(" (") {
        Some(i) => s[..i].trim_end(),
        None => s,
    }
}

/// MusicBrainz-style **sort name** `Last, First` when both sides are a **single** token (no spaces).
/// Does not match `Tom Waits, Crystal Gayle` (multi-word side).
fn swap_sort_name_last_comma_first(s: &str) -> String {
    let t = s.trim();
    let Some(comma) = t.find(',') else {
        return t.to_string();
    };
    if t[comma.saturating_add(1)..].contains(',') {
        return t.to_string();
    }
    let left = t[..comma].trim();
    let right = t[comma + 1..].trim();
    if left.is_empty()
        || right.is_empty()
        || left.contains(' ')
        || right.contains(' ')
    {
        return t.to_string();
    }
    format!("{right} {left}")
}

/// Lead segment before collaboration/list markers (leftmost wins). Spaced ` - ` is handled separately (see below).
fn cut_before_split_separators(s: &str) -> &str {
    const SEPARATORS: &[&str] = &[" / ", " + ", " & ", ", ", "; "];
    let mut best: Option<usize> = None;
    for sep in SEPARATORS {
        if let Some(i) = s.find(sep) {
            best = Some(best.map_or(i, |b| b.min(i)));
        }
    }
    match best {
        Some(i) => s[..i].trim_end(),
        None => s,
    }
}

/// `Project - Artist` tags (e.g. Rhythms Del Mundo — Coldplay): use the **last** segment after spaced ` - `.
fn take_last_after_spaced_hyphen(s: &str) -> &str {
    if !s.contains(" - ") {
        return s;
    }
    s.rsplit(" - ")
        .next()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(s)
}

/// Path/filename junk: `Michael Jackson album_Tracks…` — keep text before ` album_` (case-insensitive ASCII).
fn cut_before_album_underscore_filename(s: &str) -> &str {
    const NEEDLE: &str = " album_";
    for (i, _) in s.char_indices() {
        let rest = &s[i..];
        if rest.len() >= NEEDLE.len()
            && rest
                .get(..NEEDLE.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(NEEDLE))
        {
            return s[..i].trim_end();
        }
    }
    s
}

fn underscores_to_spaces(s: &str) -> String {
    if !s.contains('_') {
        return s.to_string();
    }
    s.replace('_', " ")
}

/// **Art lookup:** strip promo tails, then take lead segment before featured/split markers so `web=artist//` matches providers.
pub fn normalize_for_art_lookup(raw: &str) -> String {
    let t = strip_pipe_promotional_tail(raw.trim());
    let t = cut_before_space_open_paren(t);
    let t = cut_before_first_join_phrase(t);
    let swapped = swap_sort_name_last_comma_first(t);
    let t = cut_before_split_separators(&swapped);
    let t = take_last_after_spaced_hyphen(t);
    let t = cut_before_album_underscore_filename(t);
    let t = underscores_to_spaces(t);
    t.trim().to_string()
}

/// **Tile title (Browse → Artists):** remove `| www…` style junk; keep full collaboration text otherwise (Node shows tag text).
pub fn normalize_for_artist_tile_title(raw: &str) -> String {
    strip_pipe_promotional_tail(raw.trim()).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_pipe_url() {
        let s = "Bruno Mars | www.RNBxBeatz.com";
        assert_eq!(normalize_for_artist_tile_title(s), "Bruno Mars");
        assert_eq!(normalize_for_art_lookup(s), "Bruno Mars");
    }

    #[test]
    fn lead_before_ft() {
        let s = "Jamie Cullum ft. Katie Melua";
        assert_eq!(normalize_for_art_lookup(s), "Jamie Cullum");
        assert_eq!(normalize_for_artist_tile_title(s), s);
    }

    #[test]
    fn split_slash() {
        let s = "Burufunk / Global Communication";
        assert_eq!(normalize_for_art_lookup(s), "Burufunk");
    }

    #[test]
    fn split_ampersand() {
        let s = "Mark Ronson & Nick Catchdubs";
        assert_eq!(normalize_for_art_lookup(s), "Mark Ronson");
        assert_eq!(normalize_for_artist_tile_title(s), s);
    }

    #[test]
    fn split_comma() {
        let s = "Tom Waits, Crystal Gayle";
        assert_eq!(normalize_for_art_lookup(s), "Tom Waits");
    }

    #[test]
    fn split_semicolon() {
        let s = "Paco de Lucía; John McLaughlin";
        assert_eq!(normalize_for_art_lookup(s), "Paco de Lucía");
    }

    #[test]
    fn split_leftmost_wins() {
        let s = "Alpha & Bravo / Charlie";
        assert_eq!(normalize_for_art_lookup(s), "Alpha");
    }

    #[test]
    fn sort_name_collins_phil() {
        let s = "Collins, Phil";
        assert_eq!(normalize_for_art_lookup(s), "Phil Collins");
    }

    #[test]
    fn sort_name_does_not_break_two_word_artist() {
        let s = "Tom Waits, Crystal Gayle";
        assert_eq!(normalize_for_art_lookup(s), "Tom Waits");
    }

    #[test]
    fn paren_disambiguation() {
        let s = "Paul McCartney (Off The Ground)";
        assert_eq!(normalize_for_art_lookup(s), "Paul McCartney");
    }

    #[test]
    fn paren_feat_suffix() {
        let s = "James Morrison (ft Nelly Furtado)";
        assert_eq!(normalize_for_art_lookup(s), "James Morrison");
    }

    #[test]
    fn split_spaced_hyphen_collab() {
        let s = "Rhythms Del Mundo - Coldplay";
        assert_eq!(normalize_for_art_lookup(s), "Coldplay");
    }

    #[test]
    fn album_underscore_filename_junk() {
        let s = "Michael Jackson album_Michael Jackson";
        assert_eq!(normalize_for_art_lookup(s), "Michael Jackson");
    }

    #[test]
    fn underscores_to_space_when_no_album_marker() {
        assert_eq!(
            normalize_for_art_lookup("Jean_Michel_Jarre"),
            "Jean Michel Jarre"
        );
    }
}
