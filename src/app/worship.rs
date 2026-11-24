//! Worship-leader tools: songs for a passage (your library + Hymnary.org),
//! the song library, and service plans / set lists.

use super::*;
use crate::hymnary::{self, Hymn, Passage};
use crate::store::{PassageRef, Service, ServiceItem, Song};
use std::sync::mpsc;

/// Wait this long after the selection settles before asking Hymnary, so
/// shift-clicking through a range doesn't fire a request per click.
const LOOKUP_DEBOUNCE: Duration = Duration::from_millis(400);

pub(super) const SUGGESTED_THEMES: &[&str] = &[
    "Advent",
    "Christmas",
    "Epiphany",
    "Lent",
    "Easter",
    "Pentecost",
    "Communion",
    "Baptism",
    "Praise",
    "Thanksgiving",
    "Confession",
    "Lament",
    "Grace",
    "Faith",
    "Hope",
    "Love",
    "Peace",
    "Joy",
    "Salvation",
    "Cross",
    "Resurrection",
    "Holy Spirit",
    "Prayer",
    "Trust",
    "Comfort",
    "Creation",
    "Kingdom",
    "Mission",
    "Sending",
];

pub(super) enum HymnState {
    Loading,
    Ready(Vec<Hymn>),
    Failed(String),
}

type HymnReply = (Passage, Result<Vec<Hymn>, String>);

pub(super) struct SongDraft {
    song: Song,
    original: Song,
    refs_text: String,
    theme_input: String,
    title_error: bool,
    refs_error: Option<String>,
    link_error: bool,
    confirm_discard: bool,
    focus_title: bool,
}

impl SongDraft {
    fn new(song: Song) -> Self {
        let refs_text = song
            .refs
            .iter()
            .map(|r| r.label())
            .collect::<Vec<_>>()
            .join("; ");
        Self {
            original: song.clone(),
            song,
            refs_text,
            theme_input: String::new(),
            title_error: false,
            refs_error: None,
            link_error: false,
            confirm_discard: false,
            focus_title: true,
        }
    }

    fn changed(&self) -> bool {
        let refs_now = self
            .original
            .refs
            .iter()
            .map(|r| r.label())
            .collect::<Vec<_>>()
            .join("; ");
        self.song != self.original || self.refs_text != refs_now
    }
}

pub(super) struct WorshipState {
    cache: HashMap<Passage, HymnState>,
    tx: mpsc::Sender<HymnReply>,
    rx: mpsc::Receiver<HymnReply>,
    wanted: Option<(Passage, Instant)>,
    pub(super) editor: Option<SongDraft>,
    library_filter: String,
    library_theme: Option<String>,
    pub(super) note_theme: Option<String>,
    new_item: String,
    /// Per-note "add tag" input on sticky notes.
    pub(super) tag_input: HashMap<u64, String>,
}

impl WorshipState {
    pub(super) fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            cache: HashMap::new(),
            tx,
            rx,
            wanted: None,
            editor: None,
            library_filter: String::new(),
            library_theme: None,
            note_theme: None,
            new_item: String::new(),
            tag_input: HashMap::new(),
        }
    }
}

/// Deferred worship operations, applied after a tab has rendered so the
/// rendering code can borrow freely.
enum Op {
    TurnOnHymns,
    TurnOffHymns,
    Retry(Passage),
    NewSong,
    EditSong(u64),
    SaveHymnToLibrary(Hymn, Passage),
    AddToService(ServiceItem),
    NewService,
    SelectService(u64),
    DeleteService(u64),
    MoveItem(u64, usize, isize),
    RemoveItem(u64, usize),
    AddCurrentReading,
    AddOther(u64),
    CopyService(u64),
    SaveServiceFile(u64),
}

/// "1830" -> "1,830"
fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Today's date as YYYY-MM-DD (UTC), without a date library.
/// Howard Hinnant's days-to-civil algorithm.
pub(super) fn today() -> String {
    let days = (store::now_secs() / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parse "Psalm 23; John 3:16-18" into passages. Each part must name a
/// chapter. Returns the bad parts on failure.
pub(super) fn parse_refs(text: &str) -> Result<Vec<PassageRef>, String> {
    let mut out = Vec::new();
    let mut bad = Vec::new();
    for part in text
        .split([';', '\n'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        match bible::parse_reference(part).filter(|r| r.explicit_chapter) {
            Some(r) => out.push(PassageRef {
                book: r.book,
                chapter: r.chapter,
                verse_start: r.verse.unwrap_or(0),
                verse_end: r.verse_end.or(r.verse).unwrap_or(0),
            }),
            None => bad.push(format!("“{part}”")),
        }
    }
    if bad.is_empty() {
        Ok(out)
    } else {
        Err(format!(
            "Couldn't read {}. Use forms like “John 3:16”, “Psalm 23” or “1 Cor 13:4-7”, separated by semicolons.",
            bad.join(", ")
        ))
    }
}

/// Small rounded tag. Returns the click response.
pub(super) fn chip(ui: &mut Ui, text: &str, selected: bool, pal: &Palette) -> egui::Response {
    let rt = RichText::new(text)
        .size(12.5)
        .color(if selected { pal.on_accent } else { pal.text });
    let mut b = egui::Button::new(rt)
        .corner_radius(CornerRadius::same(12))
        .min_size(vec2(0.0, 22.0));
    b = if selected {
        b.fill(pal.accent)
    } else {
        b.fill(pal.surface_alt)
    };
    ui.add(b)
}

fn section_label(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).size(11.5).strong().color(pal.text_dim));
    ui.add_space(2.0);
}

fn card<R>(ui: &mut Ui, pal: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(pal.surface)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

impl BibleApp {
    /// The passage the worship tools work on: the selected verses, or else
    /// the focused pane's whole chapter.
    pub(super) fn worship_passage(&self) -> Passage {
        match self.selection {
            Some(s) => Passage {
                book: s.book,
                chapter: s.chapter,
                verses: Some((s.start, s.end)),
            },
            None => {
                let p = &self.s.panes[self.s.focused];
                Passage {
                    book: p.book,
                    chapter: p.chapter,
                    verses: None,
                }
            }
        }
    }

    fn passage_label(p: Passage) -> String {
        match p.verses {
            Some((a, b)) => ref_label(p.book, p.chapter, a, b),
            None => format!("{} {}", BOOKS[p.book as usize].name, p.chapter),
        }
    }

    fn passage_ref(p: Passage) -> PassageRef {
        let (a, b) = p.verses.unwrap_or((0, 0));
        PassageRef {
            book: p.book,
            chapter: p.chapter,
            verse_start: a,
            verse_end: b,
        }
    }

    /// Per-frame: collect finished lookups and start a debounced one.
    pub(super) fn worship_tick(&mut self, ctx: &egui::Context) {
        while let Ok((p, res)) = self.w.rx.try_recv() {
            let state = match res {
                Ok(h) => HymnState::Ready(h),
                Err(e) => HymnState::Failed(e),
            };
            self.w.cache.insert(p, state);
        }
        if let Some((p, since)) = self.w.wanted {
            if self.w.cache.contains_key(&p) {
                self.w.wanted = None;
            } else if since.elapsed() >= LOOKUP_DEBOUNCE {
                self.w.wanted = None;
                self.w.cache.insert(p, HymnState::Loading);
                let tx = self.w.tx.clone();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let res = hymnary::fetch(p);
                    // The receiver only disappears when the app is closing.
                    let _ = tx.send((p, res));
                    ctx.request_repaint();
                });
            } else {
                ctx.request_repaint_after(LOOKUP_DEBOUNCE.saturating_sub(since.elapsed()));
            }
        }
    }

    fn want_hymns(&mut self, p: Passage) {
        if !self.w.cache.contains_key(&p) && self.w.wanted.map(|w| w.0) != Some(p) {
            self.w.wanted = Some((p, Instant::now()));
        }
    }

    // ---------------------------------------------------------------------
    // Songs for the passage
    // ---------------------------------------------------------------------

    pub(super) fn songs_tab(&mut self, ui: &mut Ui, pal: &Palette) {
        let p = self.worship_passage();
        let mut ops = Vec::new();
        ui.label(
            RichText::new(format!("Songs for {}", Self::passage_label(p)))
                .font(FontId::new(17.0, theme::serif_bold()))
                .color(pal.text),
        );
        if p.verses.is_none() {
            ui.label(
                RichText::new("Select verses to narrow this down.")
                    .small()
                    .color(pal.text_dim),
            );
        }

        // Your library
        let (vs, ve) = p.verses.unwrap_or((1, u16::MAX));
        let mine: Vec<&Song> = self
            .store
            .songs
            .iter()
            .filter(|s| s.matches_passage(p.book, p.chapter, vs, ve))
            .collect();
        section_label(ui, pal, &format!("YOUR SONGS ({})", mine.len()));
        if mine.is_empty() {
            ui.label(
                RichText::new(
                    "None of your songs are linked to this passage yet. Add Bible references to songs in “My songs”.",
                )
                .color(pal.text_dim),
            );
        }
        for s in mine {
            card(ui, pal, |ui| {
                ui.label(RichText::new(&s.title).strong().color(pal.text));
                let mut meta = Vec::new();
                if !s.artist.is_empty() {
                    meta.push(s.artist.clone());
                }
                if !s.key.is_empty() {
                    meta.push(format!("Key {}", s.key));
                }
                if !s.ccli.is_empty() {
                    meta.push(format!("CCLI {}", s.ccli));
                }
                if !meta.is_empty() {
                    ui.label(
                        RichText::new(meta.join("  ·  "))
                            .small()
                            .color(pal.text_dim),
                    );
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button("➕ Add to service").clicked() {
                        ops.push(Op::AddToService(ServiceItem::Song {
                            title: s.title.clone(),
                            detail: s.artist.clone(),
                            key: s.key.clone(),
                            ccli: s.ccli.clone(),
                            link: s.link.clone(),
                            song_id: Some(s.id),
                        }));
                    }
                    if ui.small_button("✏ Edit").clicked() {
                        ops.push(Op::EditSong(s.id));
                    }
                });
            });
            ui.add_space(4.0);
        }

        // Hymnary
        section_label(ui, pal, "HYMNS FROM HYMNARY.ORG");
        if !self.s.hymns_online {
            card(ui, pal, |ui| {
                ui.label(
                    RichText::new("Find traditional hymns for this passage")
                        .strong()
                        .color(pal.text),
                );
                ui.label(
                    RichText::new(
                        "BibleWolf can look the passage up in Hymnary.org's scripture index. \
                         Only the Bible reference (e.g. “John 3:16”) is sent. You can turn this off again at any time.",
                    )
                    .color(pal.text_dim),
                );
                if ui.button("Turn on hymn lookup").clicked() {
                    ops.push(Op::TurnOnHymns);
                }
            });
        } else {
            self.want_hymns(p);
            match self.w.cache.get(&p) {
                None | Some(HymnState::Loading) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Looking up hymns…").color(pal.text_dim));
                    });
                }
                Some(HymnState::Failed(e)) => {
                    Frame::new()
                        .fill(pal.error_bg)
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.label(RichText::new(format!("⚠ {e}")).color(pal.error_fg));
                            if ui.button("Try again").clicked() {
                                ops.push(Op::Retry(p));
                            }
                        });
                }
                Some(HymnState::Ready(list)) if list.is_empty() => {
                    let hint = if p.verses.is_some() {
                        "No hymns are indexed for these exact verses. Press Esc to clear the selection and see hymns for the whole chapter."
                    } else {
                        "No hymns are indexed for this chapter."
                    };
                    ui.label(RichText::new(hint).color(pal.text_dim));
                }
                Some(HymnState::Ready(list)) => {
                    ui.label(
                        RichText::new(
                            // Hymnary returns at most 100 matches per request.
                            if list.len() >= 100 {
                                "100+ found · showing the 100 most relevant, most widely sung first"
                                    .to_owned()
                            } else {
                                format!("{} found · most widely sung first", list.len())
                            },
                        )
                        .small()
                        .color(pal.text_dim),
                    );
                    for h in list {
                        card(ui, pal, |ui| {
                            ui.label(RichText::new(&h.title).strong().color(pal.text));
                            if h.first_line != h.title {
                                ui.label(
                                    RichText::new(&h.first_line)
                                        .italics()
                                        .small()
                                        .color(pal.text_dim),
                                );
                            }
                            let people = h.people_line();
                            if !people.is_empty() {
                                ui.label(RichText::new(people).small().color(pal.text_dim));
                            }
                            let mut meta = Vec::new();
                            if h.hymnals > 0 {
                                meta.push(format!("in {} hymnals", thousands(h.hymnals)));
                            }
                            if !h.meter.is_empty() {
                                meta.push(format!("meter {}", h.meter));
                            }
                            if !h.scripture.is_empty() {
                                meta.push(h.scripture.clone());
                            }
                            if !meta.is_empty() {
                                ui.label(
                                    RichText::new(meta.join("  ·  "))
                                        .small()
                                        .color(pal.text_dim),
                                );
                            }
                            ui.horizontal_wrapped(|ui| {
                                if !h.link.is_empty() {
                                    ui.hyperlink_to(RichText::new("Words & tunes ↗").color(pal.accent), &h.link)
                                        .on_hover_text(&h.link);
                                }
                                if ui.small_button("➕ Add to service").clicked() {
                                    ops.push(Op::AddToService(ServiceItem::Song {
                                        title: h.title.clone(),
                                        detail: people_short(h),
                                        key: String::new(),
                                        ccli: String::new(),
                                        link: h.link.clone(),
                                        song_id: None,
                                    }));
                                }
                                if ui
                                    .small_button("Save to My songs")
                                    .on_hover_text("Add this hymn to your library so you can set its key and notes")
                                    .clicked()
                                {
                                    ops.push(Op::SaveHymnToLibrary(h.clone(), p));
                                }
                            });
                        });
                        ui.add_space(4.0);
                    }
                }
            }
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Hymn data:").small().color(pal.text_dim));
                ui.hyperlink_to(
                    RichText::new("Hymnary.org").small().color(pal.accent),
                    "https://hymnary.org",
                );
                if ui
                    .add(egui::Button::new(RichText::new("Turn off lookup").small()).frame(false))
                    .clicked()
                {
                    ops.push(Op::TurnOffHymns);
                }
            });
        }
        let ctx = ui.ctx().clone();
        for op in ops {
            self.apply_op(&ctx, op);
        }
    }

    // ---------------------------------------------------------------------
    // Song library
    // ---------------------------------------------------------------------

    pub(super) fn library_tab(&mut self, ui: &mut Ui, pal: &Palette) {
        let mut ops = Vec::new();
        ui.horizontal(|ui| {
            if ui.button("➕ New song").clicked() {
                ops.push(Op::NewSong);
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.w.library_filter)
                    .hint_text(RichText::new("Filter title, artist, theme…").color(pal.text_dim))
                    .desired_width(f32::INFINITY),
            );
        });
        if self.store.songs.is_empty() {
            Self::empty_state(
                ui,
                pal,
                "Your song library is empty",
                "Add the songs your team plays, with their Bible references and themes. BibleWolf then suggests them for any passage you select.",
            );
            ui.vertical_centered(|ui| {
                if ui.button("➕ Add your first song").clicked() {
                    ops.push(Op::NewSong);
                }
            });
        } else {
            let mut themes: Vec<String> = self
                .store
                .songs
                .iter()
                .flat_map(|s| s.themes.iter().cloned())
                .collect();
            themes.sort_by_key(|t| t.to_lowercase());
            themes.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
            if !themes.is_empty() {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for t in themes {
                        let sel = self
                            .w
                            .library_theme
                            .as_deref()
                            .is_some_and(|x| x.eq_ignore_ascii_case(&t));
                        if chip(ui, &t, sel, pal)
                            .on_hover_text("Filter by theme")
                            .clicked()
                        {
                            self.w.library_theme = if sel { None } else { Some(t.clone()) };
                        }
                    }
                });
            }
            let f = self.w.library_filter.trim().to_lowercase();
            let mut songs: Vec<&Song> = self
                .store
                .songs
                .iter()
                .filter(|s| {
                    self.w
                        .library_theme
                        .as_deref()
                        .is_none_or(|t| s.themes.iter().any(|x| x.eq_ignore_ascii_case(t)))
                })
                .filter(|s| {
                    f.is_empty()
                        || s.title.to_lowercase().contains(&f)
                        || s.artist.to_lowercase().contains(&f)
                        || s.themes.iter().any(|t| t.to_lowercase().contains(&f))
                })
                .collect();
            songs.sort_by_key(|s| s.title.to_lowercase());
            ui.add_space(6.0);
            if songs.is_empty() {
                ui.label(RichText::new("No songs match this filter.").color(pal.text_dim));
            }
            for s in songs {
                card(ui, pal, |ui| {
                    ui.label(RichText::new(&s.title).strong().color(pal.text));
                    let mut meta = Vec::new();
                    if !s.artist.is_empty() {
                        meta.push(s.artist.clone());
                    }
                    if !s.key.is_empty() {
                        meta.push(format!("Key {}", s.key));
                    }
                    if !s.tempo.is_empty() {
                        meta.push(s.tempo.clone());
                    }
                    if !s.ccli.is_empty() {
                        meta.push(format!("CCLI {}", s.ccli));
                    }
                    if !meta.is_empty() {
                        ui.label(
                            RichText::new(meta.join("  ·  "))
                                .small()
                                .color(pal.text_dim),
                        );
                    }
                    if !s.refs.is_empty() {
                        let refs = s
                            .refs
                            .iter()
                            .map(|r| r.label())
                            .collect::<Vec<_>>()
                            .join("; ");
                        ui.label(
                            RichText::new(format!("📖 {refs}"))
                                .small()
                                .color(pal.accent),
                        );
                    }
                    if !s.themes.is_empty() {
                        ui.label(
                            RichText::new(format!("🏷 {}", s.themes.join(", ")))
                                .small()
                                .color(pal.text_dim),
                        );
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.small_button("✏ Edit").clicked() {
                            ops.push(Op::EditSong(s.id));
                        }
                        if ui.small_button("➕ Add to service").clicked() {
                            ops.push(Op::AddToService(ServiceItem::Song {
                                title: s.title.clone(),
                                detail: s.artist.clone(),
                                key: s.key.clone(),
                                ccli: s.ccli.clone(),
                                link: s.link.clone(),
                                song_id: Some(s.id),
                            }));
                        }
                        if !s.link.is_empty() {
                            ui.hyperlink_to(RichText::new("Link ↗").color(pal.accent), &s.link);
                        }
                    });
                });
                ui.add_space(4.0);
            }
        }
        let ctx = ui.ctx().clone();
        for op in ops {
            self.apply_op(&ctx, op);
        }
    }

    // ---------------------------------------------------------------------
    // Services / set lists
    // ---------------------------------------------------------------------

    fn active_service_index(&self) -> Option<usize> {
        let id = self.s.active_service?;
        self.store.services.iter().position(|s| s.id == id)
    }

    pub(super) fn services_tab(&mut self, ui: &mut Ui, pal: &Palette) {
        let mut ops = Vec::new();
        let active = self.active_service_index();
        ui.horizontal(|ui| {
            let label = active.map_or("Choose a service".to_owned(), |i| {
                service_name(&self.store.services[i])
            });
            egui::ComboBox::from_id_salt("service-pick")
                .selected_text(label)
                .width(ui.available_width() - 110.0)
                .show_ui(ui, |ui| {
                    let mut list: Vec<&Service> = self.store.services.iter().collect();
                    list.sort_by(|a, b| b.date.cmp(&a.date).then(b.created.cmp(&a.created)));
                    for s in list {
                        if ui
                            .selectable_label(Some(s.id) == self.s.active_service, service_name(s))
                            .clicked()
                        {
                            ops.push(Op::SelectService(s.id));
                        }
                    }
                });
            if ui
                .button("➕ New")
                .on_hover_text("Plan a new service")
                .clicked()
            {
                ops.push(Op::NewService);
            }
        });

        let Some(idx) = active else {
            Self::empty_state(
                ui,
                pal,
                "Plan a service",
                "Build an order of service: Bible readings, songs and other items. Then copy it or save it as a text file for your team.",
            );
            ui.vertical_centered(|ui| {
                if ui.button("➕ New service").clicked() {
                    ops.push(Op::NewService);
                }
            });
            let ctx = ui.ctx().clone();
            for op in ops {
                self.apply_op(&ctx, op);
            }
            return;
        };
        let sid = self.store.services[idx].id;

        ui.add_space(6.0);
        let mut changed = false;
        egui::Grid::new("svc-fields")
            .num_columns(2)
            .spacing(vec2(8.0, 6.0))
            .show(ui, |ui| {
                ui.label("Title");
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut self.store.services[idx].title)
                            .desired_width(f32::INFINITY),
                    )
                    .changed();
                ui.end_row();
                ui.label("Date");
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut self.store.services[idx].date)
                            .hint_text(RichText::new("e.g. 2026-11-29").color(pal.text_dim))
                            .desired_width(f32::INFINITY),
                    )
                    .changed();
                ui.end_row();
            });

        section_label(ui, pal, "ORDER OF SERVICE");
        let n_items = self.store.services[idx].items.len();
        if n_items == 0 {
            ui.label(
                RichText::new(
                    "Nothing planned yet. Add the selected passage below, songs from the Songs or My songs tabs, or other items.",
                )
                .color(pal.text_dim),
            );
        }
        for (i, slot) in self.store.services[idx].items.iter_mut().enumerate() {
            let item = slot.clone();
            card(ui, pal, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{}.", i + 1))
                            .strong()
                            .color(pal.accent),
                    );
                    ui.vertical(|ui| {
                        match &item {
                            ServiceItem::Reading {
                                passage,
                                translation,
                            } => {
                                ui.label(
                                    RichText::new(format!(
                                        "📖 {} ({translation})",
                                        passage.label()
                                    ))
                                    .strong()
                                    .color(pal.text),
                                );
                            }
                            ServiceItem::Song {
                                title,
                                detail,
                                ccli,
                                ..
                            } => {
                                ui.label(
                                    RichText::new(format!("🎵 {title}"))
                                        .strong()
                                        .color(pal.text),
                                );
                                let mut meta = Vec::new();
                                if !detail.is_empty() {
                                    meta.push(detail.clone());
                                }
                                if !ccli.is_empty() {
                                    meta.push(format!("CCLI {ccli}"));
                                }
                                if !meta.is_empty() {
                                    ui.label(
                                        RichText::new(meta.join("  ·  "))
                                            .small()
                                            .color(pal.text_dim),
                                    );
                                }
                            }
                            ServiceItem::Other { text } => {
                                ui.label(RichText::new(text).color(pal.text));
                            }
                        }
                        if let ServiceItem::Song { key, .. } = slot {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Key").small().color(pal.text_dim));
                                changed |= ui
                                    .add(
                                        egui::TextEdit::singleline(key)
                                            .desired_width(48.0)
                                            .hint_text(RichText::new("—").color(pal.text_dim)),
                                    )
                                    .changed();
                            });
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if ui
                            .add(egui::Button::new("✖").frame(false))
                            .on_hover_text("Remove from service")
                            .clicked()
                        {
                            ops.push(Op::RemoveItem(sid, i));
                        }
                        if ui
                            .add_enabled(i + 1 < n_items, egui::Button::new("⬇").frame(false))
                            .on_hover_text("Move down")
                            .clicked()
                        {
                            ops.push(Op::MoveItem(sid, i, 1));
                        }
                        if ui
                            .add_enabled(i > 0, egui::Button::new("⬆").frame(false))
                            .on_hover_text("Move up")
                            .clicked()
                        {
                            ops.push(Op::MoveItem(sid, i, -1));
                        }
                    });
                });
            });
            ui.add_space(3.0);
        }

        ui.add_space(6.0);
        let reading = Self::passage_label(self.worship_passage());
        if ui
            .button(format!("➕ Reading: {reading}"))
            .on_hover_text("Adds the selected verses (or the focused pane's chapter) in that pane's translation")
            .clicked()
        {
            ops.push(Op::AddCurrentReading);
        }
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.w.new_item)
                    .hint_text(
                        RichText::new("Other item, e.g. Welcome, Prayer, Offering")
                            .color(pal.text_dim),
                    )
                    .desired_width(ui.available_width() - 60.0),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (ui.button("Add").clicked() || enter) && !self.w.new_item.trim().is_empty() {
                ops.push(Op::AddOther(sid));
            }
        });

        section_label(ui, pal, "NOTES FOR THE TEAM");
        changed |= ui
            .add(
                egui::TextEdit::multiline(&mut self.store.services[idx].notes)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .hint_text(
                        RichText::new("Arrangements, who's leading, rehearsal time…")
                            .color(pal.text_dim),
                    ),
            )
            .changed();
        if changed {
            // service_mut stamps `modified` and schedules a save.
            let _ = self.store.service_mut(sid);
        }

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("📋 Copy")
                .on_hover_text("Copy the plan, with reading texts, as plain text")
                .clicked()
            {
                ops.push(Op::CopyService(sid));
            }
            if ui
                .button("💾 Save as file")
                .on_hover_text("Save the plan as a Markdown text file in Documents/BibleWolf")
                .clicked()
            {
                ops.push(Op::SaveServiceFile(sid));
            }
            if ui
                .add(
                    egui::Button::new(RichText::new("🗑 Delete service").color(pal.error_fg))
                        .frame(false),
                )
                .on_hover_text("Delete this service plan (you can undo from the status bar)")
                .clicked()
            {
                ops.push(Op::DeleteService(sid));
            }
        });

        let ctx = ui.ctx().clone();
        for op in ops {
            self.apply_op(&ctx, op);
        }
    }

    /// Plain-text / Markdown service plan including the reading texts.
    fn service_text(&self, svc: &Service) -> String {
        let mut out = format!(
            "# {}\n",
            if svc.title.trim().is_empty() {
                "Service"
            } else {
                svc.title.trim()
            }
        );
        if !svc.date.trim().is_empty() {
            out.push_str(&format!("{}\n", svc.date.trim()));
        }
        out.push('\n');
        for (i, item) in svc.items.iter().enumerate() {
            match item {
                ServiceItem::Reading {
                    passage,
                    translation,
                } => {
                    out.push_str(&format!(
                        "{}. Reading: {} ({translation})\n",
                        i + 1,
                        passage.label()
                    ));
                    if let Some(t) = self.translation(translation)
                        && let Some(ch) = t.chapter(passage.book, passage.chapter)
                    {
                        let mut text = String::new();
                        for (v, vt) in &ch.verses {
                            if passage.whole_chapter()
                                || (passage.verse_start..=passage.verse_end).contains(v)
                            {
                                text.push_str(&format!("{v} {vt} "));
                            }
                        }
                        out.push_str(&format!("   > {}\n", text.trim_end()));
                    }
                }
                ServiceItem::Song {
                    title,
                    detail,
                    key,
                    ccli,
                    link,
                    ..
                } => {
                    let mut line = format!("{}. Song: {title}", i + 1);
                    if !detail.is_empty() {
                        line.push_str(&format!(" — {detail}"));
                    }
                    if !key.is_empty() {
                        line.push_str(&format!(" (Key {key})"));
                    }
                    if !ccli.is_empty() {
                        line.push_str(&format!(" CCLI {ccli}"));
                    }
                    out.push_str(&line);
                    out.push('\n');
                    if !link.is_empty() {
                        out.push_str(&format!("   {link}\n"));
                    }
                }
                ServiceItem::Other { text } => out.push_str(&format!("{}. {text}\n", i + 1)),
            }
        }
        if !svc.notes.trim().is_empty() {
            out.push_str(&format!("\nNotes:\n{}\n", svc.notes.trim()));
        }
        out.push_str("\n— Planned with BibleWolf\n");
        out
    }

    fn save_service_file(&mut self, id: u64) {
        let Some(svc) = self.store.services.iter().find(|s| s.id == id) else {
            return;
        };
        let text = self.service_text(svc);
        let stem: String = format!("{} {}", svc.date.trim(), svc.title.trim())
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == ' ' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .trim()
            .replace(' ', "-");
        let stem = if stem.is_empty() {
            "service".to_owned()
        } else {
            stem
        };
        let Some(dir) = dirs::document_dir()
            .or_else(dirs::home_dir)
            .map(|d| d.join("BibleWolf"))
        else {
            self.error("Couldn't find your Documents folder to save the plan.");
            return;
        };
        let result = (|| -> std::io::Result<std::path::PathBuf> {
            std::fs::create_dir_all(&dir)?;
            // Never overwrite an earlier export.
            let mut path = dir.join(format!("{stem}.md"));
            let mut n = 2;
            while path.exists() {
                path = dir.join(format!("{stem}-{n}.md"));
                n += 1;
            }
            std::fs::write(&path, text)?;
            Ok(path)
        })();
        match result {
            Ok(p) => self.info(format!("Saved the service plan to {}.", p.display())),
            Err(e) => self.error(format!(
                "Couldn't save the service plan in {}: {e}",
                dir.display()
            )),
        }
    }

    fn add_to_service(&mut self, item: ServiceItem) {
        let id = match self.active_service_index() {
            Some(i) => self.store.services[i].id,
            None => {
                let id = self.store.add_service("Sunday service".into(), today());
                self.s.active_service = Some(id);
                id
            }
        };
        let what = match &item {
            ServiceItem::Reading { passage, .. } => passage.label(),
            ServiceItem::Song { title, .. } => format!("“{title}”"),
            ServiceItem::Other { text } => format!("“{text}”"),
        };
        if let Some(svc) = self.store.service_mut(id) {
            svc.items.push(item);
            let name = service_name(svc);
            self.info(format!("Added {what} to {name}."));
        }
    }

    pub(super) fn add_current_reading(&mut self) {
        let p = self.worship_passage();
        let translation = self.s.panes[self.s.focused].translation.clone();
        self.add_to_service(ServiceItem::Reading {
            passage: Self::passage_ref(p),
            translation,
        });
    }

    fn apply_op(&mut self, ctx: &egui::Context, op: Op) {
        match op {
            Op::TurnOnHymns => self.s.hymns_online = true,
            Op::TurnOffHymns => {
                self.s.hymns_online = false;
                self.w.wanted = None;
            }
            Op::Retry(p) => {
                self.w.cache.remove(&p);
                self.want_hymns(p);
                ctx.request_repaint();
            }
            Op::NewSong => self.w.editor = Some(SongDraft::new(Song::default())),
            Op::EditSong(id) => {
                if let Some(s) = self.store.songs.iter().find(|s| s.id == id) {
                    self.w.editor = Some(SongDraft::new(s.clone()));
                }
            }
            Op::SaveHymnToLibrary(h, p) => {
                // Opens the editor prefilled; nothing is stored until Save.
                let mut draft = SongDraft::new(Song::default());
                draft.song.title = h.title.clone();
                draft.song.artist = people_short(&h);
                draft.song.link = h.link.clone();
                draft.song.refs = vec![Self::passage_ref(p)];
                draft.refs_text = Self::passage_ref(p).label();
                self.w.editor = Some(draft);
            }
            Op::AddToService(item) => self.add_to_service(item),
            Op::NewService => {
                let id = self.store.add_service("Sunday service".into(), today());
                self.s.active_service = Some(id);
            }
            Op::SelectService(id) => self.s.active_service = Some(id),
            Op::DeleteService(id) => {
                if let Some(svc) = self.store.remove_service(id) {
                    let name = service_name(&svc);
                    self.push_deleted(Deleted::Service(svc), format!("Deleted {name}."));
                    self.s.active_service = self.store.services.last().map(|s| s.id);
                }
            }
            Op::MoveItem(id, i, d) => {
                if let Some(svc) = self.store.service_mut(id) {
                    let j = i as isize + d;
                    if j >= 0 && (j as usize) < svc.items.len() && i < svc.items.len() {
                        svc.items.swap(i, j as usize);
                    }
                }
            }
            Op::RemoveItem(id, i) => {
                if let Some(svc) = self.store.service_mut(id)
                    && i < svc.items.len()
                {
                    svc.items.remove(i);
                }
            }
            Op::AddCurrentReading => self.add_current_reading(),
            Op::AddOther(id) => {
                let text = std::mem::take(&mut self.w.new_item).trim().to_owned();
                if let Some(svc) = self.store.service_mut(id) {
                    svc.items.push(ServiceItem::Other { text });
                }
            }
            Op::CopyService(id) => {
                if let Some(svc) = self.store.services.iter().find(|s| s.id == id) {
                    ctx.copy_text(self.service_text(svc));
                    self.info("Copied the service plan.");
                }
            }
            Op::SaveServiceFile(id) => self.save_service_file(id),
        }
    }

    // ---------------------------------------------------------------------
    // Song editor dialog
    // ---------------------------------------------------------------------

    pub(super) fn song_editor(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.w.editor.take() else {
            return;
        };
        let pal = self.pal();
        let is_new = d.song.id == 0;
        let mut close = false;
        let mut save = false;
        let mut delete = false;

        let resp = egui::Modal::new(Id::new("song-editor"))
            .frame(Frame::window(&ctx.global_style()).inner_margin(Margin::same(18)))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(
                    RichText::new(if is_new { "New song" } else { "Edit song" })
                        .font(FontId::new(20.0, theme::serif_bold()))
                        .color(pal.text),
                );
                ui.add_space(8.0);
                egui::Grid::new("song-form").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                    ui.label("Title *");
                    ui.vertical(|ui| {
                        let r = ui.add(egui::TextEdit::singleline(&mut d.song.title).desired_width(f32::INFINITY));
                        if d.focus_title {
                            r.request_focus();
                            d.focus_title = false;
                        }
                        if r.changed() {
                            d.title_error = false;
                        }
                        if d.title_error {
                            ui.label(RichText::new("Give the song a title.").small().color(pal.error_fg));
                        }
                    });
                    ui.end_row();

                    ui.label("Artist / author");
                    ui.add(egui::TextEdit::singleline(&mut d.song.artist).desired_width(f32::INFINITY));
                    ui.end_row();

                    ui.label("Key");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut d.song.key)
                                .desired_width(60.0)
                                .hint_text(RichText::new("e.g. G").color(pal.text_dim)),
                        );
                        ui.label("Tempo");
                        ui.add(
                            egui::TextEdit::singleline(&mut d.song.tempo)
                                .desired_width(90.0)
                                .hint_text(RichText::new("e.g. 72 bpm").color(pal.text_dim)),
                        );
                    });
                    ui.end_row();

                    ui.label("CCLI #");
                    ui.add(egui::TextEdit::singleline(&mut d.song.ccli).desired_width(120.0));
                    ui.end_row();

                    ui.label("Bible references");
                    ui.vertical(|ui| {
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut d.refs_text)
                                .desired_width(f32::INFINITY)
                                .hint_text(RichText::new("e.g. Psalm 23; John 3:16-18").color(pal.text_dim)),
                        );
                        if r.changed() {
                            d.refs_error = None;
                        }
                        match &d.refs_error {
                            Some(e) => {
                                ui.label(RichText::new(e).small().color(pal.error_fg));
                            }
                            None => {
                                ui.label(
                                    RichText::new("This song is suggested when you select these passages.")
                                        .small()
                                        .color(pal.text_dim),
                                );
                            }
                        }
                    });
                    ui.end_row();

                    ui.label("Themes");
                    ui.vertical(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                            let mut remove = None;
                            for (i, t) in d.song.themes.iter().enumerate() {
                                if chip(ui, &format!("{t}  ✖"), true, &pal).on_hover_text("Remove theme").clicked() {
                                    remove = Some(i);
                                }
                            }
                            if let Some(i) = remove {
                                d.song.themes.remove(i);
                            }
                        });
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut d.theme_input)
                                .desired_width(f32::INFINITY)
                                .hint_text(RichText::new("Type a theme and press Enter").color(pal.text_dim)),
                        );
                        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            add_theme(&mut d.song.themes, &d.theme_input);
                            d.theme_input.clear();
                            r.request_focus();
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                            let unused: Vec<&str> = SUGGESTED_THEMES
                                .iter()
                                .copied()
                                .filter(|t| !d.song.themes.iter().any(|x| x.eq_ignore_ascii_case(t)))
                                .collect();
                            for t in unused {
                                if chip(ui, t, false, &pal).on_hover_text("Add theme").clicked() {
                                    add_theme(&mut d.song.themes, t);
                                }
                            }
                        });
                    });
                    ui.end_row();

                    ui.label("Link");
                    ui.vertical(|ui| {
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut d.song.link)
                                .desired_width(f32::INFINITY)
                                .hint_text(RichText::new("Chords, video or lyrics page (https://…)").color(pal.text_dim)),
                        );
                        if r.changed() {
                            d.link_error = false;
                        }
                        if d.link_error {
                            ui.label(
                                RichText::new("Links must start with https:// or http://").small().color(pal.error_fg),
                            );
                        }
                    });
                    ui.end_row();

                    ui.label("Notes");
                    ui.add(
                        egui::TextEdit::multiline(&mut d.song.notes)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY)
                            .hint_text(RichText::new("Arrangement, capo, who sings…").color(pal.text_dim)),
                    );
                    ui.end_row();
                });

                ui.add_space(12.0);
                if d.confirm_discard {
                    Frame::new()
                        .fill(pal.error_bg)
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Discard your changes?").color(pal.error_fg));
                                if ui.button("Discard").clicked() {
                                    close = true;
                                }
                                if ui.button("Keep editing").clicked() {
                                    d.confirm_discard = false;
                                }
                            });
                        });
                    ui.add_space(8.0);
                }
                ui.horizontal(|ui| {
                    if !is_new
                        && ui
                            .add(egui::Button::new(RichText::new("🗑 Delete").color(pal.error_fg)).frame(false))
                            .on_hover_text("Delete this song (you can undo from the status bar)")
                            .clicked()
                    {
                        delete = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let save_btn =
                            egui::Button::new(RichText::new("Save").strong().color(pal.on_accent)).fill(pal.accent);
                        if ui.add(save_btn).clicked() {
                            save = true;
                        }
                        if ui.button("Cancel").clicked() {
                            if d.changed() {
                                d.confirm_discard = true;
                            } else {
                                close = true;
                            }
                        }
                    });
                });
            });

        // Escape cancels (asking first if there are edits). Clicking the
        // backdrop deliberately does nothing so typed work isn't lost.
        if resp.is_top_modal && !resp.any_popup_open && ctx.input(|i| i.key_pressed(Key::Escape)) {
            if d.changed() && !d.confirm_discard {
                d.confirm_discard = true;
            } else {
                close = true;
            }
        }

        if save {
            if !d.theme_input.trim().is_empty() {
                add_theme(&mut d.song.themes, &d.theme_input.clone());
                d.theme_input.clear();
            }
            d.title_error = d.song.title.trim().is_empty();
            let link = d.song.link.trim().to_owned();
            d.link_error =
                !link.is_empty() && !(link.starts_with("https://") || link.starts_with("http://"));
            match parse_refs(&d.refs_text) {
                Ok(refs) => {
                    d.song.refs = refs;
                    d.refs_error = None;
                }
                Err(e) => d.refs_error = Some(e),
            }
            if !d.title_error && !d.link_error && d.refs_error.is_none() {
                d.song.title = d.song.title.trim().to_owned();
                d.song.link = link;
                let title = d.song.title.clone();
                self.store.save_song(d.song);
                self.info(format!("Saved “{title}” to My songs."));
                return;
            }
        }
        if delete {
            if let Some(song) = self.store.remove_song(d.song.id) {
                let title = song.title.clone();
                self.push_deleted(
                    Deleted::Song(song),
                    format!("Deleted “{title}” from My songs."),
                );
            }
            return;
        }
        if !close {
            self.w.editor = Some(d);
        }
    }
}

fn add_theme(themes: &mut Vec<String>, t: &str) {
    let t = t.trim();
    if !t.is_empty() && !themes.iter().any(|x| x.eq_ignore_ascii_case(t)) {
        themes.push(t.to_owned());
    }
}

fn service_name(s: &Service) -> String {
    let title = if s.title.trim().is_empty() {
        "Untitled service"
    } else {
        s.title.trim()
    };
    if s.date.trim().is_empty() {
        title.to_owned()
    } else {
        format!("{title} — {}", s.date.trim())
    }
}

/// Main author for compact display ("Newton, John, 1725-1807").
fn people_short(h: &Hymn) -> String {
    h.people
        .iter()
        .find(|(r, _)| r == "author")
        .or_else(|| h.people.first())
        .map(|(_, n)| n.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separators() {
        assert_eq!(thousands(5), "5");
        assert_eq!(thousands(1830), "1,830");
        assert_eq!(thousands(1234567), "1,234,567");
    }

    #[test]
    fn refs_parsing() {
        let r = parse_refs("Psalm 23; John 3:16-18").unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].book, r[0].chapter, r[0].verse_start), (18, 23, 0));
        assert_eq!((r[1].verse_start, r[1].verse_end), (16, 18));
        assert_eq!(parse_refs("").unwrap(), vec![]);
        let e = parse_refs("Romans; John 3").unwrap_err();
        assert!(
            e.contains("“Romans”"),
            "bare book names need a chapter: {e}"
        );
    }

    #[test]
    fn today_is_a_plausible_date() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert!(t.starts_with("20"));
        assert_eq!(&t[4..5], "-");
    }
}
