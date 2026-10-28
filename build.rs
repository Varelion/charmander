use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=data/UnicodeData.txt");

    let raw = fs::read_to_string("data/UnicodeData.txt").expect("read UnicodeData.txt");
    let mut entries = Vec::new();

    for line in raw.lines() {
        let fields: Vec<&str> = line.split(';').collect();
        let (codepoint_hex, name) = (fields[0], fields[1]);

        // Skip range markers like "<CJK Ideograph, First>" - not real characters
        if name.starts_with('<') {
            continue;
        }

        let codepoint = u32::from_str_radix(codepoint_hex, 16).expect("valid hex codepoint");
        entries.push((codepoint, name.to_string()));
    }

    let mut out = String::from("pub static CHARACTERS: &[(u32, &str)] = &[\n");
    for (codepoint, name) in &entries {
        out.push_str(&format!("    ({codepoint}, {name:?}),\n"));
    }
    out.push_str("];\n");

    let dest = Path::new(&env::var("OUT_DIR").unwrap()).join("characters.rs");
    fs::write(dest, out).expect("write generated character table");
}
