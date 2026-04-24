use crate::ui::color_for_id;
use eframe::egui;
use egui::{Align2, Color32, FontId, Stroke, StrokeKind, Vec2};

pub struct OverlayPayload {
    pub connector: String,
    pub display_name: String,
    pub _scale: String,
    pub _mode: String,
}

pub struct IdentifyOverlayApp {
    payload: OverlayPayload,
}

impl IdentifyOverlayApp {
    pub fn new(payload: OverlayPayload) -> Self {
        Self { payload }
    }
}

impl eframe::App for IdentifyOverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                // Color should be stable per display identifier + connector so
                // identical models on different connectors get distinct colors.
                let bg = color_for_id(&format!("{}||{}", self.payload.display_name, self.payload.connector));
                let bg = Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), 220);
                // Use square corners for the overlay window background.
                ui.painter().rect_filled(rect, 0.0, bg);
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(2.0, Color32::from_rgb(113, 181, 255)),
                    StrokeKind::Outside,
                );

                // Overlay must show exactly two white lines: display name
                // and connector. Keep other visual styling unchanged.
                ui.painter().text(
                    rect.left_top() + Vec2::new(18.0, 52.0),
                    Align2::LEFT_TOP,
                    self.payload.display_name.clone(),
                    FontId::proportional(24.0),
                    Color32::WHITE,
                );
                ui.painter().text(
                    rect.left_top() + Vec2::new(18.0, 94.0),
                    Align2::LEFT_TOP,
                    self.payload.connector.clone(),
                    FontId::proportional(18.0),
                    Color32::WHITE,
                );
            });
    }
}
