use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub struct OutputMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f64,
}

impl OutputMode {
    pub fn as_kanshi_mode(&self) -> String {
        format!(
            "{}x{}@{}Hz",
            self.width,
            self.height,
            trim_float(self.refresh_hz)
        )
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeOutput {
    pub connector_name: String,
    pub make: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub active: bool,
    pub current_scale: f64,
    pub available_modes: Vec<OutputMode>,
    pub layout_x: i32,
    pub layout_y: i32,
    pub layout_width: i32,
    pub layout_height: i32,
}

impl RuntimeOutput {
    pub fn display_id(&self) -> String {
        canonical_display_id(
            self.make.as_deref(),
            self.model.as_deref(),
            self.serial.as_deref(),
            &self.connector_name,
        )
    }
}

#[derive(Clone, Debug)]
pub struct ScreenConfig {
    pub id: String,
    pub connector_name: String,
    pub enabled: bool,
    pub selected_mode: OutputMode,
    pub available_modes: Vec<OutputMode>,
    pub scale: f64,
    pub pos_x: i32,
    pub pos_y: i32,
    // Mirroring: when true this screen mirrors another target specified by
    // `mirror_target` (the target's screen id). When mirroring, the screen
    // should not be shown on the canvas and is positioned/scaled to fit the
    // target on save.
    pub mirror: bool,
    pub mirror_target: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub name: String,
    pub screens: Vec<ScreenConfig>,
    pub raw_range: Option<(usize, usize)>,
}

#[derive(Clone, Debug, Default)]
pub struct AppSettings {
    pub snap_threshold_px: i32,
}

#[derive(Clone, Debug, Default)]
pub struct AppState {
    pub connected_outputs: Vec<RuntimeOutput>,
    pub profiles: Vec<Profile>,
    pub current_profile: Option<Profile>,
    pub current_profile_name: String,
    pub current_profile_read_only: bool,
    pub status: String,
    pub settings: AppSettings,
    pub identify_enabled: bool,
    pub identify_last_reposition: Option<Instant>,
    pub identify_overlay_pids: Vec<u32>,
    // When a profile is applied we enter a confirmation period where the user
    // can confirm the new configuration. If the timer elapses or the user
    // rejects, the previous configuration (raw config file contents) is
    // restored.
    pub pending_confirmation: bool,
    pub confirm_deadline: Option<Instant>,
    pub previous_config_contents: Option<String>,
    // If true, show the one-time first-run informational dialog when the
    // dedicated kanshiui config file was empty on first load.
    pub show_first_run_dialog: bool,
    // Drag anchor state used during interactive dragging so the rectangle
    // remains under the cursor after snapping adjustments.
    pub drag_anchor_screen: Option<usize>,
    pub drag_anchor_offset: Option<(f32, f32)>,
    // Whether snapping was active during the current drag session
    pub drag_snap_active: bool,
    // Runtime-only sidebar width persistence
    pub sidebar_width: f32,
}

impl AppState {
    pub fn init_defaults(&mut self) {
        if self.settings.snap_threshold_px <= 0 {
            self.settings.snap_threshold_px = 100;
        }
        if self.sidebar_width <= 0.0 {
            self.sidebar_width = 400.0;
        }
    }
}

pub fn canonical_display_id(
    make: Option<&str>,
    model: Option<&str>,
    serial: Option<&str>,
    connector_name: &str,
) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(v) = make {
        let t = v.trim();
        if !t.is_empty() {
            parts.push(t);
        }
    }
    if let Some(v) = model {
        let t = v.trim();
        if !t.is_empty() {
            parts.push(t);
        }
    }
    if let Some(v) = serial {
        let t = v.trim();
        if !t.is_empty() {
            parts.push(t);
        }
    }
    if parts.is_empty() {
        connector_name.to_string()
    } else {
        parts.join(" ")
    }
}

pub fn best_mode(modes: &[OutputMode]) -> Option<OutputMode> {
    modes.iter().cloned().max_by(|a, b| {
        let a_area = a.width as u64 * a.height as u64;
        let b_area = b.width as u64 * b.height as u64;
        a_area.cmp(&b_area).then_with(|| {
            a.refresh_hz
                .partial_cmp(&b.refresh_hz)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    })
}

pub fn trim_float(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
