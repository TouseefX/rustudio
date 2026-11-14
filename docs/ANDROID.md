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

## What is where

| Concern | Desktop | Android |
|---|---|---|
| Entry point | `src/main.rs` → `rustudio::desktop_main()` | `crates/rustudio-android/src/lib.rs` `android_main` → `rustudio::android::run()` |
| Window / GPU | eframe + winit (Wayland, X11, Windows, macOS) | eframe + winit on `NativeActivity` (`android-native-activity`); wgpu Vulkan, GLES 3 fallback (`wgpu_options` in src/lib.rs) |
| Lifecycle | — | eframe's wgpu backend drops the window + surface in `suspended()` and recreates them in `resumed()`; a rotation is a resize and reconfigures the swapchain |
| Folders (`src/paths.rs`) | `~/.local/share`, `~/.config`, `~/.local/state`, `~/.cache`, `~/Pictures` | `getExternalFilesDir()` for data (Studio content, places), `getFilesDir()/config`, `/state`, `getCacheDir()`, `<external>/Pictures` (`src/android/mod.rs`); `HOME` and the `XDG_*` variables are set to the same so `dirs` (rustudio-account) agrees |
| `api-dump.json` | next to the executable / app bundle / source tree | APK `assets/`, unpacked to `files/resources/` at first start (`src/android/assets.rs`) |
| File dialogs | rfd (async, on a worker thread) | rustudio's own picker over the app folder (`src/android/storage.rs`); "Open with rustudio" from file managers through the VIEW intent (`content:` URIs copied in via ContentResolver) |
| Logging | stderr | logcat tag `rustudio` (`log` → android_logger), stdout/stderr forwarded by android-activity |
| Crash reports | `~/.cache/rustudio/crash/` | `<cache>/rustudio/crash/` (same `src/crash.rs`: panic hook + signal handlers chain to debuggerd) |
| Studio content download | ureq + rustls (ring) | the same; started only when ConnectivityManager reports a validated network |
| Login keyring | Secret Service / Keychain / Credential Manager | none (`keyring` returns an error on Android); the session cookie import file under `files/config/rustudio/` is the sign-in path |

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
- `crates/rustudio-account` `keyring` — compiles for Android, `Entry::new`
  returns an error at run time, which the session code treats as "no
  keyring" and falls back to the import file.
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

## Later steps (in order)

1. Touch: winit touch events → egui pointer + `UserInputService` touch
   events and gestures; default thumbstick / jump button; two-finger
   orbit / pinch zoom for the editor camera; bigger hit targets in the
   chrome (`android::touch::setup` has the egui-side settings).
2. Device properties and sensors (the `device_prop()` table, the
   `Device*Changed` events from `sensors::latest()` once per new sample).
3. Haptics (`HapticService`, `HapticEffect`) through `jni::vibrate*`.
4. Insets, orientation, soft keyboard.
5. `ContextActionService` touch buttons.

Each step closes rows in docs/ENGINE-GAPS.md; the Creator Hub reference
pages are the source of truth for every member name, argument and enum.
