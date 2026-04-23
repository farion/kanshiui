use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use regex::Regex;
use tempfile::NamedTempFile;

use crate::model::{trim_float, OutputMode, Profile, ScreenConfig};

pub fn kanshi_config_path() -> Result<PathBuf> {
    // Use a dedicated config file for the UI-managed config. The filename is
    // "kanshiui" inside XDG_CONFIG_HOME or ~/.config.
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("kanshiui"));
    }
    let home = dirs::home_dir().context("unable to determine home directory")?;
    Ok(home.join(".config").join("kanshiui"))
}

pub fn load_profiles(path: &Path) -> Result<(String, Vec<Profile>)> {
    if !path.exists() {
        return Ok((String::new(), Vec::new()));
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read kanshi config at {}", path.display()))?;
    let profiles = parse_profiles(&content)?;
    Ok((content, profiles))
}

pub fn parse_profiles(content: &str) -> Result<Vec<Profile>> {
    let mut profiles = Vec::new();
    let output_aliases = parse_output_aliases(content)?;
    let profile_re = Regex::new(r#"(?m)^\s*profile\s+(["'][^"']+["'])\s*\{"#)?;

    for caps in profile_re.captures_iter(content) {
        let full = if let Some(m) = caps.get(0) {
            m
        } else {
            continue;
        };
        let start = full.start();
        let name_token = caps.get(1).map(|m| m.as_str()).unwrap_or("\"Unnamed\"");
        let name = unquote(name_token);
        let brace_idx = full.as_str().rfind('{').map(|i| start + i).unwrap_or(start);
        if let Some(end) = find_matching_brace(content, brace_idx) {
            let body = &content[(brace_idx + 1)..end];
            let screens = parse_profile_body(body, &output_aliases)?;
            profiles.push(Profile {
                name,
                screens,
                raw_range: Some((start, end + 1)),
            });
        }
    }
    Ok(profiles)
}

pub fn upsert_profile(path: &Path, profile: &Profile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir {}", parent.display()))?;
    }

    let existing = if path.exists() {
        fs::read_to_string(path)
            .with_context(|| format!("failed to read existing config at {}", path.display()))?
    } else {
        String::new()
    };

    let parsed = parse_profiles(&existing)?;
    let replacement = generate_profile(profile);

    let mut next = existing.clone();
    if let Some(old) = parsed.into_iter().find(|p| p.name == profile.name) {
        if let Some((start, end)) = old.raw_range {
            next.replace_range(start..end, &replacement);
        } else {
            if !next.ends_with('\n') && !next.is_empty() {
                next.push('\n');
            }
            next.push_str(&replacement);
        }
    } else {
        if !next.ends_with('\n') && !next.is_empty() {
            next.push('\n');
        }
        next.push_str(&replacement);
    }

    backup_existing(path)?;
    atomic_write(path, &next)
}

/// Replace the config file contents with the provided contents (atomic write
/// with backup). Used to restore a previous config when an applied profile is
/// rejected by the user.
pub fn replace_config(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir {}", parent.display()))?;
    }
    backup_existing(path)?;
    atomic_write(path, contents)
}

pub fn generate_profile(profile: &Profile) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "profile \"{}\"{{\n",
        escape_double_quotes(&profile.name)
    ));
    // Build a lookup of screens by id to resolve mirror targets without
    // re-borrowing the profile during iteration.
    let mut by_id: HashMap<&str, &ScreenConfig> = HashMap::new();
    for s in &profile.screens {
        by_id.insert(s.id.as_str(), s);
    }

    for screen in &profile.screens {
        let id = format_output_id(&screen.id);
        let state = if screen.enabled { "enable" } else { "disable" };

        // If this screen is mirroring another and a valid target is set,
        // compute a scale and position so the mirrored content fits centered
        // inside the target's visible area while preserving aspect ratio.
        if screen.mirror {
            if let Some(ref target_id) = screen.mirror_target {
                if let Some(target) = by_id.get(target_id.as_str()) {
                    // Physical sizes (pixels)
                    let t_w = target.selected_mode.width as f64;
                    let t_h = target.selected_mode.height as f64;
                    let s_w = screen.selected_mode.width as f64;
                    let s_h = screen.selected_mode.height as f64;

                    // Target virtual sizes = physical / target.scale
                    let t_scale = if target.scale == 0.0 {
                        1.0
                    } else {
                        target.scale
                    };
                    let t_vw = t_w / t_scale;
                    let t_vh = t_h / t_scale;

                    // Choose mirror scale so the mirror's virtual size fits
                    // inside the target virtual size: s_w / m <= t_vw -> m >= s_w / t_vw
                    let scale_w = s_w / t_vw;
                    let scale_h = s_h / t_vh;
                    let mirror_scale = scale_w.max(scale_h).max(0.0001);

                    // The mirror's virtual size will be s_w / mirror_scale. To
                    // convert that into physical pixels inside the target's
                    // compositor space, multiply by the target's scale.
                    let mirror_physical_w = (s_w / mirror_scale) * t_scale;
                    let mirror_physical_h = (s_h / mirror_scale) * t_scale;

                    // Center the mirrored content inside the target in physical
                    // compositor pixels.
                    let target_center_x = target.pos_x as f64 + t_w / 2.0;
                    let target_center_y = target.pos_y as f64 + t_h / 2.0;
                    let mirror_pos_x = (target_center_x - mirror_physical_w / 2.0).round() as i32;
                    let mirror_pos_y = (target_center_y - mirror_physical_h / 2.0).round() as i32;

                    // Emit a kanshiui comment so the editor can recover mirror
                    // metadata later. This is ignored by kanshi but parsed by
                    // the UI when loading profiles.
                    out.push_str(&format!(
                        "  # kanshiui: mirror id='{}' target='{}'\n",
                        screen.id.replace('"', "'"),
                        target_id.replace('"', "'"),
                    ));
                    out.push_str(&format!(
                        "  output {id} {state} mode {} position {},{} scale {}\n",
                        screen.selected_mode.as_kanshi_mode(),
                        mirror_pos_x,
                        mirror_pos_y,
                        trim_float(mirror_scale),
                    ));
                    continue;
                }
            }
        }

        // Fallback: write the stored position/scale
        out.push_str(&format!(
            "  output {id} {state} mode {} position {},{} scale {}\n",
            screen.selected_mode.as_kanshi_mode(),
            screen.pos_x,
            screen.pos_y,
            trim_float(screen.scale),
        ));
    }
    out.push_str(&format!(
        "  exec notify-send \"{}\"\n",
        escape_double_quotes(&profile.name)
    ));
    out.push_str("}\n");
    out
}

pub fn screen_multiset(screens: &[ScreenConfig]) -> HashMap<String, usize> {
    let mut set = HashMap::new();
    for screen in screens {
        *set.entry(screen.id.clone()).or_insert(0) += 1;
    }
    set
}

fn parse_profile_body(body: &str, aliases: &HashMap<String, String>) -> Result<Vec<ScreenConfig>> {
    let inline_re = Regex::new(
        r#"^\s*output\s+('[^']+'|\"[^\"]+\"|[^\s]+)\s+(enable|disable)(?:\s+mode\s+(\d+)x(\d+)(?:@([0-9.]+)Hz)?)?(?:\s+position\s+(-?\d+),(-?\d+))?(?:\s+scale\s+([0-9.]+))?\s*$"#,
    )?;

    let mut screens = Vec::new();

    // Parse kanshiui-specific comments that persist UI-only metadata such as
    // mirror settings. Comment format:
    //   # kanshiui: mirror id='SCREEN_ID' target='TARGET_ID'
    let mirror_re =
        Regex::new(r#"^\s*#\s*kanshiui:\s*mirror\s+id='([^']+)'\s+target='([^']+)'\s*$"#)?;
    let mut mirror_map: HashMap<String, String> = HashMap::new();
    for line in body.lines() {
        if let Some(caps) = mirror_re.captures(line) {
            let id = caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
            let target = caps.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
            if !id.is_empty() && !target.is_empty() {
                mirror_map.insert(id, target);
            }
        }
    }

    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("exec ") {
            i += 1;
            continue;
        }
        if let Some(caps) = inline_re.captures(line) {
            if let Some(mut screen) = caps_to_screen(&caps, aliases) {
                // Apply kanshiui mirror metadata when present for inline
                // output lines as well.
                if let Some(t) = mirror_map.get(&screen.id) {
                    screen.mirror = true;
                    screen.mirror_target = Some(t.clone());
                }
                screens.push(screen);
            }
            i += 1;
            continue;
        }

        if line.starts_with("output ") && line.ends_with('{') {
            let output_id_token = line
                .trim_end_matches('{')
                .trim()
                .strip_prefix("output")
                .map(str::trim)
                .unwrap_or("");
            let output_id = resolve_alias(&unquote(output_id_token), aliases);
            let mut enabled = true;
            let mut mode = OutputMode {
                width: 1920,
                height: 1080,
                refresh_hz: 60.0,
            };
            let mut pos_x = 0;
            let mut pos_y = 0;
            let mut scale = 1.0;

            i += 1;
            while i < lines.len() {
                let inner = lines[i].trim();
                if inner == "}" {
                    break;
                }
                if inner == "enable" || inner == "enabled yes" {
                    enabled = true;
                } else if inner == "disable" || inner == "enabled no" {
                    enabled = false;
                } else if let Some(rem) = inner.strip_prefix("mode ") {
                    if let Some(parsed) = parse_mode_string(rem) {
                        mode = parsed;
                    }
                } else if let Some(rem) = inner.strip_prefix("position ") {
                    if let Some((x, y)) = parse_position(rem) {
                        pos_x = x;
                        pos_y = y;
                    }
                } else if let Some(rem) = inner.strip_prefix("pos ") {
                    if let Some((x, y)) = parse_space_position(rem) {
                        pos_x = x;
                        pos_y = y;
                    }
                } else if let Some(rem) = inner.strip_prefix("scale ") {
                    if let Ok(v) = rem.trim().parse::<f64>() {
                        scale = v;
                    }
                }
                i += 1;
            }
            let mut mirror = false;
            let mut mirror_target = None;
            if let Some(t) = mirror_map.get(&output_id) {
                mirror = true;
                mirror_target = Some(t.clone());
            }
            screens.push(ScreenConfig {
                id: output_id.clone(),
                connector_name: output_id,
                enabled,
                selected_mode: mode.clone(),
                available_modes: vec![mode],
                scale,
                pos_x,
                pos_y,
                mirror,
                mirror_target,
            });
        }
        i += 1;
    }

    Ok(screens)
}

fn caps_to_screen(
    caps: &regex::Captures<'_>,
    aliases: &HashMap<String, String>,
) -> Option<ScreenConfig> {
    let id = resolve_alias(&unquote(caps.get(1)?.as_str()), aliases);
    let enabled = caps.get(2)?.as_str() == "enable";
    let width = caps
        .get(3)
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .unwrap_or(1920);
    let height = caps
        .get(4)
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .unwrap_or(1080);
    let hz = caps
        .get(5)
        .and_then(|m| m.as_str().parse::<f64>().ok())
        .unwrap_or(60.0);
    let x = caps
        .get(6)
        .and_then(|m| m.as_str().parse::<i32>().ok())
        .unwrap_or(0);
    let y = caps
        .get(7)
        .and_then(|m| m.as_str().parse::<i32>().ok())
        .unwrap_or(0);
    let scale = caps
        .get(8)
        .and_then(|m| m.as_str().parse::<f64>().ok())
        .unwrap_or(1.0);

    let mode = OutputMode {
        width,
        height,
        refresh_hz: hz,
    };
    Some(ScreenConfig {
        id: id.clone(),
        connector_name: id,
        enabled,
        selected_mode: mode.clone(),
        available_modes: vec![mode],
        scale,
        pos_x: x,
        pos_y: y,
        mirror: false,
        mirror_target: None,
    })
}

fn parse_output_aliases(content: &str) -> Result<HashMap<String, String>> {
    let mut aliases = HashMap::new();
    let profile_start_re = Regex::new(r#"^\s*profile(?:\s+(["'][^"']+["']))?\s*\{"#)?;
    let output_block_re = Regex::new(r#"^\s*output\s+('[^']+'|\"[^\"]+\"|[^\s]+)\s*\{\s*$"#)?;
    let output_inline_alias_re = Regex::new(
        r#"^\s*output\s+('[^']+'|\"[^\"]+\"|[^\s]+).*(?:^|\s)alias\s+(\$[A-Za-z0-9._-]+)(?:\s|$)"#,
    )?;
    let alias_line_re = Regex::new(r#"^\s*alias\s+(\$[A-Za-z0-9._-]+)\s*$"#)?;

    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        if line.is_empty() || line.starts_with('#') || line.starts_with("include ") {
            i += 1;
            continue;
        }

        if profile_start_re.is_match(line) {
            i += 1;
            let mut depth = 1_i32;
            while i < lines.len() && depth > 0 {
                let l = lines[i];
                depth += l.matches('{').count() as i32;
                depth -= l.matches('}').count() as i32;
                i += 1;
            }
            continue;
        }

        if let Some(caps) = output_block_re.captures(line) {
            let criteria = caps.get(1).map(|m| unquote(m.as_str())).unwrap_or_default();
            i += 1;
            while i < lines.len() {
                let inner = lines[i].trim();
                if let Some(alias_caps) = alias_line_re.captures(inner) {
                    if let Some(alias_name) = alias_caps.get(1) {
                        aliases.insert(alias_name.as_str().to_string(), criteria.clone());
                    }
                }
                if inner == "}" {
                    break;
                }
                i += 1;
            }
            i += 1;
            continue;
        }

        if let Some(caps) = output_inline_alias_re.captures(line) {
            if let (Some(criteria), Some(alias)) = (caps.get(1), caps.get(2)) {
                aliases.insert(alias.as_str().to_string(), unquote(criteria.as_str()));
            }
        }

        i += 1;
    }

    Ok(aliases)
}

fn resolve_alias(criteria: &str, aliases: &HashMap<String, String>) -> String {
    aliases
        .get(criteria)
        .cloned()
        .unwrap_or_else(|| criteria.to_string())
}

fn parse_mode_string(s: &str) -> Option<OutputMode> {
    // Accept both "WIDTHxHEIGHT" and "WIDTHxHEIGHT@REFHz" where the @REFHz part is optional.
    let re = Regex::new(r"^(\d+)x(\d+)(?:@([0-9.]+)Hz)?$").ok()?;
    let caps = re.captures(s.trim())?;
    let width = caps.get(1)?.as_str().parse().ok()?;
    let height = caps.get(2)?.as_str().parse().ok()?;
    let refresh_hz = caps
        .get(3)
        .and_then(|m| m.as_str().parse::<f64>().ok())
        .unwrap_or(60.0);
    Some(OutputMode {
        width,
        height,
        refresh_hz,
    })
}

fn parse_position(s: &str) -> Option<(i32, i32)> {
    let mut parts = s.split(',');
    let x = parts.next()?.trim().parse().ok()?;
    let y = parts.next()?.trim().parse().ok()?;
    Some((x, y))
}

fn parse_space_position(s: &str) -> Option<(i32, i32)> {
    let mut parts = s.split_whitespace();
    let x = parts.next()?.trim().parse().ok()?;
    let y = parts.next()?.trim().parse().ok()?;
    Some((x, y))
}

fn find_matching_brace(content: &str, open_brace_idx: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut depth = 0_usize;
    for (idx, b) in bytes.iter().enumerate().skip(open_brace_idx) {
        if *b == b'{' {
            depth += 1;
        } else if *b == b'}' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(idx);
            }
        }
    }
    None
}

fn format_output_id(id: &str) -> String {
    let connector_like = id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if connector_like {
        id.to_string()
    } else {
        format!("'{}'", id.replace('\'', "\\'"))
    }
}

fn escape_double_quotes(s: &str) -> String {
    s.replace('"', "\\\"")
}

fn unquote(input: &str) -> String {
    let s = input.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len().saturating_sub(1)].to_string()
    } else {
        s.to_string()
    }
}

fn backup_existing(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let ts = format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
    let backup = PathBuf::from(format!("{}.bak.{}", path.display(), ts));
    fs::copy(path, &backup)
        .with_context(|| format!("failed to create backup {}", backup.display()))?;
    Ok(())
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .context("missing parent directory for config path")?;
    let mut temp = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temporary file in {}", parent.display()))?;
    use std::io::Write;
    temp.write_all(content.as_bytes())?;
    temp.flush()?;
    temp.persist(path)
        .map_err(|e| anyhow::anyhow!("failed to persist temp file: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inline_profile() {
        let input = r#"profile "Office"{
  output eDP-1 enable mode 3840x2400@59.994Hz position 0,600 scale 2
  output 'Dell Inc. DELL P2723D HJPV1L3' enable mode 2560x1440@60Hz position 4480,0 scale 1
  exec notify-send "Office"
}"#;
        let profiles = parse_profiles(input).expect("profiles parsed");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].screens.len(), 2);
        assert_eq!(profiles[0].screens[0].selected_mode.width, 3840);
    }

    #[test]
    fn parses_global_inline_alias_in_profile() {
        let input = r#"output eDP-1 alias $input
profile "Office"{
  output $input enable mode 1920x1080@60Hz position 0,0 scale 1
}"#;
        let profiles = parse_profiles(input).expect("profiles parsed");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].screens[0].id, "eDP-1");
    }

    #[test]
    fn parses_global_block_alias_in_profile() {
        let input = r#"output "Dell Inc. DELL P2723D HJPV1L3" {
  alias $desk
}
profile "Office"{
  output $desk enable mode 2560x1440@60Hz position 0,0 scale 1
}"#;
        let profiles = parse_profiles(input).expect("profiles parsed");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].screens[0].id, "Dell Inc. DELL P2723D HJPV1L3");
    }

    #[test]
    fn generates_inline_profile() {
        let profile = Profile {
            name: "Office".to_string(),
            screens: vec![ScreenConfig {
                id: "eDP-1".to_string(),
                connector_name: "eDP-1".to_string(),
                enabled: true,
                selected_mode: OutputMode {
                    width: 1920,
                    height: 1080,
                    refresh_hz: 60.0,
                },
                available_modes: vec![],
                scale: 1.0,
                pos_x: 0,
                pos_y: 0,
                mirror: false,
                mirror_target: None,
            }],
            raw_range: None,
        };
        let out = generate_profile(&profile);
        assert!(out.contains("exec notify-send \"Office\""));
        assert!(out.contains("output eDP-1 enable mode 1920x1080@60Hz position 0,0 scale 1"));
    }
}
