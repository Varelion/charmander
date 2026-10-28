use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use egui::{Color32, FontData, FontDefinitions, FontFamily, Key, RichText};
use enigo::{Enigo, Keyboard, Settings};

include!(concat!(env!("OUT_DIR"), "/characters.rs"));

// Colors and font mirrored from ~/.config/alacritty/alacritty.toml
// so the picker blends into the user's terminal.
const BG: Color32 = Color32::from_rgb(0x12, 0x08, 0x1a);
const FG: Color32 = Color32::from_rgb(0xff, 0xa0, 0x4d);
const ACCENT: Color32 = Color32::from_rgb(0xff, 0x75, 0x18);
const DIM: Color32 = Color32::from_rgb(0x4a, 0x3b, 0x52);

const FONT_PATH: &str = "/home/z/.local/share/fonts/FiraCodeNerdFontMono-Regular.ttf";
// Fallback fonts so emoji/symbol codepoints render as a glyph instead of a blank box.
const FALLBACK_FONT_PATHS: &[&str] = &[
    "/usr/share/fonts/truetype/ancient-scripts/Symbola_hint.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf",
];
const MAX_RESULTS: usize = 200;
const TYPE_DELAY: Duration = Duration::from_millis(150);

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

    let chosen: Arc<Mutex<Option<char>>> = Arc::new(Mutex::new(None));
    let chosen_for_app = chosen.clone();

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
            setup_fonts(&cc.egui_ctx);
            setup_style(&cc.egui_ctx);
            Ok(Box::new(Picker::new(chosen_for_app)))
        }),
    )?;

    if let Some(ch) = *chosen.lock().unwrap() {
        type_char(ch);
    }

    Ok(())
}

fn setup_fonts(ctx: &egui::Context) {
    let Ok(bytes) = fs::read(FONT_PATH) else {
        return;
    };

    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("firacode".to_owned(), FontData::from_owned(bytes).into());
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "firacode".to_owned());
    fonts
        .families
        .get_mut(&FontFamily::Monospace)
        .unwrap()
        .insert(0, "firacode".to_owned());

    for (idx, path) in FALLBACK_FONT_PATHS.iter().enumerate() {
        let Ok(bytes) = fs::read(path) else { continue };
        let name = format!("fallback{idx}");
        fonts.font_data.insert(name.clone(), FontData::from_owned(bytes).into());
        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .push(name.clone());
        fonts
            .families
            .get_mut(&FontFamily::Monospace)
            .unwrap()
            .push(name);
    }

    ctx.set_fonts(fonts);
}

fn setup_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(FG);
    visuals.window_fill = BG;
    visuals.panel_fill = BG;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = BG;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke.color = Color32::BLACK;
    visuals.widgets.inactive.bg_fill = BG;
    visuals.widgets.hovered.bg_fill = DIM;
    visuals.widgets.active.bg_fill = ACCENT;
    ctx.set_visuals(visuals);
}

fn favorites_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/charmander/favorites.txt")
}

fn load_favorites() -> HashSet<u32> {
    let Ok(content) = fs::read_to_string(favorites_path()) else {
        return HashSet::new();
    };
    content
        .lines()
        .filter_map(|line| u32::from_str_radix(line.trim(), 16).ok())
        .collect()
}

fn save_favorites(favorites: &HashSet<u32>) {
    let path = favorites_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let body: String = favorites.iter().map(|cp| format!("{cp:04X}\n")).collect();
    fs::write(path, body).ok();
}

fn default_view_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/charmander/default_view.txt")
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

// Draws a terminal-keycap-style badge, e.g. [Ctrl+F] favorite
fn key_badge(ui: &mut egui::Ui, key: &str, action: &str) {
    egui::Frame::new()
        .fill(DIM)
        .stroke(egui::Stroke::new(1.0, ACCENT))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(key).color(ACCENT).strong().monospace());
        });
    ui.label(RichText::new(action).color(FG).small());
}

// Windows "Alt code" entry: hold Alt, type decimal digits, release Alt.
// Values 0-255 map through Windows-1252, which is identical to Unicode for
// 0x00-0x7F and 0xA0-0xFF; only 0x80-0x9F need an explicit override.
// e.g. Alt+0150 -> EN DASH (-), Alt+0151 -> EM DASH (--).
fn alt_code_to_char(code: u32) -> Option<char> {
    let byte = u8::try_from(code).ok()?;
    let codepoint = match byte {
        0x80 => 0x20AC, // EURO SIGN
        0x82 => 0x201A, // SINGLE LOW-9 QUOTATION MARK
        0x83 => 0x0192, // LATIN SMALL LETTER F WITH HOOK
        0x84 => 0x201E, // DOUBLE LOW-9 QUOTATION MARK
        0x85 => 0x2026, // HORIZONTAL ELLIPSIS
        0x86 => 0x2020, // DAGGER
        0x87 => 0x2021, // DOUBLE DAGGER
        0x88 => 0x02C6, // MODIFIER LETTER CIRCUMFLEX ACCENT
        0x89 => 0x2030, // PER MILLE SIGN
        0x8A => 0x0160, // LATIN CAPITAL LETTER S WITH CARON
        0x8B => 0x2039, // SINGLE LEFT-POINTING ANGLE QUOTATION MARK
        0x8C => 0x0152, // LATIN CAPITAL LIGATURE OE
        0x8E => 0x017D, // LATIN CAPITAL LETTER Z WITH CARON
        0x91 => 0x2018, // LEFT SINGLE QUOTATION MARK
        0x92 => 0x2019, // RIGHT SINGLE QUOTATION MARK
        0x93 => 0x201C, // LEFT DOUBLE QUOTATION MARK
        0x94 => 0x201D, // RIGHT DOUBLE QUOTATION MARK
        0x95 => 0x2022, // BULLET
        0x96 => 0x2013, // EN DASH
        0x97 => 0x2014, // EM DASH
        0x98 => 0x02DC, // SMALL TILDE
        0x99 => 0x2122, // TRADE MARK SIGN
        0x9A => 0x0161, // LATIN SMALL LETTER S WITH CARON
        0x9B => 0x203A, // SINGLE RIGHT-POINTING ANGLE QUOTATION MARK
        0x9C => 0x0153, // LATIN SMALL LIGATURE OE
        0x9E => 0x017E, // LATIN SMALL LETTER Z WITH CARON
        0x9F => 0x0178, // LATIN CAPITAL LETTER Y WITH DIAERESIS
        other => other as u32,
    };
    char::from_u32(codepoint)
}

fn type_char(ch: char) {
    std::thread::sleep(TYPE_DELAY);
    let mut enigo = Enigo::new(&Settings::default()).expect("init enigo");
    let mut buf = [0u8; 4];
    enigo.text(ch.encode_utf8(&mut buf)).expect("type selected character");
}

struct Picker {
    query: String,
    favorites: HashSet<u32>,
    favorites_only: bool,
    selected: usize,
    results: Vec<(u32, &'static str)>,
    focus_requested: bool,
    chosen: Arc<Mutex<Option<char>>>,
    alt_buffer: String,
    alt_held_prev: bool,
    ever_focused: bool,
}

impl Picker {
    fn new(chosen: Arc<Mutex<Option<char>>>) -> Self {
        let mut picker = Self {
            query: String::new(),
            favorites: load_favorites(),
            favorites_only: load_default_view_is_favorites(),
            selected: 0,
            results: Vec::new(),
            focus_requested: true,
            chosen,
            alt_buffer: String::new(),
            alt_held_prev: false,
            ever_focused: false,
        };
        picker.refresh_results();
        picker
    }

    fn refresh_results(&mut self) {
        let query = self.query.to_lowercase();
        let favorites_only = self.favorites_only;
        let favorites = &self.favorites;

        let results: Vec<(u32, &'static str)> = CHARACTERS
            .iter()
            .filter(|(codepoint, name)| {
                if favorites_only && !favorites.contains(codepoint) {
                    return false;
                }
                query.is_empty() || name.to_lowercase().contains(&query)
            })
            .take(MAX_RESULTS)
            .map(|(cp, name)| (*cp, *name))
            .collect();

        self.selected = self.selected.min(results.len().saturating_sub(1));
        self.results = results;
    }

    fn toggle_favorite(&mut self, codepoint: u32) {
        if !self.favorites.remove(&codepoint) {
            self.favorites.insert(codepoint);
        }
        save_favorites(&self.favorites);
    }

    fn choose(&mut self, codepoint: u32, ctx: &egui::Context) {
        if let Some(glyph) = char::from_u32(codepoint) {
            *self.chosen.lock().unwrap() = Some(glyph);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    // Windows Alt-code entry: hold Alt, type digits, release Alt to commit.
    fn handle_alt_code(&mut self, ctx: &egui::Context) {
        const DIGIT_KEYS: [(Key, char); 10] = [
            (Key::Num0, '0'),
            (Key::Num1, '1'),
            (Key::Num2, '2'),
            (Key::Num3, '3'),
            (Key::Num4, '4'),
            (Key::Num5, '5'),
            (Key::Num6, '6'),
            (Key::Num7, '7'),
            (Key::Num8, '8'),
            (Key::Num9, '9'),
        ];

        let alt_now = ctx.input(|i| i.modifiers.alt);

        if alt_now && !self.alt_held_prev {
            self.alt_buffer.clear();
        }

        if alt_now {
            ctx.input(|i| {
                for event in &i.events {
                    let egui::Event::Key { key, pressed: true, repeat: false, .. } = event else {
                        continue;
                    };
                    if let Some((_, digit)) = DIGIT_KEYS.iter().find(|(k, _)| k == key) {
                        self.alt_buffer.push(*digit);
                    }
                }
            });
        }

        if !alt_now && self.alt_held_prev && !self.alt_buffer.is_empty() {
            if let Some(ch) = self.alt_buffer.parse::<u32>().ok().and_then(alt_code_to_char) {
                *self.chosen.lock().unwrap() = Some(ch);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            self.alt_buffer.clear();
            self.focus_requested = true;
        }

        self.alt_held_prev = alt_now;
    }
}

impl eframe::App for Picker {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        self.handle_alt_code(ctx);
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::F)) {
            if let Some((codepoint, _)) = self.results.get(self.selected).copied() {
                self.toggle_favorite(codepoint);
            }
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
            if let Some((codepoint, _)) = self.results.get(self.selected).copied() {
                self.choose(codepoint, ctx);
            }
        }

        egui::Panel::bottom("legend").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                key_badge(ui, "Enter", "insert");
                key_badge(ui, "\u{2191}\u{2193}", "navigate");
                key_badge(ui, "Ctrl+F", "favorite");
                key_badge(ui, "Right-click", "favorite");
                key_badge(ui, "Alt+digits", "alt code");
                key_badge(ui, "Ctrl+1 / Ctrl+2", "switch view");
                key_badge(ui, "Ctrl+D", "set default view");
                key_badge(ui, "Esc", "cancel");
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("charmander").color(FG).strong());
                ui.separator();
                if ui.selectable_label(!self.favorites_only, "All (Ctrl+1)").clicked() {
                    self.favorites_only = false;
                    self.refresh_results();
                }
                if ui
                    .selectable_label(self.favorites_only, "★ Favorites (Ctrl+2)")
                    .clicked()
                {
                    self.favorites_only = true;
                    self.refresh_results();
                }
            });

            ui.add_space(4.0);
            if self.alt_held_prev {
                // Alt is down: show the digits being captured instead of the
                // search box, so TextEdit never steals the digit key events.
                ui.label(
                    RichText::new(format!("Alt code: {}_", self.alt_buffer))
                        .color(ACCENT)
                        .monospace(),
                );
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
                for (idx, (codepoint, name)) in self.results.clone().iter().enumerate() {
                    let is_favorite = self.favorites.contains(codepoint);
                    let is_selected = idx == self.selected;
                    let glyph = char::from_u32(*codepoint).unwrap_or(' ');
                    let star = if is_favorite { "★" } else { "☆" };
                    let text = format!("{glyph}  {name}  (U+{codepoint:04X})  {star}");

                    let rich = if is_selected {
                        RichText::new(text).color(Color32::BLACK).background_color(ACCENT)
                    } else if is_favorite {
                        RichText::new(text).color(ACCENT)
                    } else {
                        RichText::new(text).color(FG)
                    };

                    let response = ui.selectable_label(is_selected, rich);
                    if response.clicked() {
                        self.choose(*codepoint, ctx);
                    }
                    if response.secondary_clicked() {
                        self.toggle_favorite(*codepoint);
                    }
                }
            });
        });
    }
}
