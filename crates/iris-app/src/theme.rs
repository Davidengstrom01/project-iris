//! The dark theme: one place for the colours the panels and the view share.

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

pub const WINDOW: Color32 = Color32::from_rgb(0x1e, 0x1f, 0x22);
pub const PANEL: Color32 = Color32::from_rgb(0x23, 0x24, 0x28);
pub const BASE: Color32 = Color32::from_rgb(0x17, 0x18, 0x1a);
pub const VIEW_BACKGROUND: Color32 = Color32::from_rgb(0x14, 0x15, 0x17);
pub const SEPARATOR: Color32 = Color32::from_rgb(0x11, 0x12, 0x14);
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe7, 0xea);
pub const BODY_TEXT: Color32 = Color32::from_rgb(0xd8, 0xd9, 0xdc);
pub const LABEL_TEXT: Color32 = Color32::from_rgb(0xb4, 0xb6, 0xbb);
pub const SECTION_TEXT: Color32 = Color32::from_rgb(0xc4, 0xc6, 0xcb);
pub const TITLE_TEXT: Color32 = Color32::from_rgb(0x9e, 0xa1, 0xa7);
pub const DIM_TEXT: Color32 = Color32::from_rgb(0x7d, 0x80, 0x86);
pub const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x8d, 0xf6);
pub const SELECTION: Color32 = Color32::from_rgb(0x34, 0x52, 0x8a);
pub const GROOVE: Color32 = Color32::from_rgb(0x3a, 0x3c, 0x41);
pub const HANDLE: Color32 = Color32::from_rgb(0xc9, 0xcb, 0xd0);
pub const BUTTON: Color32 = Color32::from_rgb(0x2c, 0x2e, 0x32);
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(0x35, 0x37, 0x3c);
pub const BUTTON_BORDER: Color32 = Color32::from_rgb(0x38, 0x3a, 0x3f);

/// Blue -> yellow, the temperature slider groove.
pub const TEMPERATURE_GRADIENT: (Color32, Color32) =
    (Color32::from_rgb(0x4a, 0x7c, 0xd6), Color32::from_rgb(0xe0, 0xbe, 0x48));
/// Green -> magenta, the tint slider groove.
pub const TINT_GRADIENT: (Color32, Color32) =
    (Color32::from_rgb(0x4c, 0xb0, 0x52), Color32::from_rgb(0xc8, 0x4c, 0xc0));

pub fn apply(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = WINDOW;
    visuals.extreme_bg_color = BASE;
    visuals.faint_bg_color = WINDOW;
    visuals.override_text_color = Some(BODY_TEXT);
    visuals.selection.bg_fill = SELECTION;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.window_stroke = Stroke::new(1.0, SEPARATOR);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR);
    visuals.widgets.inactive.weak_bg_fill = BUTTON;
    visuals.widgets.inactive.bg_fill = BUTTON;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BUTTON_BORDER);
    visuals.widgets.hovered.weak_bg_fill = BUTTON_HOVER;
    visuals.widgets.hovered.bg_fill = BUTTON_HOVER;
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(0x3d, 0x40, 0x46);
    for w in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active] {
        w.corner_radius = CornerRadius::same(3);
    }
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 3.0);
        style.spacing.button_padding = egui::vec2(8.0, 2.0);
        style.spacing.interact_size.y = 20.0;
        style.text_styles.insert(TextStyle::Body, FontId::new(13.0, FontFamily::Proportional));
        style.text_styles.insert(TextStyle::Button, FontId::new(12.5, FontFamily::Proportional));
        style.text_styles.insert(TextStyle::Small, FontId::new(11.0, FontFamily::Proportional));
    });
}
