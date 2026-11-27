//! A text object's legacy `Font` enum and its `FontFace` are one value, as
//! Roblox Studio has them: `Font` reads the enum item whose Font.fromEnum
//! face is the FontFace exactly (Enum.Font.Unknown when none is), and
//! writing `Font` writes that face. Places saved by today's Studio store
//! only FontFace, so a script reading `label.Font` for
//! TextService:GetTextSize got the class default (Legacy) and measured
//! Arimo at 1.5x the size: TECHDEV's keybind hints (Core.UI.Window.Action)
//! came out 80 px wide for "Inventory" instead of 49.
//!
//! Every expected value is Studio's own, read in a Studio Edit session
//! (vinegar-agent slot, 2026-11-24): GetTextSize for the hint strings,
//! the FontFace each Enum.Font item writes, and the item each face reads.

use crate::runtime::{RunMode, Runtime, Side, World};
use rbx_dom_weak::types::{Font, FontStyle, FontWeight};
use rbx_dom_weak::InstanceBuilder;
use rustudio_core::doc::Document;

/// Runs `src` as a client LocalScript with a child TextLabel `L` that has
/// only a stored FontFace (as a place file from Studio has it).
fn run(face: Font, src: &str) -> Vec<String> {
    let mut d = Document::new_place();
    let sp = d.find_child(d.dom.root_ref(), "StarterPlayer").unwrap();
    let sps = d.find_child(sp, "StarterPlayerScripts").unwrap();
    let s = d.dom.insert(
        sps,
        InstanceBuilder::new("LocalScript").with_name("T").with_property("Source", src.to_string()),
    );
    d.dom.insert(
        s,
        InstanceBuilder::new("TextLabel")
            .with_name("L")
            .with_property("FontFace", face)
            .with_property("TextSize", 14.0f32),
    );
    let rt = Runtime::new().unwrap();
    let mut w = World::new(d, RunMode::Run);
    w.side = Side::Client;
    rt.enter(w);
    rt.bind_globals().unwrap();
    rt.run_script(s);
    rt.step(1.0 / 60.0);
    rt.leave()
        .output
        .iter()
        .map(|(_, t)| t.split("  —").next().unwrap_or("").to_string())
        .collect()
}

fn expect(text: &[String], wants: &[&str]) {
    for want in wants {
        assert!(text.iter().any(|t| t == want), "{want:?} not in {text:#?}");
    }
}

fn source_sans() -> Font {
    Font::new("rbxasset://fonts/families/SourceSansPro.json", FontWeight::Regular, FontStyle::Normal)
}

/// TECHDEV's Action:Update, condensed: GetTextSize with the label's own
/// `.Font`. Studio: SourceSans 14 "Inventory" 49x14, "Bandage" 45x14,
/// "Sprint" 31x14; SourceSans 16 "G" 8x16, "B" 8x16, "L CTRL" 39x16. With
/// Enum.Font.Legacy (the wrong read) Studio gives 80x21, 79x21, 50x21 and
/// 17x24, 15x24, 74x24, which is what the hints were sized to before (and
/// what rustudio measured for Legacy in Play).
#[test]
fn font_reads_from_stored_font_face() {
    let has_fonts = rustudio_core::assets::resolve("rbxasset://fonts/families/SourceSansPro.json", None)
        .is_some_and(|p| p.is_file());
    let out = run(
        source_sans(),
        "local TS = game:GetService('TextService')\n\
         local l = script.L\n\
         print('font', l.Font)\n\
         for _, t in {'Inventory', 'Bandage', 'Sprint'} do\n\
             print('gts', t, TS:GetTextSize(t, l.TextSize, l.Font, Vector2.new(170, 18)))\n\
         end\n\
         for _, t in {'G', 'B', 'L CTRL'} do\n\
             print('gts', t, TS:GetTextSize(t, 16, l.Font, Vector2.new(130, 18)))\n\
         end",
    );
    expect(&out, &["font Enum.Font.SourceSans"]);
    if !has_fonts {
        eprintln!("no Studio content folder: GetTextSize widths skipped");
        return;
    }
    expect(
        &out,
        &[
            "gts Inventory 49, 14",
            "gts Bandage 45, 14",
            "gts Sprint 31, 14",
            "gts G 8, 16",
            "gts B 8, 16",
            "gts L CTRL 39, 16",
        ],
    );
}

/// The two-way mapping, Studio's answers: faces that are some Enum.Font's
/// read as it, others as Unknown; writing Font writes Font.fromEnum's face
/// (Gotham as GothamSSm.json, GothamSemibold as GothamMedium); writing
/// FontFace changes what Font reads; Enum.Font.Unknown writes nothing; a
/// write of either fires Changed for FontFace then Font; Font.fromEnum
/// gives the same faces and refuses Unknown.
#[test]
fn font_and_font_face_are_one_value() {
    let out = run(
        source_sans(),
        "local l = script.L\n\
         local function face(fam, w, s) l.FontFace = Font.new(fam, w or Enum.FontWeight.Regular, s or Enum.FontStyle.Normal) return l.Font end\n\
         local F = 'rbxasset://fonts/families/'\n\
         print('bold', face(F .. 'SourceSansPro.json', Enum.FontWeight.Bold))\n\
         print('medium', face(F .. 'SourceSansPro.json', Enum.FontWeight.Medium))\n\
         print('italic', face(F .. 'SourceSansPro.json', nil, Enum.FontStyle.Italic))\n\
         print('gothambold', face(F .. 'GothamSSm.json', Enum.FontWeight.Bold))\n\
         print('montserrat', face(F .. 'Montserrat.json'))\n\
         print('arimo', face(F .. 'Arimo.json'))\n\
         print('legacyarial', face(F .. 'LegacyArial.json'))\n\
         print('cloud', face('rbxassetid://12187365364'))\n\
         l.Font = Enum.Font.GothamBold\n\
         print('set gothambold', l.FontFace, l.Font)\n\
         l.Font = Enum.Font.GothamSemibold\n\
         print('set gothamsemibold', l.FontFace, l.Font)\n\
         l.Font = Enum.Font.Arial\n\
         print('set arial', l.FontFace)\n\
         l.Font = Enum.Font.SourceSansItalic\n\
         print('set ssitalic', l.FontFace)\n\
         l.Font = Enum.Font.Unknown\n\
         print('set unknown', l.FontFace, l.Font)\n\
         local n = Instance.new('TextButton')\n\
         print('default', n.Font, n.FontFace)\n\
         local fires = {}\n\
         n.Changed:Connect(function(p) table.insert(fires, p) end)\n\
         n.Font = Enum.Font.SourceSans\n\
         n.FontFace = Font.new(F .. 'SourceSansPro.json', Enum.FontWeight.Bold)\n\
         task.wait()\n\
         print('changed', table.concat(fires, ','))\n\
         print('fromEnum', Font.fromEnum(Enum.Font.Arial), Font.fromEnum(Enum.Font.Gotham))\n\
         print('fromEnum unknown', pcall(Font.fromEnum, Enum.Font.Unknown))",
    );
    expect(
        &out,
        &[
            "bold Enum.Font.SourceSansBold",
            "medium Enum.Font.Unknown",
            "italic Enum.Font.SourceSansItalic",
            "gothambold Enum.Font.GothamBold",
            "montserrat Enum.Font.Unknown",
            "arimo Enum.Font.Arimo",
            "legacyarial Enum.Font.Legacy",
            "cloud Enum.Font.Unknown",
            "set gothambold Font { Family = rbxasset://fonts/families/GothamSSm.json, Weight = Bold, Style = Normal } Enum.Font.GothamBold",
            "set gothamsemibold Font { Family = rbxasset://fonts/families/GothamSSm.json, Weight = Medium, Style = Normal } Enum.Font.GothamMedium",
            "set arial Font { Family = rbxasset://fonts/families/Arial.json, Weight = Regular, Style = Normal }",
            "set ssitalic Font { Family = rbxasset://fonts/families/SourceSansPro.json, Weight = Regular, Style = Italic }",
            "set unknown Font { Family = rbxasset://fonts/families/SourceSansPro.json, Weight = Regular, Style = Italic } Enum.Font.SourceSansItalic",
            "default Enum.Font.Legacy Font { Family = rbxasset://fonts/families/LegacyArial.json, Weight = Regular, Style = Normal }",
            "changed FontFace,Font,FontFace,Font",
            "fromEnum Font { Family = rbxasset://fonts/families/Arial.json, Weight = Regular, Style = Normal } Font { Family = rbxasset://fonts/families/GothamSSm.json, Weight = Regular, Style = Normal }",
            "fromEnum unknown false Enum.Font.Unknown can't be passed to Font.fromEnum",
        ],
    );
}
