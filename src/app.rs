//! The application: library tree, side-by-side reading panes, highlighter,
//! sticky notes, search and status bar.
//!
//! UI code never mutates state directly; it pushes `Action`s which are applied
//! once per frame in `apply`. That keeps the rendering functions `&self` and
//! puts every state transition (and its user-visible feedback) in one place.

use crate::bible::{self, BOOKS, FIRST_NT_BOOK, Translation};
use crate::store::{self, Note, Store, Swatch, VerseRef};
use crate::theme::{self, Palette};
use egui::text::LayoutJob;
use egui::{
    Align, Color32, CornerRadius, FontId, Frame, Id, Key, Layout, Margin, Panel, Pos2, Rect,
    RichText, Sense, Shadow, Stroke, TextFormat, Ui, Vec2, vec2,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const MAX_PANES: usize = 4;
const WEBSITE: &str = "https://wolf.uk.com";
const MIN_PANE_WIDTH: f32 = 340.0;
const MAX_SEARCH_HITS: usize = 500;
const SAVE_DEBOUNCE_SECS: f32 = 0.8;
const SAVE_RETRY: Duration = Duration::from_secs(10);
const INFO_FLASH: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Persisted UI settings
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PaneState {
    id: u64,
    translation: String,
    book: u8,
    chapter: u16,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
enum SideTab {
    Notes,
    Highlights,
    Search,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(default)]
struct Settings {
    panes: Vec<PaneState>,
    focused: usize,
    linked: bool,
    font_size: f32,
    dark: bool,
    show_side: bool,
    show_library: bool,
    side_tab: SideTab,
    next_pane_id: u64,
    pen: Swatch,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            panes: Vec::new(),
            focused: 0,
            linked: true,
            font_size: 18.0,
            dark: true,
            show_side: true,
            show_library: true,
            side_tab: SideTab::Notes,
            next_pane_id: 1,
            pen: Swatch::Yellow,
        }
    }
}

// ---------------------------------------------------------------------------
// Transient state
// ---------------------------------------------------------------------------

/// A verse range within one chapter. Selection is by reference, so the same
/// verses light up in every pane showing that chapter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Selection {
    book: u8,
    chapter: u16,
    anchor: u16,
    start: u16,
    end: u16,
}

impl Selection {
    fn single(book: u8, chapter: u16, verse: u16) -> Self {
        Self {
            book,
            chapter,
            anchor: verse,
            start: verse,
            end: verse,
        }
    }
    fn contains(&self, book: u8, chapter: u16, verse: u16) -> bool {
        self.book == book && self.chapter == chapter && (self.start..=self.end).contains(&verse)
    }
}

fn ref_label(book: u8, chapter: u16, start: u16, end: u16) -> String {
    let name = BOOKS[book as usize].name;
    if start == end {
        format!("{name} {chapter}:{start}")
    } else {
        format!("{name} {chapter}:{start}\u{2013}{end}")
    }
}

struct SearchHit {
    book: u8,
    chapter: u16,
    verse: u16,
    text: String,
    /// Byte range of the first match in `text`, when it can be mapped exactly.
    mark: Option<(usize, usize)>,
}

struct SearchState {
    query: String,
    translation: String,
    hits: Vec<SearchHit>,
    total: usize,
}

/// Transient confirmation in the status bar (auto-fades). Errors are kept
/// separately in `BibleApp::errors` and never overwritten by these.
struct Flash {
    text: String,
    at: Instant,
    /// Show an "Undo" button that restores the most recently deleted note.
    undo: bool,
}

/// How many deleted notes Undo can bring back in one session.
const UNDO_DEPTH: usize = 50;

enum Action {
    Open {
        pane: usize,
        translation: Option<String>,
        book: u8,
        chapter: u16,
        verses: Option<(u16, u16)>,
    },
    StepChapter {
        pane: usize,
        delta: i32,
    },
    SetTranslation {
        pane: usize,
        id: String,
    },
    Focus(usize),
    AddPane,
    ClosePane(usize),
    /// `from_pane` is the pane id clicked in; linked panes other than it scroll to match.
    Select {
        from_pane: u64,
        book: u8,
        chapter: u16,
        verse: u16,
        extend: bool,
        keep_if_inside: bool,
    },
    ClearSelection,
    Highlight(Option<Swatch>),
    AddNote,
    OpenNote(u64),
    HideNote(u64),
    DeleteNote(u64),
    NoteColor(u64, Swatch),
    JumpTo {
        book: u8,
        chapter: u16,
        start: u16,
        end: u16,
    },
    Copy,
    ScrollDone(u64),
    RunOmnibox,
    UndoDelete,
    DismissErrors,
    CopyErrors,
}

pub struct BibleApp {
    bibles: Vec<Translation>,
    store: Store,
    s: Settings,
    selection: Option<Selection>,
    /// pane id -> verse to bring into view (0 = top of chapter)
    pending_scroll: HashMap<u64, u16>,
    omnibox: String,
    focus_omnibox: bool,
    search: Option<SearchState>,
    tree_filter: String,
    flash: Option<Flash>,
    /// Error messages shown in the banner until the user dismisses them.
    errors: Vec<String>,
    /// Deleted notes, most recent last, for Undo.
    deleted: Vec<Note>,
    focus_note: Option<u64>,
    note_spawn_pos: Option<Pos2>,
    last_save_attempt: Option<Instant>,
    save_error: Option<String>,
    show_about: bool,
}

impl BibleApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        migration: Result<Option<String>, String>,
    ) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let (bibles, mut errors) = bible::load_all();
        let (store, store_msgs) = Store::load(Store::default_path());
        errors.extend(store_msgs);
        if let Err(e) = &migration {
            errors.insert(0, e.clone());
        }

        let mut s: Settings = cc
            .storage
            .and_then(|st| eframe::get_value(st, eframe::APP_KEY))
            .unwrap_or_default();
        Self::sanitize(&mut s, &bibles);
        theme::apply(&cc.egui_ctx, s.dark);

        let mut app = Self {
            bibles,
            store,
            s,
            selection: None,
            pending_scroll: HashMap::new(),
            omnibox: String::new(),
            focus_omnibox: false,
            search: None,
            tree_filter: String::new(),
            flash: None,
            errors,
            deleted: Vec::new(),
            focus_note: None,
            note_spawn_pos: None,
            last_save_attempt: None,
            save_error: None,
            show_about: false,
        };
        if let Ok(Some(msg)) = migration {
            app.info(msg);
        }
        app
    }

    /// Repair settings restored from disk (translations may have been removed,
    /// a chapter may not exist in a given translation, etc.).
    fn sanitize(s: &mut Settings, bibles: &[Translation]) {
        s.font_size = s.font_size.clamp(12.0, 32.0);
        s.panes.truncate(MAX_PANES);
        let default_id = bibles.first().map(|t| t.id.clone()).unwrap_or_default();
        if s.panes.is_empty() {
            let second = bibles
                .get(1)
                .map(|t| t.id.clone())
                .unwrap_or_else(|| default_id.clone());
            s.panes.push(PaneState {
                id: 0,
                translation: default_id.clone(),
                book: 42,
                chapter: 1,
            });
            s.panes.push(PaneState {
                id: 0,
                translation: second,
                book: 42,
                chapter: 1,
            });
        }
        let mut seen = std::collections::HashSet::new();
        for p in &mut s.panes {
            if !bibles.iter().any(|t| t.id == p.translation) {
                p.translation = default_id.clone();
            }
            if p.book as usize >= BOOKS.len() {
                p.book = 0;
            }
            let max_ch = bibles
                .iter()
                .find(|t| t.id == p.translation)
                .map_or(1, |t| t.chapter_count(p.book).max(1));
            p.chapter = p.chapter.clamp(1, max_ch);
            if p.id == 0 || !seen.insert(p.id) {
                p.id = s.next_pane_id;
                s.next_pane_id += 1;
                seen.insert(p.id);
            }
        }
        s.next_pane_id = s
            .next_pane_id
            .max(s.panes.iter().map(|p| p.id).max().unwrap_or(0) + 1);
        s.focused = s.focused.min(s.panes.len().saturating_sub(1));
    }

    fn translation(&self, id: &str) -> Option<&Translation> {
        self.bibles.iter().find(|t| t.id == id)
    }

    fn pal(&self) -> Palette {
        theme::palette(self.s.dark)
    }

    fn info(&mut self, text: impl Into<String>) {
        self.flash = Some(Flash {
            text: text.into(),
            at: Instant::now(),
            undo: false,
        });
    }

    /// Errors accumulate in the banner (never auto-dismissed, never replaced
    /// by later confirmations). Repeats of the latest one aren't duplicated.
    fn error(&mut self, text: impl Into<String>) {
        let text = text.into();
        if self.errors.last() != Some(&text) {
            self.errors.push(text);
        }
    }

    // -----------------------------------------------------------------------
    // State transitions
    // -----------------------------------------------------------------------

    fn apply(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Open {
                pane,
                translation,
                book,
                chapter,
                verses,
            } => {
                self.open(pane, translation, book, chapter, verses);
            }
            Action::StepChapter { pane, delta } => {
                let Some(p) = self.s.panes.get(pane) else {
                    return;
                };
                let Some(t) = self.translation(&p.translation) else {
                    return;
                };
                let (mut book, mut chapter) = (p.book as i32, p.chapter as i32 + delta);
                if chapter < 1 {
                    book -= 1;
                    if book < 0 {
                        return;
                    }
                    chapter = t.chapter_count(book as u8) as i32;
                } else if chapter > t.chapter_count(book as u8) as i32 {
                    book += 1;
                    if book >= BOOKS.len() as i32 {
                        return;
                    }
                    chapter = 1;
                }
                self.open(pane, None, book as u8, chapter as u16, None);
            }
            Action::SetTranslation { pane, id } => {
                if let Some(p) = self.s.panes.get_mut(pane) {
                    p.translation = id;
                    let (book, chapter) = (p.book, p.chapter);
                    // Re-open to clamp the chapter for this translation.
                    self.open(pane, None, book, chapter, None);
                }
            }
            Action::Focus(i) => self.s.focused = i.min(self.s.panes.len().saturating_sub(1)),
            Action::AddPane => {
                if self.s.panes.len() >= MAX_PANES {
                    self.error(format!("At most {MAX_PANES} panes fit side by side."));
                    return;
                }
                let base = self.s.panes[self.s.focused].clone();
                // Pick a translation not already on screen, so a new pane is useful.
                let used: Vec<&str> = self
                    .s
                    .panes
                    .iter()
                    .map(|p| p.translation.as_str())
                    .collect();
                let tr = self
                    .bibles
                    .iter()
                    .find(|t| !used.contains(&t.id.as_str()))
                    .map_or(base.translation.clone(), |t| t.id.clone());
                let id = self.s.next_pane_id;
                self.s.next_pane_id += 1;
                self.s.panes.push(PaneState {
                    id,
                    translation: tr.clone(),
                    book: base.book,
                    chapter: base.chapter,
                });
                self.s.focused = self.s.panes.len() - 1;
                let (b, c) = (base.book, base.chapter);
                self.open(self.s.focused, None, b, c, None);
                self.info(format!("Opened a {tr} pane."));
            }
            Action::ClosePane(i) => {
                if self.s.panes.len() > 1 && i < self.s.panes.len() {
                    let p = self.s.panes.remove(i);
                    self.pending_scroll.remove(&p.id);
                    if self.s.focused >= self.s.panes.len() || self.s.focused > i {
                        self.s.focused = self.s.focused.saturating_sub(1);
                    }
                    self.s.focused = self.s.focused.min(self.s.panes.len() - 1);
                }
            }
            Action::Select {
                from_pane,
                book,
                chapter,
                verse,
                extend,
                keep_if_inside,
            } => {
                let cur = self.selection;
                self.selection = match cur {
                    Some(sel) if keep_if_inside && sel.contains(book, chapter, verse) => Some(sel),
                    Some(sel) if extend && sel.book == book && sel.chapter == chapter => {
                        Some(Selection {
                            start: sel.anchor.min(verse),
                            end: sel.anchor.max(verse),
                            ..sel
                        })
                    }
                    Some(sel) if !extend && sel == Selection::single(book, chapter, verse) => None,
                    _ => Some(Selection::single(book, chapter, verse)),
                };
                // Linked panes follow the selection so parallel text lines up.
                if self.s.linked && self.selection.is_some() && !keep_if_inside {
                    for p in &self.s.panes {
                        if p.id != from_pane && p.book == book && p.chapter == chapter {
                            self.pending_scroll.insert(p.id, verse);
                        }
                    }
                }
            }
            Action::ClearSelection => self.selection = None,
            Action::Highlight(color) => {
                let Some(sel) = self.selection else {
                    self.error("Select a verse first, then pick a highlighter colour.");
                    return;
                };
                for v in sel.start..=sel.end {
                    let at = VerseRef {
                        book: sel.book,
                        chapter: sel.chapter,
                        verse: v,
                    };
                    self.store.set_highlight(at, color);
                }
                if let Some(c) = color {
                    self.s.pen = c;
                }
                let r = ref_label(sel.book, sel.chapter, sel.start, sel.end);
                match color {
                    Some(c) => {
                        self.info(format!("Highlighted {r} in {}.", c.name().to_lowercase()))
                    }
                    None => self.info(format!("Removed highlight from {r}.")),
                }
            }
            Action::AddNote => {
                let Some(sel) = self.selection else {
                    self.error("Select a verse first, then add a note.");
                    return;
                };
                let id = self
                    .store
                    .add_note(sel.book, sel.chapter, sel.start, sel.end, self.s.pen);
                self.focus_note = Some(id);
                self.note_spawn_pos = ctx.pointer_latest_pos();
            }
            Action::OpenNote(id) => {
                if let Some(n) = self.store.note_mut(id) {
                    if !n.open {
                        n.open = true;
                        self.store.mark_dirty();
                    }
                    ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, sticky_id(id)));
                    self.focus_note = Some(id);
                }
            }
            Action::HideNote(id) => {
                if let Some(n) = self.store.note_mut(id) {
                    n.open = false;
                    self.store.mark_dirty();
                }
            }
            Action::DeleteNote(id) => {
                if let Some(n) = self.store.remove_note(id) {
                    let r = ref_label(n.book, n.chapter, n.verse_start, n.verse_end);
                    self.deleted.push(n);
                    if self.deleted.len() > UNDO_DEPTH {
                        self.deleted.remove(0);
                    }
                    self.flash = Some(Flash {
                        text: format!("Deleted the note on {r}."),
                        at: Instant::now(),
                        undo: true,
                    });
                }
            }
            Action::UndoDelete => {
                if let Some(n) = self.deleted.pop() {
                    let r = ref_label(n.book, n.chapter, n.verse_start, n.verse_end);
                    self.store.restore_note(n);
                    // Keep offering Undo while older deletions remain.
                    self.flash = Some(Flash {
                        text: format!("Restored the note on {r}."),
                        at: Instant::now(),
                        undo: !self.deleted.is_empty(),
                    });
                }
            }
            Action::NoteColor(id, c) => {
                if let Some(n) = self.store.note_mut(id) {
                    n.color = c;
                    n.modified = store::now_secs();
                    self.store.mark_dirty();
                }
            }
            Action::JumpTo {
                book,
                chapter,
                start,
                end,
            } => {
                let pane = self.s.focused;
                self.open(pane, None, book, chapter, Some((start, end)));
            }
            Action::Copy => self.copy_selection(ctx),
            Action::ScrollDone(pane_id) => {
                self.pending_scroll.remove(&pane_id);
            }
            Action::RunOmnibox => self.run_omnibox(),
            Action::DismissErrors => self.errors.clear(),
            Action::CopyErrors => {
                ctx.copy_text(self.errors.join("\n"));
                self.info("Error text copied.");
            }
        }
    }

    /// Navigate a pane (and, when linked, all panes) to a chapter.
    fn open(
        &mut self,
        pane: usize,
        translation: Option<String>,
        book: u8,
        chapter: u16,
        verses: Option<(u16, u16)>,
    ) {
        if pane >= self.s.panes.len() || book as usize >= BOOKS.len() {
            return;
        }
        if let Some(id) = translation {
            self.s.panes[pane].translation = id;
        }
        // Resolve chapter/verses against the pane the user acted in, so the
        // selection always matches what that pane displays.
        let (chapter, verses) = {
            let tid = self.s.panes[pane].translation.clone();
            let Some(t) = self.translation(&tid) else {
                return;
            };
            let name = BOOKS[book as usize].name;
            let max_ch = t.chapter_count(book).max(1);
            let mut verses = verses;
            let ch = chapter.clamp(1, max_ch);
            let last = t
                .chapter(book, ch)
                .and_then(|c| c.verses.last())
                .map_or(0, |x| x.0);
            if ch != chapter {
                self.error(format!("{name} has {max_ch} chapters in the {tid}."));
                verses = None;
            }
            let verses = verses.and_then(|(a, b)| {
                let (a, b) = (a.min(b), a.max(b));
                (last > 0 && a >= 1 && a <= last).then_some((a, b.min(last)))
            });
            (ch, verses)
        };
        let targets: Vec<usize> = if self.s.linked {
            (0..self.s.panes.len()).collect()
        } else {
            vec![pane]
        };
        for i in targets {
            let max_ch = self
                .translation(&self.s.panes[i].translation)
                .map_or(1, |t| t.chapter_count(book).max(1));
            let p = &mut self.s.panes[i];
            let changed = p.book != book || p.chapter != chapter.min(max_ch);
            p.book = book;
            p.chapter = chapter.clamp(1, max_ch);
            if let Some((v, _)) = verses {
                self.pending_scroll.insert(p.id, v);
            } else if changed {
                self.pending_scroll.insert(p.id, 0);
            }
        }
        self.s.focused = pane;
        match verses {
            Some((a, b)) => {
                self.selection = Some(Selection {
                    book,
                    chapter,
                    anchor: a,
                    start: a,
                    end: b,
                });
            }
            None => {
                if self
                    .selection
                    .is_some_and(|s| s.book != book || s.chapter != chapter)
                {
                    self.selection = None;
                }
            }
        }
    }

    fn copy_selection(&mut self, ctx: &egui::Context) {
        let Some(sel) = self.selection else {
            self.error("Select a verse to copy.");
            return;
        };
        let Some(pane) = self.s.panes.get(self.s.focused) else {
            return;
        };
        let Some(t) = self.translation(&pane.translation) else {
            return;
        };
        let mut out = format!(
            "{} ({})\n",
            ref_label(sel.book, sel.chapter, sel.start, sel.end),
            t.id
        );
        let mut parts = Vec::new();
        for v in sel.start..=sel.end {
            if let Some(text) = t.verse_text(sel.book, sel.chapter, v) {
                if sel.start == sel.end {
                    parts.push(text.to_owned());
                } else {
                    parts.push(format!("{v} {text}"));
                }
            }
        }
        out.push_str(&parts.join(" "));
        ctx.copy_text(out);
        let msg = format!(
            "Copied {} ({}).",
            ref_label(sel.book, sel.chapter, sel.start, sel.end),
            t.id
        );
        self.info(msg);
    }

    fn run_omnibox(&mut self) {
        let q = self.omnibox.trim().to_owned();
        if q.is_empty() {
            return;
        }
        let pane = self.s.focused;
        let tid = self.s.panes[pane].translation.clone();
        // "quoted" text always searches, e.g. "John" finds the word, not the book.
        let quoted = q.len() >= 2 && q.starts_with('"') && q.ends_with('"');
        let q = if quoted {
            q[1..q.len() - 1].trim().to_owned()
        } else {
            q
        };
        if let Some(r) = bible::parse_reference(&q).filter(|_| !quoted) {
            let Some(t) = self.translation(&tid) else {
                return;
            };
            let n = t.chapter_count(r.book);
            let name = BOOKS[r.book as usize].name;
            if r.chapter > n {
                self.error(format!("{name} has {n} chapters in the {tid}."));
                return;
            }
            let last = t
                .chapter(r.book, r.chapter)
                .and_then(|c| c.verses.last())
                .map_or(0, |x| x.0);
            // Clamp the range end to the chapter (e.g. "John 3:1-999").
            let verses = r.verse.map(|v| (v, r.verse_end.unwrap_or(v).min(last)));
            if let Some((v, _)) = verses
                && (v == 0 || v > last)
            {
                self.error(format!(
                    "{name} {} has {last} verses in the {tid}.",
                    r.chapter
                ));
                return;
            }
            self.open(pane, None, r.book, r.chapter, verses);
            return;
        }
        if q.chars().count() < 2 {
            self.error("Type at least 2 letters to search, or a reference like \"John 3:16\".");
            return;
        }
        self.search = Some(self.search_text(&tid, &q));
        self.s.show_side = true;
        self.s.side_tab = SideTab::Search;
    }

    fn search_text(&self, tid: &str, query: &str) -> SearchState {
        let needle = query.to_lowercase();
        let mut hits = Vec::new();
        let mut total = 0;
        if let Some(t) = self.translation(tid) {
            for (bi, chapters) in t.books.iter().enumerate() {
                for (ci, ch) in chapters.iter().enumerate() {
                    for (v, text) in &ch.verses {
                        let lower = text.to_lowercase();
                        if let Some(pos) = lower.find(&needle) {
                            total += 1;
                            if hits.len() < MAX_SEARCH_HITS {
                                // Lowercasing can change byte lengths outside ASCII;
                                // only mark the match when offsets line up exactly.
                                let mark = (lower.len() == text.len()
                                    && text.is_char_boundary(pos)
                                    && text.is_char_boundary(pos + needle.len()))
                                .then_some((pos, pos + needle.len()));
                                hits.push(SearchHit {
                                    book: bi as u8,
                                    chapter: ci as u16 + 1,
                                    verse: *v,
                                    text: text.clone(),
                                    mark,
                                });
                            }
                        }
                    }
                }
            }
        }
        SearchState {
            query: query.to_owned(),
            translation: tid.to_owned(),
            hits,
            total,
        }
    }

    // -----------------------------------------------------------------------
    // Keyboard
    // -----------------------------------------------------------------------

    fn keyboard(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let typing = ctx.egui_wants_keyboard_input();
        if typing {
            return;
        }
        let pane = self.s.focused;
        ctx.input(|i| {
            if i.key_pressed(Key::ArrowLeft) {
                actions.push(Action::StepChapter { pane, delta: -1 });
            }
            if i.key_pressed(Key::ArrowRight) {
                actions.push(Action::StepChapter { pane, delta: 1 });
            }
            if i.key_pressed(Key::Escape) {
                actions.push(Action::ClearSelection);
            }
            if self.selection.is_some() && !i.modifiers.command {
                let keys = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5];
                for (k, s) in keys.iter().zip(Swatch::ALL) {
                    if i.key_pressed(*k) {
                        actions.push(Action::Highlight(Some(s)));
                    }
                }
                if i.key_pressed(Key::Num0)
                    || i.key_pressed(Key::Delete)
                    || i.key_pressed(Key::Backspace)
                {
                    actions.push(Action::Highlight(None));
                }
                if i.key_pressed(Key::N) {
                    actions.push(Action::AddNote);
                }
            }
            if self.selection.is_some() && i.events.iter().any(|e| matches!(e, egui::Event::Copy)) {
                actions.push(Action::Copy);
            }
        });
    }

    // -----------------------------------------------------------------------
    // Top bar
    // -----------------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let pal = self.pal();
        // Wrapped, not left/right split: on a narrow (e.g. tiled) window the
        // groups flow onto a second row instead of overlapping.
        ui.horizontal_wrapped(|ui| {
            let mut brand = LayoutJob::default();
            brand.append("🐺 ", 0.0, TextFormat { font_id: FontId::proportional(21.0), color: pal.accent, valign: Align::Center, ..Default::default() });
            brand.append("Bible", 0.0, TextFormat { font_id: FontId::new(21.0, theme::serif_bold()), color: pal.text, ..Default::default() });
            brand.append("Wolf", 0.0, TextFormat { font_id: FontId::new(21.0, theme::serif_bold()), color: pal.accent, ..Default::default() });
            if ui
                .add(egui::Label::new(brand).sense(Sense::click()))
                .on_hover_text("About BibleWolf")
                .clicked()
            {
                self.show_about = true;
            }
            ui.add_space(10.0);

            let omni = egui::TextEdit::singleline(&mut self.omnibox)
                .hint_text(RichText::new("🔍  Go to John 3:16, or search words…").color(pal.text_dim))
                .desired_width(280.0)
                .margin(Margin::symmetric(10, 6));
            let r = ui
                .add(omni)
                .on_hover_text(
                    "Ctrl+K or /  ·  Enter goes to a reference (\"John 3:16\", \"Romans\") or searches the focused pane's translation.\nPut words in quotes to search for a book name, e.g. \"John\".",
                );
            if self.focus_omnibox {
                r.request_focus();
                self.focus_omnibox = false;
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                actions.push(Action::RunOmnibox);
            }
            ui.separator();

            // Selection tools
            let has_sel = self.selection.is_some();
            ui.label(RichText::new("Highlight").color(pal.text_dim));
            for (n, s) in Swatch::ALL.into_iter().enumerate() {
                if swatch_button(ui, s, has_sel, self.s.pen == s, &pal)
                    .on_hover_text(format!("{} highlighter  ({})", s.name(), n + 1))
                    .on_disabled_hover_text("Select a verse first")
                    .clicked()
                {
                    actions.push(Action::Highlight(Some(s)));
                }
            }
            if ui
                .add_enabled(has_sel, egui::Button::new("Erase"))
                .on_hover_text("Remove highlight  (0 / Delete)")
                .on_disabled_hover_text("Select a verse first")
                .clicked()
            {
                actions.push(Action::Highlight(None));
            }
            if ui
                .add_enabled(has_sel, egui::Button::new("📝 Add note"))
                .on_hover_text("Stick a note on the selected verses  (N)")
                .on_disabled_hover_text("Select a verse first")
                .clicked()
            {
                actions.push(Action::AddNote);
            }
            if ui
                .add_enabled(has_sel, egui::Button::new("📋 Copy"))
                .on_hover_text("Copy the selected verses  (Ctrl+C)")
                .on_disabled_hover_text("Select a verse first")
                .clicked()
            {
                actions.push(Action::Copy);
            }
            ui.separator();

            // Layout and view
            if ui
                .selectable_label(self.s.show_library, "☰ Library")
                .on_hover_text("Show or hide the translations / books / chapters tree")
                .clicked()
            {
                self.s.show_library = !self.s.show_library;
            }
            if ui
                .selectable_label(self.s.show_side, "☰ Notes")
                .on_hover_text("Show or hide notes, highlights and search results")
                .clicked()
            {
                self.s.show_side = !self.s.show_side;
            }
            if ui
                .selectable_label(self.s.linked, "🔗 Link panes")
                .on_hover_text("When on, every pane follows the same book and chapter")
                .clicked()
            {
                self.s.linked = !self.s.linked;
                if self.s.linked {
                    let p = self.s.panes[self.s.focused].clone();
                    actions.push(Action::Open {
                        pane: self.s.focused,
                        translation: None,
                        book: p.book,
                        chapter: p.chapter,
                        verses: None,
                    });
                }
            }
            let can_add = self.s.panes.len() < MAX_PANES;
            if ui
                .add_enabled(can_add, egui::Button::new("➕ Pane"))
                .on_hover_text("Open another translation side by side")
                .on_disabled_hover_text(format!("At most {MAX_PANES} panes"))
                .clicked()
            {
                actions.push(Action::AddPane);
            }
            ui.separator();
            if ui.button("A−").on_hover_text("Smaller reading text").clicked() {
                self.s.font_size = (self.s.font_size - 1.0).max(12.0);
            }
            if ui.button("A+").on_hover_text("Larger reading text").clicked() {
                self.s.font_size = (self.s.font_size + 1.0).min(32.0);
            }
            let theme_label = if self.s.dark { "☀ Light" } else { "🌙 Dark" };
            if ui.button(theme_label).on_hover_text("Switch colour theme").clicked() {
                self.s.dark = !self.s.dark;
                theme::apply(ui.ctx(), self.s.dark);
            }
        });
    }

    // -----------------------------------------------------------------------
    // Library tree: translations -> books -> chapters
    // -----------------------------------------------------------------------

    fn library(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let pal = self.pal();
        ui.add_space(6.0);
        ui.label(
            RichText::new("LIBRARY")
                .size(11.5)
                .color(pal.text_dim)
                .strong(),
        );
        ui.add_space(2.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.tree_filter)
                .hint_text(RichText::new("Filter books…").color(pal.text_dim))
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(8, 5)),
        );
        ui.add_space(6.0);

        let filter = self.tree_filter.trim().to_lowercase();
        let focused = self.s.panes.get(self.s.focused).cloned();
        egui::ScrollArea::vertical()
            .id_salt("library")
            .auto_shrink(false)
            .show(ui, |ui| {
                for (ti, t) in self.bibles.iter().enumerate() {
                    let on_screen = self.s.panes.iter().any(|p| p.translation == t.id);
                    let mut head = RichText::new(format!("📖  {}", t.id)).strong();
                    if on_screen {
                        head = head.color(pal.accent);
                    }
                    let mut ch = egui::CollapsingHeader::new(head)
                        .id_salt(("tr", &t.id))
                        .default_open(ti == 0);
                    if !filter.is_empty() {
                        ch = ch.open(Some(true));
                    }
                    ch.show(ui, |ui| {
                        ui.label(RichText::new(&t.title).small().color(pal.text_dim));
                        ui.add_space(2.0);
                        for (bi, info) in BOOKS.iter().enumerate() {
                            if !filter.is_empty() && !info.name.to_lowercase().contains(&filter) {
                                continue;
                            }
                            if filter.is_empty() && (bi == 0 || bi == FIRST_NT_BOOK) {
                                ui.add_space(4.0);
                                let label = if bi == 0 {
                                    "OLD TESTAMENT"
                                } else {
                                    "NEW TESTAMENT"
                                };
                                ui.label(RichText::new(label).size(10.5).color(pal.text_dim));
                            }
                            let n = t.chapter_count(bi as u8);
                            if n == 0 {
                                continue;
                            }
                            let current = focused
                                .as_ref()
                                .is_some_and(|p| p.translation == t.id && p.book as usize == bi);
                            let mut name = RichText::new(info.name);
                            if current {
                                name = name.color(pal.accent).strong();
                            }
                            egui::CollapsingHeader::new(name)
                                .id_salt(("bk", &t.id, bi))
                                .show(ui, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                                        for c in 1..=n {
                                            let sel = current
                                                && focused.as_ref().is_some_and(|p| p.chapter == c);
                                            let mut txt = RichText::new(c.to_string()).size(12.5);
                                            if sel {
                                                txt = txt.color(pal.on_accent).strong();
                                            }
                                            let mut b = egui::Button::new(txt)
                                                .min_size(vec2(34.0, 26.0))
                                                .corner_radius(CornerRadius::same(6));
                                            if sel {
                                                b = b.fill(pal.accent);
                                            }
                                            let resp = ui.add(b).on_hover_text(format!(
                                                "Open {} {c} in {}",
                                                info.name, t.id
                                            ));
                                            if resp.clicked() {
                                                actions.push(Action::Open {
                                                    pane: self.s.focused,
                                                    translation: Some(t.id.clone()),
                                                    book: bi as u8,
                                                    chapter: c,
                                                    verses: None,
                                                });
                                            }
                                        }
                                    });
                                });
                        }
                    });
                }
                if let Some(dir) = bible::user_bible_dir() {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(format!(
                            "More translations: drop scrollmapper JSON Bibles into\n{}",
                            dir.display()
                        ))
                        .size(11.0)
                        .color(pal.text_dim),
                    );
                }
            });
    }

    // -----------------------------------------------------------------------
    // Reading panes
    // -----------------------------------------------------------------------

    /// Lay panes out side by side at equal width. Below `MIN_PANE_WIDTH`
    /// each, they keep that width and the row scrolls horizontally rather
    /// than squashing text into overlapping slivers.
    fn panes(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        const GAP: f32 = 10.0;
        let n = self.s.panes.len();
        let avail = ui.available_size();
        let fit = (avail.x - GAP * (n as f32 - 1.0)) / n as f32;
        let w = fit.max(MIN_PANE_WIDTH).floor();
        let mut row = |ui: &mut Ui| {
            let h = ui.available_height();
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                for i in 0..n {
                    ui.allocate_ui_with_layout(vec2(w, h), Layout::top_down(Align::Min), |ui| {
                        ui.set_width(w);
                        ui.set_height(h);
                        self.pane(ui, i, actions);
                    });
                }
            });
        };
        if fit < MIN_PANE_WIDTH {
            egui::ScrollArea::horizontal()
                .id_salt("panes-row")
                .auto_shrink(false)
                .show(ui, |ui| row(ui));
        } else {
            row(ui);
        }
    }

    fn pane(&self, ui: &mut Ui, idx: usize, actions: &mut Vec<Action>) {
        let pal = self.pal();
        let pane = &self.s.panes[idx];
        let focused = idx == self.s.focused && self.s.panes.len() > 1;
        let Some(t) = self.translation(&pane.translation) else {
            ui.label(
                RichText::new(format!("Translation {} is not loaded.", pane.translation))
                    .color(pal.error_fg),
            );
            return;
        };
        let fs = self.s.font_size;

        let frame = Frame::new()
            .fill(pal.surface)
            .stroke(Stroke::new(
                if focused { 1.5 } else { 1.0 },
                if focused { pal.accent } else { pal.border },
            ))
            .corner_radius(CornerRadius::same(12))
            .shadow(Shadow {
                offset: [0, 2],
                blur: 10,
                spread: 0,
                color: Color32::from_black_alpha(if pal.dark { 60 } else { 18 }),
            });

        let inner = frame.show(ui, |ui| {
            ui.set_min_height(ui.available_height());
            // --- header ---
            Frame::new()
                .fill(pal.surface_alt)
                .corner_radius(CornerRadius {
                    nw: 12,
                    ne: 12,
                    sw: 0,
                    se: 0,
                })
                .inner_margin(Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        egui::ComboBox::from_id_salt(("pane-tr", pane.id))
                            .selected_text(RichText::new(&t.id).strong().color(pal.accent))
                            .width(96.0)
                            .height(400.0)
                            .show_ui(ui, |ui| {
                                for o in &self.bibles {
                                    if ui
                                        .selectable_label(
                                            o.id == t.id,
                                            format!("{}  —  {}", o.id, o.title),
                                        )
                                        .clicked()
                                    {
                                        actions.push(Action::SetTranslation {
                                            pane: idx,
                                            id: o.id.clone(),
                                        });
                                    }
                                }
                            })
                            .response
                            .on_hover_text(&t.title);

                        if ui
                            .button("◀")
                            .on_hover_text("Previous chapter  (←)")
                            .clicked()
                        {
                            actions.push(Action::StepChapter {
                                pane: idx,
                                delta: -1,
                            });
                        }
                        egui::ComboBox::from_id_salt(("pane-bk", pane.id))
                            .selected_text(BOOKS[pane.book as usize].name)
                            .width(140.0)
                            .height(420.0)
                            .show_ui(ui, |ui| {
                                for (bi, b) in BOOKS.iter().enumerate() {
                                    if bi == FIRST_NT_BOOK {
                                        ui.separator();
                                    }
                                    if ui
                                        .selectable_label(bi == pane.book as usize, b.name)
                                        .clicked()
                                    {
                                        actions.push(Action::Open {
                                            pane: idx,
                                            translation: None,
                                            book: bi as u8,
                                            chapter: 1,
                                            verses: None,
                                        });
                                    }
                                }
                            });
                        egui::ComboBox::from_id_salt(("pane-ch", pane.id))
                            .selected_text(pane.chapter.to_string())
                            .width(56.0)
                            .height(420.0)
                            .show_ui(ui, |ui| {
                                for c in 1..=t.chapter_count(pane.book) {
                                    if ui
                                        .selectable_label(c == pane.chapter, c.to_string())
                                        .clicked()
                                    {
                                        actions.push(Action::Open {
                                            pane: idx,
                                            translation: None,
                                            book: pane.book,
                                            chapter: c,
                                            verses: None,
                                        });
                                    }
                                }
                            });
                        if ui.button("▶").on_hover_text("Next chapter  (→)").clicked() {
                            actions.push(Action::StepChapter {
                                pane: idx,
                                delta: 1,
                            });
                        }
                        if self.s.panes.len() > 1 {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(
                                        egui::Button::new(RichText::new("✖").color(pal.text_dim))
                                            .frame(false),
                                    )
                                    .on_hover_text("Close this pane")
                                    .clicked()
                                {
                                    actions.push(Action::ClosePane(idx));
                                }
                            });
                        }
                    });
                });

            // --- text ---
            let scroll_target = self.pending_scroll.get(&pane.id).copied();
            egui::ScrollArea::vertical()
                .id_salt(("pane-scroll", pane.id))
                .auto_shrink(false)
                .show(ui, |ui| {
                    Frame::new()
                        .inner_margin(Margin {
                            left: 18,
                            right: 10,
                            top: 14,
                            bottom: 24,
                        })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            self.chapter_body(ui, idx, t, scroll_target, fs, &pal, actions);
                        });
                });
        });

        // Any press inside the pane makes it the focused one.
        let rect = inner.response.rect;
        if idx != self.s.focused
            && ui.rect_contains_pointer(rect)
            && ui.input(|i| i.pointer.any_pressed())
        {
            actions.push(Action::Focus(idx));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn chapter_body(
        &self,
        ui: &mut Ui,
        idx: usize,
        t: &Translation,
        scroll_target: Option<u16>,
        fs: f32,
        pal: &Palette,
        actions: &mut Vec<Action>,
    ) {
        let pane = &self.s.panes[idx];
        let (book, chapter) = (pane.book, pane.chapter);

        // Title block
        let title = ui.label(
            RichText::new(BOOKS[book as usize].name)
                .font(FontId::new(fs * 1.75, theme::serif_bold()))
                .color(pal.text),
        );
        if scroll_target == Some(0) {
            title.scroll_to_me(Some(Align::TOP));
            actions.push(Action::ScrollDone(pane.id));
        }
        ui.horizontal(|ui| {
            let mut job = LayoutJob::default();
            job.append(
                &format!("CHAPTER {chapter}"),
                0.0,
                TextFormat {
                    font_id: FontId::new(11.5, egui::FontFamily::Proportional),
                    color: pal.verse_num,
                    extra_letter_spacing: 2.0,
                    ..Default::default()
                },
            );
            ui.label(job);
            let (r, _) = ui.allocate_exact_size(vec2(48.0, 10.0), Sense::hover());
            ui.painter().line_segment(
                [
                    Pos2::new(r.left(), r.center().y),
                    Pos2::new(r.right(), r.center().y),
                ],
                Stroke::new(1.0, pal.accent.gamma_multiply(0.6)),
            );
        });
        ui.add_space(fs * 0.6);

        let Some(ch) = t.chapter(book, chapter) else {
            ui.label(
                RichText::new(format!(
                    "{} {chapter} is not in the {}.",
                    BOOKS[book as usize].name, t.id
                ))
                .color(pal.text_dim),
            );
            if scroll_target.is_some() {
                actions.push(Action::ScrollDone(pane.id));
            }
            return;
        };

        let notes_here: Vec<&Note> = self
            .store
            .notes
            .iter()
            .filter(|n| n.book == book && n.chapter == chapter)
            .collect();

        for (v, text) in &ch.verses {
            let resp = self.verse_row(
                ui,
                pane.id,
                book,
                chapter,
                *v,
                text,
                &notes_here,
                fs,
                pal,
                actions,
            );
            if scroll_target == Some(*v) {
                resp.scroll_to_me(Some(Align::Center));
            }
        }
        // Clear the request even if the verse isn't in this translation
        // (versification differences); otherwise it would keep re-scrolling.
        if scroll_target.is_some_and(|v| v != 0) {
            actions.push(Action::ScrollDone(pane.id));
        }

        // Footer navigation
        ui.add_space(fs);
        ui.horizontal(|ui| {
            let has_prev = !(book == 0 && chapter == 1);
            let has_next = !(book as usize == BOOKS.len() - 1 && chapter >= t.chapter_count(book));
            if has_prev && ui.button("◀ Previous chapter").clicked() {
                actions.push(Action::StepChapter {
                    pane: idx,
                    delta: -1,
                });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if has_next && ui.button("Next chapter ▶").clicked() {
                    actions.push(Action::StepChapter {
                        pane: idx,
                        delta: 1,
                    });
                }
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn verse_row(
        &self,
        ui: &mut Ui,
        pane_id: u64,
        book: u8,
        chapter: u16,
        verse: u16,
        text: &str,
        notes_here: &[&Note],
        fs: f32,
        pal: &Palette,
        actions: &mut Vec<Action>,
    ) -> egui::Response {
        const GUTTER: f32 = 30.0;
        const PAD: Vec2 = Vec2::new(8.0, 4.0);
        let width = ui.available_width();
        let at = VerseRef {
            book,
            chapter,
            verse,
        };
        let hl = self.store.highlights.get(&at).copied();
        let selected = self
            .selection
            .is_some_and(|s| s.contains(book, chapter, verse));

        let mut job = LayoutJob::default();
        job.wrap.max_width = (width - GUTTER - PAD.x * 2.0).max(60.0);
        job.append(
            &format!("{verse}  "),
            0.0,
            TextFormat {
                font_id: FontId::new(fs * 0.62, theme::serif_bold()),
                color: pal.verse_num,
                valign: Align::TOP,
                ..Default::default()
            },
        );
        let missing = text.is_empty();
        job.append(
            if missing { "—" } else { text },
            0.0,
            TextFormat {
                font_id: FontId::new(
                    fs,
                    if missing {
                        theme::serif_italic()
                    } else {
                        theme::serif()
                    },
                ),
                color: if missing { pal.text_dim } else { pal.text },
                background: hl.map_or(Color32::TRANSPARENT, |c| theme::highlight_ink(c, pal.dark)),
                line_height: Some(fs * 1.6),
                ..Default::default()
            },
        );
        let galley = ui.painter().layout_job(job);
        let size = vec2(width, galley.size().y + PAD.y * 2.0);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            if selected {
                painter.rect_filled(rect, CornerRadius::same(6), pal.selected);
                painter.rect_filled(
                    Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
                    CornerRadius::same(2),
                    pal.accent,
                );
            } else if resp.hovered() {
                painter.rect_filled(rect, CornerRadius::same(6), pal.hover);
            }
            painter.galley(rect.min + PAD, galley, pal.text);
        }

        // Sticky-note markers in the right gutter.
        let mut stack = 0.0;
        for n in notes_here.iter().filter(|n| n.covers(book, chapter, verse)) {
            let paper = theme::note_paper(n.color);
            let gx = rect.right() - GUTTER + 8.0;
            if n.verse_start == verse {
                let r = Rect::from_min_size(
                    Pos2::new(gx + stack, rect.top() + PAD.y + 3.0),
                    vec2(16.0, 16.0),
                );
                stack += 5.0;
                let id = Id::new(("note-marker", pane_id, n.id));
                let mr = ui.interact(r, id, Sense::click());
                let painter = ui.painter();
                painter.rect_filled(
                    r.translate(vec2(1.0, 2.0)),
                    CornerRadius::same(2),
                    Color32::from_black_alpha(70),
                );
                painter.rect_filled(r, CornerRadius::same(2), paper);
                // folded corner
                let c = r.right_bottom();
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        c + vec2(-6.0, 0.0),
                        c + vec2(0.0, -6.0),
                        c + vec2(-6.0, -6.0),
                    ],
                    theme::note_band(n.color),
                    Stroke::NONE,
                ));
                if mr.hovered() {
                    painter.rect_stroke(
                        r.expand(1.5),
                        CornerRadius::same(3),
                        Stroke::new(1.5, pal.accent),
                        egui::StrokeKind::Outside,
                    );
                }
                let preview: String = n.text.chars().take(140).collect();
                let label = ref_label(n.book, n.chapter, n.verse_start, n.verse_end);
                mr.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        format!("Note on {label}"),
                    )
                });
                let mr = mr.on_hover_text(if preview.trim().is_empty() {
                    format!("Note on {label} (empty) — click to open")
                } else {
                    format!("{preview}\n\n— click to open")
                });
                if mr.clicked() {
                    actions.push(Action::OpenNote(n.id));
                }
            } else {
                // Continuation of a multi-verse note.
                let x = gx + 7.0;
                ui.painter().line_segment(
                    [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                    Stroke::new(3.0, paper),
                );
            }
        }

        let label = format!("{} {chapter}:{verse}", BOOKS[book as usize].name);
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{label} {text}"))
        });

        if resp.clicked() {
            let extend = ui.input(|i| i.modifiers.shift);
            actions.push(Action::Select {
                from_pane: pane_id,
                book,
                chapter,
                verse,
                extend,
                keep_if_inside: false,
            });
        }
        if resp.secondary_clicked() {
            actions.push(Action::Select {
                from_pane: pane_id,
                book,
                chapter,
                verse,
                extend: false,
                keep_if_inside: true,
            });
        }
        resp.context_menu(|ui| {
            ui.label(RichText::new(&label).strong());
            ui.separator();
            ui.label(RichText::new("Highlight").small().color(pal.text_dim));
            ui.horizontal(|ui| {
                for s in Swatch::ALL {
                    if swatch_button(ui, s, true, false, pal)
                        .on_hover_text(s.name())
                        .clicked()
                    {
                        actions.push(Action::Highlight(Some(s)));
                        ui.close();
                    }
                }
            });
            if hl.is_some() && ui.button("Erase highlight").clicked() {
                actions.push(Action::Highlight(None));
                ui.close();
            }
            ui.separator();
            if ui.button("📝 Add sticky note").clicked() {
                actions.push(Action::AddNote);
                ui.close();
            }
            if ui.button("📋 Copy").clicked() {
                actions.push(Action::Copy);
                ui.close();
            }
        });
        resp
    }

    // -----------------------------------------------------------------------
    // Side panel: notes / highlights / search
    // -----------------------------------------------------------------------

    fn side_panel(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let pal = self.pal();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (tab, name) in [
                (SideTab::Notes, "Notes"),
                (SideTab::Highlights, "Highlights"),
                (SideTab::Search, "Search"),
            ] {
                if ui.selectable_label(self.s.side_tab == tab, name).clicked() {
                    self.s.side_tab = tab;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt(("side", self.s.side_tab as u8))
            .auto_shrink(false)
            .show(ui, |ui| match self.s.side_tab {
                SideTab::Notes => self.notes_list(ui, &pal, actions),
                SideTab::Highlights => self.highlights_list(ui, &pal, actions),
                SideTab::Search => self.search_list(ui, &pal, actions),
            });
    }

    fn empty_state(ui: &mut Ui, pal: &Palette, title: &str, body: &str) {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(title)
                    .font(FontId::new(17.0, theme::serif_bold()))
                    .color(pal.text),
            );
            ui.add_space(4.0);
            ui.label(RichText::new(body).color(pal.text_dim));
        });
    }

    fn notes_list(&self, ui: &mut Ui, pal: &Palette, actions: &mut Vec<Action>) {
        if self.store.notes.is_empty() {
            Self::empty_state(
                ui,
                pal,
                "No notes yet",
                "Click a verse (Shift-click for a range), then press “📝 Add note” or N to stick a note on it.",
            );
            return;
        }
        let mut notes: Vec<&Note> = self.store.notes.iter().collect();
        notes.sort_by_key(|n| (n.book, n.chapter, n.verse_start, n.id));
        for n in notes {
            let paper = theme::note_paper(n.color);
            Frame::new()
                .fill(paper)
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(10, 8))
                .shadow(Shadow {
                    offset: [1, 3],
                    blur: 6,
                    spread: 0,
                    color: Color32::from_black_alpha(50),
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let jump = Action::JumpTo {
                        book: n.book,
                        chapter: n.chapter,
                        start: n.verse_start,
                        end: n.verse_end,
                    };
                    ui.horizontal(|ui| {
                        // Clickable pieces instead of a whole-card overlay: an
                        // overlay registered after the "Show" button would win
                        // every hit test inside it (egui: last widget on top).
                        if ui
                            .add(
                                egui::Label::new(
                                    RichText::new(ref_label(
                                        n.book,
                                        n.chapter,
                                        n.verse_start,
                                        n.verse_end,
                                    ))
                                    .strong()
                                    .underline()
                                    .color(theme::NOTE_INK),
                                )
                                .sense(Sense::click()),
                            )
                            .on_hover_text("Go to this passage")
                            .clicked()
                        {
                            actions.push(jump);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let txt = if n.open { "Showing" } else { "Show" };
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(txt).small().color(theme::NOTE_INK),
                                    )
                                    .fill(theme::note_band(n.color)),
                                )
                                .on_hover_text("Show this sticky note on screen")
                                .clicked()
                            {
                                actions.push(Action::OpenNote(n.id));
                            }
                        });
                    });
                    let body: String = n.text.chars().take(220).collect();
                    let body = if body.trim().is_empty() {
                        "(empty note)".to_owned()
                    } else {
                        body
                    };
                    if ui
                        .add(
                            egui::Label::new(RichText::new(body).color(theme::NOTE_INK_DIM))
                                .sense(Sense::click()),
                        )
                        .on_hover_text("Go to this passage")
                        .clicked()
                    {
                        actions.push(Action::JumpTo {
                            book: n.book,
                            chapter: n.chapter,
                            start: n.verse_start,
                            end: n.verse_end,
                        });
                    }
                });
            ui.add_space(8.0);
        }
    }

    fn highlights_list(&self, ui: &mut Ui, pal: &Palette, actions: &mut Vec<Action>) {
        if self.store.highlights.is_empty() {
            Self::empty_state(
                ui,
                pal,
                "No highlights yet",
                "Select a verse and pick a highlighter colour from the top bar (or press 1–5).",
            );
            return;
        }
        let tid = &self.s.panes[self.s.focused].translation;
        let t = self.translation(tid);
        let mut hs: Vec<(&VerseRef, &Swatch)> = self.store.highlights.iter().collect();
        hs.sort_by_key(|(r, _)| (r.book, r.chapter, r.verse));
        for (at, color) in hs {
            let resp = ui
                .horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    ui.painter()
                        .circle_filled(dot.center(), 5.5, theme::swatch_solid(*color));
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} {}:{}",
                                BOOKS[at.book as usize].name, at.chapter, at.verse
                            ))
                            .strong()
                            .color(pal.text),
                        );
                        if let Some(text) =
                            t.and_then(|t| t.verse_text(at.book, at.chapter, at.verse))
                        {
                            let short: String = text.chars().take(120).collect();
                            let ell = if text.chars().count() > 120 {
                                "…"
                            } else {
                                ""
                            };
                            ui.label(
                                RichText::new(format!("{short}{ell}"))
                                    .small()
                                    .color(pal.text_dim),
                            );
                        }
                    });
                })
                .response;
            let r = ui.interact(
                resp.rect,
                Id::new(("hl-row", at.book, at.chapter, at.verse)),
                Sense::click(),
            );
            if r.hovered() {
                ui.painter()
                    .rect_filled(resp.rect.expand(3.0), CornerRadius::same(6), pal.hover);
            }
            if r.on_hover_text(format!("{} highlight — go to verse", color.name()))
                .clicked()
            {
                actions.push(Action::JumpTo {
                    book: at.book,
                    chapter: at.chapter,
                    start: at.verse,
                    end: at.verse,
                });
            }
            ui.add_space(6.0);
        }
    }

    fn search_list(&self, ui: &mut Ui, pal: &Palette, actions: &mut Vec<Action>) {
        let Some(s) = &self.search else {
            Self::empty_state(
                ui,
                pal,
                "Search the Bible",
                "Type a word or phrase in the box at the top and press Enter. A reference like “Romans 8:28” jumps straight there.",
            );
            return;
        };
        let shown = s.hits.len();
        let summary = if s.total == 0 {
            format!("No verses in the {} contain “{}”.", s.translation, s.query)
        } else if shown < s.total {
            format!(
                "{} verses in the {} contain “{}” — showing the first {shown}.",
                s.total, s.translation, s.query
            )
        } else {
            format!(
                "{} verse{} in the {} contain{} “{}”.",
                s.total,
                if s.total == 1 { "" } else { "s" },
                s.translation,
                if s.total == 1 { "s" } else { "" },
                s.query
            )
        };
        ui.label(RichText::new(summary).color(pal.text_dim));
        ui.add_space(6.0);
        let fs = 14.5;
        for (i, h) in s.hits.iter().enumerate() {
            let resp = ui
                .vertical(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} {}:{}",
                            BOOKS[h.book as usize].name, h.chapter, h.verse
                        ))
                        .strong()
                        .color(pal.accent),
                    );
                    let mut job = LayoutJob::default();
                    job.wrap.max_width = ui.available_width();
                    let plain = TextFormat {
                        font_id: FontId::new(fs, theme::serif()),
                        color: pal.text,
                        ..Default::default()
                    };
                    match h.mark {
                        Some((a, b)) => {
                            job.append(&h.text[..a], 0.0, plain.clone());
                            job.append(
                                &h.text[a..b],
                                0.0,
                                TextFormat {
                                    font_id: FontId::new(fs, theme::serif_bold()),
                                    background: theme::highlight_ink(Swatch::Yellow, pal.dark),
                                    ..plain.clone()
                                },
                            );
                            job.append(&h.text[b..], 0.0, plain);
                        }
                        None => job.append(&h.text, 0.0, plain),
                    }
                    ui.label(job);
                })
                .response;
            let r = ui.interact(resp.rect, Id::new(("hit", i)), Sense::click());
            if r.hovered() {
                ui.painter()
                    .rect_filled(resp.rect.expand(3.0), CornerRadius::same(6), pal.hover);
            }
            if r.on_hover_text("Open this verse in the focused pane")
                .clicked()
            {
                actions.push(Action::JumpTo {
                    book: h.book,
                    chapter: h.chapter,
                    start: h.verse,
                    end: h.verse,
                });
            }
            ui.add_space(8.0);
        }
    }

    // -----------------------------------------------------------------------
    // Sticky notes (floating post-its)
    // -----------------------------------------------------------------------

    fn sticky_notes(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let mut edited = false;
        let focus = self.focus_note.take();
        let spawn = self.note_spawn_pos.take();
        let screen = ctx.content_rect();
        let mut cascade = 0.0;
        for note in self.store.notes.iter_mut().filter(|n| n.open) {
            let paper = theme::note_paper(note.color);
            let band = theme::note_band(note.color);
            let default_pos = match (spawn, focus) {
                (Some(p), Some(f)) if f == note.id => p + vec2(12.0, 12.0),
                _ => {
                    cascade += 24.0;
                    Pos2::new(
                        screen.right() - 320.0 - cascade,
                        screen.top() + 90.0 + cascade,
                    )
                }
            };
            let label = ref_label(note.book, note.chapter, note.verse_start, note.verse_end);
            egui::Window::new(format!("Note on {label}"))
                .id(sticky_id(note.id))
                .title_bar(false)
                .resizable(true)
                .collapsible(false)
                .default_pos(default_pos)
                .default_size(vec2(250.0, 210.0))
                .min_size(vec2(190.0, 130.0))
                .frame(
                    Frame::new()
                        .fill(paper)
                        .corner_radius(CornerRadius { nw: 2, ne: 2, sw: 2, se: 14 })
                        .shadow(Shadow { offset: [3, 8], blur: 18, spread: 0, color: Color32::from_black_alpha(100) }),
                )
                .show(ctx, |ui| {
                    // Header band doubles as the drag handle.
                    Frame::new().fill(band).inner_margin(Margin::symmetric(10, 6)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let go = ui
                                .add(
                                    egui::Label::new(RichText::new(&label).strong().color(theme::NOTE_INK))
                                        .sense(Sense::click()),
                                )
                                .on_hover_text("Go to this passage");
                            if go.clicked() {
                                actions.push(Action::JumpTo {
                                    book: note.book,
                                    chapter: note.chapter,
                                    start: note.verse_start,
                                    end: note.verse_end,
                                });
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                if ui
                                    .add(egui::Button::new(RichText::new("✖").color(theme::NOTE_INK)).frame(false))
                                    .on_hover_text("Hide note (it stays saved; reopen from the Notes panel or the marker by the verse)")
                                    .clicked()
                                {
                                    actions.push(Action::HideNote(note.id));
                                }
                                if ui
                                    .add(egui::Button::new(RichText::new("🗑").color(theme::NOTE_INK)).frame(false))
                                    .on_hover_text("Delete note (you can undo from the status bar)")
                                    .clicked()
                                {
                                    actions.push(Action::DeleteNote(note.id));
                                }
                                for s in Swatch::ALL.into_iter().rev() {
                                    let (r, resp) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
                                    let p = ui.painter();
                                    p.circle_filled(r.center(), 6.0, theme::note_paper(s));
                                    p.circle_stroke(
                                        r.center(),
                                        6.0,
                                        Stroke::new(if s == note.color { 2.0 } else { 1.0 }, theme::NOTE_INK_DIM),
                                    );
                                    resp.widget_info(|| {
                                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{} paper", s.name()))
                                    });
                                    if resp.on_hover_text(format!("{} paper", s.name())).clicked() {
                                        actions.push(Action::NoteColor(note.id, s));
                                    }
                                }
                            });
                        });
                    });
                    Frame::new().inner_margin(Margin { left: 10, right: 10, top: 6, bottom: 10 }).show(ui, |ui| {
                        let te = egui::TextEdit::multiline(&mut note.text)
                            .id(Id::new(("note-text", note.id)))
                            .frame(Frame::NONE)
                            .background_color(Color32::TRANSPARENT)
                            .text_color(theme::NOTE_INK)
                            .font(FontId::new(15.0, theme::serif()))
                            .hint_text(RichText::new("Write your note…").color(theme::NOTE_INK_DIM))
                            .desired_width(f32::INFINITY)
                            .desired_rows(6);
                        // Scroll inside the note rather than growing the window:
                        // egui windows never auto-shrink (Resize keeps the max of
                        // the last content size), so unbounded content would push
                        // the note off screen.
                        let size = ui.available_size();
                        let r = egui::ScrollArea::vertical()
                            .id_salt(("note-scroll", note.id))
                            .auto_shrink(false)
                            .max_height(size.y)
                            .show(ui, |ui| ui.add_sized(vec2(ui.available_width(), size.y), te))
                            .inner;
                        if focus == Some(note.id) {
                            r.request_focus();
                        }
                        if r.changed() {
                            note.modified = store::now_secs();
                            edited = true;
                        }
                    });
                });
        }
        if edited {
            self.store.mark_dirty();
        }
    }

    // -----------------------------------------------------------------------
    // Status bar
    // -----------------------------------------------------------------------

    fn status_bar(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let pal = self.pal();
        // Errors get their own wrapping row so long messages stay readable;
        // they are only removed by "Dismiss". The text is copied with the
        // Copy button rather than by selecting it, so Ctrl+C keeps meaning
        // "copy the selected verses".
        if !self.errors.is_empty() {
            Frame::new()
                .fill(pal.error_bg)
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Dismiss").strong().color(pal.error_fg),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            actions.push(Action::DismissErrors);
                        }
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Copy").color(pal.error_fg))
                                    .frame(false),
                            )
                            .on_hover_text("Copy the error text")
                            .clicked()
                        {
                            actions.push(Action::CopyErrors);
                        }
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("⚠ {}", self.errors.join("\n")))
                                    .color(pal.error_fg),
                            )
                            .wrap()
                            .selectable(false),
                        );
                    });
                });
            ui.add_space(4.0);
        }
        ui.horizontal(|ui| {
            // Right-hand side first so it always keeps its space.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (txt, col) = if let Some(e) = &self.save_error {
                    (format!("⚠ Not saved: {e}"), pal.error_fg)
                } else if self.store.is_dirty() {
                    ("Saving…".to_owned(), pal.text_dim)
                } else if self.store.path().is_some() {
                    ("✔ Saved".to_owned(), pal.text_dim)
                } else {
                    ("⚠ Notes are not being saved".to_owned(), pal.error_fg)
                };
                ui.hyperlink_to(RichText::new("© 2026 IntelligentWolf Ltd").color(pal.accent), WEBSITE)
                    .on_hover_text(WEBSITE);
                ui.label(RichText::new("·").color(pal.text_dim));
                let r = ui.label(RichText::new(txt).color(col));
                if let Some(p) = self.store.path() {
                    r.on_hover_text(format!("Notes and highlights are stored in {}", p.display()));
                }
                ui.label(
                    RichText::new(format!(
                        "{}  ·  {}",
                        plural(self.store.notes.len(), "note"),
                        plural(self.store.highlights.len(), "highlight")
                    ))
                    .color(pal.text_dim),
                );
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| match &self.flash {
                    Some(f) => {
                        Frame::new()
                            .fill(pal.ok_bg)
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::symmetric(8, 3))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.add(egui::Label::new(RichText::new(&f.text).color(pal.ok_fg)).truncate());
                                    if f.undo
                                        && !self.deleted.is_empty()
                                        && ui
                                            .add(egui::Button::new(RichText::new("Undo").strong().color(pal.ok_fg)).frame(false))
                                            .clicked()
                                    {
                                        actions.push(Action::UndoDelete);
                                    }
                                });
                            });
                    }
                    _ => {
                        let hint = match self.selection {
                            Some(s) => format!(
                                "{} selected  ·  1–5 highlight  ·  N note  ·  Ctrl+C copy  ·  Shift-click extends  ·  right-click for more",
                                ref_label(s.book, s.chapter, s.start, s.end)
                            ),
                            None => "Click a verse to select it  ·  Left/Right arrow keys change chapter  ·  Ctrl+K go to or search".to_owned(),
                        };
                        ui.add(egui::Label::new(RichText::new(&hint).color(pal.text_dim)).truncate())
                            .on_hover_text(hint);
                    }
                });
            });
        });
    }

    fn autosave(&mut self, ctx: &egui::Context) {
        if !self.store.is_dirty() {
            return;
        }
        let retry_ok = self
            .last_save_attempt
            .is_none_or(|t| t.elapsed() >= SAVE_RETRY)
            || self.save_error.is_none();
        if self.store.dirty_for(SAVE_DEBOUNCE_SECS) && retry_ok {
            self.last_save_attempt = Some(Instant::now());
            match self.store.save() {
                Ok(()) => self.save_error = None,
                Err(e) => {
                    if self.save_error.is_none() {
                        self.error(e.clone());
                    }
                    self.save_error = Some(e);
                }
            }
        }
        ctx.request_repaint_after(Duration::from_millis(400));
    }
}

impl BibleApp {
    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let pal = self.pal();
        let mut open = true;
        egui::Window::new("About BibleWolf")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("🐺").size(44.0).color(pal.accent));
                    let mut name = LayoutJob::default();
                    name.append("Bible", 0.0, TextFormat { font_id: FontId::new(28.0, theme::serif_bold()), color: pal.text, ..Default::default() });
                    name.append("Wolf", 0.0, TextFormat { font_id: FontId::new(28.0, theme::serif_bold()), color: pal.accent, ..Default::default() });
                    ui.label(name);
                    ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(pal.text_dim));
                    ui.add_space(10.0);
                    ui.label(RichText::new("© 2026 IntelligentWolf Ltd").strong());
                    ui.label(RichText::new("Open source under the MIT License").color(pal.text_dim));
                    ui.hyperlink_to(RichText::new(WEBSITE).color(pal.accent), WEBSITE);
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(
                            "Bible texts are in the public domain, sourced from scrollmapper/bible_databases.\n\
                             Noto Serif typeface © The Noto Project Authors, SIL Open Font License 1.1.",
                        )
                        .small()
                        .color(pal.text_dim),
                    );
                });
            });
        if !open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.show_about = false;
        }
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

fn sticky_id(note_id: u64) -> Id {
    Id::new(("sticky", note_id))
}

/// Round colour button used for highlighter pens.
fn swatch_button(
    ui: &mut Ui,
    s: Swatch,
    enabled: bool,
    current: bool,
    pal: &Palette,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        vec2(24.0, 24.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let resp = if enabled {
        resp
    } else {
        resp.on_hover_cursor(egui::CursorIcon::NotAllowed)
    };
    let p = ui.painter();
    let c = theme::swatch_solid(s);
    let c = if enabled { c } else { c.gamma_multiply(0.35) };
    let radius = if resp.hovered() && enabled { 10.5 } else { 9.0 };
    p.circle_filled(rect.center(), radius, c);
    if current && enabled {
        p.circle_stroke(rect.center(), 11.5, Stroke::new(1.5, pal.text));
    }
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            enabled,
            format!("{} highlighter", s.name()),
        )
    });
    // Disabled swatches are Sense::hover, so `clicked()` is always false for them.
    resp
}

impl eframe::App for BibleApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let pal = self.pal();
        let mut actions = Vec::new();

        // Confirmations fade on their own; errors stay until dismissed.
        if self
            .flash
            .as_ref()
            .is_some_and(|f| f.at.elapsed() >= INFO_FLASH)
        {
            self.flash = None;
        }
        if let Some(f) = &self.flash {
            ctx.request_repaint_after(INFO_FLASH.saturating_sub(f.at.elapsed()));
        }

        if ctx.input(|i| i.modifiers.command && (i.key_pressed(Key::K) || i.key_pressed(Key::F)))
            || (!ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(Key::Slash)))
        {
            self.focus_omnibox = true;
        }
        self.keyboard(&ctx, &mut actions);

        Panel::top("top")
            .frame(
                Frame::new()
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(14, 8)),
            )
            .show(ui, |ui| self.top_bar(ui, &mut actions));
        Panel::bottom("status")
            .frame(
                Frame::new()
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(14, 5)),
            )
            .show(ui, |ui| self.status_bar(ui, &mut actions));
        let library = Panel::left("library")
            .resizable(true)
            .default_size(250.0)
            .size_range(180.0..=420.0)
            .frame(
                Frame::new()
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(10, 4)),
            );
        if self.s.show_library {
            library.show(ui, |ui| self.library(ui, &mut actions));
        }
        if self.s.show_side {
            Panel::right("side")
                .resizable(true)
                .default_size(300.0)
                .size_range(220.0..=520.0)
                .frame(
                    Frame::new()
                        .fill(pal.panel)
                        .inner_margin(Margin::symmetric(10, 4)),
                )
                .show(ui, |ui| self.side_panel(ui, &mut actions));
        }
        egui::CentralPanel::default()
            .frame(Frame::new().fill(pal.bg).inner_margin(Margin::same(10)))
            .show(ui, |ui| self.panes(ui, &mut actions));

        self.sticky_notes(&ctx, &mut actions);
        self.about_window(&ctx);

        for a in actions {
            self.apply(&ctx, a);
        }
        self.autosave(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.s);
        if self.store.is_dirty()
            && let Err(e) = self.store.save()
        {
            // Exit path: nothing visible left to show it in; stderr is the last resort.
            eprintln!("biblewolf: {e}");
        }
    }
}
