use eframe::egui;
use egui::{Align2, Color32, FontId, Stroke, StrokeKind, Vec2};

pub struct OverlayPayload {
    pub connector: String,
    pub display_name: String,
    pub scale: String,
    pub mode: String,
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
                ui.painter().rect_filled(
                    rect,
                    16.0,
                    Color32::from_rgba_unmultiplied(8, 14, 25, 220),
                );
                ui.painter().rect_stroke(
                    rect,
                    16.0,
                    Stroke::new(2.0, Color32::from_rgb(113, 181, 255)),
                    StrokeKind::Outside,
                );

                ui.painter().text(
                    rect.left_top() + Vec2::new(18.0, 18.0),
                    Align2::LEFT_TOP,
                    "Screen Identifier",
                    FontId::proportional(20.0),
                    Color32::from_rgb(210, 233, 255),
                );
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
                    format!("Connection: {}", self.payload.connector),
                    FontId::proportional(18.0),
                    Color32::from_rgb(199, 219, 241),
                );
                ui.painter().text(
                    rect.left_top() + Vec2::new(18.0, 122.0),
                    Align2::LEFT_TOP,
                    format!(
                        "Current scale: {}  |  Best mode: {}",
                        self.payload.scale, self.payload.mode
                    ),
                    FontId::proportional(16.0),
                    Color32::from_rgb(176, 198, 223),
                );
            });
    }
}
