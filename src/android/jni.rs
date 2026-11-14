//! The Java side of the device services, through JNI (the `jni` crate on
//! the JavaVM android-activity registered): the Vibrator behind
//! HapticService, `Activity.setRequestedOrientation` behind
//! StarterGui.ScreenOrientation, the window insets behind GuiService /
//! ScreenGui.ScreenInsets and UserInputService.OnScreenKeyboardSize, and
//! ConnectivityManager for the content download. Every call is made on
//! whichever thread asks (the thread is attached if it wasn't) and a Java
//! exception comes back as an `Err`, never as a crash.

use std::sync::Mutex;

use jni::objects::{JObject, JValue};
use jni::{Env, JavaVM, jni_sig, jni_str};

pub type Result<T> = std::result::Result<T, String>;

fn vm() -> Result<JavaVM> {
    // jni's process-wide singleton is per jni-rs version: android-activity
    // registers the VM with its own copy of the crate, so the first call
    // here registers it with this one from the pointer AndroidApp carries.
    if let Ok(vm) = JavaVM::singleton() {
        return Ok(vm);
    }
    let app = super::app().ok_or("no AndroidApp yet")?;
    let ptr = app.vm_as_ptr();
    if ptr.is_null() {
        return Err("no JavaVM".into());
    }
    // SAFETY: the JavaVM NativeActivity was created in; it outlives the
    // process, and JavaVM::from_raw only stores the pointer.
    Ok(unsafe { JavaVM::from_raw(ptr.cast()) })
}

/// Runs `f` with this thread attached to the JVM (a local frame of its own;
/// a pending Java exception becomes the error).
pub fn with_env<T>(f: impl FnOnce(&mut Env) -> jni::errors::Result<T>) -> Result<T> {
    vm()?.attach_current_thread(|env| f(env)).map_err(|e: jni::errors::Error| e.to_string())
}

/// Runs `f` with the Activity (a borrowed reference to the one
/// android-activity keeps alive for the lifetime of `AndroidApp`).
pub fn with_activity<T>(f: impl FnOnce(&mut Env, &JObject) -> jni::errors::Result<T>) -> Result<T> {
    let app = super::app().ok_or("no AndroidApp yet")?;
    let raw: jni::sys::jobject = app.activity_as_ptr().cast();
    if raw.is_null() {
        return Err("the Activity is gone".into());
    }
    with_env(|env| {
        // SAFETY: a valid global reference, held by AndroidApp for as long
        // as `app()` hands it out; Cast never deletes it.
        let activity = unsafe { env.as_cast_raw::<JObject>(&raw)? };
        f(env, &activity)
    })
}

/// `Build.VERSION.SDK_INT`.
pub fn sdk_int() -> i32 {
    super::AndroidApp::sdk_version()
}

/// `context.getSystemService(name)` (null when the device has none).
fn system_service<'l>(env: &mut Env<'l>, context: &JObject, name: &str) -> jni::errors::Result<JObject<'l>> {
    let name = env.new_string(name)?;
    env.call_method(
        context,
        jni_str!("getSystemService"),
        jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
        &[JValue::Object(&name)],
    )?
    .l()
}

// ---------------------------------------------------------------- haptics

/// What the device's vibrator can do, asked once.
#[derive(Clone, Copy, Debug, Default)]
pub struct VibratorInfo {
    pub present: bool,
    /// `Vibrator.hasAmplitudeControl()` (API 26+): the motor strength
    /// HapticService:SetMotor sets is honoured, not just on / off.
    pub amplitude: bool,
}

static VIBRATOR: Mutex<Option<VibratorInfo>> = Mutex::new(None);

pub fn vibrator_info() -> VibratorInfo {
    let mut cached = VIBRATOR.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(v) = *cached {
        return v;
    }
    let info = with_activity(|env, activity| {
        let vib = system_service(env, activity, "vibrator")?;
        if vib.is_null() {
            return Ok(VibratorInfo::default());
        }
        let present = env.call_method(&vib, jni_str!("hasVibrator"), jni_sig!("()Z"), &[])?.z()?;
        let amplitude = present
            && sdk_int() >= 26
            && env.call_method(&vib, jni_str!("hasAmplitudeControl"), jni_sig!("()Z"), &[])?.z()?;
        Ok(VibratorInfo { present, amplitude })
    })
    .unwrap_or_else(|e| {
        log::warn!("vibrator: {e}");
        VibratorInfo::default()
    });
    *cached = Some(info);
    info
}

/// One vibration: `millis` long at `strength` 0..1 (`VibrationEffect
/// .createOneShot`, API 26+, amplitude 1..255; a strength of 0 cancels).
/// HapticService:SetMotor(Enum.UserInputType.Touch, Enum.VibrationMotor
/// .Large, s) keeps the motor on by renewing this each frame.
pub fn vibrate(millis: u32, strength: f32) -> Result<()> {
    if strength <= 0.0 || millis == 0 {
        return cancel_vibration();
    }
    let amplitude = ((strength.clamp(0.0, 1.0) * 255.0).round() as i32).clamp(1, 255);
    with_activity(|env, activity| {
        let vib = system_service(env, activity, "vibrator")?;
        if vib.is_null() {
            return Ok(());
        }
        let effect = env.call_static_method(
            jni_str!("android/os/VibrationEffect"),
            jni_str!("createOneShot"),
            jni_sig!("(JI)Landroid/os/VibrationEffect;"),
            &[JValue::Long(i64::from(millis)), JValue::Int(amplitude)],
        )?;
        let effect = effect.l()?;
        env.call_method(&vib, jni_str!("vibrate"), jni_sig!("(Landroid/os/VibrationEffect;)V"), &[JValue::Object(&effect)])?;
        Ok(())
    })
}

/// A pattern: alternating off / on lengths in ms with a strength per step
/// (`VibrationEffect.createWaveform(long[], int[], -1)`), for HapticEffect
/// presets with more than one pulse.
pub fn vibrate_pattern(steps: &[(u32, f32)]) -> Result<()> {
    if steps.is_empty() {
        return Ok(());
    }
    with_activity(|env, activity| {
        let vib = system_service(env, activity, "vibrator")?;
        if vib.is_null() {
            return Ok(());
        }
        let timings: Vec<i64> = steps.iter().map(|(ms, _)| i64::from(*ms)).collect();
        let amplitudes: Vec<i32> = steps
            .iter()
            .map(|(_, s)| if *s <= 0.0 { 0 } else { ((s.clamp(0.0, 1.0) * 255.0).round() as i32).clamp(1, 255) })
            .collect();
        let jt = env.new_long_array(timings.len())?;
        jt.set_region(env, 0, &timings)?;
        let ja = env.new_int_array(amplitudes.len())?;
        ja.set_region(env, 0, &amplitudes)?;
        let effect = env
            .call_static_method(
                jni_str!("android/os/VibrationEffect"),
                jni_str!("createWaveform"),
                jni_sig!("([J[II)Landroid/os/VibrationEffect;"),
                &[JValue::Object(&jt), JValue::Object(&ja), JValue::Int(-1)],
            )?
            .l()?;
        env.call_method(&vib, jni_str!("vibrate"), jni_sig!("(Landroid/os/VibrationEffect;)V"), &[JValue::Object(&effect)])?;
        Ok(())
    })
}

pub fn cancel_vibration() -> Result<()> {
    with_activity(|env, activity| {
        let vib = system_service(env, activity, "vibrator")?;
        if !vib.is_null() {
            env.call_method(&vib, jni_str!("cancel"), jni_sig!("()V"), &[])?;
        }
        Ok(())
    })
}

// ------------------------------------------------------------ orientation

/// `ActivityInfo.SCREEN_ORIENTATION_*` for a Roblox `Enum.ScreenOrientation`
/// name (StarterGui.ScreenOrientation / PlayerGui.ScreenOrientation).
pub fn orientation_constant(name: &str) -> Option<i32> {
    Some(match name {
        "LandscapeLeft" => 0,         // SCREEN_ORIENTATION_LANDSCAPE
        "LandscapeRight" => 8,        // SCREEN_ORIENTATION_REVERSE_LANDSCAPE
        "LandscapeSensor" => 6,       // SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        "Portrait" => 1,              // SCREEN_ORIENTATION_PORTRAIT
        "Sensor" => 10,               // SCREEN_ORIENTATION_FULL_SENSOR
        _ => return None,
    })
}

/// `Activity.setRequestedOrientation(constant)`.
pub fn set_requested_orientation(constant: i32) -> Result<()> {
    with_activity(|env, activity| {
        env.call_method(activity, jni_str!("setRequestedOrientation"), jni_sig!("(I)V"), &[JValue::Int(constant)])?;
        Ok(())
    })
}

/// `Display.getRotation()` as the Roblox orientation the screen is in now:
/// "LandscapeLeft", "LandscapeRight", "Portrait" (PlayerGui
/// .CurrentScreenOrientation; a tablet held upside down counts as
/// Portrait too, as in Roblox).
pub fn current_orientation() -> Result<&'static str> {
    with_activity(|env, activity| {
        let display = if sdk_int() >= 30 {
            env.call_method(activity, jni_str!("getDisplay"), jni_sig!("()Landroid/view/Display;"), &[])?.l()?
        } else {
            let wm = system_service(env, activity, "window")?;
            env.call_method(&wm, jni_str!("getDefaultDisplay"), jni_sig!("()Landroid/view/Display;"), &[])?.l()?
        };
        if display.is_null() {
            return Ok("LandscapeLeft");
        }
        let rotation = env.call_method(&display, jni_str!("getRotation"), jni_sig!("()I"), &[])?.i()?;
        // Surface.ROTATION_0 = 0, _90 = 1, _180 = 2, _270 = 3. A phone's
        // natural orientation is portrait, so ROTATION_90 is landscape with
        // the top of the device to the left (Roblox's LandscapeLeft).
        let natural_portrait = {
            let rect = super::app().map(|a| a.content_rect());
            let (w, h) = rect.map(|r| (r.right - r.left, r.bottom - r.top)).unwrap_or((1, 1));
            matches!((rotation, w > h), (0, false) | (2, false) | (1, true) | (3, true))
        };
        Ok(match (rotation, natural_portrait) {
            (1, true) | (0, false) => "LandscapeLeft",
            (3, true) | (2, false) => "LandscapeRight",
            _ => "Portrait",
        })
    })
}

// ----------------------------------------------------------------- insets

/// The window's insets in physical pixels: what the status bar, navigation
/// bar and display cutout cover (`system`), and the soft keyboard's height
/// (`ime`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub ime_bottom: i32,
}

/// `getWindow().getDecorView().getRootWindowInsets()`, read through the
/// API 30 `getInsets(type)` when there is one, else the API 21 system
/// window insets plus the API 28 cutout.
pub fn window_insets() -> Result<Insets> {
    with_activity(|env, activity| {
        let window = env.call_method(activity, jni_str!("getWindow"), jni_sig!("()Landroid/view/Window;"), &[])?.l()?;
        let decor = env.call_method(&window, jni_str!("getDecorView"), jni_sig!("()Landroid/view/View;"), &[])?.l()?;
        let insets = env
            .call_method(&decor, jni_str!("getRootWindowInsets"), jni_sig!("()Landroid/view/WindowInsets;"), &[])?
            .l()?;
        if insets.is_null() {
            return Ok(Insets::default());
        }
        let sdk = sdk_int();
        if sdk >= 30 {
            let bars = env.call_static_method(jni_str!("android/view/WindowInsets$Type"), jni_str!("systemBars"), jni_sig!("()I"), &[])?.i()?;
            let cutout = env.call_static_method(jni_str!("android/view/WindowInsets$Type"), jni_str!("displayCutout"), jni_sig!("()I"), &[])?.i()?;
            let ime = env.call_static_method(jni_str!("android/view/WindowInsets$Type"), jni_str!("ime"), jni_sig!("()I"), &[])?.i()?;
            let system = read_insets(env, &insets, bars | cutout)?;
            let keyboard = read_insets(env, &insets, ime)?;
            Ok(Insets {
                left: system.0,
                top: system.1,
                right: system.2,
                bottom: system.3,
                ime_bottom: keyboard.3,
            })
        } else {
            let get = |env: &mut Env, name: &'static jni::strings::JNIStr| -> jni::errors::Result<i32> {
                env.call_method(&insets, name, jni_sig!("()I"), &[])?.i()
            };
            let mut out = Insets {
                left: get(env, jni_str!("getSystemWindowInsetLeft"))?,
                top: get(env, jni_str!("getSystemWindowInsetTop"))?,
                right: get(env, jni_str!("getSystemWindowInsetRight"))?,
                bottom: get(env, jni_str!("getSystemWindowInsetBottom"))?,
                ime_bottom: 0,
            };
            if sdk >= 28 {
                let cutout = env
                    .call_method(&insets, jni_str!("getDisplayCutout"), jni_sig!("()Landroid/view/DisplayCutout;"), &[])?
                    .l()?;
                if !cutout.is_null() {
                    let get = |env: &mut Env, name: &'static jni::strings::JNIStr| -> jni::errors::Result<i32> {
                        env.call_method(&cutout, name, jni_sig!("()I"), &[])?.i()
                    };
                    out.left = out.left.max(get(env, jni_str!("getSafeInsetLeft"))?);
                    out.top = out.top.max(get(env, jni_str!("getSafeInsetTop"))?);
                    out.right = out.right.max(get(env, jni_str!("getSafeInsetRight"))?);
                    out.bottom = out.bottom.max(get(env, jni_str!("getSafeInsetBottom"))?);
                }
            }
            // Before API 30 the keyboard's height isn't an inset of its own;
            // with adjustResize it shrinks the content rect instead, which
            // `super::storage` / the input code read from AndroidApp.
            Ok(out)
        }
    })
}

/// `WindowInsets.getInsets(type)` → (left, top, right, bottom).
fn read_insets(env: &mut Env, insets: &JObject, kind: i32) -> jni::errors::Result<(i32, i32, i32, i32)> {
    let r = env
        .call_method(insets, jni_str!("getInsets"), jni_sig!("(I)Landroid/graphics/Insets;"), &[JValue::Int(kind)])?
        .l()?;
    let field = |env: &mut Env, name: &'static jni::strings::JNIStr| -> jni::errors::Result<i32> {
        env.get_field(&r, name, jni_sig!("I"))?.i()
    };
    Ok((field(env, jni_str!("left"))?, field(env, jni_str!("top"))?, field(env, jni_str!("right"))?, field(env, jni_str!("bottom"))?))
}

// ---------------------------------------------------------------- network

/// Whether the active network can reach the internet
/// (`NetworkCapabilities.NET_CAPABILITY_INTERNET` and `_VALIDATED`), for
/// deciding whether to start the Studio content download. None when the
/// question can't be asked (no ConnectivityManager, a Java error).
pub fn network_available() -> Option<bool> {
    with_activity(|env, activity| {
        let cm = system_service(env, activity, "connectivity")?;
        if cm.is_null() {
            return Ok(None);
        }
        let network = env.call_method(&cm, jni_str!("getActiveNetwork"), jni_sig!("()Landroid/net/Network;"), &[])?.l()?;
        if network.is_null() {
            return Ok(Some(false));
        }
        let caps = env
            .call_method(
                &cm,
                jni_str!("getNetworkCapabilities"),
                jni_sig!("(Landroid/net/Network;)Landroid/net/NetworkCapabilities;"),
                &[JValue::Object(&network)],
            )?
            .l()?;
        if caps.is_null() {
            return Ok(Some(false));
        }
        // NET_CAPABILITY_INTERNET = 12, NET_CAPABILITY_VALIDATED = 16.
        let internet = env.call_method(&caps, jni_str!("hasCapability"), jni_sig!("(I)Z"), &[JValue::Int(12)])?.z()?;
        let validated = env.call_method(&caps, jni_str!("hasCapability"), jni_sig!("(I)Z"), &[JValue::Int(16)])?.z()?;
        Ok(Some(internet && validated))
    })
    .unwrap_or_else(|e| {
        log::warn!("connectivity: {e}");
        None
    })
}

// ------------------------------------------------------------------ files

/// `ContentResolver.openInputStream(Uri.parse(uri))` read to the end: a
/// file the system picker or a share sheet handed over as a `content:` URI.
pub fn read_content_uri(uri: &str) -> Result<Vec<u8>> {
    with_activity(|env, activity| {
        let juri = env.new_string(uri)?;
        let uri = env
            .call_static_method(jni_str!("android/net/Uri"), jni_str!("parse"), jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"), &[JValue::Object(&juri)])?
            .l()?;
        let resolver = env
            .call_method(activity, jni_str!("getContentResolver"), jni_sig!("()Landroid/content/ContentResolver;"), &[])?
            .l()?;
        let stream = env
            .call_method(
                &resolver,
                jni_str!("openInputStream"),
                jni_sig!("(Landroid/net/Uri;)Ljava/io/InputStream;"),
                &[JValue::Object(&uri)],
            )?
            .l()?;
        if stream.is_null() {
            return Err(jni::errors::Error::NullPtr("openInputStream"));
        }
        let mut out = Vec::new();
        let chunk = env.new_byte_array(1 << 16)?;
        let mut buf = vec![0i8; 1 << 16];
        loop {
            let n = env.call_method(&stream, jni_str!("read"), jni_sig!("([B)I"), &[JValue::Object(&chunk)])?.i()?;
            if n <= 0 {
                break;
            }
            let n = n as usize;
            chunk.get_region(env, 0, &mut buf[..n])?;
            out.extend(buf[..n].iter().map(|b| *b as u8));
        }
        env.call_method(&stream, jni_str!("close"), jni_sig!("()V"), &[])?;
        Ok(out)
    })
}

/// The display name of a `content:` URI (`OpenableColumns.DISPLAY_NAME`
/// through `ContentResolver.query`), for the file's extension.
pub fn content_display_name(uri: &str) -> Result<Option<String>> {
    with_activity(|env, activity| {
        let juri = env.new_string(uri)?;
        let uri = env
            .call_static_method(jni_str!("android/net/Uri"), jni_str!("parse"), jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"), &[JValue::Object(&juri)])?
            .l()?;
        let resolver = env
            .call_method(activity, jni_str!("getContentResolver"), jni_sig!("()Landroid/content/ContentResolver;"), &[])?
            .l()?;
        let column = env.new_string("_display_name")?;
        let projection = env.new_object_array(1, jni_str!("java/lang/String"), &column)?;
        let null = JObject::null();
        let cursor = env
            .call_method(
                &resolver,
                jni_str!("query"),
                jni_sig!("(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;"),
                &[JValue::Object(&uri), JValue::Object(&projection), JValue::Object(&null), JValue::Object(&null), JValue::Object(&null)],
            )?
            .l()?;
        if cursor.is_null() {
            return Ok(None);
        }
        let mut name = None;
        if env.call_method(&cursor, jni_str!("moveToFirst"), jni_sig!("()Z"), &[])?.z()? {
            let s = env.call_method(&cursor, jni_str!("getString"), jni_sig!("(I)Ljava/lang/String;"), &[JValue::Int(0)])?.l()?;
            if !s.is_null() {
                let s = env.as_cast::<jni::objects::JString>(&s)?;
                name = Some((*s).to_string());
            }
        }
        env.call_method(&cursor, jni_str!("close"), jni_sig!("()V"), &[])?;
        Ok(name)
    })
}

/// `Intent.getData().toString()` of the Intent this activity was started
/// with (a VIEW of a place file), if any.
pub fn launch_intent_uri() -> Result<Option<String>> {
    with_activity(|env, activity| {
        let intent = env.call_method(activity, jni_str!("getIntent"), jni_sig!("()Landroid/content/Intent;"), &[])?.l()?;
        if intent.is_null() {
            return Ok(None);
        }
        let data = env.call_method(&intent, jni_str!("getData"), jni_sig!("()Landroid/net/Uri;"), &[])?.l()?;
        if data.is_null() {
            return Ok(None);
        }
        let s = env.call_method(&data, jni_str!("toString"), jni_sig!("()Ljava/lang/String;"), &[])?.l()?;
        let s = env.as_cast::<jni::objects::JString>(&s)?;
        Ok(Some((*s).to_string()))
    })
}
