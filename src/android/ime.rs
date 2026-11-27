//! The soft keyboard's editor: an `InputConnection` for the keyboard, and
//! what it types, applied to the editor's text fields.
//!
//! A `NativeActivity` offers the keyboard no `InputConnection`, so the
//! keyboard runs in the framework's fallback mode: single characters come
//! as key events, and all else — a word composed and committed, a
//! suggestion, a swiped word, the keyboard's paste key — is a
//! `KeyEvent.ACTION_MULTIPLE` whose characters only the Java object
//! carries, which native code never sees (text missing); keyboards that
//! both compose and send keys delivered some of it twice (doubled,
//! garbled); the script editor, where every word is composed, showed
//! nothing typed. The fix needs a Java `View` with an `InputConnection`,
//! which a native library can't make on its own: `ImeBridge`
//! (tools/android/java/dev/rustudio/studio/ImeBridge.java), compiled by
//! tools/android/build-dex.sh into assets/android/ImeBridge.dex, which is
//! built into this library ([`DEX`]) and loaded at start with
//! `InMemoryDexClassLoader` ([`install`]) — no Java, Gradle or SDK in the
//! build. The class's doc comment has the Java side; this is the editor's:
//!
//! * The Java view mirrors the focused field (text, selection, the
//!   keyboard's composing span) and reports each change the keyboard makes
//!   as one replacement — code points `[a, b)` become `text`, the
//!   selection moves to `[sel_start, sel_end)` — through the native method
//!   [`native_op`] (the Java main thread) into [`OPS`]. The editor's thread
//!   applies one replacement a frame ([`before_frame`], `App::raw_input_hook`):
//!   the field's cursor range is set to `[a, b)` (egui's `TextEditState`)
//!   and the text comes as `Event::Text` (or Backspace for a deletion,
//!   Enter for a line), which egui's `TextEdit` handles as a typed
//!   replacement of the selection; a Roblox TextBox takes it as a selection
//!   and an insertion of its own (gui_input.rs `Gui::keyboard`,
//!   `gui::share().tb_ime`). Replacements that follow one another at the
//!   cursor (typing) are merged into one.
//! * At the end of the frame ([`end_frame`]) the field's text and
//!   selection are read (egui's `OutputEvent::FocusGained / ValueChanged /
//!   TextSelectionChanged` carry the text; the state the selection) and
//!   checked: after an operation, against the hash the Java side computed
//!   of its mirror ([`hash`], the same on both sides); otherwise against
//!   what was last agreed. A difference — the user tapped the caret
//!   elsewhere, pasted, undid, the editor reformatted, a hardware key —
//!   sends the Java side the editor's state (`setState`), with the number of
//!   the last operation it includes, so it is applied only when no newer
//!   operation is in flight. The editor's text is the truth throughout.
//! * Characters that come as key events through the window while the
//!   keyboard is up through the bridge are held back a frame and dropped
//!   if an operation of the keyboard's arrives with them (a keyboard
//!   typing them both ways), else typed ([`key_text`]: a hardware
//!   keyboard's). An operation that merely repeats the one before it is
//!   dropped ([`repeats`]).
//! * [`show`] / [`hide`] are what keyboard.rs asks the keyboard up and down
//!   with: `show` hands the Java view the field's kind ([`Kind`]: a line,
//!   many lines, code), text and selection, gives it the view focus and
//!   asks `InputMethodManager.showSoftInput` for it. Without the bridge
//!   (its dex failed to load: logcat says) the keyboard is asked as before,
//!   through the activity, in the fallback mode.
//!
//! Positions are Unicode code points (egui's `CCursor` index); the Java
//! side converts from UTF-16. Everything is logged under `rustudio` ("ime:
//! …").

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use egui::text::{CCursor, CCursorRange};
use jni::objects::{Global, JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jint, jlong};
use jni::{EnvUnowned, jni_sig, jni_str};

use super::keyboard::focused_text;

/// The Java side, compiled by tools/android/build-dex.sh.
const DEX: &[u8] = include_bytes!("../../assets/android/ImeBridge.dex");
const CLASS: &str = "dev.rustudio.studio.ImeBridge";

/// What is typed into (ImeBridge.java `KIND_*`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// One line: a Properties value, an Explorer name, a filter, a TextBox.
    Text = 0,
    /// A multi-line TextBox.
    Multiline = 1,
    /// The script editor: no suggestions, corrections or capitals.
    Code = 2,
}

/// An operation the keyboard made on the Java mirror.
#[derive(Clone, Debug)]
struct Op {
    seq: i32,
    what: What,
    /// The selection after it (code points, sorted).
    sel: (usize, usize),
    /// The mirror's text after it ([`hash`]).
    hash: u64,
}

#[derive(Clone, Debug)]
enum What {
    /// Code points `[a, b)` become `text`.
    Replace { a: usize, b: usize, text: String },
    /// The selection alone moved.
    Select,
    /// Enter in a one-line field.
    Enter,
}

/// A field the keyboard is up for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    /// An egui text field.
    Egui(egui::Id),
    /// A Roblox TextBox (`gui::share().tb_live`).
    TextBox,
}

/// What the Java side was last told about, or last reported.
#[derive(Default)]
struct Session {
    field: Option<Field>,
    kind: Kind,
    /// The field's text as last known (egui's change events bring it).
    text: String,
    /// What the Java side holds, as last agreed: (hash, selection).
    agreed: Option<(u64, (usize, usize))>,
    /// The last operation taken from `OPS`.
    seq: i32,
    /// That operation itself, before any merging (`repeats`).
    last: Option<Op>,
    /// The operation(s) applied this frame: its number, and the hash and
    /// selection the Java side expects after it.
    applied: Option<(i32, u64, (usize, usize))>,
    /// Key-event characters held back one frame (`key_text`), and the
    /// frame they came in.
    held: Vec<String>,
    held_frame: u64,
    /// Frames of this session (`before_frame` calls).
    frame: u64,
}

impl Default for Kind {
    fn default() -> Self {
        Kind::Text
    }
}

static CLASS_REF: OnceLock<Option<Global<JClass<'static>>>> = OnceLock::new();
/// keyboard.rs has the keyboard up (asked `show`, not `hide` since).
static SHOWN: AtomicBool = AtomicBool::new(false);
static OPS: Mutex<VecDeque<Op>> = Mutex::new(VecDeque::new());
static SESSION: Mutex<Option<Session>> = Mutex::new(None);
/// Fields that are code (the script editor says so each frame it shows).
static CODE_FIELDS: Mutex<Vec<egui::Id>> = Mutex::new(Vec::new());
/// A field's text as its editor has it at the end of its frame
/// ([`field_text`]), when it differs from the session's.
static LIVE: Mutex<Option<(egui::Id, String)>> = Mutex::new(None);

fn ops() -> std::sync::MutexGuard<'static, VecDeque<Op>> {
    OPS.lock().unwrap_or_else(|e| e.into_inner())
}

fn session() -> std::sync::MutexGuard<'static, Option<Session>> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
}

/// FNV-1a, 64 bits, over the code points (ImeBridge.java `hash`).
pub fn hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for c in s.chars() {
        h ^= c as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Whether the bridge is there (its class loaded).
pub fn available() -> bool {
    CLASS_REF.get().is_some_and(|c| c.is_some())
}

/// At start, once the activity is there: loads the Java class out of
/// [`DEX`], binds [`native_op`] to it and adds its view to the activity's
/// content. A failure is logged and leaves the keyboard in the fallback
/// mode.
pub fn install() {
    let _ = CLASS_REF.get_or_init(|| match load_class() {
        Ok(class) => {
            let attached = super::jni::with_activity(|env, activity| {
                env.call_static_method(&class, jni_str!("attach"), jni_sig!("(Landroid/app/Activity;)V"), &[JValue::Object(activity)])?;
                Ok(())
            });
            match attached {
                Ok(()) => {
                    log::info!("ime: bridge loaded ({} byte dex)", DEX.len());
                    Some(class)
                }
                Err(e) => {
                    log::error!("ime: the bridge's view couldn't be added ({e}): the keyboard stays in the fallback mode");
                    None
                }
            }
        }
        Err(e) => {
            log::error!("ime: the bridge's class couldn't be loaded ({e}): the keyboard stays in the fallback mode");
            None
        }
    });
}

fn load_class() -> Result<Global<JClass<'static>>, String> {
    super::jni::with_activity(|env, activity| {
        let parent = env.call_method(activity, jni_str!("getClassLoader"), jni_sig!("()Ljava/lang/ClassLoader;"), &[])?.l()?;
        // SAFETY: DEX is a 'static slice; the buffer is read by the class
        // loader's constructor, which copies what it needs.
        let buffer = unsafe { env.new_direct_byte_buffer(DEX.as_ptr().cast_mut(), DEX.len())? };
        let loader = env.new_object(
            jni_str!("dalvik/system/InMemoryDexClassLoader"),
            jni_sig!("(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V"),
            &[JValue::Object(&buffer), JValue::Object(&parent)],
        )?;
        let name = env.new_string(CLASS)?;
        let class = env
            .call_method(&loader, jni_str!("loadClass"), jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"), &[JValue::Object(&name)])?
            .l()?;
        let class = env.as_cast::<JClass>(&class)?;
        // SAFETY: the signature matches `native_op`'s parameters (a static
        // method: EnvUnowned, the class, then the arguments).
        let methods = [
            unsafe { jni::NativeMethod::from_raw_parts(jni_str!("nativeOp"), jni_str!("(IILjava/lang/String;IIIIZJ)V"), native_op as *mut c_void) },
            unsafe {
                jni::NativeMethod::from_raw_parts(
                    jni_str!("nativeCodeFixup"),
                    jni_str!("(Ljava/lang/String;ILjava/lang/String;)Ljava/lang/String;"),
                    native_code_fixup as *mut c_void,
                )
            },
        ];
        // SAFETY: as above; the class stays loaded (a global reference is
        // taken below) for as long as the function exists.
        unsafe { env.register_native_methods(&*class, &methods)? };
        env.new_global_ref(&*class)
    })
}

/// `ImeBridge.nativeOp`: an operation of the keyboard's, on the Java main
/// thread; queued for the editor's thread.
unsafe extern "system" fn native_op<'l>(
    mut env: EnvUnowned<'l>,
    _class: JClass<'l>,
    seq: jint,
    op: jint,
    text: JString<'l>,
    a: jint,
    b: jint,
    sel_start: jint,
    sel_end: jint,
    composing: jboolean,
    hash: jlong,
) {
    // A panic must not cross back into the JVM: that aborts the whole
    // process. A Vietnamese IME commits many composed replacements in a
    // row; if one of them upsets a helper, drop that one operation.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        env.with_env(|env| -> jni::errors::Result<()> {
            let text: String = if text.is_null() { String::new() } else { text.try_to_string(env)? };
            let sel = (sel_start.max(0) as usize, sel_end.max(0) as usize);
            let what = match op {
                1 => What::Replace { a: a.max(0) as usize, b: b.max(0) as usize, text },
                2 => What::Select,
                _ => What::Enter,
            };
            log::info!(
                "ime: op {seq} {}",
                match &what {
                    What::Replace { a, b, text } => format!("replace {a}..{b} with {} chars, selection {}..{}", text.chars().count(), sel.0, sel.1),
                    What::Select => format!("select {}..{}", sel.0, sel.1),
                    What::Enter => "enter".to_string(),
                },
                if composing { " (composing)" } else { "" }
            );
            ops().push_back(Op { seq, what, sel, hash: hash as u64 });
            Ok(())
        })
        .resolve::<jni::errors::LogErrorAndDefault>();
    }));
    if result.is_err() {
        log::error!("ime: nativeOp panicked; the keyboard's operation was dropped");
    }
}

/// `ImeBridge.nativeCodeFixup`: the keyboard inserted `inserted` into the
/// mirror of a code field, whose text is now `text` with the caret at
/// `cursor` (code points): the auto-indent to make on the mirror before
/// the operation is reported (code_indent.rs), as "a b cursor\ntext", or
/// null. On the Java main thread, inside the keyboard's operation: the
/// operation then reports the indented text, and the field, the mirror
/// and the keyboard agree without a `setState` round trip (a round trip
/// is a frame in which the keyboard's next letter would be placed by the
/// old text).
unsafe extern "system" fn native_code_fixup<'l>(mut env: EnvUnowned<'l>, _class: JClass<'l>, text: JString<'l>, cursor: jint, inserted: JString<'l>) -> JObject<'l> {
    // Called on the Java main thread from inside the keyboard's operation:
    // a Rust panic here would unwind through JNI and abort the process.
    // Any language's text can reach this hook (Vietnamese composing first),
    // so a bad fixup is logged and skipped, not fatal.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        env.with_env(|env| -> jni::errors::Result<JObject<'l>> {
            if text.is_null() || inserted.is_null() {
                return Ok(JObject::null());
            }
            let text: String = text.try_to_string(env)?;
            let inserted: String = inserted.try_to_string(env)?;
            let Some(ind) = crate::code_indent::settings() else { return Ok(JObject::null()) };
            match crate::code_indent::on_insert(&text, cursor.max(0) as usize, &inserted, &ind) {
                Some(fix) => {
                    log::info!("ime: code fixup: {}..{} becomes {} chars, caret {}", fix.a, fix.b, fix.text.chars().count(), fix.cursor.0);
                    let out = env.new_string(format!("{} {} {}\n{}", fix.a, fix.b, fix.cursor.0, fix.text))?;
                    Ok(out.into())
                }
                None => Ok(JObject::null()),
            }
        })
        .resolve::<jni::errors::LogErrorAndDefault>()
    }));
    match result {
        Ok(obj) => obj,
        Err(_) => {
            log::error!("ime: code fixup panicked; the keyboard's text is used without it");
            JObject::null()
        }
    }
}

/// Asks the keyboard up for `field`'s current state (keyboard.rs
/// `soft_input`). False when the bridge isn't there.
pub fn show() -> bool {
    let Some(Some(class)) = CLASS_REF.get() else { return false };
    SHOWN.store(true, Ordering::Relaxed);
    let mut guard = session();
    let Some(s) = guard.as_mut().filter(|s| s.field.is_some()) else {
        // (the field shows up at the end of a frame: told then)
        return true;
    };
    let (kind, text, sel, seq) = (s.kind, s.text.clone(), current_selection(s), s.seq);
    s.agreed = Some((hash(&text), sel));
    drop(guard);
    let r = super::jni::with_env(|env| {
        let jtext = env.new_string(&text)?;
        env.call_static_method(
            class,
            jni_str!("show"),
            jni_sig!("(ILjava/lang/String;III)V"),
            &[JValue::Int(kind as i32), JValue::Object(&jtext), JValue::Int(sel.0 as i32), JValue::Int(sel.1 as i32), JValue::Int(seq)],
        )?;
        Ok(())
    });
    match r {
        Ok(()) => {
            log::info!("ime: show for {kind:?}, {} chars, selection {}..{}", text.chars().count(), sel.0, sel.1);
            true
        }
        Err(e) => {
            log::warn!("ime: show failed: {e}");
            false
        }
    }
}

/// Asks the keyboard down (keyboard.rs `soft_input`). False when the
/// bridge isn't there.
pub fn hide() -> bool {
    let Some(Some(class)) = CLASS_REF.get() else { return false };
    SHOWN.store(false, Ordering::Relaxed);
    match super::jni::with_env(|env| {
        env.call_static_method(class, jni_str!("hide"), jni_sig!("()V"), &[])?;
        Ok(())
    }) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("ime: hide failed: {e}");
            false
        }
    }
}

/// The Java side takes the editor's state.
fn set_state(text: &str, sel: (usize, usize), seq: i32, restart: bool) {
    let Some(Some(class)) = CLASS_REF.get() else { return };
    let r = super::jni::with_env(|env| {
        let jtext = env.new_string(text)?;
        env.call_static_method(
            class,
            jni_str!("setState"),
            jni_sig!("(Ljava/lang/String;IIIZ)V"),
            &[JValue::Object(&jtext), JValue::Int(sel.0 as i32), JValue::Int(sel.1 as i32), JValue::Int(seq), JValue::Bool(restart)],
        )?;
        Ok(())
    });
    if let Err(e) = r {
        log::warn!("ime: setState failed: {e}");
    }
}

/// A field's text as its editor has it at the end of its frame — after
/// changes egui's `TextEdit` made no event for: a completion taken, an
/// indent made by a key event, a soft Tab (script_editor.rs). The session
/// otherwise learns the text from the `TextEdit`'s change events, so such
/// a change reached the keyboard's mirror only on the next operation (the
/// one made on the old text, placed by it: a letter after an accepted
/// completion landed where the word had ended). Compared, not copied,
/// while nothing changed.
pub fn field_text(id: egui::Id, text: &str) {
    let same = session().as_ref().is_some_and(|s| s.field == Some(Field::Egui(id)) && s.text == text);
    let mut live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    if same {
        *live = None;
    } else if !live.as_ref().is_some_and(|(lid, t)| *lid == id && t == text) {
        *live = Some((id, text.to_owned()));
    }
}

/// The script editor, each frame it shows: its field is code.
pub fn code_field(id: egui::Id) {
    let mut v = CODE_FIELDS.lock().unwrap_or_else(|e| e.into_inner());
    if !v.contains(&id) {
        v.push(id);
    }
}

/// The GUI channel whose TextBox has the focus (gui.rs `share`: 0 the
/// game's, 1 the edit plugins'), with its share.
fn with_textbox_share<R>(f: impl FnOnce(&mut rustudio_engine::gui::GuiShare) -> R) -> Option<R> {
    for ch in 0..2u8 {
        let _c = rustudio_engine::gui::share_channel(ch);
        let mut sh = rustudio_engine::gui::share();
        if sh.tb_live.is_some() {
            return Some(f(&mut sh));
        }
    }
    None
}

/// The TextBox's state this frame, from the engine (gui_input.rs puts it
/// in `share().tb_live` while one has the focus): text, cursor and
/// selection anchor as 1-based byte positions (−1: none), MultiLine.
fn textbox_live() -> Option<(String, i32, i32, bool)> {
    with_textbox_share(|sh| sh.tb_live.clone()).flatten()
}

/// A TextBox's selection in code points, sorted.
fn textbox_selection(text: &str, cursor: i32, sel: i32) -> (usize, usize) {
    let to_chars = |p: i32| -> usize {
        let byte = (p.max(1) - 1) as usize;
        let byte = byte.min(text.len());
        let byte = (0..=byte).rev().find(|b| text.is_char_boundary(*b)).unwrap_or(0);
        text[..byte].chars().count()
    };
    let c = to_chars(cursor);
    if sel < 0 {
        return (c, c);
    }
    let s = to_chars(sel);
    (c.min(s), c.max(s))
}

/// The field's selection (code points, sorted) right now.
fn current_selection(s: &Session) -> (usize, usize) {
    match s.field {
        Some(Field::Egui(id)) => egui_selection(id).unwrap_or((0, 0)),
        Some(Field::TextBox) => match textbox_live() {
            Some((text, cursor, sel, _)) => textbox_selection(&text, cursor, sel),
            None => (0, 0),
        },
        None => (0, 0),
    }
}

thread_local! {
    static CTX: std::cell::RefCell<Option<egui::Context>> = const { std::cell::RefCell::new(None) };
}

fn egui_selection(id: egui::Id) -> Option<(usize, usize)> {
    CTX.with(|c| {
        let c = c.borrow();
        let ctx = c.as_ref()?;
        let st = egui::TextEdit::load_state(ctx, id)?;
        let r = st.cursor.char_range()?;
        let [lo, hi] = r.sorted_cursors();
        Some((lo.index.into(), hi.index.into()))
    })
}

fn set_egui_selection(ctx: &egui::Context, id: egui::Id, sel: (usize, usize)) {
    let Some(mut st) = egui::TextEdit::load_state(ctx, id) else { return };
    st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(sel.0), CCursor::new(sel.1))));
    st.store(ctx, id);
}

/// In `App::raw_input_hook`: the keyboard's next operation(s) go to the
/// focused field as events.
pub fn before_frame(ctx: &egui::Context, raw: &mut egui::RawInput) {
    CTX.with(|c| *c.borrow_mut() = Some(ctx.clone()));
    let mut guard = session();
    let Some(s) = guard.as_mut() else {
        // (nothing focused: operations of a field that is gone are dropped)
        ops().clear();
        return;
    };
    s.applied = None;
    s.frame += 1;
    let Some(field) = s.field else {
        ops().clear();
        return;
    };
    let mut queue = ops();
    let arriving = !queue.is_empty();
    key_text(s, raw, arriving);
    let mut op = loop {
        let Some(op) = queue.pop_front() else { return };
        if repeats(&op, s.last.as_ref()) {
            log::warn!("ime: op {} reports op {} again (the same replacement, the mirror as it left it): dropped", op.seq, s.seq);
            s.seq = op.seq;
            s.last = Some(op);
            continue;
        }
        s.last = Some(op.clone());
        break op;
    };
    // Replacements that go on at the cursor (typing) are one event.
    loop {
        let merged = match (&op.what, queue.front()) {
            (What::Replace { a, b, text }, Some(Op { what: What::Replace { a: na, b: nb, text: ntext }, seq, sel, hash })) => {
                let after = a + text.chars().count();
                (*b >= *a && *na == after && *nb == after && op.sel == (after, after))
                    .then(|| Op { seq: *seq, what: What::Replace { a: *a, b: *b, text: format!("{text}{ntext}") }, sel: *sel, hash: *hash })
            }
            _ => None,
        };
        match merged {
            Some(m) => {
                s.last = queue.pop_front();
                op = m;
            }
            None => break,
        }
    }
    drop(queue);
    unshift(s, &mut op);
    s.seq = op.seq;
    s.applied = Some((op.seq, op.hash, op.sel));
    match (&op.what, field) {
        (What::Replace { a, b, text }, Field::Egui(id)) => {
            set_egui_selection(ctx, id, (*a, *b));
            if text == "\n" {
                raw.events.push(key(egui::Key::Enter, true));
                raw.events.push(key(egui::Key::Enter, false));
            } else if text.is_empty() {
                if b > a {
                    raw.events.push(key(egui::Key::Backspace, true));
                    raw.events.push(key(egui::Key::Backspace, false));
                }
            } else {
                raw.events.push(egui::Event::Text(text.clone()));
            }
        }
        (What::Replace { a, b, text }, Field::TextBox) => {
            with_textbox_share(|sh| sh.tb_ime.push((*a, *b, text.clone())));
        }
        (What::Select, _) => {
            // (applied at the end of the frame, after the field drew)
        }
        (What::Enter, _) => {
            raw.events.push(key(egui::Key::Enter, true));
            raw.events.push(key(egui::Key::Enter, false));
        }
    }
    ctx.request_repaint();
}

/// A pure insertion is moved to where the keyboard's caret says it was
/// made when the two describe the same text. The Java side reports the
/// difference between the mirror before and after (the first and last
/// characters that differ), and an insertion at the end of a run of the
/// same characters comes out at the run's end: Enter at the end of a line
/// above other lines ("ab|\ncd" → "ab\n\ncd") is reported as a "\n" at 3,
/// with the caret at 3. Applied at 3, egui's field puts its caret at 4
/// and `end_frame` moves it back; applied at 2, the caret lands where the
/// mirror's did, and the field's own Enter handling (script_editor.rs's
/// indent) sees the line break where it was typed.
fn unshift(s: &Session, op: &mut Op) {
    let What::Replace { a, b, text } = &mut op.what else { return };
    if a != b || op.sel.0 != op.sel.1 || text.is_empty() {
        return;
    }
    let len = text.chars().count();
    let Some(at) = op.sel.0.checked_sub(len) else { return };
    if at == *a {
        return;
    }
    let chars: Vec<char> = s.text.chars().collect();
    if *a > chars.len() || at > chars.len() {
        return;
    }
    let ins: Vec<char> = text.chars().collect();
    let mut reported = chars.clone();
    reported.splice(*a..*a, ins.iter().copied());
    let mut at_caret = chars;
    at_caret.splice(at..at, ins);
    if reported == at_caret {
        *a = at;
        *b = at;
    }
}

/// Whether `op` is the operation before it (`last`) reported a second
/// time: the next number, the same replacement, and the mirror's hash the
/// same after it — a replacement changes the text, so one made anew would
/// leave another hash. The Java side reported every operation made outside
/// a batch of the keyboard's twice (ImeBridge.java `mutate`, fixed there:
/// `BaseInputConnection`'s own calls bracket their work with
/// `beginBatchEdit` / `endBatchEdit`); applied twice, every letter typed
/// twice and a paste pasted twice. Kept as the guard, and logged, should a
/// keyboard find another way to say the same thing twice. (Enter and a
/// selection move leave the text as it is: never judged repeats.)
fn repeats(op: &Op, last: Option<&Op>) -> bool {
    let Some(last) = last else { return false };
    match (&op.what, &last.what) {
        (What::Replace { a, b, text }, What::Replace { a: la, b: lb, text: ltext }) => {
            op.seq == last.seq.wrapping_add(1) && op.hash == last.hash && op.sel == last.sel && a == la && b == lb && text == ltext
        }
        _ => false,
    }
}

fn key(key: egui::Key, pressed: bool) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE }
}

/// Characters that came as key events through the window (egui-winit's
/// `Event::Text`, from winit's `KeyboardInput`) while the keyboard is up
/// through the bridge. A keyboard whose characters come through the
/// bridge's `InputConnection` sends none (the bridge swallows the
/// character keys it does send, ImeBridge.java `sendKeyEvent`) — should
/// one type a character both ways all the same, the window's copy would
/// be the same character again. So while the keyboard is up for the
/// field, key-event text is held back one frame and dropped if an
/// operation of the keyboard's arrives meanwhile (the key event may reach
/// the window a frame before its operation), else let through: a
/// hardware keyboard — whose keys reach the window before any input
/// method, so never through the bridge — types as before, a frame later.
/// (Dropping key-event text for good once the keyboard's operations had
/// been seen, as this did for a while, ate a hardware keyboard's typing
/// into a field the soft keyboard had typed in; the doubling it was meant
/// for was the bridge's own double report, `repeats`.) `Event::Key`
/// (arrows, Enter, Backspace, shortcuts) is never touched. Logged, so a
/// logcat says which way a doubled character came.
fn key_text(s: &mut Session, raw: &mut egui::RawInput, arriving: bool) {
    if !SHOWN.load(Ordering::Relaxed) {
        if !s.held.is_empty() {
            raw.events.splice(0..0, std::mem::take(&mut s.held).into_iter().map(egui::Event::Text));
        }
        return;
    }
    let mut taken: Vec<String> = Vec::new();
    raw.events.retain(|e| match e {
        egui::Event::Text(t) => {
            taken.push(t.clone());
            false
        }
        _ => true,
    });
    if arriving {
        let dropped = s.held.len() + taken.len();
        s.held.clear();
        if dropped > 0 {
            log::info!("ime: {dropped} key-event character(s) dropped: an operation of the keyboard's arrived with them");
        }
        return;
    }
    // (no operation yet: last frame's held text goes through, this
    // frame's waits one frame)
    if !s.held.is_empty() && s.held_frame < s.frame {
        let released = std::mem::take(&mut s.held);
        log::info!("ime: {} key-event character(s) typed: no operation of the keyboard's came with them", released.len());
        raw.events.splice(0..0, released.into_iter().map(egui::Event::Text));
    }
    if !taken.is_empty() {
        if s.held.is_empty() {
            s.held_frame = s.frame;
        }
        s.held.extend(taken);
    }
}

/// At the end of the frame (after every field drew, before keyboard.rs's
/// `end_frame`): follows the focus and the field's text and selection,
/// and tells the Java side what it must know (the module comment).
/// `textbox`: a Roblox TextBox has the focus (the game's in Play, a plugin
/// widget's).
pub fn end_frame(ctx: &egui::Context, textbox: bool) {
    if !available() {
        return;
    }
    let field = match focused_text(ctx) {
        Some(id) => Some(Field::Egui(id)),
        None if textbox && textbox_live().is_some() => Some(Field::TextBox),
        None => None,
    };
    // The text, from egui's change events (or the TextBox's state). The
    // events don't say which widget's they are: the focus gained is the
    // focused field's; a change in the same frame may be the field that
    // lost the focus (an Enter that committed it), so it counts only
    // while the focus stays.
    let mut text_now: Option<String> = None;
    let mut text_gained: Option<String> = None;
    let mut sel_now: Option<(usize, usize)> = None;
    let mut kind = Kind::Text;
    match field {
        Some(Field::Egui(id)) => {
            ctx.output(|o| {
                for ev in &o.events {
                    match ev {
                        egui::output::OutputEvent::FocusGained(info) if info.typ == egui::WidgetType::TextEdit => {
                            if let Some(t) = &info.current_text_value {
                                text_gained = Some(t.clone());
                            }
                        }
                        // (a tap that focuses the field reports as a click)
                        egui::output::OutputEvent::ValueChanged(info)
                        | egui::output::OutputEvent::Clicked(info)
                        | egui::output::OutputEvent::DoubleClicked(info)
                        | egui::output::OutputEvent::TripleClicked(info)
                        | egui::output::OutputEvent::TextSelectionChanged(info)
                            if info.typ == egui::WidgetType::TextEdit =>
                        {
                            if let Some(t) = &info.current_text_value {
                                text_now = Some(t.clone());
                            }
                        }
                        _ => {}
                    }
                }
            });
            sel_now = egui_selection(id);
            if CODE_FIELDS.lock().unwrap_or_else(|e| e.into_inner()).contains(&id) {
                kind = Kind::Code;
            }
            // (the editor's own word on the text comes last: after the
            // frame's events and its own changes)
            if let Some((lid, t)) = LIVE.lock().unwrap_or_else(|e| e.into_inner()).take() {
                if lid == id {
                    text_now = Some(t);
                }
            }
        }
        Some(Field::TextBox) => {
            if let Some((text, cursor, sel, multi)) = textbox_live() {
                sel_now = Some(textbox_selection(&text, cursor, sel));
                text_now = Some(text);
                kind = if multi { Kind::Multiline } else { Kind::Text };
            }
        }
        None => {}
    }
    CODE_FIELDS.lock().unwrap_or_else(|e| e.into_inner()).clear();

    let mut guard = session();
    let Some(field) = field else {
        if guard.is_some() {
            log::info!("ime: no field");
            *guard = None;
            ops().clear();
        }
        return;
    };
    let changed_field = guard.as_ref().is_none_or(|s| s.field != Some(field));
    if changed_field {
        let seq = guard.as_ref().map_or(0, |s| s.seq);
        if text_gained.is_some() {
            text_now = text_gained;
        } else if text_now.is_none() {
            log::warn!("ime: a field took the focus without reporting its text: the keyboard's mirror starts empty");
        }
        *guard = Some(Session { field: Some(field), kind, seq, ..Default::default() });
        ops().clear();
    } else if text_gained.is_some() && text_now.is_none() {
        // (the same field gained the focus again)
        text_now = text_gained;
    }
    let s = guard.as_mut().unwrap();
    if let Some(t) = text_now {
        s.text = t;
    }
    let kind_changed = s.kind != kind;
    s.kind = kind;
    let sel = sel_now.unwrap_or((0, 0));
    let h = hash(&s.text);

    if let Some((seq, expect_hash, expect_sel)) = s.applied.take() {
        if h != expect_hash {
            log::warn!("ime: op {seq} left the field another text than the keyboard's mirror: the mirror takes the editor's");
            s.agreed = Some((h, sel));
            set_state(&s.text, sel, seq, true);
        } else {
            if sel != expect_sel {
                // (where the keyboard put the cursor: a commit that leaves
                // it before the text, a selection it set)
                if let Field::Egui(id) = field {
                    set_egui_selection(ctx, id, expect_sel);
                } else {
                    with_textbox_share(|sh| sh.tb_ime_sel = Some(expect_sel));
                }
            }
            s.agreed = Some((h, expect_sel));
        }
        return;
    }
    if changed_field || kind_changed {
        // keyboard.rs asks `show` when it decides the keyboard is wanted
        // (the state goes with it); with the keyboard already up for the
        // last field, the Java side is told of this one here.
        s.agreed = Some((h, sel));
        if SHOWN.load(Ordering::Relaxed) {
            drop(guard);
            show();
        }
        return;
    }
    if s.agreed != Some((h, sel)) {
        let text_changed = s.agreed.is_none_or(|(ah, _)| ah != h);
        if ops().is_empty() {
            s.agreed = Some((h, sel));
            set_state(&s.text, sel, s.seq, false);
            if text_changed {
                log::info!("ime: the field's text changed by other means: the mirror takes it");
            }
        }
        // (operations queued: the state is judged after them)
    }
}
