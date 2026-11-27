//! Auto-indent for the script editor: what Studio does as code is typed
//! (Settings > Script Editor > Auto Indent Rule, Tab Width, Indent Using
//! Spaces).
//!
//! * Enter keeps the previous line's indentation, one level deeper after
//!   a line that opens a block (`then`, `do`, `function (...)`, `repeat`,
//!   `else`, `{`, `(`); with the caret right before the block's closer
//!   (`if x then|end`, `{|}`), the closer goes to a line of its own at the
//!   outer level and the caret stays on the inner one.
//! * A closer typed at the start of a line (`end`, `else`, `elseif`,
//!   `until`, `}`) steps the line out to its opener's indentation; typing
//!   on past it (`endTime`) steps it back in.
//! * A Tab inserts the indentation unit (the soft Tab key on Android,
//!   [`tab`] / [`untab`] for a selection's lines).
//!
//! The rules are pure functions of the text and the caret ([`on_insert`]),
//! so the same code runs for a key event in egui's `TextEdit`
//! (script_editor.rs) and, on Android, on the keyboard's mirror while the
//! keyboard's own operation is being made (android/ime.rs
//! `native_code_fixup`): the operation the keyboard reports already holds
//! the indentation, so the field, the mirror and the keyboard agree on the
//! text without another round trip. Positions are chars (code points),
//! egui's `CCursor` index.
//!
//! The tokens are read with a small Luau lexer that skips strings and `--`
//! comments on the line; a long string or block comment that spans lines
//! is not tracked (its lines are indented as code — a rare case that
//! misplaces nothing the user can't fix).

/// The indentation unit and the rule in force.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Indent {
    /// One level: a tab, or Tab Width spaces (Indent Using Spaces).
    pub unit: String,
    /// Tab Width: what a tab counts as when a line mixes tabs and spaces.
    pub width: usize,
}

/// A replacement to make: chars `[a, b)` become `text`, the caret goes to
/// `cursor` (a single caret, or a selection for [`tab`] / [`untab`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixup {
    pub a: usize,
    pub b: usize,
    pub text: String,
    pub cursor: (usize, usize),
}

impl Indent {
    /// Studio's defaults: a tab, width 4.
    pub fn default_tab() -> Self {
        Indent { unit: "\t".into(), width: 4 }
    }

    pub fn spaces(width: usize) -> Self {
        Indent { unit: " ".repeat(width.max(1)), width: width.max(1) }
    }
}

/// The indentation in force from Studio's settings, or None with Auto
/// Indent Rule set to Off.
pub fn settings() -> Option<Indent> {
    use crate::studio_settings::{self, Val};
    if let Some(Val::Enum(0)) = studio_settings::get("Studio", "Auto Indent Rule") {
        return None;
    }
    let width = studio_settings::num("Studio", "Tab Width", 4.0).clamp(1.0, 16.0) as usize;
    Some(if studio_settings::flag("Studio", "Indent Using Spaces", false) { Indent::spaces(width) } else { Indent { unit: "\t".into(), width } })
}

/// The indentation unit for a Tab regardless of the rule (Tab inserts it
/// with auto-indent off too).
pub fn tab_unit() -> Indent {
    settings().unwrap_or_else(|| {
        use crate::studio_settings;
        let width = studio_settings::num("Studio", "Tab Width", 4.0).clamp(1.0, 16.0) as usize;
        if studio_settings::flag("Studio", "Indent Using Spaces", false) { Indent::spaces(width) } else { Indent { unit: "\t".into(), width } }
    })
}

/// `inserted` was just typed into `text`, the caret now at `cursor`
/// (chars) right after it: the indentation to make, if any.
pub fn on_insert(text: &str, cursor: usize, inserted: &str, ind: &Indent) -> Option<Fixup> {
    let chars: Vec<char> = text.chars().collect();
    if cursor > chars.len() || inserted.is_empty() {
        return None;
    }
    if inserted == "\n" {
        return after_enter(&chars, cursor, ind);
    }
    if inserted.contains('\n') {
        return None;
    }
    closer_typed(&chars, cursor, ind)
}

/// Enter: the caret at `cursor`, right after the new line's `\n`.
fn after_enter(chars: &[char], cursor: usize, ind: &Indent) -> Option<Fixup> {
    if cursor == 0 || chars[cursor - 1] != '\n' {
        return None;
    }
    let prev_end = cursor - 1;
    let prev_start = line_start(chars, prev_end);
    let prev: String = chars[prev_start..prev_end].iter().collect();
    let base = leading_ws(&prev);
    let opens = opens_after_leading_closer(&prev) > 0;
    let mut indent = base.to_string();
    if opens {
        indent.push_str(&ind.unit);
    }
    // The new line as it is: its own leading whitespace is replaced.
    let cur_end = line_end(chars, cursor);
    let rest: String = chars[cursor..cur_end].iter().collect();
    let rest_trim = rest.trim_start();
    let rest_ws = rest.chars().count() - rest_trim.chars().count();
    if opens && starts_with_closer(rest_trim) {
        // `if x then|end`: the closer to a line of its own, one level out.
        let text = format!("{indent}\n{base}");
        let cursor_after = cursor + indent.chars().count();
        return Some(Fixup { a: cursor, b: cursor + rest_ws, text, cursor: (cursor_after, cursor_after) });
    }
    if indent.chars().count() == rest_ws && indent == rest.chars().take(rest_ws).collect::<String>() {
        return None;
    }
    let cursor_after = cursor + indent.chars().count();
    Some(Fixup { a: cursor, b: cursor + rest_ws, text: indent, cursor: (cursor_after, cursor_after) })
}

/// A word typed at the start of a line: a closer steps out to its
/// opener's indentation; an identifier that begins like one (`endTime`)
/// steps back in.
fn closer_typed(chars: &[char], cursor: usize, ind: &Indent) -> Option<Fixup> {
    let ls = line_start(chars, cursor);
    let le = line_end(chars, cursor);
    let before: String = chars[ls..cursor].iter().collect();
    let after: String = chars[cursor..le].iter().collect();
    if !after.trim().is_empty() {
        return None;
    }
    let word = before.trim_start();
    let ws_len = before.chars().count() - word.chars().count();
    let ws: String = before.chars().take(ws_len).collect();
    if word.is_empty() || !word.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '}') {
        return None;
    }
    let closer = is_closer_word(word);
    let ident = word.chars().all(|c| c.is_alphanumeric() || c == '_');
    let began_as_closer = !closer && ident && ["end", "else", "until"].iter().any(|k| word.starts_with(k) && word.chars().count() > k.chars().count());
    if !closer && !began_as_closer {
        return None;
    }
    let opener_ws = opener_indent(chars, ls, if closer { word } else { "end" })?;
    let target = if closer {
        opener_ws
    } else if ws == opener_ws {
        // (stepped out for `end`, now `endTime`: back to the block's level)
        format!("{opener_ws}{}", ind.unit)
    } else {
        return None;
    };
    if target == ws {
        return None;
    }
    let cursor_after = cursor - ws_len + target.chars().count();
    Some(Fixup { a: ls, b: ls + ws_len, text: target, cursor: (cursor_after, cursor_after) })
}

/// A Tab at the selection `(a, b)` (chars, in any order): the unit at the
/// caret, or in front of every line a selection touches.
pub fn tab(text: &str, sel: (usize, usize), ind: &Indent) -> Fixup {
    let chars: Vec<char> = text.chars().collect();
    let (lo, hi) = (sel.0.min(sel.1).min(chars.len()), sel.0.max(sel.1).min(chars.len()));
    if lo == hi || !chars[lo..hi].contains(&'\n') {
        let after = lo + ind.unit.chars().count();
        return Fixup { a: lo, b: hi, text: ind.unit.clone(), cursor: (after, after) };
    }
    let ls = line_start(&chars, lo);
    let le = line_end(&chars, hi.saturating_sub(1).max(lo));
    let block: String = chars[ls..le].iter().collect();
    let mut out = String::new();
    let mut lines = 0;
    for (i, line) in block.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if !line.trim().is_empty() {
            out.push_str(&ind.unit);
            lines += 1;
        }
        out.push_str(line);
    }
    let unit = ind.unit.chars().count();
    let new_lo = lo + unit;
    let new_hi = hi + unit * lines;
    Fixup { a: ls, b: le, text: out, cursor: if sel.0 <= sel.1 { (new_lo, new_hi) } else { (new_hi, new_lo) } }
}

/// Shift+Tab: one level off the front of every line the selection
/// touches (a tab, or up to Tab Width spaces).
pub fn untab(text: &str, sel: (usize, usize), ind: &Indent) -> Option<Fixup> {
    let chars: Vec<char> = text.chars().collect();
    let (lo, hi) = (sel.0.min(sel.1).min(chars.len()), sel.0.max(sel.1).min(chars.len()));
    let ls = line_start(&chars, lo);
    let le = line_end(&chars, hi.saturating_sub(1).max(lo));
    let block: String = chars[ls..le].iter().collect();
    let mut out = String::new();
    let mut removed_first = 0;
    let mut removed = 0;
    for (i, line) in block.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let take = if line.starts_with('\t') {
            1
        } else {
            line.chars().take(ind.width).take_while(|c| *c == ' ').count()
        };
        if i == 0 {
            removed_first = take;
        }
        removed += take;
        out.push_str(&line[line.char_indices().nth(take).map_or(line.len(), |(i, _)| i)..]);
    }
    if removed == 0 {
        return None;
    }
    let new_lo = lo.saturating_sub(removed_first).max(ls);
    let new_hi = hi.saturating_sub(removed).max(new_lo);
    Some(Fixup { a: ls, b: le, text: out, cursor: if sel.0 <= sel.1 { (new_lo, new_hi) } else { (new_hi, new_lo) } })
}

/// Applies a fixup to a text (chars).
pub fn apply(text: &mut String, fix: &Fixup) {
    let chars: Vec<char> = text.chars().collect();
    let a = fix.a.min(chars.len());
    let b = fix.b.clamp(a, chars.len());
    *text = chars[..a].iter().chain(fix.text.chars().collect::<Vec<_>>().iter()).chain(chars[b..].iter()).collect();
}

// ------------------------------------------------------------------ lines

fn line_start(chars: &[char], i: usize) -> usize {
    let mut s = i.min(chars.len());
    while s > 0 && chars[s - 1] != '\n' {
        s -= 1;
    }
    s
}

fn line_end(chars: &[char], i: usize) -> usize {
    let mut e = i.min(chars.len());
    while e < chars.len() && chars[e] != '\n' {
        e += 1;
    }
    e
}

fn leading_ws(line: &str) -> &str {
    let n = line.len() - line.trim_start_matches([' ', '\t']).len();
    &line[..n]
}

// ------------------------------------------------------------------ tokens

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tok {
    Open,
    Close,
    /// `else`: closes the block before it and opens the next.
    ElseLike,
}

fn is_closer_word(w: &str) -> bool {
    matches!(w, "end" | "else" | "elseif" | "until" | "}")
}

fn starts_with_closer(rest: &str) -> bool {
    if rest.starts_with('}') || rest.starts_with(')') {
        return true;
    }
    let word: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    matches!(word.as_str(), "end" | "until" | "else" | "elseif")
}

/// The block tokens of a line of Luau, strings and comments skipped.
fn tokens(line: &str) -> Vec<Tok> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '-' if chars.get(i + 1) == Some(&'-') => break,
            '"' | '\'' => {
                i += 1;
                while i < chars.len() && chars[i] != c {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            '[' if chars.get(i + 1) == Some(&'[') || chars.get(i + 1) == Some(&'=') => {
                // A long string: to its end on this line, or the line's end.
                let mut j = i + 1;
                let mut eq = 0;
                while chars.get(j) == Some(&'=') {
                    eq += 1;
                    j += 1;
                }
                if chars.get(j) == Some(&'[') {
                    let close: Vec<char> = std::iter::once(']').chain(std::iter::repeat_n('=', eq)).chain(std::iter::once(']')).collect();
                    let mut k = j + 1;
                    let mut found = None;
                    while k + close.len() <= chars.len() {
                        if chars[k..k + close.len()] == close[..] {
                            found = Some(k + close.len());
                            break;
                        }
                        k += 1;
                    }
                    i = found.unwrap_or(chars.len());
                } else {
                    i += 1;
                }
            }
            '{' | '(' => {
                out.push(Tok::Open);
                i += 1;
            }
            '}' | ')' => {
                out.push(Tok::Close);
                i += 1;
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                match word.as_str() {
                    "then" | "do" | "function" | "repeat" => out.push(Tok::Open),
                    "end" | "until" | "elseif" => out.push(Tok::Close),
                    "else" => out.push(Tok::ElseLike),
                    _ => {}
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// What a line leaves open past the closer it begins with, if any:
/// `if x then` 1, `else` 1, `end)` -1, `foo(function()` 2, `x = 1` 0.
fn opens_after_leading_closer(line: &str) -> i32 {
    let toks = tokens(line);
    let mut net = 0;
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Open => net += 1,
            Tok::Close if i == 0 => {}
            Tok::Close => net -= 1,
            Tok::ElseLike if i == 0 => net += 1,
            Tok::ElseLike => {}
        }
    }
    net
}

/// The indentation of the line that opened the block `closer` (at the
/// start of the line beginning at `line_start`) closes: the lines above
/// are read backwards, token by token, until the opener is found.
fn opener_indent(chars: &[char], line_start: usize, closer: &str) -> Option<String> {
    let _ = closer;
    let mut need: i32 = 1;
    let mut end = line_start;
    let mut lines = 0;
    while end > 0 && lines < 4000 {
        let le = end - 1; // the '\n' ending the line above
        let ls = self::line_start(chars, le);
        let line: String = chars[ls..le].iter().collect();
        for t in tokens(&line).iter().rev() {
            match t {
                Tok::Open => {
                    need -= 1;
                    if need == 0 {
                        return Some(leading_ws(&line).to_string());
                    }
                }
                Tok::Close => need += 1,
                Tok::ElseLike => {
                    need -= 1;
                    if need == 0 {
                        return Some(leading_ws(&line).to_string());
                    }
                    need += 1;
                }
            }
        }
        end = ls;
        lines += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enter(text: &str, ind: &Indent) -> String {
        // `|` marks the caret before Enter; the result has the caret as `|`.
        let caret = text.find('|').unwrap();
        let mut t = text.replace('|', "");
        let at = t[..caret].chars().count();
        let chars: Vec<char> = t.chars().collect();
        t = chars[..at].iter().chain(std::iter::once(&'\n')).chain(chars[at..].iter()).collect();
        let cursor = at + 1;
        let (t, c) = match on_insert(&t, cursor, "\n", ind) {
            Some(f) => {
                let mut t2 = t.clone();
                apply(&mut t2, &f);
                (t2, f.cursor.0)
            }
            None => (t, cursor),
        };
        let chars: Vec<char> = t.chars().collect();
        chars[..c].iter().chain(std::iter::once(&'|')).chain(chars[c..].iter()).collect()
    }

    fn typed(text: &str, ind: &Indent) -> String {
        // The text with `|` the caret after the last char typed.
        let caret = text.find('|').unwrap();
        let t = text.replace('|', "");
        let at = t[..caret].chars().count();
        let last: String = t.chars().nth(at - 1).into_iter().collect();
        match on_insert(&t, at, &last, ind) {
            Some(f) => {
                let mut t2 = t.clone();
                apply(&mut t2, &f);
                let chars: Vec<char> = t2.chars().collect();
                chars[..f.cursor.0].iter().chain(std::iter::once(&'|')).chain(chars[f.cursor.0..].iter()).collect()
            }
            None => text.to_string(),
        }
    }

    #[test]
    fn enter_keeps_and_deepens_indent() {
        let sp = Indent::spaces(4);
        assert_eq!(enter("if x then|", &sp), "if x then\n    |");
        assert_eq!(enter("    x = 1|", &sp), "    x = 1\n    |");
        assert_eq!(enter("x = 1|", &sp), "x = 1\n|");
        assert_eq!(enter("local function f(a, b)|", &sp), "local function f(a, b)\n    |");
        assert_eq!(enter("foo(function()|", &sp), "foo(function()\n    |");
        assert_eq!(enter("local t = {|", &sp), "local t = {\n    |");
        assert_eq!(enter("    else|", &sp), "    else\n        |");
        assert_eq!(enter("    elseif y then|", &sp), "    elseif y then\n        |");
        assert_eq!(enter("    end|", &sp), "    end\n    |");
        assert_eq!(enter("    end)|", &sp), "    end)\n    |");
        assert_eq!(enter("print(\"then\")|", &sp), "print(\"then\")\n|");
        assert_eq!(enter("x = 1 -- then|", &sp), "x = 1 -- then\n|");
        assert_eq!(enter("if x then return end|", &sp), "if x then return end\n|");
        let tab = Indent::default_tab();
        assert_eq!(enter("\twhile a do|", &tab), "\twhile a do\n\t\t|");
    }

    #[test]
    fn enter_in_the_middle_and_before_closers() {
        let sp = Indent::spaces(4);
        assert_eq!(enter("if x then|end", &sp), "if x then\n    |\nend");
        assert_eq!(enter("    local t = {|}", &sp), "    local t = {\n        |\n    }");
        assert_eq!(enter("f(|)", &sp), "f(\n    |\n)");
        // Enter with text after the caret: the rest goes down, indented.
        assert_eq!(enter("    x = |1", &sp), "    x = \n    |1");
        // Enter at a line's end above other lines (the diff case).
        assert_eq!(enter("if a then|\n    y()\nend", &sp), "if a then\n    |\n    y()\nend");
    }

    #[test]
    fn closers_step_out() {
        let sp = Indent::spaces(4);
        assert_eq!(typed("if a then\n    x()\n    end|", &sp), "if a then\n    x()\nend|");
        assert_eq!(typed("if a then\n    if b then\n        x()\n    end\n    end|", &sp), "if a then\n    if b then\n        x()\n    end\nend|");
        assert_eq!(typed("if a then\n    x()\n    else|", &sp), "if a then\n    x()\nelse|");
        assert_eq!(typed("if a then\n    x()\n    elseif|", &sp), "if a then\n    x()\nelseif|");
        assert_eq!(typed("repeat\n    x()\n    until|", &sp), "repeat\n    x()\nuntil|");
        assert_eq!(typed("local t = {\n    a = 1,\n    }|", &sp), "local t = {\n    a = 1,\n}|");
        assert_eq!(typed("foo(function()\n    x()\n    end|", &sp), "foo(function()\n    x()\nend|");
        // Already right: nothing to do.
        assert_eq!(typed("if a then\n    x()\nend|", &sp), "if a then\n    x()\nend|");
        // Not at the line's start, or not a closer: untouched.
        assert_eq!(typed("if a then\n    x = end|", &sp), "if a then\n    x = end|");
        assert_eq!(typed("if a then\n    en|", &sp), "if a then\n    en|");
        // An identifier that begins like `end` steps back in.
        assert_eq!(typed("if a then\nendT|", &sp), "if a then\n    endT|");
    }

    #[test]
    fn tab_and_untab() {
        let sp = Indent::spaces(4);
        assert_eq!(tab("ab", (1, 1), &sp), Fixup { a: 1, b: 1, text: "    ".into(), cursor: (5, 5) });
        let f = tab("a\nb\nc", (0, 3), &sp);
        assert_eq!(f.text, "    a\n    b");
        assert_eq!(f.cursor, (4, 11));
        let f = untab("    a\n    b", (0, 11), &sp).unwrap();
        assert_eq!(f.text, "a\nb");
        assert_eq!(f.cursor, (0, 3));
        assert!(untab("a", (0, 0), &sp).is_none());
        let tab_ = Indent::default_tab();
        let f = untab("\t\tx", (3, 3), &tab_).unwrap();
        assert_eq!((f.text.as_str(), f.cursor), ("\tx", (2, 2)));
    }

    #[test]
    fn lexer_skips_strings_and_comments() {
        assert_eq!(tokens("print('then', \"do\") -- end"), vec![Tok::Open, Tok::Close]);
        assert_eq!(tokens("x = [[then]] .. y"), vec![]);
        assert_eq!(tokens("x = [==[ then ]] ]==] do"), vec![Tok::Open]);
        assert_eq!(tokens("else"), vec![Tok::ElseLike]);
        assert_eq!(opens_after_leading_closer("end)"), -1);
        assert_eq!(opens_after_leading_closer("}, {"), 1);
    }
}
