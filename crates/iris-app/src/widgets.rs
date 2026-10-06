//! Small building blocks shared by the panels.

use egui::{Align, Color32, CornerRadius, FontId, Layout, Mesh, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use iris_render::Histogram;

use crate::theme;

/// Horizontal padding of panel content.
pub const MARGIN: f32 = 12.0;

/// A panel title ("BASIC") with optional buttons on the right.
pub fn panel_title(ui: &mut Ui, title: &str, right: impl FnOnce(&mut Ui)) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(MARGIN);
        ui.label(RichText::new(title).size(11.0).strong().color(theme::TITLE_TEXT).extra_letter_spacing(1.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(MARGIN);
            right(ui);
        });
    });
    ui.add_space(4.0);
}

/// A sub-heading inside a panel ("White Balance").
pub fn section_label(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(MARGIN);
        ui.label(RichText::new(text).strong().color(theme::SECTION_TEXT));
    });
}

pub fn small_button(ui: &mut Ui, text: &str, tooltip: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).size(11.0))).on_hover_text(tooltip)
}

pub fn small_toggle(ui: &mut Ui, selected: bool, text: &str, tooltip: &str) -> egui::Response {
    ui.add(egui::Button::selectable(selected, RichText::new(text).size(11.0))).on_hover_text(tooltip)
}

/// Lays out `add` with the panel's horizontal padding.
pub fn padded<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(MARGIN as i8, 0)).show(ui, add).inner
}

/// Maps a slider value to a position in [0, 1] and back.
#[derive(Clone, Copy)]
pub struct Response {
    pub to_position: fn(f64) -> f64,
    pub to_value: fn(f64) -> f64,
}

/// One develop control: a name, an editable value and a slider underneath.
/// Double-clicking the name resets the value to its default.
#[derive(Clone, Copy)]
pub struct Slider<'a> {
    pub name: &'a str,
    pub min: f64,
    pub max: f64,
    pub decimals: usize,
    pub default: f64,
    pub gradient: Option<(Color32, Color32)>,
    pub response: Option<Response>,
    pub tooltip: Option<&'a str>,
}

impl<'a> Slider<'a> {
    pub fn new(name: &'a str, min: f64, max: f64, decimals: usize) -> Self {
        Self { name, min, max, decimals, default: 0.0, gradient: None, response: None, tooltip: None }
    }

    pub fn default_value(mut self, value: f64) -> Self {
        self.default = value;
        self
    }

    pub fn gradient(mut self, gradient: (Color32, Color32)) -> Self {
        self.gradient = Some(gradient);
        self
    }

    pub fn response(mut self, response: Response) -> Self {
        self.response = Some(response);
        self
    }

    pub fn tooltip(mut self, tooltip: &'a str) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    fn position(&self, value: f64) -> f64 {
        let p = match self.response {
            Some(r) => (r.to_position)(value),
            None => (value - self.min) / (self.max - self.min),
        };
        p.clamp(0.0, 1.0)
    }

    fn value_at(&self, position: f64) -> f64 {
        let value = match self.response {
            Some(r) => (r.to_value)(position),
            None => self.min + position * (self.max - self.min),
        };
        self.rounded(value)
    }

    fn rounded(&self, value: f64) -> f64 {
        let scale = 10f64.powi(self.decimals as i32);
        ((value * scale).round() / scale).clamp(self.min, self.max)
    }

    /// Draws the control; returns the new value if the user changed it.
    pub fn show(self, ui: &mut Ui, value: f64) -> Option<f64> {
        let mut changed = None;
        ui.push_id(self.name, |ui| {
            padded(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal(|ui| {
                    let label = ui
                        .add(egui::Label::new(RichText::new(self.name).color(theme::LABEL_TEXT)).sense(Sense::click()))
                        .on_hover_text(self.tooltip.unwrap_or("Double-click to reset"));
                    if label.double_clicked() {
                        changed = Some(self.default);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let mut edited = value;
                        let speed = (self.max - self.min) / 400.0;
                        let response = ui.add_sized(
                            [58.0, 18.0],
                            egui::DragValue::new(&mut edited)
                                .range(self.min..=self.max)
                                .fixed_decimals(self.decimals)
                                .speed(speed),
                        );
                        if response.changed() {
                            changed = Some(self.rounded(edited));
                        }
                    });
                });
                if let Some(v) = self.track(ui, value) {
                    changed = Some(v);
                }
            });
        });
        changed.filter(|&v| v != value)
    }

    fn track(&self, ui: &mut Ui, value: f64) -> Option<f64> {
        const HANDLE_RADIUS: f32 = 5.5;
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::click_and_drag());
        let x0 = rect.left() + HANDLE_RADIUS;
        let x1 = rect.right() - HANDLE_RADIUS;
        let mut result = None;
        if (response.dragged() || response.clicked() || response.drag_started())
            && let Some(p) = response.interact_pointer_pos()
        {
            let t = f64::from(((p.x - x0) / (x1 - x0).max(1.0)).clamp(0.0, 1.0));
            result = Some(self.value_at(t));
        }

        let painter = ui.painter_at(rect.expand(1.0));
        let groove = Rect::from_min_max(pos2(x0, rect.center().y - 1.5), pos2(x1, rect.center().y + 1.5));
        match self.gradient {
            Some((left, right)) if ui.is_enabled() => {
                let mut mesh = Mesh::default();
                let i = mesh.vertices.len() as u32;
                mesh.colored_vertex(groove.left_top(), left);
                mesh.colored_vertex(groove.right_top(), right);
                mesh.colored_vertex(groove.right_bottom(), right);
                mesh.colored_vertex(groove.left_bottom(), left);
                mesh.add_triangle(i, i + 1, i + 2);
                mesh.add_triangle(i, i + 2, i + 3);
                painter.add(mesh);
            }
            _ => {
                painter.rect_filled(groove, CornerRadius::same(1), theme::GROOVE);
            }
        }
        let shown = result.unwrap_or(value);
        let x = x0 + (x1 - x0) * self.position(shown) as f32;
        let color = if !ui.is_enabled() {
            Color32::from_rgb(0x55, 0x58, 0x5e)
        } else if response.hovered() || response.dragged() {
            Color32::WHITE
        } else {
            theme::HANDLE
        };
        painter.circle_filled(pos2(x, rect.center().y), HANDLE_RADIUS, color);
        result
    }
}

/// A filled area under 256 bins (0..1), as one mesh of thin quads.
pub fn add_bins(mesh: &mut Mesh, area: Rect, heights: impl Fn(usize) -> f32, color: Color32) {
    let dx = area.width() / 255.0;
    for i in 0..256 {
        let h = heights(i).clamp(0.0, 1.0);
        if h <= 0.0 {
            continue;
        }
        let x = area.left() + dx * i as f32;
        let left = (x - dx / 2.0).max(area.left());
        let right = (x + dx / 2.0).min(area.right());
        mesh.add_colored_rect(
            Rect::from_min_max(pos2(left, area.bottom() - h * area.height()), pos2(right, area.bottom())),
            color,
        );
    }
}

/// Square-root scaling keeps small populations visible next to large peaks; the scale
/// ignores the two end bins so clipped pixels do not flatten everything else.
fn scaled(bins: &[u32; 256], peak: f64) -> [f32; 256] {
    std::array::from_fn(|i| (f64::from(bins[i]) / peak).sqrt().min(1.0) as f32)
}

/// RGB histogram with additive colours: overlapping channels mix towards white, like the
/// photo itself.
pub fn histogram(ui: &mut Ui, histogram: Option<&Histogram>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 84.0), Sense::hover());
    let area = rect.shrink2(vec2(MARGIN, 6.0));
    let painter = ui.painter_at(rect);
    painter.rect_filled(area, CornerRadius::ZERO, theme::BASE);
    let Some(h) = histogram.filter(|h| h.pixels > 0) else { return };

    let mut peak = 1.0f64;
    for i in 1..255 {
        peak = peak.max(f64::from(h.red[i].max(h.green[i]).max(h.blue[i])));
    }
    let (r, g, b) = (scaled(&h.red, peak), scaled(&h.green, peak), scaled(&h.blue, peak));

    // Each column is split by the sorted channel heights; the part covered by k channels
    // gets the sum of their colours.
    const RED: [u16; 3] = [170, 40, 40];
    const GREEN: [u16; 3] = [40, 150, 50];
    const BLUE: [u16; 3] = [40, 70, 190];
    let add = |colors: &[[u16; 3]]| {
        let base = [0x17u16, 0x18, 0x1a];
        let c: [u8; 3] = std::array::from_fn(|k| (base[k] + colors.iter().map(|c| c[k]).sum::<u16>()).min(255) as u8);
        Color32::from_rgb(c[0], c[1], c[2])
    };
    let mut mesh = Mesh::default();
    let dx = area.width() / 255.0;
    for i in 0..256 {
        let mut channels = [(r[i], RED), (g[i], GREEN), (b[i], BLUE)];
        channels.sort_by(|a, b| a.0.total_cmp(&b.0));
        let x = area.left() + dx * i as f32;
        let (left, right) = ((x - dx / 2.0).max(area.left()), (x + dx / 2.0).min(area.right()));
        let mut below = 0.0;
        for k in 0..3 {
            let top = channels[k].0;
            if top > below {
                let covering: Vec<[u16; 3]> = channels[k..].iter().map(|c| c.1).collect();
                let y0 = area.bottom() - below * area.height();
                let y1 = area.bottom() - top * area.height();
                mesh.add_colored_rect(Rect::from_min_max(pos2(left, y1), pos2(right, y0)), add(&covering));
                below = top;
            }
        }
    }
    painter.add(mesh);
}

/// Thin separator line between panels.
pub fn separator(ui: &mut Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, theme::SEPARATOR));
}

/// A key / value row of the info panel.
pub fn info_row(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal_top(|ui| {
        ui.add_space(MARGIN);
        let key_width = 84.0;
        let (rect, _) = ui.allocate_exact_size(vec2(key_width, 16.0), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            key,
            FontId::proportional(12.5),
            theme::DIM_TEXT,
        );
        ui.add(egui::Label::new(RichText::new(value).color(theme::BODY_TEXT)).wrap());
    });
}
