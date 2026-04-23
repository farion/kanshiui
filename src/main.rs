mod app;
mod kanshi_config;
mod kanshi_restart;
mod model;
mod notify;
mod overlay;
mod overlay_app;
mod sway;
mod ui;

use app::KanshiApp;
use eframe::egui;
use overlay_app::{IdentifyOverlayApp, OverlayPayload};

fn arg_value(args: &[String], key: &str) -> Option<String> {
    let mut i = 0usize;
    while i + 1 < args.len() {
        if args[i] == key {
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--identify-overlay") {
        let x = arg_value(&args, "--x")
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0);
        let y = arg_value(&args, "--y")
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0);
        let payload = OverlayPayload {
            connector: arg_value(&args, "--connector").unwrap_or_else(|| "unknown".to_string()),
            display_name: arg_value(&args, "--display-name")
                .unwrap_or_else(|| "Unknown Display".to_string()),
            scale: arg_value(&args, "--scale").unwrap_or_else(|| "1".to_string()),
            mode: arg_value(&args, "--mode").unwrap_or_else(|| "unknown mode".to_string()),
        };

        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title(format!("Identify {}", payload.connector))
                .with_inner_size([500.0, 180.0])
                .with_transparent(true)
                .with_decorations(false)
                .with_resizable(false)
                .with_always_on_top()
                .with_position(egui::pos2(x + 24.0, y + 24.0))
                .with_mouse_passthrough(true),
            ..Default::default()
        };

        return eframe::run_native(
            &format!("Identify {}", payload.connector),
            options,
            Box::new(|_cc| Ok(Box::new(IdentifyOverlayApp::new(payload)))),
        );
    }

    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "KanshiUI",
        options,
        Box::new(|_cc| Ok(Box::new(KanshiApp::new()))),
    )
}
