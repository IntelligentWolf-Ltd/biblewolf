//! Look and feel: two palettes ("Midnight" dark, "Parchment" light), the
//! embedded Noto Serif reading face, and egui style tuning.

use crate::store::Swatch;
use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Margin, Shadow, Stroke};
use std::sync::Arc;

pub const SERIF: &str = "serif";
pub const SERIF_BOLD: &str = "serif-bold";
pub const SERIF_ITALIC: &str = "serif-italic";

pub fn serif() -> FontFamily {
    FontFamily::Name(SERIF.into())
}
pub fn serif_bold() -> FontFamily {
    FontFamily::Name(SERIF_BOLD.into())
}
pub fn serif_italic() -> FontFamily {
    FontFamily::Name(SERIF_ITALIC.into())
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub surface: Color32,
    pub surface_alt: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub verse_num: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub error_fg: Color32,
    pub error_bg: Color32,
    pub ok_fg: Color32,
    pub ok_bg: Color32,
    /// Words of Jesus ("red letters") on the page.
    pub red_letter: Color32,
    /// Red letters on a highlighted verse. In the dark theme no true red keeps
    /// 4.5:1 on yellow ink, so highlighted red letters lighten to a rose.
    pub red_letter_on_ink: Color32,
}

pub const MIDNIGHT: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(0x0f, 0x11, 0x17),
    panel: Color32::from_rgb(0x14, 0x17, 0x20),
    surface: Color32::from_rgb(0x1a, 0x1e, 0x29),
    surface_alt: Color32::from_rgb(0x22, 0x27, 0x36),
    border: Color32::from_rgb(0x2e, 0x34, 0x48),
    text: Color32::from_rgb(0xe9, 0xe4, 0xd8),
    text_dim: Color32::from_rgb(0xa3, 0x9f, 0x95),
    accent: Color32::from_rgb(0xd9, 0xb2, 0x6a),
    on_accent: Color32::from_rgb(0x1a, 0x14, 0x08),
    verse_num: Color32::from_rgb(0xd0, 0xa8, 0x5f),
    hover: Color32::from_rgba_premultiplied(10, 10, 10, 10),
    selected: Color32::from_rgba_premultiplied(0x2b, 0x23, 0x15, 0x33),
    error_fg: Color32::from_rgb(0xff, 0xb4, 0xab),
    error_bg: Color32::from_rgb(0x41, 0x1d, 0x1d),
    ok_fg: Color32::from_rgb(0xb8, 0xe6, 0xb5),
    ok_bg: Color32::from_rgb(0x1c, 0x33, 0x22),
    red_letter: Color32::from_rgb(0xff, 0x8a, 0x80),
    red_letter_on_ink: Color32::from_rgb(0xff, 0xbc, 0xb2),
};

pub const PARCHMENT: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(0xec, 0xe3, 0xd0),
    panel: Color32::from_rgb(0xe4, 0xd8, 0xc0),
    surface: Color32::from_rgb(0xfb, 0xf7, 0xee),
    surface_alt: Color32::from_rgb(0xf2, 0xea, 0xdb),
    border: Color32::from_rgb(0xd3, 0xc4, 0xa6),
    text: Color32::from_rgb(0x2a, 0x22, 0x19),
    text_dim: Color32::from_rgb(0x66, 0x58, 0x44),
    accent: Color32::from_rgb(0x74, 0x48, 0x0c),
    on_accent: Color32::from_rgb(0xff, 0xfa, 0xf0),
    verse_num: Color32::from_rgb(0x74, 0x48, 0x0c),
    hover: Color32::from_rgba_premultiplied(0x07, 0x05, 0x02, 0x0d),
    selected: Color32::from_rgba_premultiplied(0x30, 0x1f, 0x06, 0x30),
    error_fg: Color32::from_rgb(0x8c, 0x1d, 0x18),
    error_bg: Color32::from_rgb(0xf9, 0xde, 0xdc),
    ok_fg: Color32::from_rgb(0x1e, 0x5b, 0x2a),
    ok_bg: Color32::from_rgb(0xdc, 0xf0, 0xdc),
    red_letter: Color32::from_rgb(0xa1, 0x15, 0x0f),
    red_letter_on_ink: Color32::from_rgb(0xa1, 0x15, 0x0f),
};

pub fn palette(dark: bool) -> Palette {
    if dark { MIDNIGHT } else { PARCHMENT }
}

/// Pure pen colour, used for swatch buttons.
pub fn swatch_solid(s: Swatch) -> Color32 {
    match s {
        Swatch::Yellow => Color32::from_rgb(250, 214, 72),
        Swatch::Green => Color32::from_rgb(112, 204, 118),
        Swatch::Blue => Color32::from_rgb(104, 164, 246),
        Swatch::Pink => Color32::from_rgb(242, 124, 184),
        Swatch::Orange => Color32::from_rgb(248, 156, 76),
    }
}

/// Highlighter "ink" laid behind verse text. Translucent so it reads like a
/// marker pen; the alpha is tuned per theme so text keeps >= 4.5:1 contrast
/// (checked in tests below).
pub fn highlight_ink(s: Swatch, dark: bool) -> Color32 {
    let c = swatch_solid(s);
    let a = if dark { 0.30 } else { 0.50 };
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0) as u8)
}

/// Sticky note paper. Always light paper with dark ink in both themes, like
/// a real post-it.
pub fn note_paper(s: Swatch) -> Color32 {
    match s {
        Swatch::Yellow => Color32::from_rgb(0xff, 0xf0, 0xa0),
        Swatch::Green => Color32::from_rgb(0xc8, 0xf0, 0xc4),
        Swatch::Blue => Color32::from_rgb(0xcd, 0xe2, 0xff),
        Swatch::Pink => Color32::from_rgb(0xff, 0xd0, 0xe6),
        Swatch::Orange => Color32::from_rgb(0xff, 0xd6, 0xad),
    }
}

pub fn note_band(s: Swatch) -> Color32 {
    let p = note_paper(s);
    let k = 0.88;
    Color32::from_rgb(
        (p.r() as f32 * k) as u8,
        (p.g() as f32 * k) as u8,
        (p.b() as f32 * k) as u8,
    )
}

pub const NOTE_INK: Color32 = Color32::from_rgb(0x2a, 0x24, 0x18);
pub const NOTE_INK_DIM: Color32 = Color32::from_rgb(0x5a, 0x4c, 0x34);

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        &mut fonts,
        SERIF,
        include_bytes!("../assets/fonts/NotoSerif-Regular.ttf"),
    );
    add(
        &mut fonts,
        SERIF_BOLD,
        include_bytes!("../assets/fonts/NotoSerif-Bold.ttf"),
    );
    add(
        &mut fonts,
        SERIF_ITALIC,
        include_bytes!("../assets/fonts/NotoSerif-Italic.ttf"),
    );
    // Fall back to the default proportional chain for symbols/emoji.
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for name in [SERIF, SERIF_BOLD, SERIF_ITALIC] {
        let mut chain = vec![name.to_owned()];
        chain.extend(fallback.iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), chain);
    }
    ctx.set_fonts(fonts);
}

fn pressed_fill(p: &Palette) -> Color32 {
    if p.dark {
        Color32::from_rgb(0x3d, 0x34, 0x20)
    } else {
        Color32::from_rgb(0xdd, 0xc8, 0x9e)
    }
}

fn visuals_for(p: &Palette) -> egui::Visuals {
    let mut v = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let r = CornerRadius::same(8);
    v.panel_fill = p.panel;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 120 } else { 50 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 110 } else { 40 }),
    };
    v.extreme_bg_color = if p.dark { p.bg } else { p.surface };
    v.faint_bg_color = p.surface_alt;
    v.text_edit_bg_color = Some(if p.dark { p.bg } else { p.surface });
    v.hyperlink_color = p.accent;
    v.error_fg_color = p.error_fg;
    v.selection.bg_fill = if p.dark {
        Color32::from_rgb(0x5a, 0x48, 0x22)
    } else {
        Color32::from_rgb(0xe8, 0xcf, 0x9a)
    };
    v.selection.stroke = Stroke::new(1.0, p.text);
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r;

    w.inactive.bg_fill = p.surface_alt;
    w.inactive.weak_bg_fill = p.surface_alt;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = r;

    let hover_fill = if p.dark {
        Color32::from_rgb(0x2c, 0x32, 0x45)
    } else {
        Color32::from_rgb(0xea, 0xdf, 0xc9)
    };
    w.hovered.bg_fill = hover_fill;
    w.hovered.weak_bg_fill = hover_fill;
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent);
    w.hovered.fg_stroke = Stroke::new(1.5, p.text);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;

    // egui uses `widgets.active` text colour for ALL `.strong()` text
    // (Visuals::strong_text_color), so it must be the normal reading colour,
    // not the dark ink used on gold buttons.
    w.active.bg_fill = pressed_fill(p);
    w.active.weak_bg_fill = pressed_fill(p);
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.5, p.text);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;

    w.open.bg_fill = hover_fill;
    w.open.weak_bg_fill = hover_fill;
    w.open.bg_stroke = Stroke::new(1.0, p.accent);
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.corner_radius = r;
    v
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    ctx.set_visuals_of(egui::Theme::Dark, visuals_for(&MIDNIGHT));
    ctx.set_visuals_of(egui::Theme::Light, visuals_for(&PARCHMENT));
    ctx.all_styles_mut(|style| {
        use egui::{FontId, TextStyle};
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::new(22.0, serif_bold()));
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(15.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(14.5));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(12.0));
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 28.0;
        style.spacing.window_margin = Margin::same(12);
        style.spacing.menu_margin = Margin::same(8);
    });
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lin(c: u8) -> f64 {
        let c = c as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    fn lum(c: Color32) -> f64 {
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }
    /// WCAG 2.x contrast ratio.
    fn contrast(a: Color32, b: Color32) -> f64 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }
    /// Composite a premultiplied-alpha colour (egui's storage) over an opaque one.
    fn over_premul(top: Color32, under: Color32) -> Color32 {
        let a = top.a() as f32 / 255.0;
        let mix = |t: u8, u: u8| (t as f32 + u as f32 * (1.0 - a)).round().min(255.0) as u8;
        Color32::from_rgb(
            mix(top.r(), under.r()),
            mix(top.g(), under.g()),
            mix(top.b(), under.b()),
        )
    }

    /// Composite an unmultiplied-alpha colour over an opaque one.
    fn over(top: Color32, under: Color32) -> Color32 {
        let [r, g, b, a] = top.to_srgba_unmultiplied();
        let a = a as f32 / 255.0;
        let mix = |t: u8, u: u8| (t as f32 * a + u as f32 * (1.0 - a)).round() as u8;
        Color32::from_rgb(mix(r, under.r()), mix(g, under.g()), mix(b, under.b()))
    }

    #[test]
    fn text_contrast_meets_wcag_aa() {
        for p in [MIDNIGHT, PARCHMENT] {
            // Bold text uses the "active" widget colour (egui strong_text_color).
            let v = visuals_for(&p);
            for bg in [p.surface, p.surface_alt, p.panel, p.bg, pressed_fill(&p)] {
                let c = contrast(v.strong_text_color(), bg);
                assert!(c >= 4.5, "strong text (dark={}) {c:.2}", p.dark);
            }
            for bg in [p.surface, p.surface_alt, p.panel, p.bg] {
                assert!(contrast(p.text, bg) >= 4.5, "text on bg (dark={})", p.dark);
                assert!(
                    contrast(p.text_dim, bg) >= 4.5,
                    "dim text (dark={}) {:.2}",
                    p.dark,
                    contrast(p.text_dim, bg)
                );
                assert!(
                    contrast(p.verse_num, bg) >= 4.5,
                    "verse num (dark={}) {:.2}",
                    p.dark,
                    contrast(p.verse_num, bg)
                );
                assert!(
                    contrast(p.accent, bg) >= 4.5,
                    "accent text (dark={}) {:.2}",
                    p.dark,
                    contrast(p.accent, bg)
                );
                assert!(
                    contrast(p.error_fg, bg) >= 4.5,
                    "error text on bg (dark={})",
                    p.dark
                );
            }
            assert!(contrast(p.error_fg, p.error_bg) >= 4.5);
            assert!(contrast(p.ok_fg, p.ok_bg) >= 4.5);
            assert!(
                contrast(p.on_accent, p.accent) >= 4.5,
                "on_accent (dark={})",
                p.dark
            );
            // Red letters: on the page, on the selection tint, and on every ink.
            let selected_bg = over_premul(p.selected, p.surface);
            for (what, bg) in [("page", p.surface), ("selection", selected_bg)] {
                let c = contrast(p.red_letter, bg);
                assert!(c >= 4.5, "red letters on {what} (dark={}) {c:.2}", p.dark);
                let c = contrast(p.text, bg);
                assert!(c >= 4.5, "text on {what} (dark={}) {c:.2}", p.dark);
            }
            for s in Swatch::ALL {
                let under = over(highlight_ink(s, p.dark), p.surface);
                let c = contrast(p.red_letter_on_ink, under);
                assert!(
                    c >= 4.5,
                    "red letters on {s:?} ink (dark={}) {c:.2}",
                    p.dark
                );
            }
            for s in Swatch::ALL {
                let under = over(highlight_ink(s, p.dark), p.surface);
                let c = contrast(p.text, under);
                assert!(
                    c >= 4.5,
                    "{:?} highlight dark={} contrast {:.2}",
                    s,
                    p.dark,
                    c
                );
            }
        }
        for s in Swatch::ALL {
            assert!(contrast(NOTE_INK, note_paper(s)) >= 4.5);
            assert!(contrast(NOTE_INK_DIM, note_band(s)) >= 4.5, "{s:?} band");
        }
    }
}
