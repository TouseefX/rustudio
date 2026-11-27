//! Studio's script editor auto-indent (Studio Settings > Script Editor >
//! Auto Indent Rule / Tab Width / Indent Using Spaces / Format On Type /
//! Auto Clean Empty Line).
//!
//! Enum.AutoIndentRule (api_docs en-us.json):
//! - Off: "Disables automatic indentation." (Enter starts at column 0)
//! - Absolute (Studio's default): "Indents new lines according to the
//!   script's block structure." One indent unit per open block line.
//! - Relative: "...according to the script's block structure and the
//!   indentation of the reference line." The previous non-blank line's
//!   indentation, one unit more after a line that opens a block.
//!
//! Openers: `function`, `do`, `then`, `repeat`, `(`, `{`, `[` (Luau's
//! `if a then b else c` expressions don't open one). Closers: `end`,
//! `until`, `)`, `}`, `]`, and `else`/`elseif` sit at their `if`'s level.
//! Several openers on one line (`foo(function()`) indent once.
//!
//! Everything here works on byte offsets; the editor converts egui's char
//! cursors at the edges.

use egui::text::{CCursor, CCursorRange};
use egui::{Event, Key};

use super::{tokenize, Tok};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    Off,
    Absolute,
    Relative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cfg {
    pub rule: Rule,
    /// Indent Using Spaces (Studio default: tabs).
    pub spaces: bool,
    /// Tab Width (Studio default 4).
    pub width: usize,
    /// Format On Type: typing a closer at a line's start re-indents it.
    pub format_on_type: bool,
    /// Auto Clean Empty Line: Enter on a whitespace-only line empties it.
    pub clean_empty: bool,
}

impl Default for Cfg {
    /// Studio's defaults (studio_settings_schema.rs).
    fn default() -> Self {
        Cfg { rule: Rule::Absolute, spaces: false, width: 4, format_on_type: true, clean_empty: true }
    }
}

impl Cfg {
    pub fn from_settings() -> Self {
        use crate::studio_settings::{flag, get, num, Val};
        let d = Cfg::default();
        Cfg {
            rule: match get("Studio", "Auto Indent Rule") {
                Some(Val::Enum(0)) => Rule::Off,
                Some(Val::Enum(2)) => Rule::Relative,
                _ => Rule::Absolute,
            },
            spaces: flag("Studio", "Indent Using Spaces", d.spaces),
            width: num("Studio", "Tab Width", d.width as f64).clamp(1.0, 16.0) as usize,
            format_on_type: flag("Studio", "Format On Type", d.format_on_type),
            clean_empty: flag("Studio", "Auto Clean Empty Line", d.clean_empty),
        }
    }

    fn unit(&self) -> String {
        if self.spaces { " ".repeat(self.width) } else { "\t".to_string() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Block,
    /// `if` used as an expression: no `end`, its `else` finishes it.
    IfExpr,
    Paren,
    Brace,
    Bracket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Frame {
    kind: Kind,
    /// Byte offset of the start of the opener's line.
    line: usize,
}

fn line_start(src: &str, at: usize) -> usize {
    src[..at].rfind('\n').map_or(0, |p| p + 1)
}

fn line_end(src: &str, at: usize) -> usize {
    src[at..].find('\n').map_or(src.len(), |p| at + p)
}

fn is_ws(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Leading spaces/tabs of the line starting at `ls`.
fn leading_ws(src: &str, ls: usize) -> &str {
    let line = &src[ls..line_end(src, ls)];
    &line[..line.len() - line.trim_start_matches(is_ws).len()]
}

fn blank(s: &str) -> bool {
    s.chars().all(|c| is_ws(c) || c == '\r')
}

/// Width of `s` in columns, tabs to the next stop.
fn columns(s: &str, width: usize) -> usize {
    s.chars().fold(0, |col, c| if c == '\t' { (col / width + 1) * width } else { col + 1 })
}

/// Tokens that aren't whitespace or comments, as (byte range, kind).
fn significant(src: &str) -> impl Iterator<Item = (std::ops::Range<usize>, Tok)> + '_ {
    tokenize(src)
        .into_iter()
        .filter(move |(r, t)| *t != Tok::Comment && !(*t == Tok::Plain && src[r.clone()].trim().is_empty()))
}

fn opener(kind: Kind, closer: &str) -> bool {
    matches!((kind, closer), (Kind::Paren, ")") | (Kind::Brace, "}") | (Kind::Bracket, "]"))
}

/// Pops what `word` closes; returns the outermost frame it closed.
fn close(stack: &mut Vec<Frame>, word: &str) -> Option<Frame> {
    let at = match word {
        "end" | "until" => stack.iter().rposition(|f| f.kind == Kind::Block)?,
        ")" | "}" | "]" => stack.iter().rposition(|f| opener(f.kind, word))?,
        _ => return None,
    };
    let f = stack[at];
    stack.truncate(at);
    Some(f)
}

/// The blocks and brackets still open at byte `end`.
fn scan(src: &str, end: usize) -> Vec<Frame> {
    let src = &src[..end];
    let mut stack: Vec<Frame> = Vec::new();
    let mut prev: Option<(Tok, &str)> = None;
    for (r, tok) in significant(src) {
        let w = &src[r.clone()];
        let ls = line_start(src, r.start);
        let push = |stack: &mut Vec<Frame>, kind| stack.push(Frame { kind, line: ls });
        let top = stack.last().map(|f| f.kind);
        match (tok, w) {
            (Tok::Keyword, "function" | "do" | "repeat") => push(&mut stack, Kind::Block),
            (Tok::Keyword, "if") => {
                // `x = if a then b else c`: an expression, never `end`ed.
                let expr = match prev {
                    Some((Tok::Op, p)) => !matches!(p, ")" | "]" | "}" | ";"),
                    Some((Tok::Keyword, p)) => matches!(p, "return" | "and" | "or" | "not" | "in" | "until" | "while" | "if" | "elseif")
                        || (matches!(p, "then" | "else") && top == Some(Kind::IfExpr)),
                    _ => false,
                };
                if expr {
                    push(&mut stack, Kind::IfExpr);
                }
            }
            (Tok::Keyword, "then") => {
                if top != Some(Kind::IfExpr) {
                    push(&mut stack, Kind::Block);
                }
            }
            // An if statement's `else`/`elseif ... then` take over its block
            // (one level, opened on their own line).
            (Tok::Keyword, "else") => match top {
                Some(Kind::IfExpr) => {
                    stack.pop();
                }
                Some(Kind::Block) => {
                    stack.pop();
                    push(&mut stack, Kind::Block);
                }
                _ => {}
            },
            (Tok::Keyword, "elseif") => {
                if top == Some(Kind::Block) {
                    stack.pop();
                }
            }
            (Tok::Keyword, "end" | "until") | (Tok::Op, ")" | "}" | "]") => {
                close(&mut stack, w);
            }
            (Tok::Op, "(") => push(&mut stack, Kind::Paren),
            (Tok::Op, "{") => push(&mut stack, Kind::Brace),
            (Tok::Op, "[") => push(&mut stack, Kind::Bracket),
            _ => {}
        }
        prev = Some((tok, w));
    }
    stack
}

/// Applies the closers a line starts with (`end`, `end)`, `}`, `else`...)
/// to `stack`; returns the outermost frame they close.
fn leading_closers(stack: &mut Vec<Frame>, line: &str) -> Option<Frame> {
    let mut first: Option<Frame> = None;
    for (i, (r, tok)) in significant(line).enumerate() {
        let w = &line[r];
        let closed = match (tok, w) {
            (Tok::Keyword, "end" | "until") | (Tok::Op, ")" | "}" | "]") => close(stack, w),
            // `else`/`elseif` line up with their `if`: out one level.
            (Tok::Keyword, "else" | "elseif") if i == 0 => {
                if matches!(stack.last(), Some(f) if matches!(f.kind, Kind::Block | Kind::IfExpr)) { stack.pop() } else { None }
            }
            _ => break,
        };
        match closed {
            Some(f) => first = Some(f),
            None => break,
        }
    }
    first
}

/// Open blocks, counting lines that opened several once.
fn levels(stack: &[Frame]) -> usize {
    let mut n = 0;
    let mut last = None;
    for f in stack {
        if last != Some(f.line) {
            n += 1;
            last = Some(f.line);
        }
    }
    n
}

/// The previous non-blank line's start before the line at `ls`.
fn reference_line(src: &str, ls: usize) -> Option<usize> {
    let mut end = ls.checked_sub(1)?;
    loop {
        let s = line_start(src, end);
        if !blank(&src[s..end]) {
            return Some(s);
        }
        end = s.checked_sub(1)?;
    }
}

/// The indentation the line starting at `ls` should have; None for Off.
fn indent_for_line(src: &str, ls: usize, cfg: &Cfg) -> Option<String> {
    let content = ls + leading_ws(src, ls).len();
    let mut stack = scan(src, ls);
    let closed = leading_closers(&mut stack, &src[content..line_end(src, ls)]);
    match cfg.rule {
        Rule::Off => None,
        Rule::Absolute => Some(cfg.unit().repeat(levels(&stack))),
        Rule::Relative => {
            if let Some(f) = closed {
                return Some(leading_ws(src, f.line).to_string());
            }
            let Some(r) = reference_line(src, ls) else { return Some(String::new()) };
            let mut ws = leading_ws(src, r).to_string();
            if stack.last().is_some_and(|f| f.line == r) {
                ws.push_str(&cfg.unit());
            }
            Some(ws)
        }
    }
}

/// Enter: replaces `a..b` with a line break and the new line's indentation.
/// Returns the cursor (byte) after it.
pub fn enter(text: &mut String, a: usize, b: usize, cfg: &Cfg) -> usize {
    let (a, b) = (a.min(b), a.max(b));
    text.replace_range(a..b, "");
    let mut at = a;
    let ls = line_start(text, at);
    let le = line_end(text, at);
    // A CRLF script keeps CRLF; a cursor between `\r` and `\n` breaks before the `\r`.
    let crlf = le > ls && text.as_bytes()[le - 1] == b'\r';
    if crlf && at == le {
        at -= 1;
    }
    let nl = if crlf { "\r\n" } else { "\n" };
    // What moves down loses its leading blanks (the new indent replaces them).
    let rest_ws = text[at..le].len() - text[at..le].trim_start_matches(is_ws).len();
    text.replace_range(at..at + rest_ws, "");
    let le = le - rest_ws;
    // `{|}` / `function()|end`: the closer goes one line further down.
    let rest = &text[at..le];
    let split = {
        let mut stack = scan(text, at);
        leading_closers(&mut stack, rest).is_some_and(|f| f.line == ls)
    };
    let mut probe = String::with_capacity(text.len() + 2);
    probe.push_str(&text[..at]);
    probe.push_str(nl);
    let new_ls = probe.len();
    if !split {
        probe.push_str(&text[at..]);
    } else {
        probe.push_str(&text[le..]);
    }
    let indent = indent_for_line(&probe, new_ls, cfg).unwrap_or_default();
    let mut insert = format!("{nl}{indent}");
    let cursor_in_insert = insert.len();
    if split {
        let mut probe2 = String::with_capacity(text.len() + 2 * nl.len() + indent.len());
        probe2.push_str(&text[..at]);
        probe2.push_str(nl);
        probe2.push_str(&indent);
        probe2.push_str(nl);
        let ls2 = probe2.len();
        probe2.push_str(&text[at..]);
        let outer = indent_for_line(&probe2, ls2, cfg).unwrap_or_default();
        insert.push_str(nl);
        insert.push_str(&outer);
    }
    text.insert_str(at, &insert);
    let mut cursor = at + cursor_in_insert;
    // Auto Clean Empty Line: the line left behind keeps no stray indent.
    if cfg.clean_empty && cfg.rule != Rule::Off && blank(&text[ls..at]) && at > ls {
        let n = text[ls..at].trim_end_matches('\r').len();
        text.replace_range(ls..ls + n, "");
        cursor -= n;
    }
    cursor
}

/// After typing at `cursor` (byte): when the line's first word just
/// became (or just stopped being) a closer, re-indent the line. Returns
/// the new cursor.
pub fn after_typing(text: &mut String, cursor: usize, typed: &str, cfg: &Cfg) -> usize {
    if !cfg.format_on_type || cfg.rule == Rule::Off {
        return cursor;
    }
    let ls = line_start(text, cursor);
    let le = line_end(text, cursor);
    let ws = leading_ws(text, ls).len();
    if ls + ws > cursor || !blank(&text[cursor..le]) {
        return cursor;
    }
    let word = &text[ls + ws..cursor];
    let is_closer = |w: &str| {
        !w.is_empty()
            && significant(w).all(|(r, t)| matches!((t, &w[r]), (Tok::Keyword, "end" | "until" | "else" | "elseif") | (Tok::Op, ")" | "}" | "]")))
    };
    // (`else` on its way to `elseif` stays put)
    let was = word.strip_suffix(typed).is_some_and(is_closer) && !"elseif".starts_with(word);
    if !is_closer(word) && !was {
        return cursor;
    }
    let Some(target) = indent_for_line(text, ls, cfg) else { return cursor };
    let current = &text[ls..ls + ws];
    // A closer only moves a line out; an identifier that began like one
    // (`endTime`) goes back to where Enter put it.
    let apply = if is_closer(word) { columns(&target, cfg.width) < columns(current, cfg.width) } else { target != current };
    if !apply {
        return cursor;
    }
    text.replace_range(ls..ls + ws, &target);
    cursor + target.len() - ws
}

fn byte_of(text: &str, ch: usize) -> usize {
    text.char_indices().nth(ch).map_or(text.len(), |(i, _)| i)
}

fn char_of(text: &str, byte: usize) -> usize {
    text[..byte].chars().count()
}

/// Whether an event could edit the text or move the cursor.
fn edits(ev: &Event) -> bool {
    matches!(
        ev,
        Event::Key { pressed: true, .. }
            | Event::Text(_)
            | Event::Paste(_)
            | Event::Cut
            | Event::Copy
            | Event::Ime(_)
            | Event::PointerButton { .. }
            | Event::Touch { .. }
    )
}

/// Runs this frame's typing, Enter and (Indent Using Spaces) Tab presses
/// for the focused TextEdit `id` on `text` ahead of egui, in order, up to
/// the first event egui must handle itself (Backspace, arrows, paste, a
/// click...), which keeps the rest of the frame's events. Each Enter
/// becomes one edit: the break and its indentation together. Returns
/// whether `text` changed.
pub fn handle_keys(ctx: &egui::Context, id: egui::Id, text: &mut String, cfg: &Cfg) -> bool {
    if cfg.rule == Rule::Off && !cfg.spaces {
        return false;
    }
    let Some(mut state) = egui::TextEdit::load_state(ctx, id) else { return false };
    let Some(range) = state.cursor.char_range() else { return false };
    let mut primary = byte_of(text, range.primary.index.0);
    let mut secondary = byte_of(text, range.secondary.index.0);
    let mut changed = false;
    ctx.input_mut(|i| {
        let mut keep = Vec::with_capacity(i.events.len());
        let mut done = false;
        for ev in std::mem::take(&mut i.events) {
            if done || !edits(&ev) {
                keep.push(ev);
                continue;
            }
            match &ev {
                Event::Key { key: Key::Enter, modifiers: m, .. }
                    if cfg.rule != Rule::Off && !m.ctrl && !m.command && !m.alt && !m.mac_cmd =>
                {
                    primary = enter(text, secondary, primary, cfg);
                }
                // egui ignores these too (Enter is the key event).
                Event::Text(t) if t == "\n" || t == "\r" => keep.push(ev),
                Event::Text(t) if !t.is_empty() && cfg.rule != Rule::Off => {
                    let (a, b) = (secondary.min(primary), secondary.max(primary));
                    text.replace_range(a..b, t);
                    primary = after_typing(text, a + t.len(), t, cfg);
                }
                Event::Key { key: Key::Tab, modifiers: m, .. } if cfg.spaces && m.is_none() && primary == secondary => {
                    let ls = line_start(text, primary);
                    let col = columns(&text[ls..primary], cfg.width);
                    let pad = " ".repeat(cfg.width - col % cfg.width);
                    text.insert_str(primary, &pad);
                    primary += pad.len();
                }
                _ => {
                    done = true;
                    keep.push(ev);
                    continue;
                }
            }
            secondary = primary;
            changed = true;
        }
        i.events = keep;
    });
    if changed {
        let c = CCursor::new(char_of(text, primary));
        state.cursor.set_char_range(Some(CCursorRange::one(c)));
        state.store(ctx, id);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABS: Cfg = Cfg { rule: Rule::Absolute, spaces: false, width: 4, format_on_type: true, clean_empty: true };
    const REL: Cfg = Cfg { rule: Rule::Relative, ..ABS };
    const OFF: Cfg = Cfg { rule: Rule::Off, ..ABS };
    const SPACES: Cfg = Cfg { spaces: true, ..ABS };

    /// Presses Enter at the `|` in `src`; returns the text with `|` at the cursor.
    fn press_enter(src: &str, cfg: &Cfg) -> String {
        let at = src.find('|').unwrap();
        let mut t = src.replacen('|', "", 1);
        let c = enter(&mut t, at, at, cfg);
        t.insert(c, '|');
        t
    }

    fn type_text(src: &str, typed: &str, cfg: &Cfg) -> String {
        let at = src.find('|').unwrap();
        let mut t = src.replacen('|', "", 1);
        t.insert_str(at, typed);
        let c = after_typing(&mut t, at + typed.len(), typed, cfg);
        t.insert(c, '|');
        t
    }

    #[test]
    fn keeps_the_current_lines_indent() {
        assert_eq!(press_enter("function f()\n\tprint(1)|", &ABS), "function f()\n\tprint(1)\n\t|");
        assert_eq!(press_enter("function f()\n\tprint(1)|", &REL), "function f()\n\tprint(1)\n\t|");
        // The user's report: inside a function, Enter went to column 0.
        assert_eq!(press_enter("local function f()\n\tlocal a = 1|\nend", &ABS), "local function f()\n\tlocal a = 1\n\t|\nend");
        // Relative keeps whatever the reference line has (4 spaces here).
        assert_eq!(press_enter("function f()\n    print(1)|", &REL), "function f()\n    print(1)\n    |");
        assert_eq!(press_enter("    x = 1|", &REL), "    x = 1\n    |");
        // Absolute indents by structure, with the configured unit.
        assert_eq!(press_enter("function f()\n    print(1)|", &SPACES), "function f()\n    print(1)\n    |");
        assert_eq!(press_enter("x = 1|", &ABS), "x = 1\n|");
        assert_eq!(press_enter("|", &ABS), "\n|");
    }

    #[test]
    fn indents_after_openers() {
        for (line, exp) in [
            ("function f()|", "function f()\n\t|"),
            ("local f = function(a, b)|", "local f = function(a, b)\n\t|"),
            ("if x then|", "if x then\n\t|"),
            ("for i = 1, 10 do|", "for i = 1, 10 do\n\t|"),
            ("while true do|", "while true do\n\t|"),
            ("do|", "do\n\t|"),
            ("repeat|", "repeat\n\t|"),
            ("local t = {|", "local t = {\n\t|"),
            ("print(|", "print(\n\t|"),
            ("foo(function()|", "foo(function()\n\t|"),
            ("if a then -- comment|", "if a then -- comment\n\t|"),
        ] {
            assert_eq!(press_enter(line, &ABS), exp, "{line}");
            assert_eq!(press_enter(line, &REL), exp, "{line} relative");
        }
        assert_eq!(press_enter("if a then\n\tx()\nelse|", &ABS), "if a then\n\tx()\nelse\n\t|");
        assert_eq!(press_enter("if a then\n\tx()\nelseif b then|", &ABS), "if a then\n\tx()\nelseif b then\n\t|");
        assert_eq!(press_enter("if a then\n\tx()\nelseif b then|", &REL), "if a then\n\tx()\nelseif b then\n\t|");
        assert_eq!(press_enter("function f()\n\tif a then|", &ABS), "function f()\n\tif a then\n\t\t|");
        assert_eq!(press_enter("function f()\n\tif a then|", &SPACES), "function f()\n\tif a then\n        |");
        assert_eq!(press_enter("function f()\n    if a then|", &Cfg { rule: Rule::Relative, spaces: true, ..ABS }), "function f()\n    if a then\n        |");
    }

    #[test]
    fn closed_blocks_and_expressions_dont_indent() {
        for line in [
            "local f = function() end|",
            "local t = {1, 2}|",
            "print(\"then do function(\")|",
            "local s = [[ function ]]|",
            "-- if a then|",
            "local x = if a then 1 else 2|",
            "return if a then b elseif c then d else e|",
        ] {
            assert_eq!(press_enter(line, &ABS), format!("{}\n|", &line[..line.len() - 1]), "{line}");
        }
        assert_eq!(press_enter("function f()\n\tx()\nend|", &ABS), "function f()\n\tx()\nend\n|");
        assert_eq!(press_enter("function f()\n\tx()\n\tend|", &REL), "function f()\n\tx()\n\tend\n\t|");
        assert_eq!(press_enter("foo(function()\n\tx()\nend)|", &ABS), "foo(function()\n\tx()\nend)\n|");
        assert_eq!(press_enter("repeat\n\tx()\nuntil done|", &ABS), "repeat\n\tx()\nuntil done\n|");
    }

    #[test]
    fn mid_line_selection_and_split() {
        // The rest of the line moves down to the new indent.
        assert_eq!(press_enter("function f()\n\tlocal a = 1|   local b = 2\nend", &ABS), "function f()\n\tlocal a = 1\n\t|local b = 2\nend");
        // A selection is replaced by the break.
        let mut t = "if a then\n\tfoo()\nend".to_string();
        let (a, b) = (t.find("foo").unwrap(), t.find("()").unwrap() + 2);
        let c = enter(&mut t, b, a, &ABS);
        assert_eq!((t.as_str(), c), ("if a then\n\n\t\nend", "if a then\n\n\t".len()));
        // Between an opener and its closer: the closer goes a line down.
        assert_eq!(press_enter("local t = {|}", &ABS), "local t = {\n\t|\n}");
        assert_eq!(press_enter("\tfoo(|)", &REL), "\tfoo(\n\t\t|\n\t)");
        assert_eq!(press_enter("function f()|end", &ABS), "function f()\n\t|\nend");
        // A closer that doesn't close what this line opened just lines up.
        assert_eq!(press_enter("function f()\n\tx()|end", &ABS), "function f()\n\tx()\n|end");
    }

    #[test]
    fn crlf_and_clean_empty_line() {
        assert_eq!(press_enter("function f()|\r\nend", &ABS), "function f()\r\n\t|\r\nend");
        assert_eq!(press_enter("function f()\r|\nend", &ABS), "function f()\r\n\t|\r\nend");
        // Enter on an indent-only line leaves it empty.
        assert_eq!(press_enter("function f()\n\t|\nend", &ABS), "function f()\n\n\t|\nend");
        assert_eq!(press_enter("function f()\n\t|", &Cfg { clean_empty: false, ..ABS }), "function f()\n\t\n\t|");
        assert_eq!(press_enter("if a then\r\n\t|\r\nend", &ABS), "if a then\r\n\r\n\t|\r\nend");
        // Relative looks past blank lines for its reference.
        assert_eq!(press_enter("if a then\n\tx()\n\n|", &REL), "if a then\n\tx()\n\n\n\t|");
    }

    #[test]
    fn off_rule_is_column_zero() {
        assert_eq!(press_enter("function f()\n\tx()|", &OFF), "function f()\n\tx()\n|");
        assert_eq!(type_text("function f()\n\t|", "end", &OFF), "function f()\n\tend|");
    }

    #[test]
    fn typing_a_closer_dedents() {
        assert_eq!(type_text("function f()\n\tx()\n\ten|", "d", &ABS), "function f()\n\tx()\nend|");
        assert_eq!(type_text("if a then\n\tx()\n\tels|", "e", &ABS), "if a then\n\tx()\nelse|");
        assert_eq!(type_text("if a then\n\tx()\nelse|", "i", &ABS), "if a then\n\tx()\nelsei|");
        assert_eq!(type_text("if a then\n\tx()\nelsei|", "f", &ABS), "if a then\n\tx()\nelseif|");
        assert_eq!(type_text("repeat\n\tx()\n\tunti|", "l", &ABS), "repeat\n\tx()\nuntil|");
        assert_eq!(type_text("local t = {\n\t1,\n\t|", "}", &ABS), "local t = {\n\t1,\n}|");
        assert_eq!(type_text("foo(function()\n\tx()\n\tend|", ")", &ABS), "foo(function()\n\tx()\nend)|");
        // Relative lines up with the opener's own indentation.
        assert_eq!(type_text("  if a then\n      x()\n      en|", "d", &REL), "  if a then\n      x()\n  end|");
        // Never moves a line in, and only at the line's first word.
        assert_eq!(type_text("function f()\nen|", "d", &ABS), "function f()\nend|");
        assert_eq!(type_text("function f()\n\tx = a.en|", "d", &ABS), "function f()\n\tx = a.end|");
        // `endTime`: back to the block's indent once it isn't `end`.
        assert_eq!(type_text("function f()\n\tx()\nend|", "T", &ABS), "function f()\n\tx()\n\tendT|");
        assert_eq!(type_text("function f()\n\tx()\n\ten|", "d", &Cfg { format_on_type: false, ..ABS }), "function f()\n\tx()\n\tend|");
    }

    /// Drives a real egui TextEdit: Enter, typing and Tab reach it through
    /// `handle_keys` the way the script tab calls it.
    #[test]
    fn through_egui_text_edit() {
        fn run(cfg: &Cfg, start: &str, events: Vec<Event>) -> String {
            let ctx = egui::Context::default();
            let id = egui::Id::new("script_text_test");
            let mut text = start.replacen('|', "", 1);
            let at = start.find('|').unwrap();
            let frame = |ctx: &egui::Context, text: &mut String, events: Vec<Event>| {
                let input = egui::RawInput { events, ..Default::default() };
                let mut out = ctx.run_ui(input, |ui| {
                    handle_keys(ui.ctx(), id, text, cfg);
                    ui.add(egui::TextEdit::multiline(text).id(id).code_editor().lock_focus(true));
                });
                // No renderer here: the font atlas upload is dropped on purpose.
                out.textures_delta.clear();
            };
            frame(&ctx, &mut text, vec![]);
            ctx.memory_mut(|m| m.request_focus(id));
            let mut st = egui::TextEdit::load_state(&ctx, id).unwrap();
            st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(char_of(&text, at)))));
            st.store(&ctx, id);
            frame(&ctx, &mut text, vec![]);
            frame(&ctx, &mut text, events);
            let c = egui::TextEdit::load_state(&ctx, id).unwrap().cursor.char_range().unwrap().primary.index;
            let b = byte_of(&text, c.0);
            text.insert(b, '|');
            text
        }
        let key = |k: Key, m: egui::Modifiers| Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m };
        let enter = key(Key::Enter, egui::Modifiers::NONE);
        let src = "local function f()\n\tlocal a = 1|\nend";
        assert_eq!(run(&ABS, src, vec![enter.clone()]), "local function f()\n\tlocal a = 1\n\t|\nend");
        // Typing, Enter and more typing in one frame stay in order.
        assert_eq!(
            run(&ABS, src, vec![Event::Text("2".into()), enter.clone(), Event::Text("x".into())]),
            "local function f()\n\tlocal a = 12\n\tx|\nend"
        );
        // Shift+Enter is a line break too.
        assert_eq!(run(&ABS, "if a then|", vec![key(Key::Enter, egui::Modifiers::SHIFT)]), "if a then\n\t|");
        // Enter, Enter, `end`: the blank line is cleaned and `end` dedents.
        assert_eq!(
            run(&ABS, "if a then|", vec![enter.clone(), enter.clone(), Event::Text("e".into()), Event::Text("n".into()), Event::Text("d".into())]),
            "if a then\n\nend|"
        );
        // A Backspace hands the rest to egui (no reordering).
        assert_eq!(run(&ABS, "if a then|", vec![key(Key::Backspace, egui::Modifiers::NONE), enter.clone()]), "if a the\n|");
        // Off: egui's own plain line break.
        assert_eq!(run(&OFF, src, vec![enter.clone()]), "local function f()\n\tlocal a = 1\n|\nend");
        // Indent Using Spaces: Tab to the next stop.
        assert_eq!(run(&SPACES, "ab|", vec![key(Key::Tab, egui::Modifiers::NONE)]), "ab  |");
        assert_eq!(run(&ABS, "ab|", vec![key(Key::Tab, egui::Modifiers::NONE)]), "ab\t|");
    }
}
