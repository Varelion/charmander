use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

mod typer;

use eframe::egui;
use egui::{Color32, FontData, FontDefinitions, FontFamily, Key, RichText};

include!(concat!(env!("OUT_DIR"), "/characters.rs"));

// Fallback font families, resolved through fontconfig, so emoji/symbol
// codepoints render as a glyph instead of a blank box.
const FALLBACK_FONT_FAMILIES: &[&str] =
    &["Symbola", "Noto Sans Symbols2", "Noto Sans Symbols", "Noto Color Emoji"];
const MAX_RESULTS: usize = 200;
// Favorites are stored as the hex codepoints of an entry, space separated
// for sequences: "1F914" or "1F468 200D 1F9B0".
const CODEPOINT_SEPARATOR: &str = " ";

fn codepoint_key(glyph: &str) -> String {
    glyph
        .chars()
        .map(|c| format!("{:04X}", c as u32))
        .collect::<Vec<_>>()
        .join(CODEPOINT_SEPARATOR)
}

fn codepoint_label(glyph: &str) -> String {
    glyph
        .chars()
        .map(|c| format!("U+{:04X}", c as u32))
        .collect::<Vec<_>>()
        .join(CODEPOINT_SEPARATOR)
}

// Look of the picker. Loaded once at startup, in priority order:
//   1. ~/.config/charmander/theme.conf   (user override, always wins)
//   2. ~/.config/alacritty/alacritty.toml (auto-detected from the terminal)
//   3. the built-in defaults below
#[derive(Clone)]
struct Theme {
    bg: Color32,
    fg: Color32,
    accent: Color32,
    dim: Color32,
    font_family: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: Color32::from_rgb(0x12, 0x08, 0x1a),
            fg: Color32::from_rgb(0xff, 0xa0, 0x4d),
            accent: Color32::from_rgb(0xff, 0x75, 0x18),
            dim: Color32::from_rgb(0x4a, 0x3b, 0x52),
            font_family: "monospace".to_owned(),
        }
    }
}

fn config_dir() -> PathBuf {
    home_config("charmander")
}

// Config root per platform: XDG_CONFIG_HOME or ~/.config on unix, %APPDATA%
// on Windows (where HOME is normally unset). Never panics - every caller
// treats missing config files as "use defaults".
fn config_root() -> PathBuf {
    let non_empty = |key| std::env::var(key).ok().filter(|value| !value.is_empty());

    if let Some(xdg) = non_empty("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg);
    }
    if let Some(appdata) = non_empty("APPDATA") {
        return PathBuf::from(appdata);
    }
    let home = non_empty("HOME").or_else(|| non_empty("USERPROFILE")).unwrap_or_default();
    PathBuf::from(home).join(".config")
}

fn home_config(relative: &str) -> PathBuf {
    config_root().join(relative)
}

// Accepts the forms terminal configs actually use: #rrggbb, rrggbb,
// and GNOME's rgb(r,g,b).
fn parse_hex_color(value: &str) -> Option<Color32> {
    let value = strip_value(value);

    if let Some(channels) = value.strip_prefix("rgb(").and_then(|rest| rest.strip_suffix(')')) {
        let mut parts = channels.split(',').map(|part| part.trim().parse::<u8>().ok());
        return Some(Color32::from_rgb(parts.next()??, parts.next()??, parts.next()??));
    }

    let hex = value.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).ok();
    Some(Color32::from_rgb(channel(0..2)?, channel(2..4)?, channel(4..6)?))
}

// Terminal font settings often carry a point size ("Fira Code 11"); egui
// wants the family alone.
fn strip_font_size(family: &str) -> String {
    let family = strip_value(family);
    match family.rsplit_once(' ') {
        Some((head, tail)) if tail.parse::<f32>().is_ok() => head.to_owned(),
        _ => family.to_owned(),
    }
}

// Picks the 9th entry (bright black) out of a delimited palette list.
fn palette_bright_black(list: &str, delimiter: char) -> Option<Color32> {
    list.trim_matches(|c| c == '[' || c == ']')
        .split(delimiter)
        .nth(8)
        .and_then(parse_hex_color)
}

fn strip_value(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'')
}

// theme.conf is a flat `key = value` file; anything missing keeps its
// previous value, so a partial override is fine.
fn apply_theme_conf(content: &str, theme: &mut Theme) {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "background" => theme.bg = parse_hex_color(value).unwrap_or(theme.bg),
            "foreground" => theme.fg = parse_hex_color(value).unwrap_or(theme.fg),
            "accent" => theme.accent = parse_hex_color(value).unwrap_or(theme.accent),
            "dim" => theme.dim = parse_hex_color(value).unwrap_or(theme.dim),
            "font_family" => theme.font_family = strip_value(value).to_owned(),
            _ => {}
        }
    }
}

// Theme sources, keyed by the terminal's binary name so the user's default
// terminal can be tried first. Each parser returns None when its config is
// absent or has nothing we recognize.
type ThemeParser = fn() -> Option<Theme>;

const THEME_SOURCES: &[(&str, ThemeParser)] = &[
    ("alacritty", theme_from_alacritty),
    ("kitty", theme_from_kitty),
    ("xfce4-terminal", theme_from_xfce4_terminal),
    ("gnome-terminal", theme_from_gnome_terminal),
];

// Note on `dim`: every parser takes *bright* black (palette index 8), not
// normal black, which is usually too close to the background for the badge
// fills to be visible against it.

// Section-aware line scan of alacritty.toml rather than a full TOML parse:
// the handful of keys we want are all flat strings.
fn theme_from_alacritty() -> Option<Theme> {
    let content = fs::read_to_string(home_config("alacritty/alacritty.toml")).ok()?;

    let mut theme = Theme::default();
    let mut section = String::new();
    let mut matched = false;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(header) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            section = header.to_owned();
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        let slot = match (section.as_str(), key.trim()) {
            ("colors.primary", "background") => &mut theme.bg,
            ("colors.primary", "foreground") => &mut theme.fg,
            ("colors.cursor", "cursor") => &mut theme.accent,
            ("colors.bright", "black") => &mut theme.dim,
            ("font.normal", "family") => {
                theme.font_family = strip_value(value).to_owned();
                matched = true;
                continue;
            }
            _ => continue,
        };

        if let Some(color) = parse_hex_color(value) {
            *slot = color;
            matched = true;
        }
    }

    matched.then_some(theme)
}

// kitty.conf is flat and space-separated: `foreground #ffa04d`.
fn theme_from_kitty() -> Option<Theme> {
    let content = fs::read_to_string(home_config("kitty/kitty.conf")).ok()?;

    let mut theme = Theme::default();
    let mut matched = false;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };

        let slot = match key {
            "background" => &mut theme.bg,
            "foreground" => &mut theme.fg,
            "cursor" => &mut theme.accent,
            "color8" => &mut theme.dim,
            "font_family" => {
                theme.font_family = strip_value(value).to_owned();
                matched = true;
                continue;
            }
            _ => continue,
        };

        if let Some(color) = parse_hex_color(value) {
            *slot = color;
            matched = true;
        }
    }

    matched.then_some(theme)
}

// terminalrc is INI-style, with the 16 palette entries in one
// semicolon-separated ColorPalette value.
fn theme_from_xfce4_terminal() -> Option<Theme> {
    let content = fs::read_to_string(home_config("xfce4/terminal/terminalrc")).ok()?;

    let mut theme = Theme::default();
    let mut matched = false;

    for line in content.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };

        let color = match key {
            "ColorBackground" => parse_hex_color(value).map(|c| (c, &mut theme.bg)),
            "ColorForeground" => parse_hex_color(value).map(|c| (c, &mut theme.fg)),
            "ColorCursor" => parse_hex_color(value).map(|c| (c, &mut theme.accent)),
            "ColorPalette" => palette_bright_black(value, ';').map(|c| (c, &mut theme.dim)),
            "FontName" => {
                theme.font_family = strip_font_size(value);
                matched = true;
                continue;
            }
            _ => continue,
        };

        if let Some((color, slot)) = color {
            *slot = color;
            matched = true;
        }
    }

    matched.then_some(theme)
}

fn gsettings_profile_get(profile_schema: &str, key: &str) -> Option<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", profile_schema, key])
        .output()
        .ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

// GNOME Terminal keeps its colors in dconf, under a per-profile UUID.
fn theme_from_gnome_terminal() -> Option<Theme> {
    let uuid_raw = gsettings_profile_get("org.gnome.Terminal.ProfilesList", "default")?;
    let uuid = strip_value(&uuid_raw);
    if uuid.is_empty() {
        return None;
    }
    let schema = format!(
        "org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:{uuid}/"
    );

    let mut theme = Theme::default();
    let mut matched = false;

    let mut take = |key: &str, slot: &mut Color32| {
        if let Some(color) = gsettings_profile_get(&schema, key).as_deref().and_then(parse_hex_color)
        {
            *slot = color;
            matched = true;
        }
    };
    take("background-color", &mut theme.bg);
    take("foreground-color", &mut theme.fg);
    take("cursor-background-color", &mut theme.accent);

    if let Some(palette) = gsettings_profile_get(&schema, "palette")
        && let Some(color) = palette_bright_black(&palette, ',')
    {
        theme.dim = color;
        matched = true;
    }

    if let Some(font) = gsettings_profile_get(&schema, "font") {
        theme.font_family = strip_font_size(&font);
        matched = true;
    }

    matched.then_some(theme)
}

// The user's default terminal, so its config wins over any other terminal
// config that happens to be lying around on the system.
fn default_terminal_hint() -> Option<String> {
    if let Ok(terminal) = std::env::var("TERMINAL")
        && !terminal.is_empty()
    {
        return Some(terminal.to_lowercase());
    }

    // XFCE records its preferred terminal here.
    if let Ok(content) = fs::read_to_string(home_config("xfce4/helpers.rc"))
        && let Some(value) =
            content.lines().find_map(|line| line.trim().strip_prefix("TerminalEmulator="))
    {
        return Some(value.trim().to_lowercase());
    }

    // Debian/Ubuntu record the system-wide choice as an alternatives symlink.
    fs::read_link("/etc/alternatives/x-terminal-emulator")
        .ok()?
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
}

fn detect_theme() -> Option<Theme> {
    let hint = default_terminal_hint().unwrap_or_default();

    let preferred = THEME_SOURCES
        .iter()
        .find(|(name, _)| hint.contains(name))
        .and_then(|(_, parse)| parse());

    preferred.or_else(|| THEME_SOURCES.iter().find_map(|(_, parse)| parse()))
}

// Leaves a commented template behind on first run, so there's always a file
// to edit even when auto-detection worked.
fn write_theme_conf_template(theme: &Theme) {
    let path = config_dir().join("theme.conf");
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }

    let hex = |color: Color32| format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b());
    let template = format!(
        "# charmander theme. Uncomment a line to override the auto-detected value.\n\
         # Values below are what charmander is currently using.\n\
         #background = \"{}\"\n\
         #foreground = \"{}\"\n\
         #accent = \"{}\"\n\
         #dim = \"{}\"\n\
         #font_family = \"{}\"\n",
        hex(theme.bg),
        hex(theme.fg),
        hex(theme.accent),
        hex(theme.dim),
        theme.font_family,
    );
    fs::write(path, template).ok();
}

fn load_theme() -> Theme {
    let detected = detect_theme().unwrap_or_default();
    write_theme_conf_template(&detected);

    let mut theme = detected;
    if let Ok(content) = fs::read_to_string(config_dir().join("theme.conf")) {
        apply_theme_conf(&content, &mut theme);
    }
    theme
}

const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

fn load_icon() -> egui::IconData {
    let image = image::load_from_memory(ICON_PNG)
        .expect("decode icon.png")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData { rgba: image.into_raw(), width, height }
}

#[cfg(target_os = "linux")]
const XFCE_SHORTCUT_PROPERTY: &str = "/commands/custom/<Primary><Shift>u";
#[cfg(target_os = "linux")]
const GNOME_KEYBINDING_PATH: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/charmander/";
#[cfg(target_os = "linux")]
const GNOME_BINDING_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";

fn print_manual_instructions(exe: &str) {
    println!("Bind Ctrl+Shift+U (or any key) to this command yourself:");
    println!("  {exe}");
}

// `charmander --setup`: best-effort hotkey install, one code path per
// platform/desktop. Where there's no scriptable keybinding API (KDE's is
// version-fragile, macOS has none at all), we print instructions instead of
// risking a broken config.
fn run_setup() {
    let exe = std::env::current_exe().expect("resolve own executable path");
    let exe = exe.to_string_lossy().into_owned();

    #[cfg(target_os = "windows")]
    return setup_windows(&exe);

    #[cfg(target_os = "macos")]
    {
        println!("Automatic setup isn't available on macOS.");
        println!("Add it under System Settings > Keyboard > Keyboard Shortcuts > App Shortcuts:");
        println!("  {exe}");
        return;
    }

    #[cfg(target_os = "linux")]
    {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
        if desktop.contains("xfce") {
            setup_xfce(&exe);
        } else if desktop.contains("gnome") {
            setup_gnome(&exe);
        } else if desktop.contains("kde") {
            println!("Automatic setup isn't supported on KDE yet (its shortcut config");
            println!("differs too much across Plasma versions to script safely).");
            println!("Add it under System Settings > Shortcuts > Custom Shortcuts:");
            println!("  {exe}");
        } else {
            println!("Automatic setup isn't supported on this desktop environment.");
            print_manual_instructions(&exe);
        }
        return;
    }

    #[allow(unreachable_code)]
    {
        println!("Automatic setup isn't supported on this platform.");
        print_manual_instructions(&exe);
    }
}

#[cfg(target_os = "linux")]
fn setup_xfce(exe: &str) {
    let property_exists = std::process::Command::new("xfconf-query")
        .args(["-c", "xfce4-keyboard-shortcuts", "-p", XFCE_SHORTCUT_PROPERTY])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);

    let mut cmd = std::process::Command::new("xfconf-query");
    cmd.args(["-c", "xfce4-keyboard-shortcuts", "-p", XFCE_SHORTCUT_PROPERTY]);
    if property_exists {
        cmd.args(["-s", exe]);
    } else {
        cmd.args(["-n", "-t", "string", "-s", exe]);
    }

    match cmd.status() {
        Ok(status) if status.success() => println!("Bound Ctrl+Shift+U to charmander."),
        _ => {
            println!("Failed to set the XFCE shortcut.");
            print_manual_instructions(exe);
        }
    }
}

#[cfg(target_os = "linux")]
fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    let output = std::process::Command::new("gsettings").args(["get", schema, key]).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
fn gsettings_set(schema: &str, key: &str, value: &str) -> bool {
    std::process::Command::new("gsettings")
        .args(["set", schema, key, value])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

// GNOME custom keybindings live in a gsettings array of dconf paths; each
// path then holds its own name/command/binding under a child schema.
#[cfg(target_os = "linux")]
fn setup_gnome(exe: &str) {
    const LIST_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
    const LIST_KEY: &str = "custom-keybindings";

    let existing = gsettings_get(LIST_SCHEMA, LIST_KEY).unwrap_or_default();
    let mut paths: Vec<String> = existing
        .trim_start_matches("@as")
        .trim()
        .trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .map(|entry| entry.trim().trim_matches('\'').to_string())
        .filter(|entry| !entry.is_empty())
        .collect();

    if !paths.iter().any(|path| path == GNOME_KEYBINDING_PATH) {
        paths.push(GNOME_KEYBINDING_PATH.to_string());
    }

    let list_literal = format!(
        "[{}]",
        paths.iter().map(|path| format!("'{path}'")).collect::<Vec<_>>().join(", ")
    );

    let binding_schema = format!("{GNOME_BINDING_SCHEMA}:{GNOME_KEYBINDING_PATH}");
    let ok = gsettings_set(LIST_SCHEMA, LIST_KEY, &list_literal)
        && gsettings_set(&binding_schema, "name", "'charmander'")
        && gsettings_set(&binding_schema, "command", &format!("'{exe}'"))
        && gsettings_set(&binding_schema, "binding", "'<Primary><Shift>u'");

    if ok {
        println!("Bound Ctrl+Shift+U to charmander.");
    } else {
        println!("Failed to set the GNOME shortcut (is gsettings installed?).");
        print_manual_instructions(exe);
    }
}

// Windows hotkeys are set on a Start Menu shortcut's embedded "hotkey" field;
// Windows registers it globally as long as the .lnk lives in the Start Menu.
#[cfg(target_os = "windows")]
fn setup_windows(exe: &str) {
    use mslnk::{HotkeyKey, HotkeyModifiers, ShellLink};

    let Ok(appdata) = std::env::var("APPDATA") else {
        println!("Couldn't resolve %APPDATA%.");
        return print_manual_instructions(exe);
    };
    let shortcut_path =
        std::path::Path::new(&appdata).join("Microsoft/Windows/Start Menu/Programs/charmander.lnk");

    let Ok(mut link) = ShellLink::new(exe) else {
        println!("Failed to build the shortcut.");
        return print_manual_instructions(exe);
    };

    let hotkey = link.header_mut().hotkey_mut();
    hotkey.set_key(HotkeyKey::KeyU);
    hotkey.set_modifiers(HotkeyModifiers::HOTKEYF_CONTROL | HotkeyModifiers::HOTKEYF_SHIFT);

    match link.create_lnk(&shortcut_path) {
        Ok(()) => println!("Bound Ctrl+Shift+U to charmander."),
        Err(_) => {
            println!("Failed to write the Start Menu shortcut.");
            print_manual_instructions(exe);
        }
    }
}

fn main() -> eframe::Result<()> {
    if std::env::args().any(|arg| arg == "--setup") {
        run_setup();
        return Ok(());
    }

    let chosen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let chosen_for_app = chosen.clone();
    let theme = load_theme();
    write_settings_template();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 440.0])
            .with_min_inner_size([320.0, 260.0])
            .with_decorations(false)
            .with_always_on_top()
            .with_active(true)
            .with_resizable(true)
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "charmander",
        options,
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx, &theme);
            setup_style(&cc.egui_ctx, &theme);
            Ok(Box::new(Picker::new(chosen_for_app, theme)))
        }),
    )?;

    if let Some(glyph) = chosen.lock().unwrap().as_deref() {
        typer::type_str(glyph);
    }

    Ok(())
}

// Asks fontconfig for the file backing a family name, so we never hardcode
// font paths (they differ per distro, and don't exist at all elsewhere).
fn resolve_font(family: &str) -> Option<Vec<u8>> {
    let output = std::process::Command::new("fc-match")
        .args([&format!("{family}:style=Regular"), "--format=%{file}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    fs::read(path).ok()
}

fn setup_fonts(ctx: &egui::Context, theme: &Theme) {
    let mut fonts = FontDefinitions::default();
    let mut register = |name: String, bytes: Vec<u8>, first: bool| {
        fonts.font_data.insert(name.clone(), FontData::from_owned(bytes).into());
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let list = fonts.families.get_mut(&family).unwrap();
            if first {
                list.insert(0, name.clone());
            } else {
                list.push(name.clone());
            }
        }
    };

    if let Some(bytes) = resolve_font(&theme.font_family) {
        register("primary".to_owned(), bytes, true);
    }

    for (idx, family) in FALLBACK_FONT_FAMILIES.iter().enumerate() {
        if let Some(bytes) = resolve_font(family) {
            register(format!("fallback{idx}"), bytes, false);
        }
    }

    ctx.set_fonts(fonts);
}

fn setup_style(ctx: &egui::Context, theme: &Theme) {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(theme.fg);
    visuals.window_fill = theme.bg;
    visuals.panel_fill = theme.bg;
    visuals.extreme_bg_color = theme.bg;
    visuals.faint_bg_color = theme.bg;
    visuals.selection.bg_fill = theme.accent;
    visuals.selection.stroke.color = Color32::BLACK;
    visuals.widgets.inactive.bg_fill = theme.bg;
    visuals.widgets.hovered.bg_fill = theme.dim;
    visuals.widgets.active.bg_fill = theme.accent;
    ctx.set_visuals(visuals);
}

fn favorites_path() -> PathBuf {
    config_dir().join("favorites.txt")
}

fn load_favorites() -> HashSet<String> {
    let Ok(content) = fs::read_to_string(favorites_path()) else {
        return HashSet::new();
    };
    content
        .lines()
        .map(|line| line.trim().to_uppercase())
        .filter(|line| !line.is_empty())
        .collect()
}

fn save_favorites(favorites: &HashSet<String>) {
    let path = favorites_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let body: String = favorites.iter().map(|key| format!("{key}\n")).collect();
    fs::write(path, body).ok();
}

fn default_view_path() -> PathBuf {
    config_dir().join("default_view.txt")
}

fn load_default_view_is_favorites() -> bool {
    fs::read_to_string(default_view_path())
        .map(|content| content.trim() == "favorites")
        .unwrap_or(false)
}

fn save_default_view(favorites_only: bool) {
    let path = default_view_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let value = if favorites_only { "favorites" } else { "all" };
    fs::write(path, value).ok();
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.conf")
}

// settings.conf uses the same flat `key = value` shape as theme.conf.
fn parse_quick_alt(content: &str) -> bool {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .any(|(key, value)| key.trim() == "quick_alt" && strip_value(value) == "on")
}

fn load_quick_alt() -> bool {
    fs::read_to_string(settings_path()).map(|content| parse_quick_alt(&content)).unwrap_or(false)
}

// Leaves a commented template behind so the setting is discoverable
// without reading the docs.
fn write_settings_template() {
    let path = settings_path();
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(
        path,
        "# charmander settings.\n\
         #\n\
         # quick_alt: insert an alt code the moment Alt is released, instead\n\
         # of narrowing the list and waiting for Enter.\n\
         #quick_alt = on\n",
    )
    .ok();
}

// Draws a terminal-keycap-style badge, e.g. [Ctrl+F] favorite
fn key_badge(ui: &mut egui::Ui, theme: &Theme, key: &str, action: &str) {
    egui::Frame::new()
        .fill(theme.dim)
        .stroke(egui::Stroke::new(1.0, theme.accent))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(key).color(theme.accent).strong().monospace());
        });
    ui.label(RichText::new(action).color(theme.fg).small());
}

// CP437 (DOS/OEM) codes 1-31. Alt codes render this range as graphics
// rather than as the control characters the bytes nominally mean.
#[rustfmt::skip]
const CP437_GRAPHICS: [char; 32] = [
    '\0', '☺', '☻', '♥', '♦', '♣', '♠', '•', '◘', '○', '◙', '♂', '♀', '♪', '♫', '☼',
    '►', '◄', '↕', '‼', '¶', '§', '▬', '↨', '↑', '↓', '→', '←', '∟', '↔', '▲', '▼',
];

// CP437 codes 128-255: accented latin, box drawing, blocks and maths.
#[rustfmt::skip]
const CP437_HIGH: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å',
    'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ',
    'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»',
    '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐',
    '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧',
    '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀',
    'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩',
    '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{a0}',
];

// Windows-1252 (ANSI) differs from Unicode only across 0x80-0x9F.
#[rustfmt::skip]
const CP1252_HIGH: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

// Resolves a typed alt code the way Windows does, which depends on whether
// the user typed a leading zero:
//
//   Alt+219  -> OEM  (CP437)         -> █
//   Alt+0219 -> ANSI (Windows-1252)  -> Û
//
// The leading zero is why the digits are kept as text rather than a number.
fn alt_code_to_char(digits: &str) -> Option<char> {
    let byte = u8::try_from(digits.parse::<u32>().ok()?).ok()?;

    if digits.starts_with('0') {
        return Some(match byte {
            0x80..=0x9F => CP1252_HIGH[byte as usize - 0x80],
            other => char::from_u32(other as u32)?,
        });
    }

    Some(match byte {
        0..=31 => CP437_GRAPHICS[byte as usize],
        127 => '⌂',
        128..=255 => CP437_HIGH[byte as usize - 128],
        other => char::from_u32(other as u32)?,
    })
}

// egui maps both the number row and the numpad onto these, so alt codes can
// be typed from either.
fn digit_key(key: Key) -> Option<char> {
    match key {
        Key::Num0 => Some('0'),
        Key::Num1 => Some('1'),
        Key::Num2 => Some('2'),
        Key::Num3 => Some('3'),
        Key::Num4 => Some('4'),
        Key::Num5 => Some('5'),
        Key::Num6 => Some('6'),
        Key::Num7 => Some('7'),
        Key::Num8 => Some('8'),
        Key::Num9 => Some('9'),
        _ => None,
    }
}

struct Picker {
    query: String,
    favorites: HashSet<String>,
    favorites_only: bool,
    selected: usize,
    results: Vec<(String, &'static str)>,
    focus_requested: bool,
    chosen: Arc<Mutex<Option<String>>>,
    alt_buffer: String,
    alt_active_prev: bool,
    alt_held_prev: bool,
    quick_alt: bool,
    ever_focused: bool,
    theme: Theme,
}

impl Picker {
    fn new(chosen: Arc<Mutex<Option<String>>>, theme: Theme) -> Self {
        let mut picker = Self {
            query: String::new(),
            favorites: load_favorites(),
            favorites_only: load_default_view_is_favorites(),
            selected: 0,
            results: Vec::new(),
            focus_requested: true,
            chosen,
            alt_buffer: String::new(),
            alt_active_prev: false,
            alt_held_prev: false,
            quick_alt: load_quick_alt(),
            ever_focused: false,
            theme,
        };
        picker.refresh_results();
        picker
    }

    fn refresh_results(&mut self) {
        let query = self.query.to_lowercase();
        let favorites_only = self.favorites_only;
        let favorites = &self.favorites;

        let results: Vec<(String, &'static str)> = CHARACTERS
            .iter()
            .filter(|(glyph, name)| {
                if favorites_only && !favorites.contains(&codepoint_key(glyph)) {
                    return false;
                }
                query.is_empty() || name.to_lowercase().contains(&query)
            })
            .take(MAX_RESULTS)
            .map(|(glyph, name)| (glyph.to_string(), *name))
            .collect();

        self.selected = self.selected.min(results.len().saturating_sub(1));
        self.results = results;
    }

    fn toggle_favorite(&mut self, glyph: &str) {
        let key = codepoint_key(glyph);
        if !self.favorites.remove(&key) {
            self.favorites.insert(key);
        }
        save_favorites(&self.favorites);
    }

    fn choose(&mut self, glyph: &str, ctx: &egui::Context) {
        *self.chosen.lock().unwrap() = Some(glyph.to_string());
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    // Alt-code entry. Holding Alt and typing a digit starts it; the digits
    // then stay put after Alt is released so they can be corrected with
    // Backspace. Nothing is inserted until Enter, so releasing Alt on a
    // mistyped code no longer commits it.
    //
    // Returns whether entry is active, which takes over Enter and Esc.
    fn handle_alt_code(&mut self, ctx: &egui::Context) -> bool {
        let alt_now = ctx.input(|i| i.modifiers.alt);

        // Digits only *start* entry while Alt is down; once it's underway
        // they're accepted on their own, so the search box must stay out of
        // the way until the buffer is empty again.
        let mut edited = false;
        if alt_now || !self.alt_buffer.is_empty() {
            let buffer = &mut self.alt_buffer;
            ctx.input(|i| {
                for event in &i.events {
                    let egui::Event::Key { key, pressed: true, repeat, .. } = event else {
                        continue;
                    };

                    if *key == Key::Backspace {
                        buffer.pop();
                        edited = true;
                    } else if let Some(digit) = digit_key(*key)
                        && !repeat
                    {
                        buffer.push(digit);
                        edited = true;
                    }
                }
            });
        }

        let released = self.alt_held_prev && !alt_now && !self.alt_buffer.is_empty();
        self.alt_held_prev = alt_now;

        // Quick mode trades the confirmation step for speed: the code fires
        // the moment Alt comes up.
        if released && self.quick_alt {
            self.commit_alt_code(ctx);
        }

        // Otherwise, once Alt is up the list collapses to just the character
        // the code resolves to, so it's confirmed like any searched one.
        // Only recomputed on an actual edit, since it scans the whole table.
        if !alt_now && !self.alt_buffer.is_empty() && (edited || released) {
            self.show_alt_result();
        }

        let active = alt_now || !self.alt_buffer.is_empty();

        // Entry just ended: hand focus back to the search box, which lost it
        // while the indicator stood in for it, and restore the full list.
        if self.alt_active_prev && !active {
            self.focus_requested = true;
            self.refresh_results();
        }
        self.alt_active_prev = active;

        active
    }

    // The single row shown after Alt is released. An unresolvable code (out
    // of range, or no digits yet) leaves the list empty.
    fn show_alt_result(&mut self) {
        let resolved = alt_code_to_char(&self.alt_buffer);
        self.results = resolved
            .map(|ch| {
                let glyph = ch.to_string();
                let name = CHARACTERS
                    .iter()
                    .find(|(candidate, _)| *candidate == glyph)
                    .map(|(_, name)| *name)
                    .unwrap_or("UNNAMED CHARACTER");
                vec![(glyph, name)]
            })
            .unwrap_or_default();
        self.selected = 0;
    }

    // Inserts the typed alt code. An out-of-range code is left in place
    // rather than discarded, so it can be corrected.
    fn commit_alt_code(&mut self, ctx: &egui::Context) {
        let Some(ch) = alt_code_to_char(&self.alt_buffer) else {
            return;
        };
        *self.chosen.lock().unwrap() = Some(ch.to_string());
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn cancel_alt_code(&mut self) {
        self.alt_buffer.clear();
        self.focus_requested = true;
    }
}

impl eframe::App for Picker {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        let entering_alt_code = self.handle_alt_code(ctx);

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if entering_alt_code {
                self.cancel_alt_code();
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        // Lost window focus (alt-tab, click elsewhere, switch workspace): close.
        // `ever_focused` guards against the brief unfocused state some window
        // managers report on the very first frame, before focus is granted.
        match ctx.input(|i| i.viewport().focused) {
            Some(true) => self.ever_focused = true,
            Some(false) if self.ever_focused => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowDown)) {
            self.selected = (self.selected + 1).min(self.results.len().saturating_sub(1));
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowUp)) {
            self.selected = self.selected.saturating_sub(1);
        }
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::F))
            && let Some((glyph, _)) = self.results.get(self.selected).cloned()
        {
            self.toggle_favorite(&glyph);
        }
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Num1)) {
            self.favorites_only = false;
            self.refresh_results();
        }
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Num2)) {
            self.favorites_only = true;
            self.refresh_results();
        }
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::D)) {
            save_default_view(self.favorites_only);
        }
        if ctx.input(|i| i.key_pressed(Key::Enter)) {
            if entering_alt_code {
                self.commit_alt_code(ctx);
            } else if let Some((glyph, _)) = self.results.get(self.selected).cloned() {
                self.choose(&glyph, ctx);
            }
        }

        let theme = self.theme.clone();
        egui::Panel::bottom("legend").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                key_badge(ui, &theme, "\u{2191}\u{2193}", "navigate");
                key_badge(ui, &theme, "Enter", "insert");
                key_badge(ui, &theme, "Right-click", "favorite");
                key_badge(ui, &theme, "Alt+digits", "alt code");
            });
            ui.horizontal_wrapped(|ui| {
                key_badge(ui, &theme, "Ctrl+F", "favorite");
                key_badge(ui, &theme, "Ctrl+1 / Ctrl+2", "switch view");
                key_badge(ui, &theme, "Ctrl+D", "set default view");
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("charmander").color(theme.fg).strong());
                ui.separator();
                if ui.selectable_label(!self.favorites_only, "All").clicked() {
                    self.favorites_only = false;
                    self.refresh_results();
                }
                if ui.selectable_label(self.favorites_only, "★ Favorites").clicked() {
                    self.favorites_only = true;
                    self.refresh_results();
                }
            });

            ui.add_space(4.0);
            if entering_alt_code {
                // Replaces the search box while entry is active, so TextEdit
                // never swallows the digit keys. Previewing the character
                // means Enter never inserts a surprise.
                let preview = alt_code_to_char(&self.alt_buffer)
                    .map(|ch| format!("  \u{2192}  {ch}"))
                    .unwrap_or_default();

                // Which code page is in play is worth stating: the same
                // digits mean different characters with and without the
                // leading zero.
                let hint = match self.alt_buffer.chars().next() {
                    Some('0') => "ANSI \u{00b7} Enter inserts \u{00b7} Backspace edits",
                    Some(_) => "OEM \u{00b7} Enter inserts \u{00b7} Backspace edits",
                    None => "type digits while holding Alt",
                };

                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("Alt code: {}_{preview}", self.alt_buffer))
                            .color(theme.accent)
                            .monospace(),
                    );
                    ui.label(RichText::new(hint).color(theme.dim).small()).on_hover_text(
                        "A leading zero selects ANSI (Windows-1252); without one the code is \
                         OEM (CP437).\nAlt+219 is a full block, Alt+0219 is U with circumflex.",
                    );
                });

                // Surfaced inline rather than as a hover tooltip: this is a
                // keyboard flow, so a hint that needs the mouse to find is a
                // hint nobody reads.
                if !self.quick_alt {
                    ui.label(
                        RichText::new(
                            "Tip: set quick_alt = on in settings.conf to insert the \
                             moment Alt is released",
                        )
                        .color(theme.dim)
                        .small(),
                    );
                }
            } else {
                let search = ui.text_edit_singleline(&mut self.query);
                if self.focus_requested {
                    search.request_focus();
                    self.focus_requested = false;
                }
                if search.changed() {
                    self.refresh_results();
                }
            }

            ui.add_space(6.0);
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                for (idx, (glyph, name)) in self.results.clone().iter().enumerate() {
                    let is_favorite = self.favorites.contains(&codepoint_key(glyph));
                    let is_selected = idx == self.selected;
                    let star = if is_favorite { "★" } else { "☆" };
                    let text = format!("{glyph}  {name}  ({})  {star}", codepoint_label(glyph));

                    let rich = if is_selected {
                        RichText::new(text).color(Color32::BLACK).background_color(theme.accent)
                    } else if is_favorite {
                        RichText::new(text).color(theme.accent)
                    } else {
                        RichText::new(text).color(theme.fg)
                    };

                    let response = ui.selectable_label(is_selected, rich);
                    if response.clicked() {
                        self.choose(glyph, ctx);
                    }
                    if response.secondary_clicked() {
                        self.toggle_favorite(glyph);
                    }
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The formal name alone hides emoji behind words nobody searches for.
    #[test]
    fn lists_common_emoji_names() {
        let name = |glyph: &str| CHARACTERS.iter().find(|(g, _)| *g == glyph).map(|(_, n)| *n);
        assert_eq!(name("\u{1F346}"), Some("AUBERGINE / EGGPLANT"));
        assert_eq!(name("\u{1F914}"), Some("THINKING FACE"));
        assert_eq!(name("\u{1F468}\u{200D}\u{1F9B0}"), Some("MAN: RED HAIR"));
    }

    #[test]
    fn parses_color_formats() {
        let expected = Color32::from_rgb(0xff, 0xa0, 0x4d);
        assert_eq!(parse_hex_color("#ffa04d"), Some(expected));
        assert_eq!(parse_hex_color("ffa04d"), Some(expected));
        assert_eq!(parse_hex_color("  \"#ffa04d\"  "), Some(expected));
        assert_eq!(parse_hex_color("rgb(255,160,77)"), Some(expected));
        assert_eq!(parse_hex_color("#fff"), None);
        assert_eq!(parse_hex_color("not a color"), None);
    }

    #[test]
    fn strips_trailing_font_size() {
        assert_eq!(strip_font_size("FiraCode Nerd Font Mono 11"), "FiraCode Nerd Font Mono");
        assert_eq!(strip_font_size("'Monospace 10'"), "Monospace");
        assert_eq!(strip_font_size("JetBrains Mono"), "JetBrains Mono");
        assert_eq!(strip_font_size("Noto Sans Symbols2"), "Noto Sans Symbols2");
    }

    #[test]
    fn takes_bright_black_from_palettes() {
        let xfce = "#1a0a1a;#d9410e;#4e8234;#e8a33d;#4b2e83;#8b2fc9;#3d8f8f;#d9c8b4;#4a3b52";
        assert_eq!(palette_bright_black(xfce, ';'), Some(Color32::from_rgb(0x4a, 0x3b, 0x52)));

        let gnome = "['#000', '#1', '#2', '#3', '#4', '#5', '#6', '#7', '#4a3b52']";
        assert_eq!(palette_bright_black(gnome, ','), Some(Color32::from_rgb(0x4a, 0x3b, 0x52)));

        assert_eq!(palette_bright_black("#1a0a1a;#d9410e", ';'), None);
    }

    #[test]
    fn theme_conf_overrides_only_listed_keys() {
        let mut theme = Theme::default();
        let original_fg = theme.fg;

        apply_theme_conf(
            "# comment\nbackground = \"#000000\"\nfont_family = \"DejaVu Sans Mono\"\n",
            &mut theme,
        );

        assert_eq!(theme.bg, Color32::from_rgb(0, 0, 0));
        assert_eq!(theme.font_family, "DejaVu Sans Mono");
        assert_eq!(theme.fg, original_fg, "unlisted keys must keep their value");
    }

    #[test]
    fn theme_conf_ignores_unparseable_color() {
        let mut theme = Theme::default();
        let original_bg = theme.bg;
        apply_theme_conf("background = \"octarine\"\n", &mut theme);
        assert_eq!(theme.bg, original_bg);
    }

    // A leading zero selects ANSI (Windows-1252); without one the code is
    // OEM (CP437), which is a different character for the same number.
    #[test]
    fn maps_ansi_alt_codes() {
        assert_eq!(alt_code_to_char("0150"), Some('\u{2013}'), "Alt+0150 is an en dash");
        assert_eq!(alt_code_to_char("0151"), Some('\u{2014}'), "Alt+0151 is an em dash");
        assert_eq!(alt_code_to_char("0128"), Some('€'));
        assert_eq!(alt_code_to_char("0163"), Some('£'));
        // Outside 0x80-0x9F, Windows-1252 matches Unicode exactly.
        assert_eq!(alt_code_to_char("065"), Some('A'));
        assert_eq!(alt_code_to_char("0233"), Some('é'));
    }

    #[test]
    fn maps_oem_alt_codes() {
        assert_eq!(alt_code_to_char("1"), Some('☺'));
        assert_eq!(alt_code_to_char("3"), Some('♥'));
        assert_eq!(alt_code_to_char("15"), Some('☼'));
        assert_eq!(alt_code_to_char("219"), Some('█'), "Alt+219 is a full block");
        assert_eq!(alt_code_to_char("220"), Some('▄'));
        assert_eq!(alt_code_to_char("156"), Some('£'));
        assert_eq!(alt_code_to_char("251"), Some('√'));
        assert_eq!(alt_code_to_char("65"), Some('A'), "the ASCII range is unchanged");
        // Same number, different code page.
        assert_eq!(alt_code_to_char("150"), Some('û'));
    }

    #[test]
    fn reads_quick_alt_setting() {
        assert!(parse_quick_alt("quick_alt = on"));
        assert!(parse_quick_alt("# comment\n\nquick_alt=\"on\"\n"));
        assert!(!parse_quick_alt("quick_alt = off"));
        assert!(!parse_quick_alt("#quick_alt = on"), "the template default stays off");
        assert!(!parse_quick_alt(""));
    }

    #[test]
    fn rejects_out_of_range_alt_codes() {
        assert_eq!(alt_code_to_char("256"), None, "alt codes address a single byte");
        assert_eq!(alt_code_to_char("0256"), None);
        assert_eq!(alt_code_to_char(""), None);
    }
}
