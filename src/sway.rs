use anyhow::{Context, Result};
use swayipc::{Connection, Mode as SwayMode, Rect};

use crate::model::{best_mode, OutputMode, RuntimeOutput, ScreenConfig};

pub fn rescan_outputs() -> Result<Vec<RuntimeOutput>> {
    let mut conn = Connection::new().context("failed to connect to sway IPC")?;
    let outputs = conn
        .get_outputs()
        .context("failed to query outputs from sway")?;

    let mut result = Vec::new();
    for output in outputs {
        if !output.active {
            continue;
        }

        let modes = output
            .modes
            .iter()
            .map(sway_mode_to_mode)
            .collect::<Vec<_>>();

        result.push(RuntimeOutput {
            connector_name: output.name,
            make: non_empty(output.make),
            model: non_empty(output.model),
            serial: non_empty(output.serial),
            active: output.active,
            current_scale: output.scale.unwrap_or(1.0),
            available_modes: modes,
            layout_x: output.rect.x,
            layout_y: output.rect.y,
            layout_width: output.rect.width,
            layout_height: output.rect.height,
        });
    }
    Ok(result)
}

pub fn default_screen_from_runtime(output: &RuntimeOutput, index: usize) -> Option<ScreenConfig> {
    let mode = best_mode(&output.available_modes)?;
    Some(ScreenConfig {
        id: output.display_id(),
        connector_name: output.connector_name.clone(),
        enabled: true,
        selected_mode: mode.clone(),
        available_modes: output.available_modes.clone(),
        scale: 1.0,
        pos_x: (index as i32) * mode.width as i32,
        pos_y: 0,
        mirror: false,
        mirror_target: None,
    })
}

fn sway_mode_to_mode(mode: &SwayMode) -> OutputMode {
    let hz = (mode.refresh as f64) / 1000.0;
    OutputMode {
        width: mode.width as u32,
        height: mode.height as u32,
        refresh_hz: hz,
    }
}

#[allow(dead_code)]
fn _rect_to_tuple(rect: Rect) -> (i32, i32, i32, i32) {
    (rect.x, rect.y, rect.width, rect.height)
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
