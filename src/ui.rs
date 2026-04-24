use std::time::Instant;

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

use crate::app::KanshiApp;
use crate::model::{trim_float, OutputMode, ScreenConfig};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

const CANVAS_MARGIN: f32 = 24.0;
const PIXELS_PER_MODE_PIXEL: f32 = 0.12;

// Deterministic palette of background colors that work with white text.
// Palette chosen for perceptual distinctness while keeping white text
// readable. Avoid near-duplicates (yellow/orange) that are too similar.
const COLOR_PALETTE: [Color32; 12] = [
    Color32::from_rgb(31, 119, 180),  // blue
    Color32::from_rgb(255, 65, 54),   // vivid red-orange
    Color32::from_rgb(44, 160, 44),   // green
    Color32::from_rgb(214, 39, 40),   // red
    Color32::from_rgb(148, 103, 189), // purple
    Color32::from_rgb(0, 150, 136),   // teal
    Color32::from_rgb(227, 119, 194), // pink
    Color32::from_rgb(127, 127, 127), // gray
    Color32::from_rgb(255, 193, 7),   // amber (but distinct from orange)
    Color32::from_rgb(23, 190, 207),  // cyan
    Color32::from_rgb(82, 84, 163),   // indigo
    Color32::from_rgb(255, 102, 0),   // strong orange
];

pub(crate) fn color_for_id(id: &str) -> Color32 {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    let idx = (hasher.finish() as usize) % COLOR_PALETTE.len();
    COLOR_PALETTE[idx]
}

pub fn render_main_ui(app: &mut KanshiApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
        // Add vertical padding inside the top bar
        ui.vertical(|ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                // Left-aligned group: Identify, Rescan, Align
                let identify_label = if app.state.identify_enabled {
                    "Hide Overlays"
                } else {
                    "Identify Screens"
                };
                if ui
                    .add_sized([160.0, 36.0], egui::Button::new(identify_label))
                    .clicked()
                {
                    app.identify_screens();
                }

                if ui
                    .add_sized([120.0, 36.0], egui::Button::new("Rescan"))
                    .clicked()
                {
                    app.refresh_all();
                }

                // Align button removed per UI request.

                // Spacer to push the Apply button to the right edge. Keep a 36px margin.
                let remaining = ui.available_size_before_wrap().x;
                let apply_w = 120.0f32;
                // No extra right margin requested; align Apply to right edge.
                let spacer = (remaining - apply_w).max(0.0);
                ui.add_space(spacer);

                // Right-aligned Apply button
                if ui
                    .add_enabled(
                        !app.state.current_profile_read_only,
                        egui::Button::new("Apply").min_size(Vec2::new(apply_w, 36.0)),
                    )
                    .clicked()
                {
                    app.apply_current_profile();
                }
            });
            ui.add_space(8.0);
        });
    });

    egui::SidePanel::right("profiles_panel")
        .resizable(true)
        .default_width(400.0)
        .min_width(200.0)
        .max_width(800.0)
        .show(ctx, |ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(profile) = app.state.current_profile.as_mut() {
                        if app.state.current_profile_read_only {
                            ui.label(
                                "This profile does not match current hardware and is read-only.",
                            );
                        }
                        // Build a map of runtime-available modes keyed by display id and connector name,
                        // and a map to resolve a screen id to the actual connector name (eg. DVI-I-2).
                        let mut runtime_modes: HashMap<String, Vec<OutputMode>> = HashMap::new();
                        let mut runtime_connectors: HashMap<String, String> = HashMap::new();
                        for out in &app.state.connected_outputs {
                            let did = out.display_id();
                            runtime_modes.insert(did.clone(), out.available_modes.clone());
                            runtime_modes
                                .insert(out.connector_name.clone(), out.available_modes.clone());
                            // Map both display id and connector name to the connector name
                            runtime_connectors.insert(did, out.connector_name.clone());
                            runtime_connectors
                                .insert(out.connector_name.clone(), out.connector_name.clone());
                        }

                        // Make each screen box fill the full sidebar width and have
                        // the same fixed height so the layout looks consistent.
                        // Reduced height to avoid visible empty space at the bottom.
                        let box_height = 120.0f32;
                        // Render boxes without background/border and insert a horizontal
                        // separator between them. Boxes size to their content.
                        let gap = 6.0f32;
                        ui.add_space(6.0);
                        let last = profile.screens.len().saturating_sub(1);

                        // Precompute candidate mirror target lists for each screen to
                        // avoid immutable borrows of profile.screens while we iterate
                        // mutably. Each entry is a Vec<String> of ids that are enabled
                        // and not mirroring (excluding the screen itself).
                        let mut candidate_lists: Vec<Vec<String>> = Vec::new();
                        // Snapshot some physical info to decide repositioning when a
                        // screen is unmirrored. This avoids borrowing profile while we
                        // hold a mutable reference to one of its screens.
                        let mut physical_snapshot: Vec<(String, i32, i32, i32, i32, bool)> =
                            Vec::new();
                        for s in &profile.screens {
                            let list = profile
                                .screens
                                .iter()
                                .filter(|o| o.enabled && !o.mirror && o.id != s.id)
                                .map(|o| o.id.clone())
                                .collect::<Vec<_>>();
                            candidate_lists.push(list);
                            physical_snapshot.push((
                                s.id.clone(),
                                s.pos_x,
                                s.pos_y,
                                s.selected_mode.width as i32,
                                s.selected_mode.height as i32,
                                s.mirror,
                            ));
                        }

                        for (idx, screen) in profile.screens.iter_mut().enumerate() {
                            // Inner left padding
                            ui.add_space(8.0);

                            // Create a short stable UID for this screen derived from
                            // both its friendly id and connector so UI element IDs
                            // remain unique even when multiple screens share the
                            // same friendly name.
                            let id_clone = screen.id.clone();
                            let conn_clone = screen.connector_name.clone();
                            let uid = {
                                let mut hasher = DefaultHasher::new();
                                id_clone.hash(&mut hasher);
                                conn_clone.hash(&mut hasher);
                                format!("{:x}", hasher.finish())
                            };

                            ui.add_enabled_ui(!app.state.current_profile_read_only, |ui| {
                                ui.horizontal(|ui| {
                                    // Left color bar spanning the whole card height
                                    let left_color = color_for_id(&format!("{}||{}", screen.id, screen.connector_name));
                                    let (bar_rect, _resp) = ui.allocate_exact_size(
                                        Vec2::new(8.0, box_height),
                                        Sense::hover(),
                                    );
                                    ui.painter().rect_filled(bar_rect, 0.0, left_color);
                                    ui.add_space(8.0);

                                    ui.vertical(|ui| {
                                        // Title: show screen id and connector at the top of the card
                                        // Use white text for the title so it contrasts with the
                                        // color bar background.
                                        // Title: screen id as main title, connector in parentheses
                                        // If the connector stored in the screen struct isn't
                                        // the canonical connector, try to resolve it from
                                        // the runtime_connectors map.
                                        // Prefer the connector stored in the ScreenConfig
                                        // when present; fall back to the runtime map
                                        // when the ScreenConfig only contains a
                                        // friendly id.
                                        let conn = if screen.connector_name != screen.id {
                                            screen.connector_name.clone()
                                        } else {
                                            runtime_connectors
                                                .get(&screen.id)
                                                .cloned()
                                                .unwrap_or_else(|| screen.connector_name.clone())
                                        };
                                        ui.colored_label(
                                            Color32::WHITE,
                                            format!("{} ({})", screen.id, conn),
                                        );
                                        egui::Grid::new(format!("grid-{}", uid))
                                            .num_columns(2)
                                            .spacing([8.0, 6.0])
                                            .striped(false)
                                            .show(ui, |ui| {
                                                // Enabled
                                                ui.label("Enabled");
                                                ui.checkbox(&mut screen.enabled, "");
                                                ui.end_row();

                                                // Scale
                                                ui.label("Scale");
                                                ui.add_enabled_ui(screen.enabled, |ui| {
                                                    ui.add(
                                                        egui::DragValue::new(&mut screen.scale)
                                                            .speed(0.1)
                                                            .range(0.5..=4.0),
                                                    );
                                                });
                                                ui.end_row();

                                                // Resolution
                                                ui.label("Resolution");
                                                ui.add_enabled_ui(screen.enabled, |ui| {
                                                    egui::ComboBox::from_id_salt(format!(
                                                        "mode-side-{}",
                                                        uid
                                                    ))
                                                    .selected_text(
                                                        screen.selected_mode.as_kanshi_mode(),
                                                    )
                                                    .show_ui(ui, |ui| {
                                                        // Prefer runtime modes keyed by connector
                                                        // name (more precise), fall back to the
                                                        // friendly id when necessary.
                                                        let modes = runtime_modes
                                                            .get(&screen.connector_name)
                                                            .cloned()
                                                            .or_else(|| runtime_modes.get(&screen.id).cloned())
                                                            .unwrap_or_else(|| screen.available_modes.clone());
                                                        for mode in modes {
                                                            let selected =
                                                                mode == screen.selected_mode;
                                                            if ui
                                                                .selectable_label(
                                                                    selected,
                                                                    mode.as_kanshi_mode(),
                                                                )
                                                                .clicked()
                                                            {
                                                                screen.selected_mode = OutputMode {
                                                                    width: mode.width,
                                                                    height: mode.height,
                                                                    refresh_hz: mode.refresh_hz,
                                                                };
                                                            }
                                                        }
                                                    });
                                                });
                                                ui.end_row();

                                                // Mirror: always show the pulldown. "None" means not mirrored.
                                                ui.label("Mirror");
                                                // capture previous mirror state to detect unmirror
                                                let prev_mirror = screen.mirror;
                                                let candidates = &candidate_lists[idx];
                                                // Ensure current target is valid
                                                if let Some(ref t) = screen.mirror_target {
                                                    if !candidates.contains(t) {
                                                        screen.mirror_target = None;
                                                    }
                                                }
                                                let selected_text = screen
                                                    .mirror_target
                                                    .as_deref()
                                                    .unwrap_or("None");
                                                egui::ComboBox::from_id_salt(format!(
                                                    "mirror-target-{}",
                                                    uid
                                                ))
                                                .selected_text(selected_text)
                                                .show_ui(ui, |ui| {
                                                    ui.selectable_value(
                                                        &mut screen.mirror_target,
                                                        None,
                                                        "None",
                                                    );
                                                    for cand in candidates.iter() {
                                                        // Ensure the candidate label shows the
                                                        // friendly id but mirror_target stores
                                                        // the friendly id, so this remains stable.
                                                        ui.selectable_value(
                                                            &mut screen.mirror_target,
                                                            Some(cand.clone()),
                                                            cand,
                                                        );
                                                    }
                                                });

                                                // Keep mirror flag consistent and handle unmirror reposition
                                                screen.mirror = screen.mirror_target.is_some();
                                                if prev_mirror && !screen.mirror {
                                                    // compute current bbox in physical pixels
                                                    let left = screen.pos_x;
                                                    let top = screen.pos_y;
                                                    let right =
                                                        left + screen.selected_mode.width as i32;
                                                    let bottom =
                                                        top + screen.selected_mode.height as i32;
                                                    let mut overlaps = false;
                                                    let mut max_right = right;
                                                    for (oid, ox, oy, ow, oh, omirror) in
                                                        &physical_snapshot
                                                    {
                                                        if *oid == screen.id {
                                                            continue;
                                                        }
                                                        // skip mirrored screens; they are not visible
                                                        if *omirror {
                                                            continue;
                                                        }
                                                        let oleft = *ox;
                                                        let otop = *oy;
                                                        let oright = oleft + *ow;
                                                        let obottom = otop + *oh;
                                                        if !(right <= oleft
                                                            || left >= oright
                                                            || bottom <= otop
                                                            || top >= obottom)
                                                        {
                                                            overlaps = true;
                                                        }
                                                        if oright > max_right {
                                                            max_right = oright;
                                                        }
                                                    }
                                                    if overlaps {
                                                        let padding = 40; // physical px
                                                        screen.pos_x = max_right + padding;
                                                    }
                                                }
                                                ui.end_row();
                                            });
                                    });
                                });
                            });

                            // Add small vertical gap and separator unless last
                            if idx != last {
                                ui.add_space(4.0);
                                ui.separator();
                                ui.add_space(gap - 4.0);
                            }
                        }
                    }
                });
        });

    // Status bar removed

    // Confirmation modal: when a profile is applied we show a modal dialog with
    // a 10-second countdown asking the user to confirm the change. If the
    // deadline passes or the user clicks "No" we restore the previous config.
    if app.state.pending_confirmation {
        // calculate remaining seconds
        let now = Instant::now();
        let remaining = app
            .state
            .confirm_deadline
            .map(|d| d.saturating_duration_since(now).as_secs())
            .unwrap_or(0);

        egui::Window::new("Confirm configuration")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                // Top margin for visual separation from the window title
                // keep small to avoid overly-tall dialogs
                ui.add_space(8.0);
                ui.label(format!("Works as expected? Reset in {}s", remaining));
                ui.horizontal_centered(|ui| {
                    if ui
                        .add_sized([160.0, 40.0], egui::Button::new("Yes"))
                        .clicked()
                    {
                        // confirm: stop pending confirmation
                        app.state.pending_confirmation = false;
                        app.state.confirm_deadline = None;
                        app.state.previous_config_contents = None;
                        app.state.status = format!(
                            "Profile '{}' confirmed",
                            app.state.current_profile_name.trim()
                        );
                    }
                    ui.add_space(8.0);
                    if ui
                        .add_sized([160.0, 40.0], egui::Button::new("No"))
                        .clicked()
                    {
                        // reject and restore previous config
                        app.state.pending_confirmation = false;
                        app.state.confirm_deadline = None;
                        if let Some(prev) = app.state.previous_config_contents.take() {
                            if let Err(err) =
                                crate::kanshi_config::replace_config(&app.config_path, &prev)
                            {
                                app.state.status =
                                    format!("Failed to restore previous config: {err}");
                            } else {
                                app.state.status = "Previous configuration restored".to_string();
                                // re-load profiles from restored file
                                app.refresh_all();
                                // try to restart kanshi again to apply restored config
                                let _ = crate::kanshi_restart::restart_kanshi();
                            }
                        }
                    }
                });
            });

        // If countdown expired, treat as rejection
        if remaining == 0 {
            app.state.pending_confirmation = false;
            app.state.confirm_deadline = None;
            if let Some(prev) = app.state.previous_config_contents.take() {
                if let Err(err) = crate::kanshi_config::replace_config(&app.config_path, &prev) {
                    app.state.status = format!("Failed to restore previous config: {err}");
                } else {
                    app.state.status =
                        "Confirmation timeout — previous configuration restored".to_string();
                    app.refresh_all();
                    let _ = crate::kanshi_restart::restart_kanshi();
                }
            }
        } else {
            // request repaint so the countdown updates
            ctx.request_repaint();
        }
    }

    // First-run informational dialog when using a dedicated kanshiui file.
    if app.state.show_first_run_dialog {
        egui::Window::new("About KanshiUI")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                // Top margin for dialog content (small)
                ui.add_space(8.0);
                ui.label("KanshiUI will manage display configuration using a dedicated file and a user systemd service.");
                ui.label("Your existing kanshi configuration will not be modified, but it will not be used while KanshiUI manages output profiles.");
                ui.label("If you only want to temporarily adjust your screens, this is fine — KanshiUI keeps backups and lets you restore the previous configuration.");
                ui.horizontal_centered(|ui| {
                    if ui
                        .add_sized([160.0, 40.0], egui::Button::new("OK"))
                        .clicked()
                    {
                        app.state.show_first_run_dialog = false;
                    }
                });
            });
        // ensure repaint so the dialog is interactive
        ctx.request_repaint();
    }

    egui::CentralPanel::default().show(ctx, |ui| {
        // Read flags we need before mutably borrowing current_profile
        let read_only = app.state.current_profile_read_only;
        if app.state.current_profile.is_some() {
            render_canvas(app, ui, !read_only);
        } else {
            ui.centered_and_justified(|ui| {
                ui.label("No profile loaded");
            });
        }
    });
}

// (identify overlay rendering moved to overlay_app.rs)

fn render_canvas(app: &mut KanshiApp, ui: &mut egui::Ui, editable: bool) {
    let profile = match app.state.current_profile.as_mut() {
        Some(p) => p,
        None => return,
    };
    let available = ui.available_size();
    let (canvas_rect, _) = ui.allocate_exact_size(available, Sense::hover());
    let painter = ui.painter_at(canvas_rect);

    painter.rect_stroke(
        canvas_rect,
        0.0,
        Stroke::new(1.0, Color32::from_gray(80)),
        StrokeKind::Inside,
    );

    // Build a list of visible (non-mirroring) screen indices and precompute
    // UI rects only for those. Mirrored screens are not shown on the canvas.
    // Only show screens that are enabled and not mirroring on the canvas.
    let visible_indices: Vec<usize> = (0..profile.screens.len())
        .filter(|&i| profile.screens[i].enabled && !profile.screens[i].mirror)
        .collect();

    let mut rects: Vec<Rect> = visible_indices
        .iter()
        .map(|&i| screen_rect_from_config(&profile.screens[i], canvas_rect))
        .collect();

    for vis_idx in 0..visible_indices.len() {
        let i = visible_indices[vis_idx];
        // Use the precomputed rect for UI interaction.
        let mut screen_rect = rects[vis_idx];
        let response = ui.allocate_rect(screen_rect, Sense::click_and_drag());

        if editable && response.dragged() {
            // current pointer position
            let pointer_pos = ui
                .ctx()
                .pointer_interact_pos()
                .unwrap_or_else(|| Pos2::new(screen_rect.left(), screen_rect.top()));

            // If this is a new drag for this screen, capture the anchor offset
            if app.state.drag_anchor_screen != Some(i) {
                let anchor_offset_x = pointer_pos.x - screen_rect.left();
                let anchor_offset_y = pointer_pos.y - screen_rect.top();
                app.state.drag_anchor_screen = Some(i);
                app.state.drag_anchor_offset = Some((anchor_offset_x, anchor_offset_y));
                app.state.drag_snap_active = false;
            }

            let (anchor_x, anchor_y) = app.state.drag_anchor_offset.unwrap_or((
                pointer_pos.x - screen_rect.left(),
                pointer_pos.y - screen_rect.top(),
            ));

            // Desired rectangle positioned so the pointer stays at the same relative offset
            let desired_left = pointer_pos.x - anchor_x;
            let desired_top = pointer_pos.y - anchor_y;
            let desired =
                Rect::from_min_size(Pos2::new(desired_left, desired_top), screen_rect.size());

            // Build list of other screens' UI rects for snapping. rects[]
            // were created by screen_rect_from_config and already map stored
            // pos_x/pos_y (physical) to UI positions, and width/height are
            // virtual sizes mapped to UI.
            let mut others_ui: Vec<Rect> = Vec::new();
            for (j, r) in rects.iter().enumerate() {
                if j == vis_idx {
                    continue;
                }
                others_ui.push(*r);
            }

            let shift_held = ui.ctx().input(|i| i.modifiers.shift);

            // Apply snapping in UI coordinates (consistent units). Threshold
            // is 100 virtual px -> convert to UI pixels.
            let final_rect = if !shift_held {
                let threshold_ui = 100.0 * PIXELS_PER_MODE_PIXEL;
                let snapped = snap_rect_to_others(desired, &others_ui, threshold_ui);
                if (snapped.left() - desired.left()).abs() > 0.1
                    || (snapped.top() - desired.top()).abs() > 0.1
                {
                    app.state.drag_snap_active = true;
                    snapped
                } else {
                    app.state.drag_snap_active = false;
                    desired
                }
            } else {
                app.state.drag_snap_active = false;
                desired
            };

            // Convert final_rect UI coords back to stored physical pos_x/pos_y
            let new_x_ui = final_rect.left() - (canvas_rect.left() + CANVAS_MARGIN);
            let new_y_ui = final_rect.top() - (canvas_rect.top() + CANVAS_MARGIN);
            let new_pos_x = (new_x_ui / PIXELS_PER_MODE_PIXEL) as i32;
            let new_pos_y = (new_y_ui / PIXELS_PER_MODE_PIXEL) as i32;

            let screen = &mut profile.screens[i];
            screen.pos_x = new_pos_x;
            screen.pos_y = new_pos_y;

            // Recompute the UI rect for drawing after the update
            screen_rect = screen_rect_from_config(screen, canvas_rect);
            rects[vis_idx] = screen_rect;

            // If dragging ended, clear the anchor
            if response.drag_stopped() {
                app.state.drag_anchor_screen = None;
                app.state.drag_anchor_offset = None;
                app.state.drag_snap_active = false;
            }
        }

        // Borrow the (possibly-updated) screen for drawing
        let screen = &profile.screens[i];

        // Use deterministic color per-screen as background when enabled,
        // fallback to gray when disabled so the user can tell disabled
        // screens apart. Keep text white for contrast.
        let fill = if screen.enabled {
            let mut c = color_for_id(&format!("{}||{}", screen.id, screen.connector_name));
            // make slightly darker / semi-opaque for canvas readability
            c = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 230);
            c
        } else {
            Color32::from_rgb(65, 65, 65)
        };
        painter.rect_filled(screen_rect, 0.0, fill);

        // Show centered title + metadata. Title is slightly larger.
        let s = screen.scale as f32;
        let virtual_w = (screen.selected_mode.width as f32 / s) as u32;
        let virtual_h = (screen.selected_mode.height as f32 / s) as u32;

        // Title should be the human-readable display id (vendor/model),
        // and the second line is the connector name (eg. DVI-I-2).
        let title = screen.id.clone();
        // Show the connector stored in the ScreenConfig (eg. DVI-I-2) as the
        // second line in the canvas rectangle.
        let connector = screen.connector_name.clone();
        let res_line = format!(
            "{}x{}@{}Hz",
            virtual_w,
            virtual_h,
            trim_float(screen.selected_mode.refresh_hz)
        );
        let scale_line = format!("Scale {}", trim_float(screen.scale));

        // Layout constraints
        let padding = 6.0;
        let avail_w = (screen_rect.width() - padding * 2.0).max(16.0);
        let avail_h = (screen_rect.height() - padding * 2.0).max(16.0);

        // Initial font sizes
        let mut f_title = 15.0f32;
        let mut f_display = 13.0f32;
        let mut f_res = 12.0f32;
        let mut f_scale = 12.0f32;

        // Approximate text width: avg_char_width ~= fs * 0.6
        let approx_w = |text: &str, fs: f32| -> f32 { text.len() as f32 * fs * 0.6 };

        // If any line is too wide, scale that line down individually
        let w_title = approx_w(&title, f_title);
        if w_title > avail_w {
            let scale = (avail_w / w_title).clamp(0.4, 1.0);
            f_title *= scale;
        }
        let w_display = approx_w(&connector, f_display);
        if w_display > avail_w {
            let scale = (avail_w / w_display).clamp(0.4, 1.0);
            f_display *= scale;
        }
        let w_res = approx_w(&res_line, f_res);
        if w_res > avail_w {
            let scale = (avail_w / w_res).clamp(0.4, 1.0);
            f_res *= scale;
        }
        let w_scale = approx_w(&scale_line, f_scale);
        if w_scale > avail_w {
            let scale = (avail_w / w_scale).clamp(0.4, 1.0);
            f_scale *= scale;
        }

        // If total height still too large, scale everything down uniformly
        let line_h = |fs: f32| fs * 1.2;
        let total_h = line_h(f_title) + line_h(f_display) + line_h(f_res) + line_h(f_scale);
        if total_h > avail_h {
            let scale = (avail_h / total_h).clamp(0.4, 1.0);
            f_title *= scale;
            f_display *= scale;
            f_res *= scale;
            f_scale *= scale;
        }

        // Draw lines centered and vertically stacked, with small spacing
        let center_x = screen_rect.center().x;
        let mut y = screen_rect.center().y
            - (line_h(f_title) + line_h(f_display) + line_h(f_res) + line_h(f_scale)) / 2.0;

        painter.text(
            Pos2::new(center_x, y + line_h(f_title) / 2.0),
            Align2::CENTER_CENTER,
            title,
            FontId::proportional(f_title),
            Color32::WHITE,
        );
        y += line_h(f_title);

        painter.text(
            Pos2::new(center_x, y + line_h(f_display) / 2.0),
            Align2::CENTER_CENTER,
            connector,
            FontId::proportional(f_display),
            Color32::WHITE,
        );
        y += line_h(f_display);

        painter.text(
            Pos2::new(center_x, y + line_h(f_res) / 2.0),
            Align2::CENTER_CENTER,
            res_line,
            FontId::proportional(f_res),
            Color32::WHITE,
        );
        y += line_h(f_res);

        painter.text(
            Pos2::new(center_x, y + line_h(f_scale) / 2.0),
            Align2::CENTER_CENTER,
            scale_line,
            FontId::proportional(f_scale),
            Color32::WHITE,
        );
    }

    // Align button moved to the bottom bar; no canvas overlay here.
}

fn screen_rect_from_config(screen: &ScreenConfig, canvas: Rect) -> Rect {
    // Show the virtual (logical) resolution when scale is applied. Virtual
    // width/height are physical dimensions divided by scale. Positions are
    // stored in physical pixels, so convert them to virtual coordinates for
    // display by dividing by the scale.
    let scale = screen.scale as f32;
    // Width/height are shown as virtual (physical / scale). Positions are
    // stored in physical pixels and must NOT be scaled; only the size is
    // reduced by the scale when displayed.
    let w = (screen.selected_mode.width as f32 / scale) * PIXELS_PER_MODE_PIXEL;
    let h = (screen.selected_mode.height as f32 / scale) * PIXELS_PER_MODE_PIXEL;
    let x = canvas.left() + CANVAS_MARGIN + (screen.pos_x as f32) * PIXELS_PER_MODE_PIXEL;
    let y = canvas.top() + CANVAS_MARGIN + (screen.pos_y as f32) * PIXELS_PER_MODE_PIXEL;
    Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h))
}

/// Snap `rect` to nearby edges in `others` if within `threshold_ui` pixels.
/// Returns a new rect translated by the chosen snap offsets.
fn snap_rect_to_others(mut rect: Rect, others: &[Rect], threshold_ui: f32) -> Rect {
    let mut best_dx: Option<f32> = None;
    let mut best_dy: Option<f32> = None;

    let cand_left = rect.left();
    let cand_right = rect.right();
    let cand_top = rect.top();
    let cand_bottom = rect.bottom();

    for o in others {
        let other_left = o.left();
        let other_right = o.right();
        let other_top = o.top();
        let other_bottom = o.bottom();

        // horizontal edges: candidate left/right to other left/right
        let candidates_x = [
            other_left - cand_left,
            other_left - cand_right,
            other_right - cand_left,
            other_right - cand_right,
        ];
        for &dx in &candidates_x {
            let adx = dx.abs();
            if adx <= threshold_ui {
                if best_dx.map(|b| adx < b.abs()).unwrap_or(true) {
                    best_dx = Some(dx);
                }
            }
        }

        // vertical edges
        let candidates_y = [
            other_top - cand_top,
            other_top - cand_bottom,
            other_bottom - cand_top,
            other_bottom - cand_bottom,
        ];
        for &dy in &candidates_y {
            let ady = dy.abs();
            if ady <= threshold_ui {
                if best_dy.map(|b| ady < b.abs()).unwrap_or(true) {
                    best_dy = Some(dy);
                }
            }
        }
    }

    let dx = best_dx.unwrap_or(0.0);
    let dy = best_dy.unwrap_or(0.0);
    rect = rect.translate(Vec2::new(dx, dy));
    rect
}
// consider_axis_candidate removed (unused)
