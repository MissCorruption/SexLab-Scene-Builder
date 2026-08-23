//! Shared visual tokens for the egui UI.

use egui::style::WidgetVisuals;
use egui::{Color32, CornerRadius, FontFamily, FontId, Shadow, Stroke, Style, TextStyle, Visuals};
use std::path::{Path, PathBuf};

/// Shell background (light `#f5f5f5`, dark `#141414`).
pub fn shell_bg(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x14, 0x14, 0x14)
    } else {
        Color32::from_rgb(0xf5, 0xf5, 0xf5)
    }
}

/// Panel / card surface (light white, dark charcoal).
pub fn panel_bg(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x14, 0x14, 0x14)
    } else {
        Color32::WHITE
    }
}

/// Slightly raised surface (menus, alt panels) — dark `#1f1f1f`, light `#ffffff`.
pub fn panel_alt(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x1f, 0x1f, 0x1f)
    } else {
        Color32::WHITE
    }
}

/// Primary accent — light `#1677ff`, dark `#5b9bd5`.
pub fn accent(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x5b, 0x9b, 0xd5)
    } else {
        Color32::from_rgb(0x16, 0x77, 0xff)
    }
}

pub fn accent_hover(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x7e, 0xb0, 0xdf)
    } else {
        Color32::from_rgb(0x40, 0x90, 0xff)
    }
}

pub fn text_strong(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 217) // ~0.85
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, 224) // ~0.88
    }
}

pub fn text_muted(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 166) // ~0.65
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, 115) // ~0.45
    }
}

pub fn border(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 46) // ~0.18
    } else {
        Color32::from_rgba_unmultiplied(33, 35, 48, 71) // ~0.28
    }
}

pub fn border_strong(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 64)
    } else {
        Color32::from_rgba_unmultiplied(33, 35, 48, 230) // ~0.90 node border
    }
}

pub fn hover_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 20) // ~0.08
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, 15)
    }
}

pub fn input_bg(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 20) // ~0.08
    } else {
        Color32::WHITE
    }
}

/// Classic X6 stage node fill `rgb(221, 235, 217)` — same in light and dark.
pub const SCENE_NODE_BG: Color32 = Color32::from_rgb(221, 235, 217);
pub const SCENE_NODE_TEXT: Color32 = Color32::from_rgb(0, 0, 0);
pub const SCENE_NODE_CONNECT: Color32 = Color32::from_rgb(240, 220, 160);

const RADIUS: u8 = 6;

fn widget(bg: Color32, weak_bg: Color32, stroke: Color32, fg: Color32) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: weak_bg,
        bg_stroke: Stroke::new(1.0, stroke),
        corner_radius: CornerRadius::same(RADIUS),
        fg_stroke: Stroke::new(1.0, fg),
        expansion: 0.0,
    }
}

pub fn visuals(dark: bool) -> Visuals {
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    let shell = shell_bg(dark);
    let panel = panel_bg(dark);
    let alt = panel_alt(dark);
    let acc = accent(dark);
    let acc_h = accent_hover(dark);
    let fg = text_strong(dark);
    let muted = text_muted(dark);
    let bd = border(dark);
    let hover = hover_fill(dark);
    let inp = input_bg(dark);

    v.dark_mode = dark;
    v.override_text_color = Some(fg);
    v.hyperlink_color = if dark {
        Color32::from_rgb(0x8b, 0xb8, 0xe8)
    } else {
        acc
    };
    v.warn_fg_color = Color32::from_rgb(0xfa, 0xad, 0x14);
    v.error_fg_color = Color32::from_rgb(0xff, 0x4d, 0x4f);
    v.window_fill = alt;
    v.panel_fill = panel;
    v.extreme_bg_color = shell;
    v.faint_bg_color = if dark {
        Color32::from_rgb(0x1a, 0x1a, 0x1a)
    } else {
        Color32::from_rgb(0xf0, 0xf0, 0xf0)
    };
    v.code_bg_color = if dark {
        Color32::from_rgb(0x1f, 0x1f, 0x1f)
    } else {
        Color32::from_rgb(0xfa, 0xfa, 0xfa)
    };
    v.window_stroke = Stroke::new(1.0, bd);
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.window_shadow = if dark {
        Shadow {
            offset: [0, 4],
            blur: 16,
            spread: 0,
            color: Color32::from_black_alpha(120),
        }
    } else {
        Shadow {
            offset: [0, 2],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(40),
        }
    };
    v.popup_shadow = v.window_shadow;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(acc.r(), acc.g(), acc.b(), 64);
    v.selection.stroke = Stroke::new(1.0, acc);

    v.widgets.noninteractive = widget(panel, shell, bd, muted);
    v.widgets.inactive = widget(inp, panel, bd, fg);
    v.widgets.hovered = widget(hover, hover, acc_h, fg);
    v.widgets.active = widget(
        Color32::from_rgba_unmultiplied(acc.r(), acc.g(), acc.b(), 48),
        hover,
        acc,
        fg,
    );
    v.widgets.open = widget(alt, alt, acc, fg);

    v.text_cursor.stroke = Stroke::new(2.0, acc);
    v
}

pub fn style(dark: bool) -> Style {
    let mut style = Style {
        visuals: visuals(dark),
        ..Default::default()
    };

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    style.spacing.indent = 18.0;
    style.spacing.window_margin = egui::Margin::same(10);
    style.spacing.menu_margin = egui::Margin::same(4);
    style.interaction.selectable_labels = false;

    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(10.5, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(12.5, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(12.5, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(15.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.5, FontFamily::Monospace),
    );

    style
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    ctx.set_style(style(dark));
}

/// Compact "Info" control that shows `tip` as soon as the pointer is over it.
///
/// Uses [`egui::Response::show_tooltip_text`] (always-on path) instead of
/// `on_hover_text`, which stays suppressed for `tooltip_delay` after any
/// `ScrollArea` scroll — common in the stage editor and side panels.
pub fn info_tip(ui: &mut egui::Ui, tip: &str) {
    let resp = ui.add(
        egui::Button::new(egui::RichText::new("Info").small())
            .frame(true)
            .small()
            .min_size(egui::vec2(36.0, 18.0)),
    );
    if resp.contains_pointer() {
        resp.show_tooltip_text(tip);
    }
}

/// Prefer Tahoma for UI text; fall back to Verdana / Liberation / Segoe.
pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let sans_candidates = [
        "C:\\Windows\\Fonts\\verdana.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/TTF/LiberationSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    let mono_candidates = [
        "/usr/share/fonts/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/TTF/LiberationMono-Regular.ttf",
        "C:\\Windows\\Fonts\\lucon.ttf",
        "C:\\Windows\\Fonts\\consola.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    ];

    let sans_data = load_tahoma().or_else(|| load_first_font(&sans_candidates));
    if let Some(data) = sans_data {
        fonts
            .font_data
            .insert("slsb_sans".into(), egui::FontData::from_owned(data).into());
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "slsb_sans".into());
    }
    if let Some(data) = load_first_font(&mono_candidates) {
        fonts
            .font_data
            .insert("slsb_mono".into(), egui::FontData::from_owned(data).into());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "slsb_mono".into());
    }

    // Tahoma / Liberation omit dingbats (✎ ✕ ⚠ ◆). egui's last fallback
    // (emoji-icon-font) maps those codepoints to empty boxes, so icons look
    // like tofu unless a real symbol font sits ahead of it.
    let symbol_candidates = [
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansSymbols-Regular.ttf",
        "C:\\Windows\\Fonts\\seguisym.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
    ];
    if let Some(data) = load_first_font(&symbol_candidates) {
        fonts.font_data.insert(
            "slsb_symbols".into(),
            egui::FontData::from_owned(data).into(),
        );
        prepend_after_primary(
            fonts.families.entry(FontFamily::Proportional).or_default(),
            "slsb_sans",
            "slsb_symbols",
        );
        prepend_after_primary(
            fonts.families.entry(FontFamily::Monospace).or_default(),
            "slsb_mono",
            "slsb_symbols",
        );
    }

    ctx.set_fonts(fonts);
}

/// Put `fallback` just after the custom primary face so Latin stays Tahoma
/// (or similar) while missing glyphs resolve before egui's box font.
fn prepend_after_primary(family: &mut Vec<String>, primary: &str, fallback: &str) {
    let idx = usize::from(family.first().map(String::as_str) == Some(primary));
    family.insert(idx, fallback.into());
}

fn load_tahoma() -> Option<Vec<u8>> {
    let mut paths: Vec<PathBuf> = vec![
        PathBuf::from(r"C:\Windows\Fonts\tahoma.ttf"),
        PathBuf::from("/usr/share/fonts/TTF/tahoma.ttf"),
        PathBuf::from("/usr/share/fonts/truetype/msttcorefonts/Tahoma.ttf"),
        PathBuf::from("/usr/share/fonts/truetype/msttcorefonts/tahoma.ttf"),
        PathBuf::from("/usr/share/wine/fonts/tahoma.ttf"),
        PathBuf::from("/usr/share/fonts/wine/tahoma.ttf"),
    ];
    if let Ok(windir) = std::env::var("WINDIR") {
        paths.push(PathBuf::from(windir).join("Fonts").join("tahoma.ttf"));
    }
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".fonts/tahoma.ttf"));
        paths.push(home.join(".local/share/fonts/tahoma.ttf"));
        paths.push(home.join(".wine/drive_c/windows/fonts/tahoma.ttf"));
        for compat in [
            home.join(".local/share/Steam/steamapps/compatdata"),
            home.join(".steam/steam/steamapps/compatdata"),
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/compatdata"),
        ] {
            if let Some(found) = tahoma_in_compatdata(&compat) {
                paths.push(found);
            }
        }
    }
    for path in &paths {
        if let Ok(bytes) = std::fs::read(path) {
            if !bytes.is_empty() {
                return Some(bytes);
            }
        }
    }
    fontconfig_family("Tahoma")
}

fn tahoma_in_compatdata(root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten().take(80) {
        let font = entry.path().join("pfx/drive_c/windows/fonts/tahoma.ttf");
        if font.is_file() {
            return Some(font);
        }
    }
    None
}

/// `fc-match` only if it actually resolved to `family` (not a substitute).
fn fontconfig_family(family: &str) -> Option<Vec<u8>> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{family}\n%{file}", family])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let resolved = lines.next()?.trim();
    if !resolved.eq_ignore_ascii_case(family) {
        return None;
    }
    let file = lines.next()?.trim();
    let bytes = std::fs::read(file).ok()?;
    (!bytes.is_empty()).then_some(bytes)
}

fn load_first_font(paths: &[&str]) -> Option<Vec<u8>> {
    for path in paths {
        if let Ok(bytes) = std::fs::read(path) {
            if !bytes.is_empty() {
                return Some(bytes);
            }
        }
    }
    None
}

/// Pin the current UI to the parent's allocated width (avoids Frame/ScrollArea shrink-wrap).
pub fn fill_width(ui: &mut egui::Ui) {
    let w = ui.available_width();
    if w.is_finite() && w > 0.0 {
        ui.set_min_width(w);
        ui.set_max_width(w);
    }
}

/// Label + expanding numeric field on one row (fills remaining horizontal space).
pub fn labeled_drag(ui: &mut egui::Ui, label: &str, drag: egui::DragValue<'_>) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(label);
        let h = ui.spacing().interact_size.y;
        let w = ui.available_width().max(40.0);
        ui.add_sized([w, h], drag)
    })
    .inner
}

/// High-contrast radio-style chip (clear on/off vs white-on-grey native radios).
pub fn choice_chip(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let acc = accent(dark);
    let (fill, text, ring, dot) = if !enabled {
        if dark {
            (
                Color32::from_rgb(0x22, 0x22, 0x22),
                Color32::from_gray(110),
                Color32::from_gray(70),
                Color32::from_gray(70),
            )
        } else {
            (
                Color32::from_gray(235),
                Color32::from_gray(140),
                Color32::from_gray(180),
                Color32::from_gray(180),
            )
        }
    } else if selected {
        (acc, Color32::WHITE, acc, Color32::WHITE)
    } else if dark {
        (
            Color32::from_rgb(0x32, 0x32, 0x32),
            Color32::from_gray(245),
            Color32::from_gray(175),
            Color32::TRANSPARENT,
        )
    } else {
        (
            Color32::WHITE,
            Color32::from_gray(25),
            Color32::from_gray(90),
            Color32::TRANSPARENT,
        )
    };

    let font = egui::FontId::new(13.0, egui::FontFamily::Proportional);
    let text_w = ui.fonts(|f| {
        f.layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
    });
    let h = 24.0;
    let pad_x = 8.0;
    let radio_r = 5.5;
    let gap = 6.0;
    let w = pad_x + radio_r * 2.0 + gap + text_w + pad_x;
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(w, h), sense);
    if enabled {
        resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let stroke_w = if selected || resp.hovered() { 1.5 } else { 1.0 };
        painter.rect(
            rect,
            4.0,
            fill,
            egui::Stroke::new(
                stroke_w,
                if resp.hovered() && enabled {
                    accent_hover(dark)
                } else {
                    ring
                },
            ),
            egui::StrokeKind::Inside,
        );
        let c = egui::pos2(rect.left() + pad_x + radio_r, rect.center().y);
        painter.circle_stroke(c, radio_r, egui::Stroke::new(1.5, ring));
        if selected {
            painter.circle_filled(c, radio_r * 0.55, dot);
        }
        painter.text(
            egui::pos2(c.x + radio_r + gap, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            font,
            text,
        );
    }
    resp
}

/// Exclusive Male / Female / Futa choice chips.
/// When `futa_enabled` is false, Futa is disabled and remapped away if selected.
pub fn sex_radios(
    ui: &mut egui::Ui,
    sex: &mut scene_builder_core::project::define::Sex,
    futa_enabled: bool,
) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Choice {
        Male,
        Female,
        Futa,
    }
    let mut choice = if sex.futa && futa_enabled {
        Choice::Futa
    } else if sex.female {
        Choice::Female
    } else if sex.male {
        Choice::Male
    } else if sex.futa {
        Choice::Female
    } else {
        Choice::Male
    };

    if choice_chip(ui, choice == Choice::Male, "Male", true).clicked() {
        choice = Choice::Male;
    }
    if choice_chip(ui, choice == Choice::Female, "Female", true).clicked() {
        choice = Choice::Female;
    }
    if choice_chip(ui, choice == Choice::Futa, "Futa", futa_enabled).clicked() && futa_enabled {
        choice = Choice::Futa;
    }
    if !futa_enabled && choice == Choice::Futa {
        choice = Choice::Female;
    }

    let male = choice == Choice::Male;
    let female = choice == Choice::Female;
    let futa = choice == Choice::Futa;
    if sex.male != male || sex.female != female || sex.futa != futa {
        sex.male = male;
        sex.female = female;
        sex.futa = futa;
        true
    } else {
        false
    }
}

/// Multi-select actor state chips (same radio look as sex; flags combine freely).
pub fn state_flags(
    ui: &mut egui::Ui,
    submissive: &mut bool,
    vampire: &mut bool,
    dead: &mut bool,
    vampire_enabled: bool,
    roomy_labels: bool,
) -> bool {
    let mut changed = false;
    let sub_l = if roomy_labels { "Submissive" } else { "Sub" };
    let dead_l = if roomy_labels { "Unconscious" } else { "Uncon" };

    if choice_chip(ui, *submissive, sub_l, true)
        .on_hover_text("Passive / Taker / Bottom position.")
        .clicked()
    {
        *submissive = !*submissive;
        changed = true;
    }
    if choice_chip(ui, *vampire, "Vampire", vampire_enabled)
        .on_hover_text("Actor is a vampire.")
        .clicked()
        && vampire_enabled
    {
        *vampire = !*vampire;
        changed = true;
    }
    if !vampire_enabled && *vampire {
        *vampire = false;
        changed = true;
    }
    if choice_chip(ui, *dead, dead_l, true)
        .on_hover_text("Unconscious / dead.")
        .clicked()
    {
        *dead = !*dead;
        changed = true;
    }
    changed
}
