Kanshi GUI SPEC
================

Purpose
-------
Provide a GUI application (Rust + egui) that manages kanshi 1.7+ profiles for sway. The app:

- Queries sway for connected outputs (via swayipc).
- Presents screens graphically on a canvas as draggable rectangles.
- Shows per-screen overlays with identifying information (full make+model+serial and connector name).
- Allows enable/disable, mode selection (resolution+refresh), scale, and free-position arrangement with optional snap-to-grid.
- Detects and loads an existing kanshi profile that matches the currently connected set of physical displays.
- Saves named setups (profiles) into $XDG_CONFIG_HOME/kanshi/default (fallback ~/.config/kanshi/default) in inline format and injects an exec notify-send "{profile name}" into the profile block.
- Restarts kanshi using `systemctl --user restart kanshi.service` (preferred) with a fallback to kill+spawn if service is missing.

Key design decisions
--------------------

- Kanshi version: 1.7.0+ (profile and inline-style output lines supported).
- Restart: use `systemctl --user restart kanshi.service`; fallback to pkill + spawn kanshi if unit missing.
- Mode selection default: the "best available" mode per screen = mode with largest area (width*height); if tie, pick highest refresh.
- Default scale for new profiles/screens: 1.0.
- Profile generation format: inline single-line output entries (matching examples provided) by default.
- Profile injection: append `exec notify-send "{profile name}"` inside the profile block. The app will also optionally run notify-send locally for immediate feedback.
- Identifiers: screens are identified by full make+model+serial when possible (e.g., "Dell Inc. DELL P2723D HJPV1L3"); if not available, fall back to connector name (e.g., eDP-1).

Repository layout (recommended)
------------------------------

Cargo.toml

src/
- main.rs          - eframe bootstrap
- app.rs           - egui App implementation and top-level state
- ui.rs            - UI helpers (canvas, sidebars, dialogs)
- model.rs         - data model types: RuntimeOutput, OutputMode, ScreenConfig, Profile
- sway.rs          - swayipc wrapper: rescan outputs, mode list, conversion helpers
- kanshi_config.rs - parser & generator + safe write and backups
- kanshi_restart.rs- systemd restart + fallback
- notify.rs        - helper for notify-send
- tests/           - unit tests for parser & matching

Data model
----------

RuntimeOutput
- connector_name: String (sway name, e.g. "DP-1")
- make: Option<String>
- model: Option<String>
- serial: Option<String>
- modes: Vec<OutputMode>
- current_mode: Option<OutputMode>
- current_scale: f64
- active: bool

OutputMode
- width: u32
- height: u32
- refresh: f64

ScreenConfig (per-profile)
- id: String               // canonical identifier (make+model[ serial] or connector)
- connector_name: String   // sway connector name (useful for unquoted output names)
- enabled: bool
- selected_mode: OutputMode
- available_modes: Vec<OutputMode>
- scale: f64
- pos_x: i32
- pos_y: i32

Profile
- name: String
- screens: Vec<ScreenConfig>
- raw_text_range: Option<Range<usize>>  // region in the original file if parsed from disk

Canonical identifier rule
-------------------------

- If make and model present: identifier = "<make> <model>" and append serial if present (space-separated).
  Example: "Dell Inc. DELL P2723D HJPV1L3".
- If make/model absent: fallback to connector name such as "DP-1" (unquoted in generated config).

Quoting rules when generating output lines
----------------------------------------

- If identifier matches the regex ^[A-Za-z0-9_-]+$ (connector-like), emit it unquoted (e.g., eDP-1).
- Otherwise single-quote the identifier (e.g., 'Dell Inc. DELL P2723D HJPV1L3'). Escape single quotes inside identifiers.

Kanshi config parsing and generation
-----------------------------------

File
- Path: $XDG_CONFIG_HOME/kanshi/default, fallback: ~/.config/kanshi/default.

Parsing goals
- Extract profile blocks (profile "Name" { ... }).
- Support both inline-style output declarations and block-style declarations.
- For each output capture: identifier, enabled/disabled state, mode (WxH@Hz), position (X,Y), scale.
- Keep original file content and the start/end byte ranges of each parsed profile block so we can replace only that block when updating.

Parsing approach
- Read the full file into a String.
- Locate profile start tokens (regex to match profile followed by brace). For each, find matching closing brace by scanning and counting braces — that yields block ranges.
- Inside a profile block, find output declarations with regexes that match both inline and block styles. Parse allowed fields leniently (missing fields are allowed).

Generation approach
- Generate inline-style profile blocks by default:

  profile "Office MA 2"{
    output eDP-1 enable mode 3840x2400@59.994Hz position 0,600 scale 2
    output 'Dell Inc. DELL P2723D HJPV1L3' enable mode 2560x1440@60Hz position 4480,0 scale 1
    exec notify-send "Office MA 2"
  }

- The generator will place an `exec notify-send "{profile name}"` line within the profile block.
- When replacing a parsed profile, replace the substring for that profile block. When appending a new profile, append at the end of the file separated by newlines.

File write safety
- Make a backup copy before overwrite: <path>.bak.<timestamp>.
- Write new content to a temporary file in same directory then rename into place (atomic replace).

Mode selection algorithm
------------------------

- Best default mode per-screen: pick mode with maximum area (width*height); tiebreaker: highest refresh.
- Present all available modes in the UI per-screen so user can override.

Matching profiles to current outputs
----------------------------------

- Build the current multiset of canonical identifiers from sway outputs (include duplicates if multiple identical displays present).
- For each profile parsed from the file build the multiset of identifiers present in that profile.
- A profile matches iff multisets are equal (same identifiers and same counts).
- If multiple matches exist prefer the most recently modified by default; otherwise present candidates to the user.
- If identifiers are ambiguous (identical devices without serials), show a clear UI warning and require user confirmation for assignment when necessary.

UI (eframe/egui)
----------------

Main areas
- Top bar: actions (Rescan, Load config, Settings, Apply, Save as).
- Canvas: large interactive canvas that renders screen rectangles to scale (proportional to selected mode) and supports pan/zoom.
- Per-screen overlays: anchored UI panels for each screen showing identification and quick controls.
- Right sidebar: inspector for the selected screen or listing of saved profiles and Save/Apply controls.
- Status bar: last action result and quick messages.

Canvas & interactions
- Draw each ScreenConfig as a rectangle sized proportional to its selected_mode (scaled to canvas coordinates).
- Dragging a rectangle updates pos_x/pos_y (snap-to-grid optional).
- Zoom/pan supported for large virtual desktops.

Overlays (per-screen)
- Compact default overlay (one-line) visible for all screens by default (configurable):
  - Primary label: canonical identifier (make/model/serial) or connector name if missing.
  - Secondary label: connector name in parentheses (e.g., "(eDP-1)").
- Expanded overlay (on hover or pinned):
  - Selected mode (WxH@Hz) and available modes count (click to expand list).
  - Scale (quick +/- and text input).
  - Enabled toggle.
  - Quick coordinates live during drag.
  - Pin/unpin control.
  - Edit action to open full sidebar inspector for detailed editing.
- Overlays are anchored to the top-left of the screen rect with flip behavior to keep them visible inside canvas bounds.
- Pinned overlays remain visible and move with their screen during drag.

Accessibility & UX
- Hover delay and collapse timings configurable.
- Keyboard navigation: overlays focusable and operable via keyboard (tab, Enter toggles enable, Space opens edit).

Restarting kanshi & notifications
---------------------------------

Apply flow
1. Save profile into kanshi/default (replace or append). Backup created first.
2. Attempt to restart kanshi with `systemctl --user restart kanshi.service`.
3. If systemctl is unavailable or unit not found, fallback to:
   - `pkill -TERM -x kanshi` (graceful stop), short sleep, then spawn `kanshi` detached.
4. Inside the generated profile block there is `exec notify-send "{profile name}"` so kanshi will notify when it activates that profile.
5. Optionally the app can also run `notify-send` locally for immediate feedback — configurable.

Error handling
- If restart fails: show a detailed UI error and keep a copy of the generated config for inspection.
- If config write fails due to permissions: show error and do not attempt restart.
- If notify-send missing: warn user; still write profile but injected exec will fail until notify-send is installed.

APIs and function signatures (suggested)
--------------------------------------

// sway.rs
pub fn rescan_outputs() -> anyhow::Result<Vec<RuntimeOutput>>;
pub fn default_screen_config(ro: &RuntimeOutput) -> ScreenConfig;

// kanshi_config.rs
pub fn load_profiles(path: &Path) -> anyhow::Result<(String /*full contents*/, Vec<Profile>)>;
pub fn write_profile_replace_or_append(path: &Path, orig_content: &str, profile: &Profile, profiles: &Vec<Profile>) -> anyhow::Result<()>;

// matching
pub fn match_profile_for_current(current_ids: &Vec<String>, profiles: &Vec<Profile>) -> MatchResult;

// restart
pub fn restart_kanshi(preferred: RestartPreference) -> anyhow::Result<()>;

Development & testing plan
--------------------------

Unit tests
- Parser round-trip tests: parse sample profile strings (inline + block), then generate and parse again.
- Matching tests: sets with serials, without serials, duplicates.

Manual/integration tests
- Run app with real multi-monitor setups.
- Test saving, applying, and kanshi restart via systemd and fallback.

Implementation milestones (MVP)
----------------------------

Phase 1 (foundation)
- Project skeleton, Cargo.toml, eframe app skeleton.
- Implement sway rescan and model.
- Basic canvas: draw rectangles, drag, and little overlays (compact info).

Phase 2 (config & matching)
- Implement kanshi parser/generator and matching logic.
- Load profiles from disk and show matched profile in UI.
- Implement Save (write profile, backup).

Phase 3 (apply & restart)
- Implement kanshi restart via systemctl and fallback.
- Inject exec notify-send into generated profile.
- Hook Apply button to write + restart.

Phase 4 (polish)
- Per-screen overlays with full controls (pin, mode dropdown, scale input).
- Snap-to-grid, auto-arrange, pan/zoom improvements.
- Tests and documentation.

Edge cases & known limitations
----------------------------

- Multiple identical displays with no serial: matching is done by multiset count. The UI will warn about ambiguous mappings and require manual confirmation.
- When replacing a profile we replace the whole profile block; unknown inner attributes inside that profile block will be lost. The rest of the file is preserved. Consider adding preservation of unknown inner lines in a future revision.
- If kanshi refuses to start due to invalid config, the app will report the failure; the user must inspect and fix the generated profile (the app keeps backups).

Next steps
----------

When ready proceed to implementation (create repo files, implement modules, and tests). Follow the milestones above. Use the function signatures and file layout in this SPEC as the blueprint.

Contact / decisions pending
-------------------------

- Confirm whether the app should also run notify-send locally in addition to injecting exec into the profile (configurable in settings). By default the SPEC recommends both.
- Confirm systemd unit name: `kanshi.service` will be used; fallback may try `kanshi` without ".service" if necessary.
