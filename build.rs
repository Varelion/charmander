use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

const UNICODE_DATA: &str = "data/UnicodeData.txt";
const EMOJI_TEST: &str = "data/emoji-test.txt";
// Only this status marks the canonical form of an emoji; the other
// statuses are the same emoji with selectors missing or just components.
const FULLY_QUALIFIED: &str = "; fully-qualified";

// Every single codepoint with a name, e.g. "1F914;THINKING FACE;So;..."
fn single_codepoints(raw: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for line in raw.lines() {
        let fields: Vec<&str> = line.split(';').collect();
        let (codepoint_hex, name) = (fields[0], fields[1]);

        // Skip range markers like "<CJK Ideograph, First>" - not real characters
        if name.starts_with('<') {
            continue;
        }

        let codepoint = u32::from_str_radix(codepoint_hex, 16).expect("valid hex codepoint");
        let glyph = char::from_u32(codepoint).expect("scalar value").to_string();
        entries.push((glyph, name.to_string()));
    }
    entries
}

const NAME_SEPARATOR: &str = " / ";

// Emoji from emoji-test.txt, e.g.
//   "1F468 200D 1F9B0 ; fully-qualified # 👨‍🦰 E11.0 man: red hair"
// Returns the codepoints and the upper-cased common name.
fn emoji_entries(raw: &str) -> Vec<(Vec<u32>, String)> {
    let mut entries = Vec::new();
    for line in raw.lines() {
        let Some((codepoints, rest)) = line.split_once(FULLY_QUALIFIED) else {
            continue;
        };
        let codepoints: Vec<u32> = codepoints
            .split_whitespace()
            .map(|hex| u32::from_str_radix(hex, 16).expect("valid hex codepoint"))
            .collect();

        // Comment is "# <glyph> E<version> <name>"
        let comment = rest.split_once('#').map(|(_, c)| c.trim()).unwrap_or_default();
        let name = comment.splitn(3, ' ').nth(2).expect("emoji name").to_uppercase();
        entries.push((codepoints, name));
    }
    entries
}

fn glyph(codepoints: &[u32]) -> String {
    codepoints.iter().map(|&cp| char::from_u32(cp).expect("scalar value")).collect()
}

fn main() {
    println!("cargo:rerun-if-changed={UNICODE_DATA}");
    println!("cargo:rerun-if-changed={EMOJI_TEST}");

    let unicode_data = fs::read_to_string(UNICODE_DATA).expect("read UnicodeData.txt");
    let emoji_test = fs::read_to_string(EMOJI_TEST).expect("read emoji-test.txt");

    let (singles, sequences): (Vec<_>, Vec<_>) =
        emoji_entries(&emoji_test).into_iter().partition(|(cps, _)| cps.len() == 1);

    // Single emoji are already in UnicodeData.txt, but often under another
    // name: U+1F346 is "AUBERGINE" there and "eggplant" everywhere else.
    // Both names are kept so either one finds it.
    let common_names: HashMap<String, String> =
        singles.into_iter().map(|(cps, name)| (glyph(&cps), name)).collect();
    let mut entries = single_codepoints(&unicode_data);
    for (glyph, name) in &mut entries {
        let Some(common) = common_names.get(glyph) else {
            continue;
        };
        if common != name {
            name.push_str(NAME_SEPARATOR);
            name.push_str(common);
        }
    }

    // Sequences sort next to their base character, so "👍" is followed by
    // its skin tones. The sort is stable, so singles stay ahead of them.
    entries.extend(sequences.into_iter().map(|(cps, name)| (glyph(&cps), name)));
    entries.sort_by_key(|(glyph, _)| glyph.chars().next().map(|c| c as u32));

    let mut out = String::from("pub static CHARACTERS: &[(&str, &str)] = &[\n");
    for (glyph, name) in &entries {
        out.push_str(&format!("    ({glyph:?}, {name:?}),\n"));
    }
    out.push_str("];\n");

    let dest = Path::new(&env::var("OUT_DIR").unwrap()).join("characters.rs");
    fs::write(dest, out).expect("write generated character table");
}
