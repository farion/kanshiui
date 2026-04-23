use std::process::{Command, Stdio};
use std::time::Duration;
use std::time::Instant;

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

use crate::app::KanshiApp;
use crate::model::{trim_float, OutputMode, RuntimeOutput, ScreenConfig};
use std::collections::HashMap;

const CANVAS_MARGIN: f32 = 24.0;
const PIXELS_PER_MODE_PIXEL: f32 = 0.12;

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

                if ui
                    .add_enabled(
                        app.state.current_profile.is_some(),
                        egui::Button::new("Align to 0,0").min_size(Vec2::new(140.0, 36.0)),
                    )
                    .clicked()
                {
                    match app.align_current_profile() {
                        Ok(()) => {
                            app.state.status = "Aligned current profile to 0,0".to_string();
                        }
                        Err(e) => {
                            app.state.status = format!("Align failed: {e}");
                        }
                    }
                }

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
        .default_width(300.0)
        .min_width(200.0)
        .max_width(500.0)
        .show(ctx, |ui| {
            if let Some(profile) = app.state.current_profile.as_mut() {
                if app.state.current_profile_read_only {
                    ui.label("This profile does not match current hardware and is read-only.");
                }
                // Build a map of runtime-available modes keyed by display id and connector name
                let mut runtime_modes: HashMap<String, Vec<OutputMode>> = HashMap::new();
                for out in &app.state.connected_outputs {
                    runtime_modes.insert(out.display_id(), out.available_modes.clone());
                    runtime_modes.insert(out.connector_name.clone(), out.available_modes.clone());
                }

                // Make each screen box fill the full sidebar width and have
                // the same fixed height so the layout looks consistent.
                // Reduced height to avoid visible empty space at the bottom.
                let box_height = 120.0f32;
                // Compute available width once so allocations don't change per-iteration.
                let avail_w = ui.available_width();

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
                for s in &profile.screens {
                    let list = profile
                        .screens
                        .iter()
                        .filter(|o| o.enabled && !o.mirror && o.id != s.id)
                        .map(|o| o.id.clone())
                        .collect::<Vec<_>>();
                    candidate_lists.push(list);
                }

                for (idx, screen) in profile.screens.iter_mut().enumerate() {
                    // Inner left padding
                    ui.add_space(8.0);

                    ui.add_enabled_ui(!app.state.current_profile_read_only, |ui| {
                        ui.label(format!("{} ({})", screen.id, screen.connector_name));
                        ui.horizontal(|ui| {
                            ui.label("Enabled");
                            ui.checkbox(&mut screen.enabled, "");
                            ui.label("Scale");
                            ui.add(
                                egui::DragValue::new(&mut screen.scale)
                                    .speed(0.1)
                                    .range(0.5..=4.0),
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label("Position");
                            ui.add(egui::DragValue::new(&mut screen.pos_x));
                            ui.add(egui::DragValue::new(&mut screen.pos_y));
                        });

                        // Mirroring controls
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut screen.mirror, "Mirror");
                            if screen.mirror {
                                // Use the precomputed candidate list for this index
                                let candidates = &candidate_lists[idx];
                                // Ensure the current target is present; if not, clear it
                                if let Some(ref t) = screen.mirror_target {
                                    if !candidates.contains(t) {
                                        screen.mirror_target = None;
                                    }
                                }
                                let selected_text =
                                    screen.mirror_target.as_deref().unwrap_or("None");
                                egui::ComboBox::from_id_salt(format!(
                                    "mirror-target-{}",
                                    screen.id
                                ))
                                .selected_text(selected_text)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut screen.mirror_target, None, "None");
                                    for cand in candidates.iter() {
                                        ui.selectable_value(
                                            &mut screen.mirror_target,
                                            Some(cand.clone()),
                                            cand.clone(),
                                        );
                                    }
                                });
                            }
                        });

                        egui::ComboBox::from_id_salt(format!("mode-side-{}", screen.id))
                            .selected_text(screen.selected_mode.as_kanshi_mode())
                            .show_ui(ui, |ui| {
                                let modes = runtime_modes
                                    .get(&screen.id)
                                    .cloned()
                                    .or_else(|| runtime_modes.get(&screen.connector_name).cloned())
                                    .unwrap_or_else(|| screen.available_modes.clone());
                                for mode in modes {
                                    let selected = mode == screen.selected_mode;
                                    if ui
                                        .selectable_label(selected, mode.as_kanshi_mode())
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

                    // Add small vertical gap and separator unless last
                    if idx != last {
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(gap - 4.0);
                    }
                }
            }
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
                ui.label(format!("Works as expected? Reset in {}s", remaining));
                ui.horizontal(|ui| {
                    if ui.button("Yes").clicked() {
                        // confirm: stop pending confirmation
                        app.state.pending_confirmation = false;
                        app.state.confirm_deadline = None;
                        app.state.previous_config_contents = None;
                        app.state.status = format!(
                            "Profile '{}' confirmed",
                            app.state.current_profile_name.trim()
                        );
                    }
                    if ui.button("No").clicked() {
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
                ui.label("KanshiUI will manage display configuration using a dedicated file and a user systemd service.");
                ui.label("Your existing kanshi configuration will not be modified, but it will not be used while KanshiUI manages output profiles.");
                ui.label("If you only want to temporarily adjust your screens, this is fine — KanshiUI keeps backups and lets you restore the previous configuration.");
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
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

pub fn render_output_identify_overlays(
    ctx: &egui::Context,
    outputs: &[RuntimeOutput],
    last_reposition: &mut Option<Instant>,
) {
    let now = Instant::now();
    // Reposition overlays only once when identify mode is turned on.
    // Repeated external calls can stall the UI.
    let should_reposition = last_reposition.is_none();

    for output in outputs {
        if output.layout_width <= 0 || output.layout_height <= 0 {
            continue;
        }

        let viewport_id = egui::ViewportId::from_hash_of((
            "output-ident",
            output.connector_name.as_str(),
            output.layout_x,
            output.layout_y,
        ));

        let title = format!("Identify {}", output.connector_name);
        // Convert compositor physical coordinates to egui logical points
        // so the builder and viewport commands use the coordinate space the
        // egui/eframe backend expects. This uses the current ctx pixels_per_point.
        let ppp = ctx.pixels_per_point();
        let phys_x = output.layout_x as f32 + 24.0;
        let phys_y = output.layout_y as f32 + 24.0;
        let logical_pos = Pos2::new(phys_x / ppp, phys_y / ppp);

        let mut builder = egui::ViewportBuilder::default();
        builder = builder
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(true)
            // Set initial builder position in logical points.
            .with_position(logical_pos)
            .with_inner_size(Vec2::new(500.0, 180.0))
            .with_title(title);

        let display_name = output.display_id();
        let connector = output.connector_name.clone();
        let mode = output
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
            .cloned();

        let mode_label = mode
            .map(|m| m.as_kanshi_mode())
            .unwrap_or_else(|| "unknown mode".to_string());

        ctx.show_viewport_immediate(viewport_id, builder, move |ctx, _class| {
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
                        display_name.clone(),
                        FontId::proportional(24.0),
                        Color32::WHITE,
                    );
                    ui.painter().text(
                        rect.left_top() + Vec2::new(18.0, 94.0),
                        Align2::LEFT_TOP,
                        format!("Connection: {connector}"),
                        FontId::proportional(18.0),
                        Color32::from_rgb(199, 219, 241),
                    );
                    ui.painter().text(
                        rect.left_top() + Vec2::new(18.0, 122.0),
                        Align2::LEFT_TOP,
                        format!(
                            "Current scale: {}  |  Best mode: {}",
                            trim_float(output.current_scale),
                            mode_label
                        ),
                        FontId::proportional(16.0),
                        Color32::from_rgb(176, 198, 223),
                    );
                });
        });

        // Request the native window be moved to the desired global position.
        // `send_viewport_cmd_to` is used rather than spawning swaymsg so we
        // don't block the UI thread. Only send the command periodically to
        // avoid flooding the platform with repeated commands.
        if should_reposition {
            // Try native viewport move first.
            let _ = ctx.send_viewport_cmd_to(
                viewport_id,
                egui::ViewportCommand::OuterPosition(logical_pos),
            );
        }
    }

    if should_reposition {
        *last_reposition = Some(now);
    }
}

fn move_overlay_to_output(connector_name: &str) {
    let title = format!("Identify {connector_name}");
    let criteria = format!("[title=\"{}\"]", title.replace('"', "\\\""));
    let command =
        format!("floating enable, move window to output {connector_name}, move position 24 24");
    let _ = Command::new("swaymsg")
        .arg(criteria)
        .arg(command)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub fn schedule_overlay_reposition(connectors: Vec<String>) {
    if connectors.is_empty() {
        return;
    }

    std::thread::spawn(move || {
        // Delay slightly so overlay windows have time to be created, then
        // retry a few times in case the compositor maps them asynchronously.
        std::thread::sleep(Duration::from_millis(80));
        for _ in 0..3 {
            for connector in &connectors {
                move_overlay_to_output(connector);
            }
            std::thread::sleep(Duration::from_millis(80));
        }
    });
}

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
    let visible_indices: Vec<usize> = (0..profile.screens.len())
        .filter(|&i| !profile.screens[i].mirror)
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

        let fill = if screen.enabled {
            Color32::from_rgb(42, 66, 100)
        } else {
            Color32::from_rgb(65, 65, 65)
        };
        painter.rect_filled(screen_rect, 0.0, fill);
        painter.rect_stroke(
            screen_rect,
            0.0,
            Stroke::new(1.0, Color32::from_rgb(175, 201, 232)),
            StrokeKind::Inside,
        );

        // Show label including effective virtual resolution to help the user
        // reason about scaling.
        let s = screen.scale as f32;
        let virtual_w = (screen.selected_mode.width as f32 / s) as u32;
        let virtual_h = (screen.selected_mode.height as f32 / s) as u32;
        let screen_label = if screen.id == screen.connector_name {
            format!(
                "{}\n{}x{}@{}Hz\nscale {}",
                screen.id,
                virtual_w,
                virtual_h,
                trim_float(screen.selected_mode.refresh_hz),
                trim_float(screen.scale)
            )
        } else {
            format!(
                "{}\n{}\n{}x{}@{}Hz scale {}",
                screen.id,
                screen.connector_name,
                virtual_w,
                virtual_h,
                trim_float(screen.selected_mode.refresh_hz),
                trim_float(screen.scale)
            )
        };

        painter.text(
            screen_rect.center(),
            Align2::CENTER_CENTER,
            screen_label,
            FontId::proportional(13.0),
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
fn consider_axis_candidate(
    _current_edge: i32,
    _other_edge: i32,
    _threshold: i32,
    _best: &mut Option<(i32, i32)>,
) {
    // kept for compatibility but snapping was removed; no-op
}
