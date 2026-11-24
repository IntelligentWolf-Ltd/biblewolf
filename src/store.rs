//! Highlights and sticky notes, persisted as JSON in the user's data dir.
//!
//! Annotations are keyed by reference (book/chapter/verse), not by
//! translation, so a highlight made in the KJV also shows in the BSB pane.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct VerseRef {
    pub book: u8,
    pub chapter: u16,
    pub verse: u16,
}

/// Colour shared by highlighter pens and sticky notes.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Swatch {
    Yellow,
    Green,
    Blue,
    Pink,
    Orange,
}

impl Swatch {
    pub const ALL: [Swatch; 5] = [
        Swatch::Yellow,
        Swatch::Green,
        Swatch::Blue,
        Swatch::Pink,
        Swatch::Orange,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Swatch::Yellow => "Yellow",
            Swatch::Green => "Green",
            Swatch::Blue => "Blue",
            Swatch::Pink => "Pink",
            Swatch::Orange => "Orange",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Note {
    pub id: u64,
    pub book: u8,
    pub chapter: u16,
    pub verse_start: u16,
    pub verse_end: u16,
    pub text: String,
    pub color: Swatch,
    /// Unix seconds.
    pub created: u64,
    pub modified: u64,
    /// Whether the sticky is currently pinned open on screen.
    #[serde(default)]
    pub open: bool,
}

impl Note {
    pub fn covers(&self, book: u8, chapter: u16, verse: u16) -> bool {
        self.book == book
            && self.chapter == chapter
            && (self.verse_start..=self.verse_end).contains(&verse)
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct Highlight {
    #[serde(flatten)]
    at: VerseRef,
    color: Swatch,
}

/// On-disk layout. `version` lets a future format change migrate cleanly.
#[derive(Serialize, Deserialize)]
struct FileFormat {
    version: u32,
    next_id: u64,
    highlights: Vec<Highlight>,
    notes: Vec<Note>,
}

const FORMAT_VERSION: u32 = 1;

/// Folder name under the user data dir. Must equal the lowercased app name
/// passed to `eframe::run_native`, which eframe uses for its own storage.
pub const APP_DIR: &str = "biblewolf";
/// Folder used before the app was named BibleWolf.
const LEGACY_APP_DIR: &str = "bible-study";

pub fn app_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_DIR))
}

/// Move data saved under the pre-rename folder to the BibleWolf folder.
/// Runs before eframe opens its storage. Never merges: if both folders exist
/// the old one is left untouched.
pub fn migrate_legacy_dir() -> Result<Option<String>, String> {
    let Some(base) = dirs::data_dir() else {
        return Ok(None);
    };
    let (old, new) = (base.join(LEGACY_APP_DIR), base.join(APP_DIR));
    if !old.is_dir() || new.exists() {
        return Ok(None);
    }
    std::fs::rename(&old, &new)
        .map(|()| Some(format!("Moved your notes and settings to {}.", new.display())))
        .map_err(|e| {
            format!(
                "Could not move your existing notes from {} to {}: {e}. They are still in the old folder.",
                old.display(),
                new.display()
            )
        })
}

/// Keep only entries that point at a real book/chapter/verse position.
/// Returns (highlights, notes, number dropped).
fn validate(hs: Vec<Highlight>, ns: Vec<Note>) -> (HashMap<VerseRef, Swatch>, Vec<Note>, usize) {
    let books = crate::bible::BOOKS.len();
    let ok = |b: u8, c: u16, v: u16| (b as usize) < books && c >= 1 && v >= 1;
    let mut dropped = 0;
    let mut highlights = HashMap::new();
    for h in hs {
        if ok(h.at.book, h.at.chapter, h.at.verse) {
            highlights.insert(h.at, h.color);
        } else {
            dropped += 1;
        }
    }
    let mut notes = Vec::new();
    for mut n in ns {
        if ok(n.book, n.chapter, n.verse_start.min(n.verse_end)) {
            let (a, b) = (
                n.verse_start.min(n.verse_end),
                n.verse_start.max(n.verse_end),
            );
            n.verse_start = a;
            n.verse_end = b;
            notes.push(n);
        } else {
            dropped += 1;
        }
    }
    (highlights, notes, dropped)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub struct Store {
    /// `None` = read-only session (nothing will be written).
    path: Option<PathBuf>,
    /// Advisory lock held for the app's lifetime so a second BibleWolf
    /// window can't silently overwrite this one's notes (last-writer-wins).
    _lock: Option<std::fs::File>,
    pub highlights: HashMap<VerseRef, Swatch>,
    pub notes: Vec<Note>,
    next_id: u64,
    dirty_since: Option<Instant>,
}

impl Store {
    pub fn default_path() -> Option<PathBuf> {
        app_data_dir().map(|d| d.join("annotations.json"))
    }

    fn empty(path: Option<PathBuf>) -> Self {
        Self {
            path,
            _lock: None,
            highlights: HashMap::new(),
            notes: Vec::new(),
            next_id: 1,
            dirty_since: None,
        }
    }

    /// Load annotations. Returns the store plus any problems to show the user.
    ///
    /// Safety rules — the user's notes are never destroyed:
    /// - missing file: normal first run;
    /// - another BibleWolf already has the file locked: read-only session;
    /// - unparseable file: moved aside as `annotations.corrupt-<time>.json`;
    /// - written by a newer format version: read-only session;
    /// - entries pointing at impossible verses: the original is copied to
    ///   `annotations.invalid-<time>.json` and only valid entries are loaded.
    pub fn load(path: Option<PathBuf>) -> (Self, Vec<String>) {
        let mut msgs = Vec::new();
        let Some(p) = path.clone() else {
            msgs.push(
                "No user data directory found; notes and highlights will not be saved.".into(),
            );
            return (Self::empty(None), msgs);
        };

        let lock = match Self::acquire_lock(&p) {
            Ok(l) => l,
            Err(e) => {
                msgs.push(e);
                None
            }
        };
        let read_only = lock.is_none();
        if read_only && msgs.is_empty() {
            msgs.push(
                "BibleWolf is already open in another window. Notes and highlights you change here will not be saved."
                    .into(),
            );
        }

        let raw = match std::fs::read(&p) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut st = Self::empty(if read_only { None } else { path });
                st._lock = lock;
                return (st, msgs);
            }
            Err(e) => {
                msgs.push(format!(
                    "Could not read {}: {e}. Notes and highlights will not be saved this session.",
                    p.display()
                ));
                return (Self::empty(None), msgs);
            }
        };
        let f = match serde_json::from_slice::<FileFormat>(&raw) {
            Ok(f) => f,
            Err(e) if read_only => {
                msgs.push(format!("Your notes file could not be read ({e})."));
                return (Self::empty(None), msgs);
            }
            Err(e) => {
                let backup = p.with_file_name(format!("annotations.corrupt-{}.json", now_secs()));
                match std::fs::rename(&p, &backup) {
                    Ok(()) => {
                        msgs.push(format!(
                            "Your notes file could not be read ({e}). It was kept as {} and a fresh one started.",
                            backup.display()
                        ));
                        let mut st = Self::empty(path);
                        st._lock = lock;
                        return (st, msgs);
                    }
                    Err(re) => {
                        msgs.push(format!(
                            "Your notes file could not be read ({e}) and could not be moved aside ({re}). \
                             Saving is disabled this session so it is not overwritten."
                        ));
                        return (Self::empty(None), msgs);
                    }
                }
            }
        };

        let mut writable = !read_only;
        if f.version > FORMAT_VERSION {
            msgs.push(format!(
                "Your notes were saved by a newer version of BibleWolf (format {}). \
                 They are shown read-only so this version doesn't overwrite them.",
                f.version
            ));
            writable = false;
        }

        let (highlights, notes, dropped) = validate(f.highlights, f.notes);
        if dropped > 0 {
            if writable {
                let backup = p.with_file_name(format!("annotations.invalid-{}.json", now_secs()));
                match std::fs::copy(&p, &backup) {
                    Ok(_) => msgs.push(format!(
                        "{dropped} saved note/highlight entries point at verses that don't exist and were skipped. \
                         The original file was kept as {}.",
                        backup.display()
                    )),
                    Err(e) => {
                        msgs.push(format!(
                            "{dropped} saved entries are invalid and a backup could not be made ({e}). \
                             Saving is disabled this session so nothing is lost."
                        ));
                        writable = false;
                    }
                }
            } else {
                msgs.push(format!(
                    "{dropped} saved note/highlight entries are invalid and were skipped."
                ));
            }
        }

        let max_id = notes.iter().map(|n| n.id).max().unwrap_or(0);
        let store = Self {
            path: if writable { path } else { None },
            _lock: lock,
            highlights,
            notes,
            next_id: f.next_id.max(max_id + 1),
            dirty_since: None,
        };
        (store, msgs)
    }

    /// `Ok(Some)` = we own the lock; `Ok(None)` = another instance holds it.
    fn acquire_lock(annotations: &std::path::Path) -> Result<Option<std::fs::File>, String> {
        let lock_path = annotations.with_file_name("biblewolf.lock");
        let open = || -> std::io::Result<std::fs::File> {
            if let Some(dir) = lock_path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&lock_path)
        };
        let file = open().map_err(|e| {
            format!(
                "Could not open {}: {e}. Notes and highlights will not be saved this session.",
                lock_path.display()
            )
        })?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => Err(format!(
                "Could not lock {}: {e}. Notes and highlights will not be saved this session.",
                lock_path.display()
            )),
        }
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    pub fn mark_dirty(&mut self) {
        if self.dirty_since.is_none() {
            self.dirty_since = Some(Instant::now());
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty_since.is_some()
    }

    /// Dirty for at least `secs` — used to batch rapid edits (typing in a note).
    pub fn dirty_for(&self, secs: f32) -> bool {
        self.dirty_since
            .is_some_and(|t| t.elapsed().as_secs_f32() >= secs)
    }

    /// Write atomically: temp file in the same directory, fsync, rename.
    pub fn save(&mut self) -> Result<(), String> {
        let Some(path) = self.path.clone() else {
            self.dirty_since = None;
            return Ok(());
        };
        let mut highlights: Vec<Highlight> = self
            .highlights
            .iter()
            .map(|(at, color)| Highlight {
                at: *at,
                color: *color,
            })
            .collect();
        highlights.sort_by_key(|h| (h.at.book, h.at.chapter, h.at.verse));
        let file = FileFormat {
            version: FORMAT_VERSION,
            next_id: self.next_id,
            highlights,
            notes: self.notes.clone(),
        };
        let json = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
        let write = || -> std::io::Result<()> {
            use std::io::Write;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("json.saving");
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&json)?;
            f.sync_all()?;
            drop(f);
            std::fs::rename(&tmp, &path)
        };
        write().map_err(|e| format!("Could not save notes to {}: {e}", path.display()))?;
        self.dirty_since = None;
        Ok(())
    }

    // --- highlights ---

    pub fn set_highlight(&mut self, at: VerseRef, color: Option<Swatch>) {
        match color {
            Some(c) => self.highlights.insert(at, c),
            None => self.highlights.remove(&at),
        };
        self.mark_dirty();
    }

    // --- notes ---

    pub fn add_note(&mut self, book: u8, chapter: u16, start: u16, end: u16, color: Swatch) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let t = now_secs();
        self.notes.push(Note {
            id,
            book,
            chapter,
            verse_start: start.min(end),
            verse_end: start.max(end),
            text: String::new(),
            color,
            created: t,
            modified: t,
            open: true,
        });
        self.mark_dirty();
        id
    }

    pub fn note_mut(&mut self, id: u64) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    pub fn remove_note(&mut self, id: u64) -> Option<Note> {
        let i = self.notes.iter().position(|n| n.id == id)?;
        self.mark_dirty();
        Some(self.notes.remove(i))
    }

    /// Put a deleted note back (undo).
    pub fn restore_note(&mut self, note: Note) {
        self.notes.push(note);
        self.mark_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        // Disk-backed, inside the build dir (not /tmp).
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("annotations.json")
    }

    #[test]
    fn round_trip() {
        let p = temp_path("roundtrip");
        let (mut s, err) = Store::load(Some(p.clone()));
        assert!(err.is_empty(), "{err:?}");
        let at = VerseRef {
            book: 42,
            chapter: 3,
            verse: 16,
        };
        s.set_highlight(at, Some(Swatch::Green));
        let id = s.add_note(42, 3, 18, 16, Swatch::Pink);
        s.note_mut(id).unwrap().text = "God so loved".into();
        s.save().unwrap();
        assert!(!s.is_dirty());
        drop(s); // release the instance lock

        let (s2, err) = Store::load(Some(p));
        assert!(err.is_empty(), "{err:?}");
        assert_eq!(s2.highlights.get(&at), Some(&Swatch::Green));
        let n = &s2.notes[0];
        assert_eq!((n.verse_start, n.verse_end), (16, 18));
        assert!(n.covers(42, 3, 17) && !n.covers(42, 3, 19));
        assert_eq!(n.text, "God so loved");
        assert!(s2.next_id > id);
    }

    #[test]
    fn corrupt_file_is_moved_aside_not_overwritten() {
        let p = temp_path("corrupt");
        std::fs::write(&p, b"{ not json").unwrap();
        let (s, err) = Store::load(Some(p.clone()));
        assert_eq!(err.len(), 1);
        assert!(s.path().is_some());
        assert!(!p.exists());
        let backups: Vec<_> = std::fs::read_dir(p.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("annotations.corrupt-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(backups[0].path()).unwrap(), b"{ not json");
    }

    #[test]
    fn second_instance_is_read_only() {
        let p = temp_path("lock");
        let (first, err) = Store::load(Some(p.clone()));
        assert!(err.is_empty() && first.path().is_some());
        let (second, err) = Store::load(Some(p.clone()));
        assert!(second.path().is_none(), "second instance must not write");
        assert_eq!(err.len(), 1);
        drop(first);
        drop(second);
        let (third, err) = Store::load(Some(p));
        assert!(
            err.is_empty() && third.path().is_some(),
            "lock released on drop"
        );
    }

    #[test]
    fn invalid_entries_skipped_and_original_backed_up() {
        let p = temp_path("invalid");
        let json = r#"{"version":1,"next_id":3,
            "highlights":[{"book":70,"chapter":1,"verse":1,"color":"Yellow"},
                          {"book":42,"chapter":3,"verse":16,"color":"Blue"}],
            "notes":[{"id":1,"book":0,"chapter":0,"verse_start":1,"verse_end":1,"text":"x",
                      "color":"Pink","created":0,"modified":0},
                     {"id":2,"book":0,"chapter":1,"verse_start":3,"verse_end":1,"text":"ok",
                      "color":"Pink","created":0,"modified":0}]}"#;
        std::fs::write(&p, json).unwrap();
        let (s, err) = Store::load(Some(p.clone()));
        assert_eq!(err.len(), 1, "{err:?}");
        assert_eq!(s.highlights.len(), 1);
        assert_eq!(s.notes.len(), 1);
        assert_eq!((s.notes[0].verse_start, s.notes[0].verse_end), (1, 3));
        assert!(s.path().is_some());
        let backups = std::fs::read_dir(p.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("annotations.invalid-")
            })
            .count();
        assert_eq!(backups, 1);
        assert!(p.exists(), "original left in place");
    }

    #[test]
    fn newer_format_is_read_only() {
        let p = temp_path("newer");
        std::fs::write(
            &p,
            r#"{"version":99,"next_id":1,"highlights":[],"notes":[]}"#,
        )
        .unwrap();
        let (s, err) = Store::load(Some(p));
        assert_eq!(err.len(), 1);
        assert!(s.path().is_none());
    }
}
