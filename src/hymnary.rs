//! Hymn lookup by Bible passage, via Hymnary.org's scripture API.
//!
//! API (https://hymnary.org/api/scripture), behaviour verified against live
//! responses on 2026-11-24:
//! - params: `book`, `fromChapter` (required), `fromVerse`, `toChapter`,
//!   `toVerse` (optional). All 66 of our `BOOKS` names are accepted.
//! - hits: a JSON object keyed by the hymn's first line; each value is an
//!   object whose values are all strings: "title", "text link",
//!   "number of hymnals", "scripture references", "meter", "date",
//!   "place of origin", "original language", plus people keyed by role
//!   ("author", "translator", "versifier", "author (st. 3)", ...).
//! - no hits: `[]` (an empty JSON *array*).
//! - unknown book: HTTP 200 with plain text "Unknown Bible book: ...".
//!
//! Only the passage reference is sent. Nothing is cached on disk.

use std::time::Duration;

const ENDPOINT: &str = "https://hymnary.org/api/scripture";
/// Links we let the user open must point back at Hymnary itself.
const LINK_PREFIX: &str = "https://hymnary.org/";
const MAX_BODY: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Hymn {
    pub first_line: String,
    pub title: String,
    /// (role, name) e.g. ("author", "Newton, John, 1725-1807")
    pub people: Vec<(String, String)>,
    pub meter: String,
    pub date: String,
    /// How many hymnals print this text: a good "how well known" signal.
    pub hymnals: u32,
    pub scripture: String,
    /// Validated to start with `LINK_PREFIX`; empty if the API gave none.
    pub link: String,
}

impl Hymn {
    /// "Newton, John, 1725-1807" style names, joined for display.
    pub fn people_line(&self) -> String {
        self.people
            .iter()
            .map(|(role, name)| {
                if role == "author" {
                    name.clone()
                } else {
                    format!("{name} ({role})")
                }
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// The passage to look up. `verses: None` means the whole chapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Passage {
    pub book: u8,
    pub chapter: u16,
    pub verses: Option<(u16, u16)>,
}

const KNOWN: &[&str] = &[
    "title",
    "text link",
    "number of hymnals",
    "scripture references",
    "meter",
    "date",
    "place of origin",
    "original language",
];

/// Parse a response body. Results are sorted best-known first.
pub fn parse(body: &str) -> Result<Vec<Hymn>, String> {
    let trimmed = body.trim();
    if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
        let snippet: String = trimmed.chars().take(120).collect();
        return Err(format!("Hymnary.org replied: {snippet}"));
    }
    let v: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|e| format!("Unexpected reply from Hymnary.org: {e}"))?;
    let map = match v {
        serde_json::Value::Array(a) if a.is_empty() => return Ok(Vec::new()),
        serde_json::Value::Object(m) => m,
        _ => return Err("Unexpected reply from Hymnary.org (not a list of hymns).".into()),
    };
    let mut out = Vec::with_capacity(map.len());
    for (first_line, fields) in map {
        let Some(obj) = fields.as_object() else {
            continue;
        };
        let get = |k: &str| {
            obj.get(k)
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_owned()
        };
        let link = get("text link");
        let people = obj
            .iter()
            .filter(|(k, _)| !KNOWN.contains(&k.as_str()))
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.trim().to_owned())))
            .filter(|(_, s)| !s.is_empty())
            .collect();
        let title = get("title");
        out.push(Hymn {
            title: if title.is_empty() {
                first_line.clone()
            } else {
                title
            },
            first_line,
            people,
            meter: get("meter"),
            date: get("date"),
            hymnals: get("number of hymnals").parse().unwrap_or(0),
            scripture: get("scripture references"),
            link: if link.starts_with(LINK_PREFIX) {
                link
            } else {
                String::new()
            },
        });
    }
    out.sort_by(|a, b| {
        b.hymnals
            .cmp(&a.hymnals)
            .then_with(|| a.title.cmp(&b.title))
    });
    Ok(out)
}

/// Blocking fetch. Call from a background thread, never the UI thread.
pub fn fetch(p: Passage) -> Result<Vec<Hymn>, String> {
    let book = crate::bible::BOOKS
        .get(p.book as usize)
        .ok_or("Unknown book")?
        .name;
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(concat!(
            "BibleWolf/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/IntelligentWolf-Ltd/biblewolf)"
        ))
        .http_status_as_error(true)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut req = agent
        .get(ENDPOINT)
        .query("book", book)
        .query("fromChapter", p.chapter.to_string());
    if let Some((a, b)) = p.verses {
        req = req
            .query("fromVerse", a.to_string())
            .query("toChapter", p.chapter.to_string())
            .query("toVerse", b.to_string());
    }
    let mut resp = req.call().map_err(|e| match e {
        ureq::Error::StatusCode(code) => format!("Hymnary.org returned HTTP {code}."),
        other => format!("Couldn't reach Hymnary.org: {other}"),
    })?;
    let body = resp
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_string()
        .map_err(|e| format!("Couldn't read Hymnary.org's reply: {e}"))?;
    parse(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed from a real response for John 3:16 (2026-11-24).
    const SAMPLE: &str = r#"{"A ruler once came to Jesus by night":{"title":"Ye Must Be Born Again","meter":"10.11.11.6 with refrain","text link":"https:\/\/hymnary.org\/text\/a_ruler_once_came_to_jesus_by_night","number of hymnals":"231","scripture references":"John 3","author":"Sleeper, William T. (William True), 1819-1904"},"Ah, holy Jesus, how hast Thou offended":{"title":"Ah, holy Jesus, how hast Thou offended","meter":"11.11.11.5","place of origin":"Germany","original language":"German","text link":"https:\/\/hymnary.org\/text\/ah_holy_jesus_how_hast_thou_offended","number of hymnals":"162","scripture references":"John 3:16-17","author":"Heermann, Johann, 1585-1647","translator":"Bridges, Robert, 1844-1930"},"Amazing grace! (how sweet the sound) That saved a wretch like me":{"title":"Amazing grace! (how sweet the sound)","meter":"8.6.8.6","text link":"https:\/\/hymnary.org\/text\/amazing_grace_how_sweet_the_sound","number of hymnals":"1830","scripture references":"John 3:14-21","author":"Newton, John, 1725-1807"},"Evil link":{"title":"X","text link":"javascript:alert(1)","number of hymnals":"x"}}"#;

    #[test]
    fn parses_and_sorts_by_hymnal_count() {
        let h = parse(SAMPLE).unwrap();
        assert_eq!(h.len(), 4);
        assert_eq!(h[0].title, "Amazing grace! (how sweet the sound)");
        assert_eq!(h[0].hymnals, 1830);
        assert_eq!(h[1].hymnals, 231);
        let ah = h
            .iter()
            .find(|x| x.first_line.starts_with("Ah, holy"))
            .unwrap();
        assert_eq!(
            ah.people,
            vec![
                (
                    "author".to_owned(),
                    "Heermann, Johann, 1585-1647".to_owned()
                ),
                (
                    "translator".to_owned(),
                    "Bridges, Robert, 1844-1930".to_owned()
                ),
            ]
        );
        assert_eq!(
            ah.people_line(),
            "Heermann, Johann, 1585-1647; Bridges, Robert, 1844-1930 (translator)"
        );
        assert_eq!(
            ah.link,
            "https://hymnary.org/text/ah_holy_jesus_how_hast_thou_offended"
        );
    }

    #[test]
    fn rejects_foreign_links_and_bad_numbers() {
        let h = parse(SAMPLE).unwrap();
        let evil = h.iter().find(|x| x.first_line == "Evil link").unwrap();
        assert_eq!(evil.link, "");
        assert_eq!(evil.hymnals, 0);
    }

    #[test]
    fn empty_and_error_replies() {
        assert_eq!(parse("[]").unwrap(), Vec::new());
        assert!(
            parse("Unknown Bible book: Nonsense.")
                .unwrap_err()
                .contains("Unknown Bible book")
        );
        assert!(parse("[1,2]").is_err());
    }
}

/// Live network check, run by hand: `cargo test -- --ignored live_lookup`.
#[cfg(test)]
mod live {
    #[test]
    #[ignore = "hits hymnary.org"]
    fn live_lookup() {
        let h = super::fetch(super::Passage {
            book: 42,
            chapter: 3,
            verses: Some((16, 16)),
        })
        .unwrap();
        assert!(
            h.iter().any(|x| x.title.starts_with("Amazing grace")),
            "{:?}",
            h.iter().map(|x| &x.title).collect::<Vec<_>>()
        );
        let none = super::fetch(super::Passage {
            book: 3,
            chapter: 7,
            verses: Some((20, 20)),
        })
        .unwrap();
        assert!(none.is_empty());
    }
}
