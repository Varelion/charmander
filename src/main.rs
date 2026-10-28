use std::io::Write;
use std::process::{Command, Stdio};

use enigo::{Enigo, Keyboard, Settings};

include!(concat!(env!("OUT_DIR"), "/characters.rs"));

const ROFI_PROMPT: &str = "charmander";

fn main() {
    let Some(selection) = pick_character() else {
        return;
    };

    let Some(glyph) = selection.split_whitespace().next() else {
        return;
    };

    type_text(glyph);
}

// Shows every named Unicode character in a rofi dmenu and returns the chosen line.
fn pick_character() -> Option<String> {
    let mut rofi = Command::new("rofi")
        .args(["-dmenu", "-i", "-p", ROFI_PROMPT])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("launch rofi (is it installed?)");

    let mut stdin = rofi.stdin.take().expect("rofi stdin");
    for (codepoint, name) in CHARACTERS {
        let Some(glyph) = char::from_u32(*codepoint) else {
            continue;
        };
        writeln!(stdin, "{glyph}  {name} (U+{codepoint:04X})").ok();
    }
    drop(stdin);

    let output = rofi.wait_with_output().expect("wait for rofi");
    if !output.status.success() {
        return None;
    }

    let selection = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if selection.is_empty() {
        None
    } else {
        Some(selection)
    }
}

// Types the given text into whichever window currently has focus.
fn type_text(text: &str) {
    let mut enigo = Enigo::new(&Settings::default()).expect("init enigo");
    enigo.text(text).expect("type selected character");
}
