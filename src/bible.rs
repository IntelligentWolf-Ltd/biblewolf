//! Bible text: the 66-book canon, translation loading and reference parsing.
//!
//! Built-in translations are embedded as gzipped TSV produced by
//! `tools/convert_scrollmapper.py` from scrollmapper/bible_databases
//! (`formats/json/<ID>.json`). Extra translations in that same JSON format can
//! be dropped into `<data dir>/biblewolf/bibles/`.

use std::io::Read;
use std::path::Path;

/// Display metadata for one book of the 66-book Protestant canon.
pub struct BookInfo {
    pub name: &'static str,
    pub short: &'static str,
}

/// Canonical order. Index matches the book order in every scrollmapper JSON
/// file we ship (verified: Genesis .. "Revelation of John", 66 entries).
pub const BOOKS: [BookInfo; 66] = [
    BookInfo {
        name: "Genesis",
        short: "Gen",
    },
    BookInfo {
        name: "Exodus",
        short: "Exod",
    },
    BookInfo {
        name: "Leviticus",
        short: "Lev",
    },
    BookInfo {
        name: "Numbers",
        short: "Num",
    },
    BookInfo {
        name: "Deuteronomy",
        short: "Deut",
    },
    BookInfo {
        name: "Joshua",
        short: "Josh",
    },
    BookInfo {
        name: "Judges",
        short: "Judg",
    },
    BookInfo {
        name: "Ruth",
        short: "Ruth",
    },
    BookInfo {
        name: "1 Samuel",
        short: "1 Sam",
    },
    BookInfo {
        name: "2 Samuel",
        short: "2 Sam",
    },
    BookInfo {
        name: "1 Kings",
        short: "1 Kgs",
    },
    BookInfo {
        name: "2 Kings",
        short: "2 Kgs",
    },
    BookInfo {
        name: "1 Chronicles",
        short: "1 Chr",
    },
    BookInfo {
        name: "2 Chronicles",
        short: "2 Chr",
    },
    BookInfo {
        name: "Ezra",
        short: "Ezra",
    },
    BookInfo {
        name: "Nehemiah",
        short: "Neh",
    },
    BookInfo {
        name: "Esther",
        short: "Esth",
    },
    BookInfo {
        name: "Job",
        short: "Job",
    },
    BookInfo {
        name: "Psalms",
        short: "Ps",
    },
    BookInfo {
        name: "Proverbs",
        short: "Prov",
    },
    BookInfo {
        name: "Ecclesiastes",
        short: "Eccl",
    },
    BookInfo {
        name: "Song of Solomon",
        short: "Song",
    },
    BookInfo {
        name: "Isaiah",
        short: "Isa",
    },
    BookInfo {
        name: "Jeremiah",
        short: "Jer",
    },
    BookInfo {
        name: "Lamentations",
        short: "Lam",
    },
    BookInfo {
        name: "Ezekiel",
        short: "Ezek",
    },
    BookInfo {
        name: "Daniel",
        short: "Dan",
    },
    BookInfo {
        name: "Hosea",
        short: "Hos",
    },
    BookInfo {
        name: "Joel",
        short: "Joel",
    },
    BookInfo {
        name: "Amos",
        short: "Amos",
    },
    BookInfo {
        name: "Obadiah",
        short: "Obad",
    },
    BookInfo {
        name: "Jonah",
        short: "Jonah",
    },
    BookInfo {
        name: "Micah",
        short: "Mic",
    },
    BookInfo {
        name: "Nahum",
        short: "Nah",
    },
    BookInfo {
        name: "Habakkuk",
        short: "Hab",
    },
    BookInfo {
        name: "Zephaniah",
        short: "Zeph",
    },
    BookInfo {
        name: "Haggai",
        short: "Hag",
    },
    BookInfo {
        name: "Zechariah",
        short: "Zech",
    },
    BookInfo {
        name: "Malachi",
        short: "Mal",
    },
    BookInfo {
        name: "Matthew",
        short: "Matt",
    },
    BookInfo {
        name: "Mark",
        short: "Mark",
    },
    BookInfo {
        name: "Luke",
        short: "Luke",
    },
    BookInfo {
        name: "John",
        short: "John",
    },
    BookInfo {
        name: "Acts",
        short: "Acts",
    },
    BookInfo {
        name: "Romans",
        short: "Rom",
    },
    BookInfo {
        name: "1 Corinthians",
        short: "1 Cor",
    },
    BookInfo {
        name: "2 Corinthians",
        short: "2 Cor",
    },
    BookInfo {
        name: "Galatians",
        short: "Gal",
    },
    BookInfo {
        name: "Ephesians",
        short: "Eph",
    },
    BookInfo {
        name: "Philippians",
        short: "Phil",
    },
    BookInfo {
        name: "Colossians",
        short: "Col",
    },
    BookInfo {
        name: "1 Thessalonians",
        short: "1 Thess",
    },
    BookInfo {
        name: "2 Thessalonians",
        short: "2 Thess",
    },
    BookInfo {
        name: "1 Timothy",
        short: "1 Tim",
    },
    BookInfo {
        name: "2 Timothy",
        short: "2 Tim",
    },
    BookInfo {
        name: "Titus",
        short: "Titus",
    },
    BookInfo {
        name: "Philemon",
        short: "Phlm",
    },
    BookInfo {
        name: "Hebrews",
        short: "Heb",
    },
    BookInfo {
        name: "James",
        short: "Jas",
    },
    BookInfo {
        name: "1 Peter",
        short: "1 Pet",
    },
    BookInfo {
        name: "2 Peter",
        short: "2 Pet",
    },
    BookInfo {
        name: "1 John",
        short: "1 John",
    },
    BookInfo {
        name: "2 John",
        short: "2 John",
    },
    BookInfo {
        name: "3 John",
        short: "3 John",
    },
    BookInfo {
        name: "Jude",
        short: "Jude",
    },
    BookInfo {
        name: "Revelation",
        short: "Rev",
    },
];

/// Psalms has the most chapters (150); anything far beyond that is bad data
/// and would otherwise allocate thousands of empty chapters.
const MAX_CHAPTER: u16 = 200;

/// Index of Matthew: books before this are the Old Testament.
pub const FIRST_NT_BOOK: usize = 39;

pub struct Chapter {
    /// (verse number, text). Stored explicitly rather than by position so an
    /// imported translation with gaps in its numbering still lines up.
    pub verses: Vec<(u16, String)>,
}

pub struct Translation {
    pub id: String,
    pub title: String,
    /// `books[book_index][chapter - 1]`
    pub books: Vec<Vec<Chapter>>,
}

impl Translation {
    pub fn chapter(&self, book: u8, chapter: u16) -> Option<&Chapter> {
        self.books
            .get(book as usize)?
            .get((chapter as usize).checked_sub(1)?)
    }

    pub fn chapter_count(&self, book: u8) -> u16 {
        self.books.get(book as usize).map_or(0, |b| b.len() as u16)
    }

    pub fn verse_text(&self, book: u8, chapter: u16, verse: u16) -> Option<&str> {
        self.chapter(book, chapter)?
            .verses
            .iter()
            .find(|(n, _)| *n == verse)
            .map(|(_, t)| t.as_str())
    }
}

static EMBEDDED: &[(&str, &[u8])] = &[
    ("KJV", include_bytes!("../assets/bibles/KJV.tsv.gz")),
    ("BSB", include_bytes!("../assets/bibles/BSB.tsv.gz")),
    ("ASV", include_bytes!("../assets/bibles/ASV.tsv.gz")),
    ("NHEB", include_bytes!("../assets/bibles/NHEB.tsv.gz")),
    ("YLT", include_bytes!("../assets/bibles/YLT.tsv.gz")),
    ("Darby", include_bytes!("../assets/bibles/Darby.tsv.gz")),
    ("Webster", include_bytes!("../assets/bibles/Webster.tsv.gz")),
    (
        "Geneva1599",
        include_bytes!("../assets/bibles/Geneva1599.tsv.gz"),
    ),
];

fn empty_books() -> Vec<Vec<Chapter>> {
    (0..BOOKS.len()).map(|_| Vec::new()).collect()
}

/// Place a verse, growing the chapter list as needed.
fn push_verse(books: &mut [Vec<Chapter>], book: usize, chapter: u16, verse: u16, text: String) {
    let chapters = &mut books[book];
    while chapters.len() < chapter as usize {
        chapters.push(Chapter { verses: Vec::new() });
    }
    chapters[chapter as usize - 1].verses.push((verse, text));
}

/// Parse the TSV format written by `tools/convert_scrollmapper.py`.
fn parse_tsv(name: &str, gz: &[u8]) -> Result<Translation, String> {
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_string(&mut text)
        .map_err(|e| format!("{name}: cannot decompress: {e}"))?;
    let mut lines = text.lines();
    let header = lines.next().ok_or_else(|| format!("{name}: empty file"))?;
    let (id, title) = header
        .strip_prefix('#')
        .and_then(|h| h.split_once('\t'))
        .ok_or_else(|| format!("{name}: bad header line"))?;
    let mut books = empty_books();
    for (n, line) in lines.enumerate() {
        let mut parts = line.splitn(4, '\t');
        let bad = || format!("{name}: malformed line {}", n + 2);
        let book: usize = parts.next().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
        let chapter: u16 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
        let verse: u16 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
        let body = parts.next().ok_or_else(bad)?;
        if book >= BOOKS.len() || chapter == 0 || chapter > MAX_CHAPTER {
            return Err(bad());
        }
        push_verse(&mut books, book, chapter, verse, body.to_owned());
    }
    Ok(Translation {
        id: id.to_owned(),
        title: title.to_owned(),
        books,
    })
}

// --- scrollmapper JSON (user-supplied translations) ---

#[derive(serde::Deserialize)]
struct SmFile {
    translation: String,
    books: Vec<SmBook>,
}
#[derive(serde::Deserialize)]
struct SmBook {
    chapters: Vec<SmChapter>,
}
#[derive(serde::Deserialize)]
struct SmChapter {
    chapter: u16,
    verses: Vec<SmVerse>,
}
#[derive(serde::Deserialize)]
struct SmVerse {
    verse: u16,
    text: String,
}

fn parse_scrollmapper_json(path: &Path) -> Result<Translation, String> {
    let shown = path.display();
    let raw = std::fs::read(path).map_err(|e| format!("{shown}: {e}"))?;
    let file: SmFile = serde_json::from_slice(&raw).map_err(|e| format!("{shown}: {e}"))?;
    if file.books.len() != BOOKS.len() {
        return Err(format!(
            "{shown}: has {} books; only complete 66-book Bibles are supported",
            file.books.len()
        ));
    }
    // "KJV: King James Version (1769)" -> id "KJV", title "King James Version (1769)"
    let (id, title) = match file.translation.split_once(':') {
        Some((i, t)) => (i.trim().to_owned(), t.trim().to_owned()),
        None => {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
            (stem.unwrap_or_default(), file.translation.trim().to_owned())
        }
    };
    if id.is_empty() {
        return Err(format!("{shown}: translation has no identifier"));
    }
    let mut books = empty_books();
    for (bi, b) in file.books.into_iter().enumerate() {
        for c in b.chapters {
            if c.chapter == 0 || c.chapter > MAX_CHAPTER {
                return Err(format!(
                    "{shown}: impossible chapter {} in {}",
                    c.chapter, BOOKS[bi].name
                ));
            }
            for v in c.verses {
                let text = v.text.split_whitespace().collect::<Vec<_>>().join(" ");
                push_verse(&mut books, bi, c.chapter, v.verse, text);
            }
        }
    }
    Ok(Translation { id, title, books })
}

/// Directory users can drop extra scrollmapper JSON Bibles into.
pub fn user_bible_dir() -> Option<std::path::PathBuf> {
    crate::store::app_data_dir().map(|d| d.join("bibles"))
}

/// Load every built-in translation (decoded in parallel) plus any user JSON
/// files. Problems are returned as messages instead of aborting: one bad file
/// must not stop the app from opening.
pub fn load_all() -> (Vec<Translation>, Vec<String>) {
    let mut errors = Vec::new();
    let results: Vec<Result<Translation, String>> = std::thread::scope(|s| {
        let handles: Vec<_> = EMBEDDED
            .iter()
            .map(|(name, gz)| s.spawn(move || parse_tsv(name, gz)))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("translation loader panicked".into()))
            })
            .collect()
    });
    let mut out = Vec::new();
    for r in results {
        match r {
            Ok(t) => out.push(t),
            Err(e) => errors.push(e),
        }
    }

    if let Some(dir) = user_bible_dir()
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        let mut paths: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("json"))
            })
            .collect();
        paths.sort();
        for p in paths {
            match parse_scrollmapper_json(&p) {
                Ok(t) if out.iter().any(|o| o.id == t.id) => errors.push(format!(
                    "{}: a translation with id \"{}\" is already loaded; skipped",
                    p.display(),
                    t.id
                )),
                Ok(t) => out.push(t),
                Err(e) => errors.push(e),
            }
        }
    }
    (out, errors)
}

// --- reference parsing ("John 3:16", "1 cor 13", "ps 23:1-4") ---

/// A parsed reference. `verse_end` is set only for ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedRef {
    pub book: u8,
    pub chapter: u16,
    pub verse: Option<u16>,
    pub verse_end: Option<u16>,
}

/// Lowercase, map leading roman numerals / ordinals to digits, keep only
/// alphanumerics: "I John" -> "1john", "Song of Solomon" -> "songofsolomon".
fn normalize_book(s: &str) -> String {
    let lower = s.trim().to_lowercase();
    let mut rest = lower.as_str();
    let mut prefix = "";
    for (word, digit) in [
        ("iii ", "3"),
        ("ii ", "2"),
        ("i ", "1"),
        ("third ", "3"),
        ("second ", "2"),
        ("first ", "1"),
        ("3rd ", "3"),
        ("2nd ", "2"),
        ("1st ", "1"),
    ] {
        if let Some(r) = rest.strip_prefix(word) {
            prefix = digit;
            rest = r;
            break;
        }
    }
    let mut out = String::from(prefix);
    out.extend(rest.chars().filter(|c| c.is_alphanumeric()));
    out
}

/// Common abbreviations that are not unique prefixes of a book name.
const ALIASES: &[(&str, usize)] = &[
    ("jn", 42),
    ("jhn", 42),
    ("mt", 39),
    ("mk", 40),
    ("mrk", 40),
    ("lk", 41),
    ("jdg", 6),
    ("judg", 6),
    ("jud", 64),
    ("ps", 18),
    ("psa", 18),
    ("psalm", 18),
    ("phil", 49),
    ("php", 49),
    ("phlm", 56),
    ("phm", 56),
    ("song", 21),
    ("songofsongs", 21),
    ("sos", 21),
    ("canticles", 21),
    ("ezek", 25),
    ("ezk", 25),
    ("jas", 58),
    ("jm", 58),
    ("rev", 65),
    ("revelations", 65),
    ("eccles", 20),
    ("qoh", 20),
];

pub fn find_book(input: &str) -> Option<u8> {
    find_book_inner(input, true)
}

/// Exact names, aliases or abbreviations only (no prefixes), so a bare word
/// like "mark" or "number" isn't mistaken for a book unless it is one.
fn find_book_exact(input: &str) -> Option<u8> {
    find_book_inner(input, false)
}

fn find_book_inner(input: &str, allow_prefix: bool) -> Option<u8> {
    let key = normalize_book(input);
    if key.is_empty() {
        return None;
    }
    let names: Vec<String> = BOOKS.iter().map(|b| normalize_book(b.name)).collect();
    if let Some(i) = names.iter().position(|n| *n == key) {
        return Some(i as u8);
    }
    if let Some((_, i)) = ALIASES.iter().find(|(a, _)| *a == key) {
        return Some(*i as u8);
    }
    let shorts: Vec<String> = BOOKS.iter().map(|b| normalize_book(b.short)).collect();
    if let Some(i) = shorts.iter().position(|n| *n == key) {
        return Some(i as u8);
    }
    if !allow_prefix {
        return None;
    }
    // Unique prefix only: "gen" -> Genesis, but "jo" is ambiguous.
    let mut hits = names
        .iter()
        .enumerate()
        .filter(|(_, n)| n.starts_with(&key));
    match (hits.next(), hits.next()) {
        (Some((i, _)), None) => Some(i as u8),
        _ => None,
    }
}

pub fn parse_reference(input: &str) -> Option<ParsedRef> {
    let s = input.trim();
    // Split where the trailing "3:16-18" part begins (scan back over digits,
    // ':', '.', '-', spaces). A leading book number ("1 John") stays with the
    // book because a letter stops the scan.
    let split = s
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit() || matches!(c, ':' | '.' | '-' | ' '))
        .last()
        .map(|(i, _)| i);
    let Some(split) = split else {
        // A bare book name ("Romans", "1 John") opens chapter 1.
        let book = find_book_exact(s)?;
        return Some(ParsedRef {
            book,
            chapter: 1,
            verse: None,
            verse_end: None,
        });
    };
    let (book_part, nums) = s.split_at(split);
    let book = find_book(book_part)?;
    let nums = nums.trim();
    let (chap_s, verse_s) = match nums.split_once([':', '.']) {
        Some((c, v)) => (c.trim(), Some(v.trim())),
        None => (nums, None),
    };
    let chapter: u16 = chap_s.parse().ok().filter(|c| *c > 0)?;
    let (verse, verse_end) = match verse_s {
        None => (None, None),
        Some(v) => match v.split_once('-') {
            Some((a, b)) => {
                let a: u16 = a.trim().parse().ok()?;
                let b: u16 = b.trim().parse().ok()?;
                (Some(a), Some(b.max(a)))
            }
            None => (Some(v.parse().ok()?), None),
        },
    };
    Some(ParsedRef {
        book,
        chapter,
        verse,
        verse_end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_translations_load() {
        let (ts, errs) = load_all();
        assert!(ts.len() >= EMBEDDED.len(), "errors: {errs:?}");
        for t in &ts[..EMBEDDED.len()] {
            assert_eq!(t.books.len(), 66, "{}", t.id);
            assert_eq!(t.chapter_count(0), 50, "{} Genesis", t.id);
            assert_eq!(t.chapter_count(18), 150, "{} Psalms", t.id);
            assert_eq!(t.chapter_count(65), 22, "{} Revelation", t.id);
        }
        let kjv = ts.iter().find(|t| t.id == "KJV").unwrap();
        assert_eq!(
            kjv.verse_text(0, 1, 1),
            Some("In the beginning God created the heaven and the earth.")
        );
    }

    #[test]
    fn references() {
        let book = |s: &str| parse_reference(s).map(|p| BOOKS[p.book as usize].name);
        let full = parse_reference("John 3:16").unwrap();
        assert_eq!(BOOKS[full.book as usize].name, "John");
        assert_eq!(
            (full.chapter, full.verse, full.verse_end),
            (3, Some(16), None)
        );
        let range = parse_reference("ps 23:1-4").unwrap();
        assert_eq!(BOOKS[range.book as usize].name, "Psalms");
        assert_eq!(
            (range.chapter, range.verse, range.verse_end),
            (23, Some(1), Some(4))
        );
        assert_eq!(parse_reference("1 cor 13").map(|p| p.chapter), Some(13));
        assert_eq!(book("1 cor 13"), Some("1 Corinthians"));
        assert_eq!(book("I John 1:9"), Some("1 John"));
        assert_eq!(book("3john 1"), Some("3 John"));
        assert_eq!(book("gen 1"), Some("Genesis"));
        assert_eq!(book("Song of Solomon 2:1"), Some("Song of Solomon"));
        assert_eq!(book("jude 3"), Some("Jude"));
        assert_eq!(book("judges 3"), Some("Judges"));
        assert_eq!(book("phil 4:13"), Some("Philippians"));
        assert_eq!(book("phlm 1"), Some("Philemon"));
        assert_eq!(book("jn 1"), Some("John"));
        assert_eq!(book("mt 5"), Some("Matthew"));
        assert_eq!(book("jas 1"), Some("James"));
        assert_eq!(book("ezek 37"), Some("Ezekiel"));
        assert_eq!(book("eccles 3"), Some("Ecclesiastes"));
        assert_eq!(book("rev 21"), Some("Revelation"));
        assert_eq!(BOOKS[FIRST_NT_BOOK].name, "Matthew");
        assert_eq!(parse_reference("jo 3"), None); // ambiguous prefix
        assert_eq!(book("Romans"), Some("Romans"));
        assert_eq!(
            parse_reference("1 John").map(|p| (p.chapter, p.verse)),
            Some((1, None))
        );
        assert_eq!(book("1st John 2"), Some("1 John"));
        assert_eq!(book("Song of Songs 2"), Some("Song of Solomon"));
        assert_eq!(parse_reference("mark").map(|p| p.book), Some(40)); // exact name
        assert_eq!(parse_reference("number"), None); // prefix only: stays a search
        assert_eq!(parse_reference("grace"), None);
        assert_eq!(parse_reference("love"), None);
        assert_eq!(parse_reference("John 0"), None);
    }
}
