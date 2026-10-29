// Skin tones are emoji modifiers: a base such as 👍 followed by one of five
// modifier codepoints gives 👍🏻 … 👍🏿. Multi-person emoji carry one per
// person (🧑🏻‍🤝‍🧑🏼). The picker lists only bases and keeps the toned
// variants behind Alt+Enter, or applies a configured default on Enter.

use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkinTone {
    Light,
    MediumLight,
    Medium,
    MediumDark,
    Dark,
}

impl SkinTone {
    const ALL: [SkinTone; 5] =
        [Self::Light, Self::MediumLight, Self::Medium, Self::MediumDark, Self::Dark];

    // Emoji modifier codepoints U+1F3FB..U+1F3FF (Fitzpatrick types 1-2 to 6).
    fn modifier(self) -> char {
        match self {
            Self::Light => '\u{1F3FB}',
            Self::MediumLight => '\u{1F3FC}',
            Self::Medium => '\u{1F3FD}',
            Self::MediumDark => '\u{1F3FE}',
            Self::Dark => '\u{1F3FF}',
        }
    }

    // Value accepted in settings.conf: `skin_tone = medium-dark`.
    pub(crate) fn setting_name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::MediumLight => "medium-light",
            Self::Medium => "medium",
            Self::MediumDark => "medium-dark",
            Self::Dark => "dark",
        }
    }

    pub(crate) fn from_setting(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tone| tone.setting_name() == value)
    }

    pub(crate) fn setting_names() -> Vec<&'static str> {
        Self::ALL.into_iter().map(Self::setting_name).collect()
    }
}

fn is_modifier(ch: char) -> bool {
    SkinTone::ALL.iter().any(|tone| tone.modifier() == ch)
}

pub(crate) fn has_tone(glyph: &str) -> bool {
    glyph.chars().any(is_modifier)
}

// The glyph with every skin tone removed: 🧑🏻‍🤝‍🧑🏼 → 🧑‍🤝‍🧑.
pub(crate) fn base_of(glyph: &str) -> String {
    glyph.chars().filter(|ch| !is_modifier(*ch)).collect()
}

// Every base that has at least one toned variant, for a cheap per-row check.
pub(crate) fn toned_bases(table: &[(&str, &str, &str)]) -> HashSet<String> {
    table.iter().filter(|(glyph, _, _)| has_tone(glyph)).map(|(glyph, _, _)| base_of(glyph)).collect()
}

// All toned variants of a base, in table order.
pub(crate) fn variants_of<'a>(
    base: &str,
    table: &'a [(&'a str, &'a str, &'a str)],
) -> Vec<(&'a str, &'a str, &'a str)> {
    table
        .iter()
        .filter(|(glyph, _, _)| has_tone(glyph) && base_of(glyph) == base)
        .map(|(glyph, name, keywords)| (*glyph, *name, *keywords))
        .collect()
}

// The variant where every person has the given tone, if the table has one.
pub(crate) fn with_tone<'a>(
    base: &str,
    tone: SkinTone,
    table: &'a [(&'a str, &'a str, &'a str)],
) -> Option<&'a str> {
    variants_of(base, table)
        .into_iter()
        .map(|(glyph, _, _)| glyph)
        .find(|glyph| glyph.chars().filter(|ch| is_modifier(*ch)).all(|ch| ch == tone.modifier()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &[(&str, &str, &str)] = &[
        ("\u{1F44D}", "THUMBS UP SIGN", ""),
        ("\u{1F44D}\u{1F3FB}", "THUMBS UP: LIGHT SKIN TONE", ""),
        ("\u{1F44D}\u{1F3FD}", "THUMBS UP: MEDIUM SKIN TONE", ""),
        ("\u{1F91D}", "HANDSHAKE", ""),
        ("\u{1FAF1}\u{1F3FB}\u{200D}\u{1FAF2}\u{1F3FC}", "HANDSHAKE: LIGHT, MEDIUM-LIGHT", ""),
        ("\u{1FAF1}\u{1F3FD}\u{200D}\u{1FAF2}\u{1F3FD}", "HANDSHAKE: MEDIUM, MEDIUM", ""),
        ("\u{1F346}", "AUBERGINE / EGGPLANT", ""),
    ];

    #[test]
    fn strips_tones_to_base() {
        assert_eq!(base_of("\u{1F44D}\u{1F3FD}"), "\u{1F44D}");
        assert_eq!(base_of("\u{1FAF1}\u{1F3FB}\u{200D}\u{1FAF2}\u{1F3FC}"), "\u{1FAF1}\u{200D}\u{1FAF2}");
        assert!(has_tone("\u{1F44D}\u{1F3FD}"));
        assert!(!has_tone("\u{1F44D}"));
    }

    #[test]
    fn lists_variants_of_base() {
        let names: Vec<&str> = variants_of("\u{1F44D}", TABLE).into_iter().map(|(_, n, _)| n).collect();
        assert_eq!(names, ["THUMBS UP: LIGHT SKIN TONE", "THUMBS UP: MEDIUM SKIN TONE"]);
        assert!(variants_of("\u{1F346}", TABLE).is_empty());
    }

    #[test]
    fn collects_bases_with_tones() {
        let bases = toned_bases(TABLE);
        assert!(bases.contains("\u{1F44D}"));
        assert!(bases.contains("\u{1FAF1}\u{200D}\u{1FAF2}"));
        assert!(!bases.contains("\u{1F346}"));
    }

    #[test]
    fn applies_default_tone_to_every_person() {
        assert_eq!(with_tone("\u{1F44D}", SkinTone::Medium, TABLE), Some("\u{1F44D}\u{1F3FD}"));
        assert_eq!(with_tone("\u{1F44D}", SkinTone::Dark, TABLE), None);
        assert_eq!(
            with_tone("\u{1FAF1}\u{200D}\u{1FAF2}", SkinTone::Medium, TABLE),
            Some("\u{1FAF1}\u{1F3FD}\u{200D}\u{1FAF2}\u{1F3FD}")
        );
        assert_eq!(with_tone("\u{1FAF1}\u{200D}\u{1FAF2}", SkinTone::Light, TABLE), None);
    }

    #[test]
    fn parses_setting_names() {
        assert_eq!(SkinTone::from_setting("medium-dark"), Some(SkinTone::MediumDark));
        assert_eq!(SkinTone::from_setting("none"), None);
        assert_eq!(SkinTone::from_setting(""), None);
        assert_eq!(SkinTone::setting_names().join(", "), "light, medium-light, medium, medium-dark, dark");
    }
}
