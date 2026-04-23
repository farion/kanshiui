use std::process::{Command, Stdio};

use crate::model::{trim_float, RuntimeOutput};

fn launch_overlay_process(output: &RuntimeOutput) -> Option<u32> {
    let exe = std::env::current_exe().ok()?;

    let display_name = output.display_id();
    let connector = output.connector_name.clone();
    let best_mode = output
        .available_modes
        .iter()
        .max_by(|a, b| {
            let area_a = a.width as u64 * a.height as u64;
            let area_b = b.width as u64 * b.height as u64;
            area_a.cmp(&area_b).then_with(|| {
                a.refresh_hz
                    .partial_cmp(&b.refresh_hz)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        })
        .map(|m| m.as_kanshi_mode())
        .unwrap_or_else(|| "unknown mode".to_string());

    eprintln!(
        "launching overlay exe={:?} connector={} x={} y={}",
        exe, connector, output.layout_x, output.layout_y
    );
    let child = match Command::new(exe)
        .arg("--identify-overlay")
        .arg("--connector")
        .arg(&connector)
        .arg("--display-name")
        .arg(&display_name)
        .arg("--x")
        .arg(output.layout_x.to_string())
        .arg("--y")
        .arg(output.layout_y.to_string())
        .arg("--scale")
        .arg(trim_float(output.current_scale))
        .arg("--mode")
        .arg(best_mode)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "failed to spawn overlay process for connector {}: {e}",
                connector
            );
            return None;
        }
    };

    let pid = child.id();
    eprintln!("spawned overlay pid={} for connector={}", pid, connector);

    // Move overlay to target output asynchronously with a few retries so it
    // lands correctly once the compositor maps the window.
    let connector_for_move = connector.clone();
    std::thread::spawn(move || {
        for _ in 0..10 {
            let criteria = format!("[pid=\"{pid}\"]");
            let command = format!(
                "floating enable, sticky enable, border pixel 0, move window to output {connector_for_move}, move position 24 24"
            );
            let _ = Command::new("swaymsg")
                .arg(criteria)
                .arg(command)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    });

    Some(pid)
}

pub fn spawn_identify_overlays(outputs: &[RuntimeOutput]) -> Vec<u32> {
    let mut pids = Vec::new();
    for output in outputs {
        if output.layout_width <= 0 || output.layout_height <= 0 {
            continue;
        }
        if let Some(pid) = launch_overlay_process(output) {
            pids.push(pid);
        }
    }
    pids
}

pub fn kill_identify_overlays(pids: &mut Vec<u32>) {
    for pid in pids.iter().copied() {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    pids.clear();
}
