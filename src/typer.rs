// Delivers the chosen text to whatever window has keyboard focus once the
// picker is gone.

use std::time::Duration;

// Lets the window manager hand focus back to the previous window after the
// picker closes.
const TYPE_DELAY: Duration = Duration::from_millis(150);

pub(crate) fn type_str(text: &str) {
    std::thread::sleep(TYPE_DELAY);
    inject(text).expect("type selected text");
}

#[cfg(not(target_os = "linux"))]
fn inject(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    use enigo::{Enigo, Keyboard, Settings};

    let mut enigo = Enigo::new(&Settings::default())?;
    enigo.text(text)?;
    Ok(())
}

// X11 has no "type this character" request. Each character is bound to a
// spare keycode, those keycodes are pressed, then the bindings are removed:
//
//   picker ──ChangeKeyboardMapping──▶ X server ──MapNotify──▶ terminal
//                                                             (re-reads
//          ◀────────── sync ──────────                          keymap)
//          ⏳ KEYMAP_SETTLE
//          ──XTest press/release────▶ X server ──KeyPress───▶ terminal
//          ⏳ KEYMAP_SETTLE
//          ──ChangeKeyboardMapping──▶ (revert)
//
// The terminal re-reads the keymap with its own round trip, so the press
// must wait for that to finish or the keycode resolves to nothing. The
// same wait before the revert keeps the press readable while it is handled.
//
// A sequence such as "👨‍🦰" (3 codepoints) takes one spare keycode per
// character so the whole batch costs the same two waits as a single one.
#[cfg(target_os = "linux")]
fn inject(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt as _, KEY_PRESS_EVENT, KEY_RELEASE_EVENT, Keycode, Keysym};
    use x11rb::protocol::xtest::ConnectionExt as _;
    use x11rb::wrapper::ConnectionExt as _;

    const KEYMAP_SETTLE: Duration = Duration::from_millis(50);
    // Keysyms below this value are the Latin-1 range and equal the codepoint;
    // everything else is encoded as 0x0100_0000 + codepoint.
    const LATIN1_END: u32 = 0x100;
    const UNICODE_KEYSYM_BASE: u32 = 0x0100_0000;
    const NO_SYMBOL: Keysym = 0;
    // Both the plain and shifted slot must hold the keysym, otherwise the
    // server fills the group with case variants.
    const KEYSYMS_PER_KEYCODE: u8 = 2;
    const CORE_KEYBOARD: u8 = 0;
    const NOW: u32 = 0;

    let (conn, screen_num) = x11rb::connect(None)?;
    let setup = conn.setup();
    let root = setup.roots[screen_num].root;
    let (min, max) = (setup.min_keycode, setup.max_keycode);

    // Spare keycodes: those with no keysym bound in any slot. Taken from the
    // top of the range, where real keys are least likely to appear later.
    let mapping = conn.get_keyboard_mapping(min, max - min + 1)?.reply()?;
    let per_code = usize::from(mapping.keysyms_per_keycode);
    let spares: Vec<Keycode> = mapping
        .keysyms
        .chunks(per_code)
        .enumerate()
        .rev()
        .filter(|(_, slots)| slots.iter().all(|&sym| sym == NO_SYMBOL))
        .map(|(offset, _)| min + offset as Keycode)
        .collect();
    if spares.is_empty() {
        return Err("no spare keycode".into());
    }

    let keysyms: Vec<Keysym> = text
        .chars()
        .map(|ch| ch as u32)
        .map(|cp| if cp < LATIN1_END { cp } else { UNICODE_KEYSYM_BASE + cp })
        .collect();

    // Longer than the spare pool: typed in batches, each reusing the pool.
    for batch in keysyms.chunks(spares.len()) {
        let bound: Vec<(Keycode, Keysym)> = spares.iter().copied().zip(batch.iter().copied()).collect();

        for &(keycode, keysym) in &bound {
            conn.change_keyboard_mapping(1, keycode, KEYSYMS_PER_KEYCODE, &[keysym, keysym])?;
        }
        conn.sync()?;
        std::thread::sleep(KEYMAP_SETTLE);

        for &(keycode, _) in &bound {
            conn.xtest_fake_input(KEY_PRESS_EVENT, keycode, NOW, root, 0, 0, CORE_KEYBOARD)?;
            conn.xtest_fake_input(KEY_RELEASE_EVENT, keycode, NOW, root, 0, 0, CORE_KEYBOARD)?;
        }
        conn.sync()?;
        std::thread::sleep(KEYMAP_SETTLE);

        for &(keycode, _) in &bound {
            conn.change_keyboard_mapping(1, keycode, KEYSYMS_PER_KEYCODE, &[NO_SYMBOL, NO_SYMBOL])?;
        }
        conn.sync()?;
    }
    Ok(())
}

// Needs a live X display, `alacritty` and `xdotool`: run with `--ignored`.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::process::{Child, Command};

    const ROUNDS: usize = 20;
    const EMOJI: &str = "\u{1F914}"; // thinking face, not on any keymap
    const SEQUENCE: &str = "\u{1F468}\u{200D}\u{1F9B0}"; // man: red hair, ZWJ sequence
    const FLAG: &str = "\u{1F1E8}\u{1F1E6}"; // flag: Canada, regional indicators
    const WINDOW_TITLE: &str = "charmander-typer-test";
    const WINDOW_STARTUP: Duration = Duration::from_millis(2000);
    const SETTLE: Duration = Duration::from_millis(300);

    struct Terminal {
        process: Child,
        output: std::path::PathBuf,
    }

    // A focused alacritty whose raw keyboard input is captured to a file.
    impl Terminal {
        fn open() -> Self {
            let output =
                std::env::temp_dir().join(format!("{WINDOW_TITLE}-{}.txt", std::process::id()));
            let _ = std::fs::remove_file(&output);

            let process = Command::new("alacritty")
                .args(["--title", WINDOW_TITLE, "-e", "bash", "-c"])
                .arg(format!("stty -icanon; cat > {}", output.display()))
                .spawn()
                .expect("spawn alacritty");
            std::thread::sleep(WINDOW_STARTUP);

            let pid = process.id().to_string();
            let search = Command::new("xdotool")
                .args(["search", "--sync", "--pid", &pid, "--name", WINDOW_TITLE])
                .output()
                .expect("xdotool search");
            let wid = String::from_utf8_lossy(&search.stdout).lines().next().unwrap().to_owned();
            Command::new("xdotool")
                .args(["windowactivate", "--sync", &wid])
                .status()
                .expect("xdotool windowactivate");
            std::thread::sleep(SETTLE);

            Self { process, output }
        }

        fn received(&self) -> String {
            std::thread::sleep(SETTLE);
            std::fs::read_to_string(&self.output).unwrap_or_default()
        }
    }

    impl Drop for Terminal {
        fn drop(&mut self) {
            let _ = self.process.kill();
            let _ = std::fs::remove_file(&self.output);
        }
    }

    // Terminals re-read the keymap asynchronously when a keycode is rebound,
    // so a synthetic key press fired too early resolves to nothing. Every
    // round must land, not just most of them.
    #[test]
    #[ignore]
    fn every_char_lands_in_terminal() {
        let terminal = Terminal::open();

        for _ in 0..ROUNDS {
            type_str(EMOJI);
            std::thread::sleep(SETTLE);
        }

        let landed = terminal.received().matches(EMOJI).count();
        assert_eq!(landed, ROUNDS, "{landed}/{ROUNDS} characters reached the terminal");
    }

    // Multi-codepoint emoji must arrive whole and in order, otherwise the
    // terminal shows the parts instead of the composed glyph.
    #[test]
    #[ignore]
    fn sequences_land_intact() {
        let terminal = Terminal::open();

        type_str(SEQUENCE);
        type_str(FLAG);

        assert_eq!(terminal.received(), format!("{SEQUENCE}{FLAG}"));
    }
}
