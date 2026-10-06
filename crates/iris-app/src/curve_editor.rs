//! Interactive tone curve: input on the x axis, output on the y axis, with the photo's
//! luminance histogram behind it.
//!   click        add a point (and drag it)
//!   drag         move a point
//!   double-click / right-click / Delete   remove a point (not the end points)

use egui::{Align2, Color32, CornerRadius, FontId, Mesh, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use iris_core::curve::{MAX_CURVE_POINTS, MIN_CURVE_GAP, normalized_curve};
use iris_core::{CurvePoint, CurvePoints, CurveSpline};

use crate::theme;
use crate::widgets::{MARGIN, add_bins};

const HIT_RADIUS: f32 = 9.0;

#[derive(Default)]
pub struct CurveEditor {
    drag_index: Option<usize>,
    selected: Option<usize>,
}

fn to_screen(r: Rect, p: CurvePoint) -> Pos2 {
    pos2(r.left() + p.x * r.width(), r.bottom() - p.y * r.height())
}

fn from_screen(r: Rect, pos: Pos2) -> CurvePoint {
    CurvePoint {
        x: ((pos.x - r.left()) / r.width()).clamp(0.0, 1.0),
        y: ((r.bottom() - pos.y) / r.height()).clamp(0.0, 1.0),
    }
}

fn hit_test(r: Rect, points: &[CurvePoint], pos: Pos2) -> Option<usize> {
    let mut best = None;
    let mut best_distance = HIT_RADIUS;
    for (i, p) in points.iter().enumerate() {
        let d = to_screen(r, *p).distance(pos);
        if d <= best_distance {
            best_distance = d;
            best = Some(i);
        }
    }
    best
}

/// Moves a point: end points stay at the edges; inner points stay between their neighbours.
fn move_point(points: &mut CurvePoints, index: usize, mut target: CurvePoint) -> bool {
    let last = points.len() - 1;
    if index == 0 {
        target.x = 0.0;
    } else if index == last {
        target.x = 1.0;
    } else {
        target.x = target.x.clamp(points[index - 1].x + MIN_CURVE_GAP, points[index + 1].x - MIN_CURVE_GAP);
    }
    target.y = target.y.clamp(0.0, 1.0);
    if points[index] == target {
        return false;
    }
    points[index] = target;
    true
}

/// Inserts a point if there is room between its neighbours; returns its index.
fn insert_point(points: &mut CurvePoints, p: CurvePoint) -> Option<usize> {
    if points.len() >= MAX_CURVE_POINTS {
        return None;
    }
    let index = points.partition_point(|q| q.x <= p.x);
    if index == 0
        || index == points.len()
        || p.x - points[index - 1].x < MIN_CURVE_GAP
        || points[index].x - p.x < MIN_CURVE_GAP
    {
        return None;
    }
    points.insert(index, p);
    Some(index)
}

impl CurveEditor {
    /// Draws the editor; returns the edited points if the user changed them.
    /// `histogram` is the luminance histogram, normalised to 0..1.
    pub fn ui(&mut self, ui: &mut Ui, curve: &[CurvePoint], histogram: Option<&[f32; 256]>) -> Option<CurvePoints> {
        let mut points = normalized_curve(curve);
        if self.drag_index.is_some_and(|i| i >= points.len()) {
            self.drag_index = None;
        }
        if self.selected.is_some_and(|i| i >= points.len()) {
            self.selected = None;
        }

        let side = (ui.available_width() - 2.0 * MARGIN).max(120.0);
        let (outer, response) = ui.allocate_exact_size(vec2(ui.available_width(), side + 8.0), Sense::click_and_drag());
        let r = Rect::from_center_size(outer.center(), vec2(side, side));
        let enabled = ui.is_enabled();
        let mut edited = false;

        if enabled {
            let pointer = ui.input(|i| i.pointer.clone());
            let pos = pointer.interact_pos();
            if response.hovered()
                && let Some(pos) = pointer.hover_pos()
            {
                let over_point = hit_test(r, &points, pos).is_some();
                ui.ctx().set_cursor_icon(if over_point {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::Crosshair
                });
            }
            if let Some(pos) = pos.filter(|_| response.hovered()) {
                let hit = hit_test(r, &points, pos);
                if pointer.button_double_clicked(egui::PointerButton::Primary) || response.secondary_clicked() {
                    edited |= self.remove_point(&mut points, hit);
                } else if pointer.button_pressed(egui::PointerButton::Primary) {
                    response.request_focus();
                    if let Some(hit) = hit {
                        self.drag_index = Some(hit);
                        self.selected = Some(hit);
                    } else if let Some(index) = insert_point(&mut points, from_screen(r, pos)) {
                        // Add a point where the user clicked, and drag it.
                        self.drag_index = Some(index);
                        self.selected = Some(index);
                        edited = true;
                    }
                }
            }
            if let (Some(index), Some(pos)) = (self.drag_index, pos)
                && pointer.primary_down()
            {
                edited |= move_point(&mut points, index, from_screen(r, pos));
            }
            if !pointer.primary_down() {
                self.drag_index = None;
            }
            if response.has_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
            {
                edited |= self.remove_point(&mut points, self.selected);
            }
        }

        self.paint(ui, r, &points, histogram, enabled);
        edited.then_some(points)
    }

    fn remove_point(&mut self, points: &mut CurvePoints, index: Option<usize>) -> bool {
        match index {
            Some(i) if i > 0 && i + 1 < points.len() => {
                points.remove(i);
                self.selected = None;
                self.drag_index = None;
                true
            }
            _ => false,
        }
    }

    fn paint(&self, ui: &Ui, r: Rect, points: &[CurvePoint], histogram: Option<&[f32; 256]>, enabled: bool) {
        let painter = ui.painter_at(r.expand(8.0));
        painter.rect_filled(r, CornerRadius::ZERO, theme::BASE);

        // Histogram of the photo behind the curve.
        if let Some(h) = histogram {
            let mut mesh = Mesh::default();
            add_bins(&mut mesh, r, |i| h[i] * 0.9, Color32::from_rgb(0x3a, 0x3c, 0x42));
            painter.add(mesh);
        }

        // Grid in quarters and the identity diagonal.
        let grid = Stroke::new(1.0, Color32::from_rgb(0x2e, 0x30, 0x35));
        for i in 1..4 {
            let x = r.left() + r.width() * i as f32 / 4.0;
            let y = r.top() + r.height() * i as f32 / 4.0;
            painter.vline(x, r.y_range(), grid);
            painter.hline(r.x_range(), y, grid);
        }
        painter.extend(Shape::dashed_line(
            &[r.left_bottom(), r.right_top()],
            Stroke::new(1.0, Color32::from_rgb(0x4a, 0x4c, 0x52)),
            4.0,
            4.0,
        ));
        painter.rect_stroke(
            r,
            CornerRadius::ZERO,
            Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3c, 0x42)),
            egui::StrokeKind::Inside,
        );

        // The curve.
        let spline = CurveSpline::new(points);
        let curve: Vec<Pos2> = (0..=160)
            .map(|i| {
                let x = i as f32 / 160.0;
                to_screen(r, CurvePoint { x, y: spline.eval(x) })
            })
            .collect();
        let color = if enabled { theme::TEXT } else { Color32::from_rgb(0x60, 0x62, 0x68) };
        painter.add(Shape::line(curve, Stroke::new(1.6, color)));

        // Control points.
        for (i, p) in points.iter().enumerate() {
            let selected = Some(i) == self.selected;
            let fill = if selected { theme::ACCENT } else { theme::BASE };
            painter.circle(to_screen(r, *p), if selected { 5.0 } else { 4.0 }, fill, Stroke::new(1.4, theme::TEXT));
        }

        // Read-out of the selected point.
        if let Some(p) = self.selected.and_then(|i| points.get(i)) {
            painter.text(
                r.left_top() + vec2(6.0, 4.0),
                Align2::LEFT_TOP,
                format!("In {}  ·  Out {}", (p.x * 100.0).round(), (p.y * 100.0).round()),
                FontId::proportional(12.0),
                theme::TITLE_TEXT,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_core::curve::{linear_curve, s_curve};

    #[test]
    fn points_stay_ordered() {
        let mut points = s_curve();
        assert!(move_point(&mut points, 1, CurvePoint::new(0.9, 0.5)));
        assert!((points[1].x - (0.5 - MIN_CURVE_GAP)).abs() < 1e-6); // stops before its neighbour
        assert!(move_point(&mut points, 0, CurvePoint::new(0.3, 0.1)));
        assert_eq!(points[0], CurvePoint::new(0.0, 0.1)); // end points stay at the edge
    }

    #[test]
    fn points_are_inserted_where_there_is_room() {
        let mut points = linear_curve();
        assert_eq!(insert_point(&mut points, CurvePoint::new(0.5, 0.6)), Some(1));
        assert_eq!(insert_point(&mut points, CurvePoint::new(0.505, 0.6)), None); // too close
        assert_eq!(insert_point(&mut points, CurvePoint::new(0.0, 0.6)), None); // on an end point
        assert_eq!(points.len(), 3);
    }
}
