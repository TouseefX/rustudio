//! The legacy `Font` enum of text objects as a view of their `FontFace`,
//! as Roblox has it (the reflection database: Font serializes as FontFace,
//! migration FontToFontFace). Reading Font gives the Enum.Font item whose
//! face is the FontFace exactly, Enum.Font.Unknown when none is; writing
//! Font writes that item's face; writing Enum.Font.Unknown changes nothing.
//!
//! The table is Studio's own: each Enum.Font item assigned to a TextLabel
//! and its FontFace read back, in a Studio Edit session (2026-11-24,
//! vinegar-agent; selftest/text_font.rs pins it). These are the faces
//! Studio stores, not the files it draws: Arial.json and GothamSSm.json
//! aren't in Studio's content folder (gui.rs draws Arimo and Montserrat).

use rbx_dom_weak::types::{Font, FontStyle, FontWeight};

use FontWeight::{Bold, ExtraBold, Heavy, Light, Medium, Regular, SemiBold};

/// (Enum.Font value, family file name, weight, italic).
const FACES: &[(u32, &str, FontWeight, bool)] = &[
    (0, "LegacyArial", Regular, false),
    (1, "Arial", Regular, false),
    (2, "Arial", Bold, false),
    (3, "SourceSansPro", Regular, false),
    (4, "SourceSansPro", Bold, false),
    (5, "SourceSansPro", Light, false),
    (6, "SourceSansPro", Regular, true),
    (7, "AccanthisADFStd", Regular, false),
    (8, "Guru", Regular, false),
    (9, "ComicNeueAngular", Regular, false),
    (10, "Inconsolata", Regular, false),
    (11, "HighwayGothic", Regular, false),
    (12, "Zekton", Regular, false),
    (13, "PressStart2P", Regular, false),
    (14, "Balthazar", Regular, false),
    (15, "RomanAntique", Regular, false),
    (16, "SourceSansPro", SemiBold, false),
    (17, "GothamSSm", Regular, false),
    (18, "GothamSSm", Medium, false),
    (19, "GothamSSm", Bold, false),
    (20, "GothamSSm", Heavy, false),
    (21, "AmaticSC", Regular, false),
    (22, "Bangers", Regular, false),
    (23, "Creepster", Regular, false),
    (24, "DenkOne", Regular, false),
    (25, "Fondamento", Regular, false),
    (26, "FredokaOne", Regular, false),
    (27, "GrenzeGotisch", Regular, false),
    (28, "IndieFlower", Regular, false),
    (29, "JosefinSans", Regular, false),
    (30, "Jura", Regular, false),
    (31, "Kalam", Regular, false),
    (32, "LuckiestGuy", Regular, false),
    (33, "Merriweather", Regular, false),
    (34, "Michroma", Regular, false),
    (35, "Nunito", Regular, false),
    (36, "Oswald", Regular, false),
    (37, "PatrickHand", Regular, false),
    (38, "PermanentMarker", Regular, false),
    (39, "Roboto", Regular, false),
    (40, "RobotoCondensed", Regular, false),
    (41, "RobotoMono", Regular, false),
    (42, "Sarpanch", Regular, false),
    (43, "SpecialElite", Regular, false),
    (44, "TitilliumWeb", Regular, false),
    (45, "Ubuntu", Regular, false),
    (46, "BuilderSans", Regular, false),
    (47, "BuilderSans", Medium, false),
    (48, "BuilderSans", Bold, false),
    (49, "BuilderSans", ExtraBold, false),
    (50, "Arimo", Regular, false),
    (51, "Arimo", Bold, false),
];

/// Enum.Font.Unknown's value.
pub const UNKNOWN: u32 = 100;

const PREFIX: &str = "rbxasset://fonts/families/";

/// The FontFace Studio writes for an Enum.Font value (None: Unknown or not
/// an item).
pub fn face_of_enum(value: u32) -> Option<Font> {
    FACES.iter().find(|f| f.0 == value).map(|&(_, fam, weight, italic)| {
        Font::new(
            &format!("{PREFIX}{fam}.json"),
            weight,
            if italic { FontStyle::Italic } else { FontStyle::Normal },
        )
    })
}

/// The Enum.Font value Studio reads for a FontFace (UNKNOWN when no item's
/// face is exactly this one).
pub fn enum_of_face(face: &Font) -> u32 {
    let Some(fam) = face.family.strip_prefix(PREFIX).and_then(|f| f.strip_suffix(".json")) else {
        return UNKNOWN;
    };
    let italic = matches!(face.style, FontStyle::Italic);
    FACES
        .iter()
        .find(|f| f.1 == fam && f.2 == face.weight && f.3 == italic)
        .map_or(UNKNOWN, |f| f.0)
}

/// The family file drawn for a stored family: Studio's content has no
/// GothamSSm.json or Arial.json (what Font.Gotham* / Font.Arial* store); it
/// draws Montserrat and Arimo for them (GetTextSize: Arial = Arimo).
pub fn drawn_family(family: &str) -> String {
    if family.ends_with("/GothamSSm.json") {
        format!("{PREFIX}Montserrat.json")
    } else if family == "rbxasset://fonts/families/Arial.json" {
        format!("{PREFIX}Arimo.json")
    } else {
        family.to_string()
    }
}

/// Whether `class` has the legacy Font enum as a view of FontFace (TextLabel,
/// TextButton, TextBox: the reflection database's FontToFontFace migration).
pub fn has_font_view(class: &str) -> bool {
    static CACHE: std::sync::Mutex<Option<std::collections::HashMap<String, bool>>> =
        std::sync::Mutex::new(None);
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(Default::default);
    if let Some(&b) = m.get(class) {
        return b;
    }
    let db = rustudio_core::doc::db();
    let b = db.classes.get(class).is_some_and(|c| {
        db.superclasses_iter(c).any(|c| {
            c.properties
                .get("Font")
                .is_some_and(|p| format!("{:?}", p.kind).contains("FontToFontFace"))
        })
    });
    m.insert(class.to_string(), b);
    b
}
