# rustudio on Android

The Android app is the same editor as the desktop one — the `rustudio`
library (src/lib.rs) — inside a `NativeActivity`. What differs is in
`src/android/` (compiled only for `target_os = "android"`) and in
`crates/rustudio-android/` (the `cdylib` the activity loads, with the
`manifest.yaml` xbuild turns into the AndroidManifest). Targets:
`aarch64-linux-android` (arm64-v8a, every phone and tablet) and
`x86_64-linux-android` (the emulator).

## Building

CI (`.github/workflows/android.yml`) builds four APKs on every push / PR
that touches Rust sources — arm64 and x64, debug and release — and uploads
them as artifacts (`rustudio-<arch>-<profile>.apk`). Locally:

```sh
# once
rustup target add aarch64-linux-android x86_64-linux-android
sudo apt install clang lld llvm            # the NDK targets link with these
cargo install --git https://github.com/rust-mobile/xbuild \
  --rev e67b501cbe0e9ca5436c223aa7ec0fe5e27544d1 xbuild
x doctor                                   # what it found; nothing here fails

# an APK (xbuild fetches its NDK sysroot and android.jar into ~/.cache/x)
x build -p rustudio-android --platform android --arch arm64 --format apk
x build -p rustudio-android --platform android --arch arm64 --format apk --release
ls target/x/{debug,release}/android/rustudio-android.apk

# on a device / emulator
adb install -r target/x/debug/android/rustudio-android.apk
adb logcat -s rustudio RustStdoutStderr
```

`x build` without `--pem` signs with xbuild's debug key (installs with
adb). A release key goes in as `--pem key.pem` or the `X_PEM` environment
variable; CI reads it from the `ANDROID_SIGNING_PEM` secret when set.

Without a device, `cargo check` for the target works with the same
environment xbuild uses (sysroot, `--target=aarch64-linux-android33`, clang
and lld) — see the `Build APK` step of the workflow for the variables.

xbuild quirks worth knowing (commit e67b501):

- A workspace member list must not contain `"."`: xbuild walks
  `workspace.members` and rejects a member whose manifest has a
  `[workspace]` table (*Did not expect a `[workspace]`*). Cargo adds the
  root package by itself, so the root isn't listed.
- `opengles_version` in `manifest.yaml` can't be used: it is written as
  `android:glEsVersion="0x00030000"` but xbuild's own manifest compiler
  parses integer attributes as decimal and `x build` stops at
  *[3/3] Create apk — invalid digit found in string*. The GLES fallback
  doesn't depend on the declaration (wgpu picks it at run time), and every
  API 26 device has GLES 3.0, so the manifest declares Vulkan only.

## What is where

| Concern | Desktop | Android |
|---|---|---|
| Entry point | `src/main.rs` → `rustudio::desktop_main()` | `crates/rustudio-android/src/lib.rs`: `rustudio_on_create` (the manifest's `android.app.func_name`) → android-activity's `ANativeActivity_onCreate` + the destroy hook; `android_main` → `rustudio::android::run()` |
| Window / GPU | eframe + winit (Wayland, X11, Windows, macOS) | eframe + winit on `NativeActivity` (`android-native-activity`); wgpu Vulkan, GLES 3 fallback (`wgpu_options` in src/lib.rs) |
| Lifecycle | — | eframe's wgpu backend drops the window + surface in `suspended()` and recreates them in `resumed()`; a rotation is a resize and reconfigures the swapchain |
| Folders (`src/paths.rs`) | `~/.local/share`, `~/.config`, `~/.local/state`, `~/.cache`, `~/Pictures` | `getExternalFilesDir()` for data (Studio content, places), `getFilesDir()/config`, `/state`, `getCacheDir()`, `<external>/Pictures` (`src/android/mod.rs`); `HOME` and the `XDG_*` variables are set to the same so `dirs` (rustudio-account) agrees |
| `api-dump.json` | next to the executable / app bundle / source tree | APK `assets/`, unpacked to `files/resources/` at first start (`src/android/assets.rs`) |
| File dialogs | rfd (async, on a worker thread) | rustudio's own picker (`src/android/storage.rs`) over the app folder and, once the storage permission is granted from the picker's own button, shared storage (Download, Documents, …); "Open with rustudio" from file managers through the VIEW intent (`content:` URIs copied in via ContentResolver) |
| Input | mouse + keyboard | fingers through `src/touch_input.rs` (below); hardware mice / keyboards work as on a desktop |
| Clipboard | egui-winit (arboard) | egui's copies mirrored to `ClipboardManager` after each frame, pastes read from it (`src/android/clipboard.rs`) |
| Browser links (`platform::open_url`) | `ctx.open_url` (webbrowser) | `ACTION_VIEW` Intent from the activity (`jni::open_url`); eframe's `links` feature isn't built for Android |
| Back button | — | closes what is open on top (Escape), otherwise `moveTaskToBack` — the activity isn't destroyed (`src/android/lifecycle.rs`) |
| Activity destroyed | — | `onDestroy` hook: close request, then `process::exit(0)`; `android_main` always leaves the process when the loop ends (`lifecycle.rs`) |
| UI zoom | 1.0 of the display's scale | first run: the zoom that fits ~1100 points across (0.8 … 1.6 of the density), then View > Zoom In / Out as saved |
| 3D view render scale | 1.0 | first run 0.7 (View > Render Scale 100 / 70 / 50 %); saved with the layout |
| Logging | stderr | logcat tag `rustudio` (`log` → android_logger), stdout/stderr forwarded by android-activity |
| Crash reports | `~/.cache/rustudio/crash/` | `<cache>/rustudio/crash/` (same `src/crash.rs`: panic hook + signal handlers chain to debuggerd) |
| Studio content download | ureq + rustls (ring) | the same; started only when ConnectivityManager reports a validated network |
| Login keyring | `keyring` 4 (Secret Service / Keychain / Credential Manager) | `keyring-core` + `android-native-keyring-store` (SharedPreferences + Android Keystore) behind the same `Entry` API (`crates/rustudio-account/src/store.rs`) |
| TLS roots (`rustudio-account` MessagingService socket) | OS store (`rustls-native-certs`) | Mozilla's (`webpki-roots`, enabled alongside): Android's CA store isn't in a form `rustls-native-certs` reads; `ureq` already used `webpki-roots` |

## Audit: desktop-only code

What the Android build compiles out or replaces, and why. Everything not
listed compiles unchanged for the Android targets (checked with `cargo
check --target aarch64-linux-android`).

Gated with `#[cfg(not(target_os = "android"))]` (would not compile):

| Item | Reason | Android replacement |
|---|---|---|
| `rfd` (Cargo.toml) and the rfd code in `src/file_dialog.rs` | rfd 0.15 has no Android backend | `file_dialog::Request` + `android::storage::Picker` |
| `libc::getloadavg` in `src/platform.rs::load_average` | Bionic has it only from API 29; the symbol is absent from the libc crate's Android bindings | returns "?" |
| `eframe` features `wayland`, `x11` | desktop window systems | `android-native-activity` (target-specific dependency tables in Cargo.toml) |

Desktop-only by behaviour (compile fine, never active on Android, left
ungated so the shared code paths stay one):

- `src/hypr.rs` (Hyprland IPC through `hyprctl`), `src/layout_store.rs`
  window geometry — no `HYPRLAND_INSTANCE_SIGNATURE`, nothing happens.
- `src/mouse_lock.rs` — pointer lock / confine logic over egui types; no
  pointer on a touch screen, so the Default/LockCenter paths are no-ops.
- `src/play.rs` gamepad scan of `/dev/input/by-id` — the folder isn't
  readable in an app sandbox; gamepads will come through winit / `AInputEvent`
  in a later step.
- `src/app.rs` and `src/selftest/fidelity.rs` self-relaunch with
  `current_exe()` (`--screenshot`, `--self-test`) — harness-only, never
  reached without a command line (`Harness::interactive()`).
- `src/lib.rs` `rerun_headless` / `restore_parent_fds` — already
  `cfg(target_os = "linux")` only.
- `src/macos.rs`, `windows-sys` — already cfg'd by OS.
- `crates/rustudio-account` `keyring` 4 — not built for Android (no store
  there: every `Entry::new` failed, which broke the session check and the
  Quick Sign-in flow on the first device runs). `store.rs` has a
  `keyring` shim module over `keyring-core` with
  `android-native-keyring-store` as the default store, so `session.rs` and
  `store.rs` compile unchanged.
- `src/domains/media_video.rs` `dlopen` of libvpx / libopus — Android has
  no system copies; the loaders fail gracefully (no video decoding).

## Audit: Roblox services on a device

Column "today" is what the shared Luau/Rust layer already does (the same
on every system); "Android" is what the device plumbing in `src/android/`
provides or still has to.

| Service / member | Today (shared) | Android plumbing |
|---|---|---|
| `UserInputService.TouchEnabled / MouseEnabled / KeyboardEnabled / AccelerometerEnabled / GyroscopeEnabled / PreferredInput` | constants in `runtime_ext.rs device_prop()` (desktop values) | **to do**: true/false from the activity (touch screen, `sensors::available()`), `PreferredInput = Touch`, `PreferredInputChanged` |
| `TouchStarted / TouchMoved / TouchEnded`, `TouchTap`, `TouchTapInWorld`, `TouchLongPress`, `TouchPan`, `TouchPinch`, `TouchRotate`, `TouchSwipe` | declared (`runtime_ext.rs` 116-154), never fired; `__rs_input_event` carries mouse/keyboard | **to do**: winit `Touch` events (egui `PointerButton` + `multi_touch`) → `InputObject` with `UserInputType.Touch` per finger id, gesture recognisers, `gameProcessedEvent` from egui's `wants_pointer_input` |
| `GetDeviceAcceleration / GetDeviceGravity / GetDeviceRotation`, `DeviceAccelerationChanged / DeviceGravityChanged / DeviceRotationChanged` | zeros (`gui_input.luau` 299-307, `library.luau` 852) | `sensors.rs` (ASensorManager: linear acceleration, gravity, gyroscope, game rotation vector, 60 Hz on its own thread) — **wiring to Luau to do** (g units, CFrame from the quaternion) |
| `UserInputService.ModalEnabled` | stored | **to do**: hides the default thumbstick / jump button once those exist |
| `OnScreenKeyboardVisible / OnScreenKeyboardSize / OnScreenKeyboardPosition`, `TextBoxFocused / TextBoxFocusReleased` | focus events fire (`play_game.rs` 247); keyboard size zero | `jni::window_insets().ime_bottom` (API 30+), `AndroidApp::show_soft_input / hide_soft_input` on TextBox focus — **wiring to do** |
| `HapticService.IsVibrationSupported / IsMotorSupported / SetMotor / GetMotor` | gamepad-only flags, in-memory motor table (`gui_input.luau` 1796-1845) | `jni::vibrator_info()` (hasVibrator, hasAmplitudeControl) and `jni::vibrate(ms, strength)` — **wiring to do**: `UserInputType.Touch` supported, `VibrationMotor.Large` (and `Small` when amplitude control exists) |
| `HapticEffect:Play / Stop`, `HapticEffect.Type` presets | simulated durations (UIHover .035 s, UIClick .055, UINotification .4, GameplayCollision .3, GameplayExplosion .8) | `jni::vibrate_pattern` (VibrationEffect.createWaveform) — **mapping of each preset to do** |
| `ContextActionService.BindAction(..., createTouchButton)`, `GetButton / SetTitle / SetImage / SetPosition` | bindings stored, priority dispatch (`library.luau` 671-767); buttons never drawn | **to do**: draw the buttons in the play viewport (`ContextActionGui` ScreenGui as Roblox does: 70×70 ImageButton, default positions right of the thumbstick), route taps to the bound function with `UserInputType.Touch` |
| `GuiService:GetGuiInset()`, `TopbarInset`, `GetInsetArea`, `ScreenGui.ScreenInsets` (`None / DeviceSafeInsets / CoreUISafeInsets / TopbarSafeInsets`), `ScreenGui.SafeAreaCompatibility` | `(0, 58)` constant (`mech::GUI_TOP_INSET`), `TopbarInset` zero rect, inset root in `gui_input.rs` ~498 | `jni::window_insets()` (system bars + display cutout; the 4 sides) — **wiring to do**: DeviceSafeInsets = those, CoreUISafeInsets = those + topbar, TopbarSafeInsets = topbar only |
| `StarterGui.ScreenOrientation`, `PlayerGui.ScreenOrientation / CurrentScreenOrientation` | stored (`start_page/builtin.rs` 144); `CurrentScreenOrientation = LandscapeLeft` | `jni::set_requested_orientation` + `jni::current_orientation()` — **wiring to do**: apply at Play start, restore `sensorLandscape` at stop |
| `Player:GetMouse()`, `Mouse` while playing | desktop pointer | **to do**: the first touch stands in (as Roblox does) |
| `VRService`, gamepads | stubs | unchanged |

## Fingers (`src/touch_input.rs`)

egui-winit makes the first finger the mouse pointer with the left button
down, which is wrong for most of the editor: a finger laid on an Explorer
row starts dragging the row instead of scrolling the list, and a finger on
the 3D view box-selects. `Touch::filter` runs in `App::raw_input_hook`,
before egui sees the frame's events, and rewrites the `Event::Touch`
stream (dropping egui-winit's stand-in pointer events):

| Finger | Becomes |
|---|---|
| moves at once (> 8 pt before 320 ms) on the chrome | a scroll of the panel under it (`MouseWheel` events in a Start/Move/End phase, applied 1:1), with a fling after the lift |
| still for 320 ms | a normal press, dragging from then on (Explorer rows, text selection, sliders, splitters, windows); a long press without moving opens egui's context menu (its own long-touch) |
| lifted without moving | a tap (press + release at the landing point) |
| moves at once on the 3D view | a right-button drag: the editor camera's look; in Play the game camera (the PlayerModule's right-drag orbit, mouse deltas from the finger) |
| on a transform handle | a left press at once (drags the handle) |
| second finger on the 3D view | ends the first one's drag; `InputState::multi_touch` gives the viewport a pinch (zoom toward the fingers; in Play the game's wheel: `mech::PINCH_NOTCHES_PER_DOUBLING`) and a two-finger pan |
| in Play, lands bottom-left / bottom-right of the view | the movement stick (W/A/S/D held by its deflection, drawn under the finger) / the jump button (Space) — `touch_input::play_zones`; released when Play stops or the window loses focus. A quick tap in the stick zone is still a tap (GUI buttons there keep working). Stick and jump fingers are kept out of egui's touch stream, so a look finger beside them is never read as a pinch |

The viewport publishes where things are each frame (`Zones`: its rect,
whether the game has the input, the stick and jump areas, a handle under
the pointer) and draws the stick and button in Play (`Touch::paint`). The
chrome's egui settings for fingers are in `android::touch::setup`. The
tuning numbers (hold 320 ms, slop 8 pt, fling, stick and button sizes)
are `mech::TOUCH_*` with their rows in docs/MECHANICS.md ("Touch
screens") and mechanics.json. The module has no Android dependency, so
its nine tests run on the desktop (`cargo test touch_input`).

The layer is **off on the Start Page** (`Touch::set_enabled` from
`raw_input_hook`, while `start_page_ui` would draw): that page is plain
egui — lists that scroll by drag (`ScrollArea`'s own touch rule), cards
that are taps, long presses for its menus — and has none of the editor's
reasons to tell a scroll from a drag. Switching off lets go of anything
held; a finger down at a switch stays egui's until it lifts, so no press
is left half delivered. Once a place opens, the layer is on.

## Frame time on a phone

Every few seconds the editor logs a line like

```
perf: 136 frames in 5.0 s: 36.9 ms apart (p95 72.3, worst 89); editor 5.2 ms + egui 1.2 ms on the CPU, 30.5 ms GPU/vsync wait; 3D GPU 9.8 ms per drawn frame = shadows 3.1 + scene 4.2 + ssao/bloom/dof 1.5 + post 1.0 (68 measured); window 1550x720 px at 1.40 px/pt, 3D view 816x310 px (scale 0.70, AO off), 3D frames drawn 68 / shown again 68; sections (ms/frame): dock_panels 4.3, viewport 3.4, vp_render 3.0
```

(`src/perf_log.rs`; `adb logcat -s rustudio`, or `RUSTUDIO_PERF_LOG=1` on
a desktop). "editor" is `App::ui`'s own time, "egui" the layout and
painting eframe adds, the "wait" what is left of the interval (the GPU,
vsync, the loop). "3D GPU" is the renderer's own GPU time by timestamp
queries (`render::GpuTimer`, where the device has them), per frame drawn
afresh and by section; what the wait holds beyond it is egui's own GPU
pass and vsync. The first line names the GPU and the wgpu backend (Vulkan
is the one wanted; a GL fallback is slow). An idle editor logs nothing: no
input, no frames.

Switches for a device, which has no shell environment to launch from:

```
adb shell setprop debug.rustudio.env "RUSTUDIO_SUN_SHADOWS=0 RUSTUDIO_VIEW_REUSE=0"
```

then relaunch (`setprop debug.rustudio.env ""` clears). Space-separated
`RUSTUDIO_*=value` pairs, 91 characters at most (`android::export_env`,
`perf_log::env_switch`): `RUSTUDIO_SSAO`, `RUSTUDIO_VIEW_REUSE`,
`RUSTUDIO_RENDER_SCALE`, `RUSTUDIO_SUN_SHADOWS` (the sun's shadow maps,
as Lighting.GlobalShadows false).

First measurements (S20 Ultra, Adreno 650 on Vulkan, release APK, the
window 1550 × 720 px, the 3D view 816 × 310): ~6 ms of CPU a frame and
~31 ms waited on the GPU whether the camera moved or not, 21–27 fps; the
"3D GPU" figure above is what the next run has to show.

What the port does about it:

- **Render scale** (View > Render Scale; `RUSTUDIO_RENDER_SCALE`): the 3D
  view at 0.7 of the panel's pixels on a first run.
- **Ambient occlusion off** (View > Ambient Occlusion; `RUSTUDIO_SSAO`):
  Studio's SSAO is five passes over the view for a darkening in corners
  a phone's screen barely shows.
- **The last 3D frame shown again** while nothing in it changed
  (`viewport::ViewReuse`; `RUSTUDIO_VIEW_REUSE=0|1`, on by default on
  Android only): the frame's inputs are hashed (`render::frame_key`), the
  renderer counts its own changes (`Renderer::generation`, bumped by
  every mesh, texture, item, terrain, sky and grid upload), and a frame
  is drawn afresh when either moved, when time shows (a game running,
  terrain water, world GUIs and effects, the test harness) and at least
  every 500 ms. With a still camera the chrome then redraws at egui's
  own pace — taps and scrolls in the Explorer and Properties no longer
  wait for a 3D frame each. (The first device run showed exactly every
  other frame reused: the effects system hands over a frame of effects
  whether or not it holds any, and an empty one counted as "effects on a
  reused frame", which asks for a fresh one.)
- **No cursor commands in Play.** `mouse_lock::note_window` took the
  Android window for one that can't lock the cursor, so a game's
  LockCenter grab fell back to Confined and sent a cursor position every
  frame, which winit rejected with a warning logged each time.
- The release APK is built with thin LTO (`CARGO_PROFILE_RELEASE_LTO` in
  android.yml; the workspace profile is unchanged for desktops).

Still to do, with measurements from the line above: a phone tier for the
sun's shadow tiles (all four cascades are redrawn, with their EVSM
blurs, whenever the camera moves) and for the light grid's size, and a
leaner default layout for small screens (every panel open at once is
most of the CPU time on a phone).

## Device-run notes (first APKs)

Found on devices with the phase-1 APKs; the fixes are in the table above
and here:

- **Black window at first start, until leaving and coming back.** winit
  0.30 drops `MainEvent::Destroy`, so whenever the system destroyed the
  activity under a running loop (a swipe from Recents followed by a quick
  relaunch, a configuration change outside `configChanges`), the Java
  main thread sat in `onDestroy` waiting for `android_main`; the next
  launch landed in that process and drew nothing. `lifecycle.rs` hooks
  `onDestroy` and leaves the process; `android_main` exits the process
  when the loop returns; the manifest declares every configuration change
  the API 33 android.jar knows; the first three seconds request repaints
  so an early lost redraw can't leave the splash up; `App::save` keeps the
  layout at eframe's save moments (the surface going away).
  The hook gets the `ANativeActivity` from the library's own entry point
  (`rustudio_on_create`, named by `android.app.func_name` in
  manifest.yaml), not from `AndroidApp::activity_as_ptr()`, which is the
  Java `Activity`'s JNI reference: the first phase-2 APK read that as the
  struct and crashed at start (`SIGSEGV` at a small address in
  `lifecycle::hook`).
- **Heavy lag.** The 3D view rendered at the panel's full pixel size — a
  phone's 2–3× density screen is more pixels than a laptop's — plus debug
  builds. First run now renders the view at 0.7 scale (View > Render
  Scale), and `x build --release` (or the `profiling` profile) is the
  build to judge speed by; `RUSTUDIO_RENDER_SCALE` overrides.
- **Clipboard.** egui-winit has no Android clipboard; see
  `android/clipboard.rs`.
- **Browser sign-in never opened the code page.** Two causes: eframe's
  `links` feature (and so `ctx.open_url`) isn't in the Android feature
  set, and `keyring::Entry::new` failed on Android so the session check
  errored before the code showed. `platform::open_url` and the keyring
  shim.
- **Opening a file asked for All-files access.** NativeActivity gets no
  `onActivityResult`, so the system document picker can't hand a choice
  back; the in-app picker lists the app's own folder without any
  permission and offers a button for shared storage (All files access on
  API 30+, the runtime permission below). "Open with rustudio" from a
  Files app needs nothing.
- **Panels didn't scroll, no camera on touch.** `src/touch_input.rs`.
- **Start Page lists wouldn't scroll / taps misfired.** The finger layer
  ran there too, with no viewport to publish its zones, over a page that
  is plain egui; it is off until a place opens (see "Fingers").
- **Lag on a Baseplate with a release APK.** The chrome redrew the 3D
  view every frame it drew itself; see "Frame time on a phone".

## Later steps (in order)

1. Touch for the game: `UserInputService` touch events and gestures
   from the same finger stream (`touch_input.rs` already tells fingers
   apart), `TouchEnabled` / `PreferredInput`, `ModalEnabled` hiding the
   stick and button (they are the editor's own for now, holding keys).
2. Device properties and sensors (the `device_prop()` table, the
   `Device*Changed` events from `sensors::latest()` once per new sample).
3. Haptics (`HapticService`, `HapticEffect`) through `jni::vibrate*`.
4. Insets, orientation, soft keyboard.
5. `ContextActionService` touch buttons.

Each step closes rows in docs/ENGINE-GAPS.md; the Creator Hub reference
pages are the source of truth for every member name, argument and enum.
