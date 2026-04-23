use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;

use crate::kanshi_config::{kanshi_config_path, load_profiles, screen_multiset, upsert_profile};
use crate::kanshi_restart::restart_kanshi;
use crate::model::{AppState, Profile};
use crate::notify::notify_profile;
use crate::overlay::{kill_identify_overlays, spawn_identify_overlays};
use crate::sway::{default_screen_from_runtime, rescan_outputs};
use crate::ui::render_main_ui;

pub struct KanshiApp {
    pub state: AppState,
    pub config_path: PathBuf,
}

impl KanshiApp {
    pub fn new() -> Self {
        let mut state = AppState::default();
        state.init_defaults();

        let config_path =
            kanshi_config_path().unwrap_or_else(|_| PathBuf::from("~/.config/kanshi/default"));
        let mut app = Self { state, config_path };
        app.refresh_all();
        // Ensure the kanshi user service references the kanshiui file and is
        // running. Do this at startup so the service is corrected early.
        match crate::kanshi_restart::ensure_kanshi_user_service() {
            Ok(()) => {
                // no-op
            }
            Err(err) => {
                app.state.status = format!("Failed to ensure kanshi user service: {err}");
            }
        }
        app
    }

    pub fn refresh_all(&mut self) {
        match rescan_outputs() {
            Ok(outputs) => {
                self.state.connected_outputs = outputs;
            }
            Err(err) => {
                self.state.status = format!("Failed to scan sway outputs: {err}");
                return;
            }
        }

        match load_profiles(&self.config_path) {
            Ok((raw, profiles)) => {
                self.state.profiles = profiles;
                // If the dedicated config file is empty (first run) we should
                // show an informational dialog once.
                if raw.trim().is_empty() {
                    self.state.show_first_run_dialog = true;
                }
            }
            Err(err) => {
                self.state.status = format!("Failed to load kanshi config: {err}");
                self.state.profiles.clear();
            }
        }

        self.state.current_profile = self.find_matching_profile();
        if let Some(profile) = &self.state.current_profile {
            self.state.current_profile_name = profile.name.clone();
            self.state.current_profile_read_only = false;
        } else {
            // No matching profile found: generate a new profile for the current
            // connected outputs and give it a unique name derived from the
            // connector names (in-memory only until the user saves).
            let mut generated = self.default_profile(String::new());
            // Build a base name from connector names in stable order.
            let mut connectors: Vec<String> = self
                .state
                .connected_outputs
                .iter()
                .map(|o| o.display_id())
                .collect();
            connectors.sort();
            let base = connectors.join("+");
            let mut candidate = if base.is_empty() {
                "Profile".to_string()
            } else {
                format!("Auto - {}", base)
            };

            // Ensure uniqueness among existing profile names in state.profiles.
            let mut idx = 1;
            let mut exists = |name: &str| self.state.profiles.iter().any(|p| p.name == name);
            while exists(&candidate) {
                idx += 1;
                candidate = format!("{} ({})", candidate, idx);
            }

            generated.name = candidate.clone();
            // Add to in-memory profiles list if not already present so the UI
            // shows it in the picker. Do not write to disk until the user
            // explicitly presses Save.
            if !exists(&generated.name) {
                self.state.profiles.push(generated.clone());
            }
            self.state.current_profile = Some(generated);
            self.state.current_profile_name = candidate;
            self.state.current_profile_read_only = false;
        }
        self.state.status = "Loaded current outputs and profiles".to_string();
    }

    pub fn save_current_profile(&mut self) {
        if self.state.current_profile_read_only {
            self.state.status =
                "Profile is read-only because it does not match current hardware".to_string();
            return;
        }

        let mut profile = if let Some(profile) = self.state.current_profile.clone() {
            profile
        } else {
            self.state.status = "No profile in editor".to_string();
            return;
        };

        if self.state.current_profile_name.trim().is_empty() {
            self.state.status = "Please provide a profile name".to_string();
            return;
        }

        profile.name = self.state.current_profile_name.trim().to_string();

        // NOTE: alignment to 0,0 is now a user action (button) and is not
        // applied automatically on save.

        // Read previous contents before we overwrite the file so we can restore
        // them if the user rejects the changes.
        let prev = std::fs::read_to_string(&self.config_path).ok();

        match upsert_profile(&self.config_path, &profile) {
            Ok(()) => {
                self.state.previous_config_contents = prev;

                // Start confirmation window unconditionally after save.
                self.state.pending_confirmation = true;
                self.state.confirm_deadline =
                    Some(Instant::now() + std::time::Duration::from_secs(10));

                // Try to restart kanshi to apply the new config; report any
                // error but keep the confirmation dialog active so the user can
                // revert if needed.
                match restart_kanshi() {
                    Ok(()) => {
                        if !self.state.current_profile_name.trim().is_empty() {
                            notify_profile(self.state.current_profile_name.trim());
                        }
                        self.state.status = format!(
                            "Applied profile '{}' and restarted kanshi (confirming...)",
                            self.state.current_profile_name.trim()
                        );
                    }
                    Err(err) => {
                        self.state.status = format!(
                            "Saved profile '{}' but failed to restart kanshi: {err} (confirming...)",
                            profile.name
                        );
                    }
                }

                // Update in-memory profiles to reflect the saved profile so
                // UI-only metadata (like mirroring) is preserved until the
                // user or external event forces a full reload. Avoid a full
                // refresh here because parsing the file currently does not
                // retain UI-only fields and would wipe them.
                if let Some(pos) = self
                    .state
                    .profiles
                    .iter()
                    .position(|p| p.name == profile.name)
                {
                    self.state.profiles[pos] = profile.clone();
                } else {
                    self.state.profiles.push(profile.clone());
                }
                self.state.current_profile = Some(profile);
            }
            Err(err) => {
                self.state.status = format!("Save failed: {err}");
            }
        }
    }

    pub fn apply_current_profile(&mut self) {
        if self.state.current_profile_read_only {
            self.state.status =
                "Profile is read-only because it does not match current hardware".to_string();
            return;
        }

        self.save_current_profile();
        if self.state.status.starts_with("Save failed")
            || self.state.status == "Please provide a profile name"
        {
            return;
        }

        // Restart kanshi and enter a confirmation window where the user must
        // accept the changes. If they don't within 10s or explicitly reject,
        // restore previous config.
        match restart_kanshi() {
            Ok(()) => {
                if !self.state.current_profile_name.trim().is_empty() {
                    notify_profile(self.state.current_profile_name.trim());
                }
                self.state.status = format!(
                    "Applied profile '{}' and restarted kanshi (confirming...)",
                    self.state.current_profile_name.trim()
                );
                // start confirmation countdown
                self.state.pending_confirmation = true;
                self.state.confirm_deadline =
                    Some(Instant::now() + std::time::Duration::from_secs(10));
                // leave previous_config_contents as set by save_current_profile
            }
            Err(err) => {
                self.state.status = format!("Profile saved but failed to restart kanshi: {err}");
            }
        }
    }

    pub fn load_profile_into_editor(&mut self, profile_name: &str) {
        if let Some(profile) = self
            .state
            .profiles
            .iter()
            .find(|p| p.name == profile_name)
            .cloned()
        {
            let read_only = !self.profile_matches_current_hardware(&profile);
            self.state.current_profile_name = profile.name.clone();
            self.state.current_profile = Some(profile);
            self.state.current_profile_read_only = read_only;
            self.state.status = if read_only {
                format!("Loaded profile '{profile_name}' as read-only (hardware mismatch)")
            } else {
                format!("Loaded profile '{profile_name}' in editor")
            };
        }
    }

    pub fn profile_matches_current_hardware(&self, profile: &Profile) -> bool {
        let mut output_ids = HashMap::<String, usize>::new();
        let mut output_conn = HashMap::<String, usize>::new();
        for output in &self.state.connected_outputs {
            *output_ids.entry(output.display_id()).or_insert(0) += 1;
            *output_conn
                .entry(output.connector_name.clone())
                .or_insert(0) += 1;
        }

        let profile_set = screen_multiset(&profile.screens);
        profile_set == output_ids || profile_set == output_conn
    }

    pub fn identify_screens(&mut self) {
        self.state.identify_enabled = !self.state.identify_enabled;
        if self.state.identify_enabled {
            self.state.identify_last_reposition = None;
            let pids = spawn_identify_overlays(&self.state.connected_outputs);
            self.state.identify_overlay_pids = pids.clone();
            if pids.is_empty() {
                self.state.status = "Failed to spawn identify overlays".to_string();
            } else {
                self.state.status = format!(
                    "Screen identification overlays enabled ({} windows)",
                    pids.len()
                );
            }
            eprintln!("spawn_identify_overlays returned pids={:?}", pids);
        } else {
            kill_identify_overlays(&mut self.state.identify_overlay_pids);
            self.state.status = "Screen identification overlays disabled".to_string();
        }
    }

    fn default_profile(&self, name: String) -> Profile {
        let mut screens = Vec::new();
        for (idx, output) in self.state.connected_outputs.iter().enumerate() {
            if let Some(screen) = default_screen_from_runtime(output, idx) {
                screens.push(screen);
            }
        }
        Profile {
            name,
            screens,
            raw_range: None,
        }
    }

    // Shift all screen positions so the minimal x/y among enabled screens
    // becomes 0,0. This ensures saved profiles have their bounding rect
    // anchored at the origin.
    fn align_profile_positions(profile: &mut Profile) {
        let mut min_x: Option<i32> = None;
        let mut min_y: Option<i32> = None;
        for s in &profile.screens {
            if !s.enabled {
                continue;
            }
            min_x = Some(min_x.map(|m| m.min(s.pos_x)).unwrap_or(s.pos_x));
            min_y = Some(min_y.map(|m| m.min(s.pos_y)).unwrap_or(s.pos_y));
        }
        let min_x = min_x.unwrap_or(0);
        let min_y = min_y.unwrap_or(0);
        for s in &mut profile.screens {
            s.pos_x -= min_x;
            s.pos_y -= min_y;
        }
    }

    // Public helper to align the currently loaded profile in-place. Returns
    // an error string when there is no profile loaded.
    pub fn align_current_profile(&mut self) -> Result<(), &'static str> {
        if let Some(profile) = self.state.current_profile.as_mut() {
            Self::align_profile_positions(profile);
            Ok(())
        } else {
            Err("No profile loaded")
        }
    }

    fn find_matching_profile(&self) -> Option<Profile> {
        self.state
            .profiles
            .iter()
            .find(|profile| self.profile_matches_current_hardware(profile))
            .cloned()
    }
}

impl eframe::App for KanshiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        render_main_ui(self, ctx);
    }
}

impl Drop for KanshiApp {
    fn drop(&mut self) {
        kill_identify_overlays(&mut self.state.identify_overlay_pids);
    }
}
