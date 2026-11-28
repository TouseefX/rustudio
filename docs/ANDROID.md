# rustudio on Android

The Android app is the same editor as the desktop one — the `rustudio`
library (src/lib.rs) — inside a `NativeActivity`. What differs is in
`src/android/` (compiled only for `target_os = "android"`), in
`crates/rustudio-android/` (the `cdylib` the activity loads, with the
`manifest.yaml` xbuild turns into the AndroidManifest) and in
`crates/rustudio-core/src/android_dirs.rs` (the app's folders, which core's
`paths.rs` and `crash.rs` read; `android::run` sets them). The pieces that
work on any system with a touch screen or a device's sensors live in the
workspace's crates like the rest of the engine: `touch_input.rs` and
`device_sensors.rs` in rustudio-engine (the sensors behind a `Source` the
Android entry point installs), `touch_gestures.rs` in rustudio-player.
Targets:
`aarch64-linux-android` (arm64-v8a, every phone and tablet) and
`x86_64-linux-android` (the emulator).

## Building

CI (`.github/workflows/android.yml`) builds four APKs on every push / PR
that touches Rust sources — arm64 and x64, debug and release — and uploads
them as artifacts (`rustudio-<arch>-<profile>.apk`). A `v*` tag builds the
two release APKs and adds them to the tag's GitHub release beside the
desktop downloads (`rustudio-android-arm64-v8a.apk`,
`rustudio-android-x86_64.apk`, `SHA256SUMS-android`); the editor's update
notice (src/update.rs) stays off on Android, where the system installs
APKs. Locally:

```sh
# once
rustup target add aarch64-linux-android x86_64-linux-android
sudo apt install clang lld llvm            # the NDK targets link with these
cargo install --git https://github.com/rust-mobile/xbuild \
  --rev e67b501cbe0e9ca5436c223aa7ec0fe5e27544d1 xbuild
x doctor                                   # what it found; nothing here fails

# the Mesa Vulkan drivers (Turnip, PanVK) + libadrenotools into the arm64
# APK's lib/arm64-v8a ("Turnip driver" below); skip it for a plain APK.
# Needs llvm-readobj (the llvm package; xbuild needs it too): it ends
# with xbuild's library check, below under "xbuild quirks"
tools/android/fetch-turnip.sh

# an APK (xbuild fetches its NDK sysroot and android.jar into ~/.cache/x)
x build -p rustudio-android --platform android --arch arm64 --format apk
x build -p rustudio-android --platform android --arch arm64 --format apk --release
ls target/x/{debug,release}/android/rustudio-android.apk

# on a device / emulator
adb install -r target/x/debug/android/rustudio-android.apk
adb logcat -s rustudio RustStdoutStderr
```

**Install the release APK to judge speed.** The debug one builds the
editor at opt-level 1 with debug assertions and overflow checks in every
crate (Cargo's dev profile): several times slower in the physics, the Luau
runtime and the renderer's CPU side, and a phone shows it first. The
start line in logcat says which one is running (`rustudio … starting
(Android API 33; debug build, aarch64 android)` / `release build, …`).
The release profile (root `Cargo.toml`) builds with thin LTO and one
codegen unit a crate: a frame crosses the crate boundaries thousands of
times (rustudio-engine into rustudio-core's document and variants for
every property a script reads, the physics solver's modules, the
renderer's), and without LTO none of those calls can be inlined. It costs
build time (the final crate's codegen is serial) and CI's link step
memory; `CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`
restores Cargo's defaults for a quick local build.

`x build` without `--pem` signs with xbuild's debug key (installs with
adb). A release key goes in as `--pem key.pem` or the `X_PEM` environment
variable; CI reads it from the `ANDROID_SIGNING_PEM` secret when set.

Without a device, `cargo check` for the target works with the same
environment xbuild uses (sysroot, `--target=aarch64-linux-android33`, clang
and lld) — see the `Build APK` step of the workflow for the variables. A
check links nothing, though: an undefined C symbol (Luau's code generator
calling `__clear_cache`, "Luau native code" below) only shows in a real
link — `cargo build -p rustudio-android --target aarch64-linux-android`
with those variables, or `x build` — which
`crates/rustudio-android/build.rs` makes strict (`-Wl,--no-undefined`).
The Android targets link libc++ statically (`.cargo/config.toml`, the
reason in the same section).

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
- Before *[3/3] Create apk*, xbuild reads every bundled library's NEEDED
  list (`llvm-readobj --needed-libs`; the `runtime_libs` files included)
  and wants each name either bundled too or present in its NDK sysroot
  (`~/.cache/x/Android.ndk/usr/lib/aarch64-linux-android/` and the
  `26/` folder under it, the minSdk's), else it stops: *Failed to
  collect all required libraries for `…/libvulkan_panfrost.so` … Shared
  library `libhardware.so` not found*. PanVK links `libhardware.so`
  (Mesa's Android gralloc path) and `libc++.so` — system libraries every
  device has (`/system/lib64`, VNDK-SP) that the NDK doesn't export, and
  which the driver reaches at run time because libadrenotools loads it
  in a namespace cloned from the system Vulkan loader's own
  (`hook_impl.cpp`, which links `libc++.so` over by name). xbuild only
  reads the *names* in its sysroot folders, so `fetch-turnip.sh` ends by
  leaving an empty marker file of each such name there (it fetches the
  sysroot tarball first when xbuild hasn't yet — the same file, which
  xbuild then skips; `X_CACHE_DIR` names another cache folder,
  `XBUILD_SYSROOT=0` skips the step) and prints which; nothing is added
  to the APK. A stub in `libs/arm64-v8a/` would be shipped and would
  shadow the real library in the driver's namespace — don't.

## What is where

| Concern | Desktop | Android |
|---|---|---|
| Entry point | `src/main.rs` → `rustudio::run()` | `crates/rustudio-android/src/lib.rs`: `rustudio_on_create` (the manifest's `android.app.func_name`) → android-activity's `ANativeActivity_onCreate` + the destroy hook; `android_main` → `rustudio::android::run()` |
| Window / GPU | eframe + winit (Wayland, X11, Windows, macOS) | eframe + winit on `NativeActivity` (`android-native-activity`); wgpu Vulkan, GLES 3 fallback (`wgpu_options` in src/lib.rs) |
| Lifecycle | — | eframe's wgpu backend drops the window + surface in `suspended()` and recreates them in `resumed()`; a rotation is a resize and reconfigures the swapchain |
| Folders (`crates/rustudio-core/src/paths.rs`) | `~/.local/share`, `~/.config`, `~/.local/state`, `~/.cache`, `~/Pictures` | `getExternalFilesDir()` for data (Studio content, places), `getFilesDir()/config`, `/state`, `getCacheDir()`, `<external>/Pictures` (`src/android/mod.rs`); `HOME` and the `XDG_*` variables are set to the same so `dirs` (rustudio-account) agrees |
| `api-dump.json` | next to the executable / app bundle / source tree | APK `assets/`, unpacked to `files/resources/` at first start (`src/android/assets.rs`) |
| File dialogs | rfd (async, on a worker thread) | rustudio's own picker (`src/android/storage.rs`) over the app folder and, once the storage permission is granted from the picker's own button, shared storage (Download, Documents, …); "Open with rustudio" from file managers through the VIEW intent (`content:` URIs copied in via ContentResolver) |
| Input | mouse + keyboard | fingers through `crates/rustudio-engine/src/touch_input.rs` (below); hardware mice / keyboards work as on a desktop |
| Clipboard | egui-winit (arboard) | egui's copies mirrored to `ClipboardManager` after each frame, pastes read from it (`src/android/clipboard.rs`) |
| Browser links (`platform::open_url`) | `ctx.open_url` (webbrowser) | `ACTION_VIEW` Intent from the activity (`jni::open_url`); eframe's `links` feature isn't built for Android |
| System bars | — | hidden (immersive mode): the navigation bar — Back / Home / Recents or the gesture pill, the right edge in landscape — and the status bar; a swipe from the edge shows them for a moment, translucent over the editor, and they go again by themselves (`src/android/system_bars.rs`: API 30+ `WindowInsetsController.hide(systemBars)` with transient-by-swipe, before that the sticky immersive view flags; asked at start and again on focus regained / keyboard down, on the Java main thread). `RUSTUDIO_SYSTEM_BARS=1` keeps them |
| Display cutout (the camera hole / notch) | — | the window goes under it on the screen's short edges (`layoutInDisplayCutoutMode` shortEdges, API 28+, set on the window's attributes from the Java main thread at start: `src/android/cutout.rs`, `jni::set_layout_in_display_cutout_mode`) where Android had letterboxed the hole's whole side in landscape, black, like the navigation bar's strip before; the editor pads itself by the hole's safe insets instead — `App::ui` lays the frame out in a child `Ui` that ends short of the hole (`Cutout::child`, as keyboard.rs's `shrink` takes the keyboard's room), the strip painted the window's colour, and the insets are egui's safe-area insets too (`Cutout::before_frame`, with the keyboard's bottom), so windows, menus and popups keep off it. The insets come from `jni::display_cutout_later` on the Java main thread, never waited for: at start (again every ½ s until the window has insets), on focus regained and after a resize (a rotation moves the hole: three readings over the second after). A played game lies inside the padded area — its view and GUIs never reach the hole, so the engine's `DeviceSafeInsets` stay zero; drawing the 3D view under the hole with only the GUIs inset would need those first. `RUSTUDIO_CUTOUT=0` leaves the window out of the cutout, as before |
| Back button | — | closes what is open on top (Escape); in Play opens / closes the Roblox menu (Escape's job, as Back does in Roblox's app; its Leave ends the session, as the Stop button at the game's top right does); otherwise `moveTaskToBack` — the activity isn't destroyed (`src/android/lifecycle.rs`) |
| Roblox menu and chat in Play | Esc and `/` | the top bar over the game view is upstream's Luau one (`players_chat_ui.luau`: `CoreGui.TopBarApp`, the Roblox button and the unibar pill with the menu and chat buttons, drawn as GUI and so hit by fingers like any button; the chat button opens the chat window with its box, the keyboard comes up for the box) when the CoreScripts are off, and Roblox's own TopBarApp / Chrome unibar when they run (Settings > rustudio > Play > CoreScripts). `SetCore("TopbarEnabled", false)` hides it; `GuiService.TopbarInset` is the bar's free area either way (`World::topbar_inset`, upstream's) |
| Activity destroyed | — | `onDestroy` hook: close request, then `process::exit(0)`; `android_main` always leaves the process when the loop ends (`lifecycle.rs`): status 0 for a close, 2 for an eframe error, 101 for a panic (named in logcat with its crash report) |
| "Open with rustudio" while it runs | Finder's hand-over (`macos::take_opened`) | a second instance of the activity per VIEW intent (`launchMode` `standard`), caught at the library's entry point: its file is copied in and queued for the editor's next frame, the editor's task comes to the front, the new activity finishes unseen (`src/android/intent.rs`). A place the last start died within 20 s of opening by itself isn't opened again by itself (`state/opening`, `storage.rs`) |
| UI zoom | 1.0 of the display's scale | first run: the zoom that fits ~1100 points across (0.8 … 1.6 of the density), then View > Zoom In / Out as saved |
| A played game | the dock's 3D view, at the UI zoom (View > Play Fills the Screen off) | View > Play Fills the Screen on: the game alone over the window at zoom 1 (a point is a dp, Roblox's unit on Android) until it ends (`RUSTUDIO_PLAY_FULL`; "The game's canvas" below) |
| GPU driver | the system's | the device's Vulkan driver; on Snapdragon phones the Turnip (Mesa) driver shipped in the APK can be swapped in before wgpu starts — **opt-in, off by default** (View > GPU Driver; `src/android/turnip.rs`; "Turnip driver" below): faster on many phones, a crash on some — on an Adreno 6xx with its bandwidth compression (UBWC) off, which restarted a Snapdragon 865 phone outright ("UBWC and the whole-device restarts" below); OpenGL ES as the fallback either way |
| Graphics quality (File > Studio Settings > rustudio > Performance) | Manual at the full profile, 100% 3D resolution; Automatic from level 5 | weak-GPU mode (`gpu_caps.rs`: every Android device): Automatic from level 1 — no shadows, SSAO, clouds, bloom, DoF, probe or grass until the frame times allow — and 75% 3D resolution (`rustudio_settings::choice_default`); "Frame time on a phone" below. View > GPU Driver names the adapter and holds the Mesa driver switch |
| Logging | stderr | logcat tag `rustudio` (`log` → android_logger), stdout/stderr forwarded by android-activity |
| Crash reports | `~/.cache/rustudio/crash/` | `Android/data/dev.rustudio.studio/files/crash/<timestamp>-<pid>.txt` on the shared volume (`adb pull`, or a PC over USB; the private cache folder needed root — and since Android 11 the Files apps can't open `Android/data` either, so the start after a crash copies the report, the session logs (`src/android/logcat.rs`: this run's and the crashed run's) and the app's own `logcat -d` into **`Download/rustudio/rustudio-logs-<stamp>.txt`**, a file any Files app shows and a user can send; Help > Copy Logs to Download does the same at any time: `src/android/logs.rs`, a `MediaStore.Downloads` row the app inserts, no permission needed). Same `crates/rustudio-core/src/crash.rs`: panic hook + signal handlers chain to debuggerd; the next start names the report in Output and logcat (`rustudio crashed last time: panic in thread … at file:line — message`). A report ends with the last 24 notes (`crash::note`): the GPU errors wgpu reported, a lost device, so it says what led up to it |
| GPU device lost, GPU out of memory | wgpu's default: a panic | The device gets an uncaptured-error handler and a device-lost callback (`android/mod.rs install_gpu_error_handler`): errors go to logcat and the report notes, the frame goes on. The vendored egui-wgpu (upstream's `renderer.rs` `upload_failed`, since 0.3.3-alpha.2) skips a frame whose index/vertex upload the device refuses instead of panicking (`renderer.rs:984 Failed to create staging buffer for index data` — what a reset Adreno driver looked like). A lost device, or out-of-memory errors every frame for 5 s, ends the run in order (`gpu_watch`): the open place is written to `Android/data/dev.rustudio.studio/files/recovery/<name>-<stamp>.rbxl`, a report says so for the next start, and the activity is relaunched (`jni::relaunch`). The desktop's own check (`gpu_health::check` in `App::ui`, which panics on a lost device so a desktop run ends with a report) is not run on Android: it ran before `gpu_watch` got its turn, and a 2026-11-24 device — 7.2 GB resident after four minutes of a 288,000-instance place, the driver out of memory — crashed in it (`gpu_health.rs:88`) instead of saving the place and relaunching. The same frame's later wgpu calls must not be fatal either: `Device::poll` on a lost device is one of wgpu's panics ("Error in Device::poll: Parent device is lost" — a 2026-11-26 report, `gpu_timing::end_frame` resolving the frame's timestamps three beams into a fight), so the per-frame polls go through `gpu_health::poll`, which skips a lost device (gpu_timing, the cloud readback, the texture upload flush); the world GUI's staging upload skips too (`world_gui.rs stage`, which also marks the loss when the belt's buffer can't be mapped). Since the 0.6.0 rebase upstream's own low-VRAM handling (`gpu_caps.rs` budgets, `gpu_health::mark_oom`, `world_gui.rs` `purge_mips` / `evict_mips_for`) is in; on Android the world-GUI mip copies keep to 64 MiB (`gpu_caps::world_gui_mips_budget`; `RUSTUDIO_GUI_VRAM_MB` lifts it) because a phone's GPU reports the whole system memory as its heap |
| Studio content download | ureq + rustls (ring) | the same, three packages at a time; Play can use the content once the packages are in (the retired textures and View Selector follow in the background). Without a network at start (ConnectivityManager, INTERNET capability — not VALIDATED, which some networks never get) it starts when one comes. A `network check: …` line in Output (and logcat) at every start says how Roblox's servers are reached, phase by phase — DNS, a TCP connect to the first IPv4 and the first IPv6 address, an HTTPS GET — with the time each took; `rustudio --network-check` prints the same on a desktop. Every agent resolves a host IPv4 first (`rustudio_account::http`): a network that advertises an IPv6 that doesn't route — some phones' Wi-Fi and mobile networks — otherwise cost every new connection a connect timeout, and the app "worked, but very slowly". Connects time out after 10 s; one agent (connection pool) per purpose for the process, so an asset doesn't pay DNS, TCP and TLS twice over |
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
| the rfd dialogs plugins open (`src/plugin_host/prompts.rs`: PromptImportFile, ImportFbxRig, PromptSaveSelection, ExportPlace / ExportSelection; 0.2.0) | as above | the same `Request`, drawn as the in-app picker by `prompts::ui` each frame (one file per prompt; the harness's `plugin-prompt:` answers work unchanged) |
| the `xdg-open` / `open` / `cmd start` spawns in `src/toolbox/ui.rs::open_url` and `src/plugin_store/ui.rs::open_plugins_folder` (0.2.0) | no such programs in an app sandbox | an ACTION_VIEW Intent (`android::jni::open_url`); the Plugins folder's path is reported in the Plugins panel instead of opened |
| `libc::getloadavg` in `crates/rustudio-core/src/platform.rs::load_average` | Bionic has it only from API 29; the symbol is absent from the libc crate's Android bindings | returns "?" |
| `eframe` features `wayland`, `x11` | desktop window systems | `android-native-activity` (target-specific dependency tables in Cargo.toml) |

Desktop-only by behaviour (compile fine, never active on Android, left
ungated so the shared code paths stay one):

- `src/hypr.rs` (Hyprland IPC through `hyprctl`), `src/layout_store.rs`
  window geometry — no `HYPRLAND_INSTANCE_SIGNATURE`, nothing happens.
- `crates/rustudio-engine/src/mouse_lock.rs` — pointer lock / confine logic over egui types; no
  pointer on a touch screen, so the Default/LockCenter paths are no-ops.
- `crates/rustudio-player/src/play.rs` gamepad scan of `/dev/input/by-id` — the folder isn't
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
- `crates/rustudio-engine/src/domains/media_video.rs` `dlopen` of libvpx / libopus — Android has
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
| `OnScreenKeyboardVisible / OnScreenKeyboardSize / OnScreenKeyboardPosition`, `TextBoxFocused / TextBoxFocusReleased`, `TextBox.ReturnPressedFromOnScreenKeyboard` | focus events fire (`play_game.rs` 247); keyboard size zero | the soft keyboard comes up for a focused TextBox and goes with its focus (`keyboard.rs` `end_frame`, `gui::share().focus`); the user hiding it releases the focus (FocusLost). While it is up for a game's TextBox it lies over the view, which slides up to show the box (`Editor::keyboard_pan`, src/viewport.rs), and `OnScreenKeyboardVisible / Position / Size` report it in the game's coordinates (`touch_input::soft_keyboard`, `runtime_ext.rs` `device_prop`); Enter on a touch screen fires `ReturnPressedFromOnScreenKeyboard` before `FocusLost` (gui_input.luau `lose`) |
| `HapticService.IsVibrationSupported / IsMotorSupported / SetMotor / GetMotor` | gamepad-only flags, in-memory motor table (`gui_input.luau` 1796-1845) | `jni::vibrator_info()` (hasVibrator, hasAmplitudeControl) and `jni::vibrate(ms, strength)` — **wiring to do**: `UserInputType.Touch` supported, `VibrationMotor.Large` (and `Small` when amplitude control exists) |
| `HapticEffect:Play / Stop`, `HapticEffect.Type` presets | simulated durations (UIHover .035 s, UIClick .055, UINotification .4, GameplayCollision .3, GameplayExplosion .8) | `jni::vibrate_pattern` (VibrationEffect.createWaveform) — **mapping of each preset to do** |
| `ContextActionService.BindAction(..., createTouchButton)`, `GetButton / SetTitle / SetImage / SetPosition` | bindings stored, priority dispatch (`library.luau` ContextActionService section); with the CoreScripts on, Roblox's own ContextActionTouch draws the buttons (PlayerGui `ContextActionGui`) from `BoundActionAdded / Removed / Changed` and answers `GetButton` through `GetActionButtonEvent` / `FireActionButtonFoundSignal`, as StarterScript starts it on a touch screen (`UserInputService.TouchScreenEnabled`, RobloxScriptSecurity, is `TouchEnabled`); without them no buttons | without them, this build's own (`crates/rustudio-engine/src/domains/context_action_touch.luau`, started by `gui_input.luau start_touch_buttons` in Play on a touch screen once the LocalPlayer's PlayerGui is there): Roblox's ContextActionTouch rebuilt the way it builds — `PlayerGui.ContextActionGui.ContextButtonFrame` (30 % × 50 % of the screen from (70 %, 50 %)), a 45 × 45 `ContextActionButton` ImageButton per action (35 × 35 on a screen 320 tall or less) at one of its seven places, its `ActionIcon` / `ActionTitle` from `SetImage` / `SetTitle`, `SetPosition` moving it, the frame visible while `PreferredInput == Touch` and `GuiService.TouchControlsEnabled` — so a game that finds the button with `GetButton` and restyles it finds what it finds in Roblox. A finger on a button runs the handler with `Begin`, its moves `Change`, its lift `End` (a lift after it dragged off the button: `Cancel`) through `ContextActionService:CallFunction` (RobloxScriptSecurity, new in `library.luau`: Roblox's own ContextActionTouch presses its buttons with it too, so with the CoreScripts on the buttons now work as well as show). Test: `touch_buttons_without_the_core_scripts` |
| `GuiService:GetGuiInset()`, `TopbarInset`, `GetInsetArea`, `ScreenGui.ScreenInsets` (`None / DeviceSafeInsets / CoreUISafeInsets / TopbarSafeInsets`), `ScreenGui.SafeAreaCompatibility` | `(0, 58)` constant (`mech::GUI_TOP_INSET`); `TopbarInset` in Play is the top bar's free area — from the end of the menu and chat buttons (128 px with the chat button, 72 without: `gui::fallback_topbar_inset`; the CoreScripts' own `GuiService:SetTopbarInset` when they run) to the viewport's right edge, 58 high — and `GetPropertyChangedSignal("TopbarInset")` fires when the viewport or the chat button changes (`gui_input.luau step_topbar`, upstream's; LBG's ClientRenderer and TopbarPlus wait on it); an empty Rect in the edit DataModel; a `TopbarSafeInsets` ScreenGui is that rect (TopbarPlus's icons: starts after the buttons, 58 high; `gui_input.rs` roots, `runtime_ext.rs gui_rect`) | `jni::window_insets()` (system bars + display cutout; the 4 sides) — **wiring to do**: DeviceSafeInsets = those, CoreUISafeInsets = those + topbar, TopbarSafeInsets = topbar only. With the system bars hidden and the editor laying the game out short of the display cutout (`src/android/cutout.rs`) all four are zero today, so the constant answer is the right one on a device |
| `StarterGui.ScreenOrientation`, `PlayerGui.ScreenOrientation / CurrentScreenOrientation` | stored (`start_page/builtin.rs` 144); `CurrentScreenOrientation = LandscapeLeft` | `jni::set_requested_orientation` + `jni::current_orientation()` — **wiring to do**: apply at Play start, restore `sensorLandscape` at stop |
| `Player:GetMouse()`, `Mouse` while playing | desktop pointer | **to do**: the first touch stands in (as Roblox does) |
| `VRService`, gamepads | stubs | unchanged |

## Fingers (`crates/rustudio-engine/src/touch_input.rs`)

egui-winit makes the first finger the mouse pointer with the left button
down, which is wrong for most of the editor: a finger laid on an Explorer
row starts dragging the row instead of scrolling the list, and a finger on
the 3D view box-selects. `Touch::filter` runs in `App::raw_input_hook`,
before egui sees the frame's events, and rewrites the `Event::Touch`
stream (dropping egui-winit's stand-in pointer events):

| Finger | Becomes |
|---|---|
| moves at once (> 8 pt before 320 ms) on the chrome | a scroll of the panel under it (`MouseWheel` events in a Start/Move/End phase, applied 1:1), with a fling after the lift |
| still for 320 ms on the 3D view | a left press (grabs the part or handle under it), dragging from then on |
| still for 320 ms on the chrome, then moves | a left press where it landed, dragging from then on (Explorer rows, text selection, sliders, splitters, windows) |
| still for 650 ms on the chrome | a right click where it landed: the context menus (Explorer rows, dock tabs) and the text fields' Cut / Copy / Paste / Select all menu (`src/android/text_menu.rs`); no left press goes out before it, so a selection under the finger is kept for the menu; the lift is nothing (and if no menu opened, a move from there is the drag below after all) |
| lifted without moving | a tap (press + release at the landing point) |
| moves at once on the 3D view | a right-button drag: the editor camera's look; in Play the game camera (the PlayerModule's right-drag orbit, mouse deltas from the finger) |
| on a transform handle | a left press at once (drags the handle) |
| second finger on the 3D view | ends the first one's drag; `InputState::multi_touch` gives the viewport a pinch (zoom toward the fingers; in Play the game's wheel: `mech::PINCH_NOTCHES_PER_DOUBLING`) and a two-finger pan |
| in Play, lands bottom-left / bottom-right of the view | the movement stick (W/A/S/D held by its deflection, drawn under the finger) / the jump button (Space) — `touch_input::play_zones`; released when Play stops or the window loses focus. A quick tap in the stick zone is still a tap (GUI buttons there keep working). Stick and jump fingers are kept out of egui's touch stream, so a look finger beside them is never read as a pinch |

A lift leaves the pointer where it was for the rest of its frame: the
`PointerGone` goes to egui at the start of the next one. egui clears
`latest_pos` — what `Response::hover_pos` reads — at a `PointerGone` in
the frame it comes, and the GUI input shared by the game's ScreenGuis in
Play, the plugin widgets and plugins' CoreGui (`crates/rustudio-engine/src/gui_input.rs`
`input`) takes a press or a release only where the pointer hovers, so a
tap whose press, release and `PointerGone` came in one frame was no click
on any GuiButton: nothing in a plugin widget answered a tap, nor TopbarPlus
icons or a game's own buttons in Play, while egui's own widgets (which
use `interact_pos`) were fine. With the pointer gone a frame later the
release lands, the GuiButton gets its MouseButton1Down / Up / Click /
Activated, and the MouseLeave follows next frame.

The viewport publishes where things are each frame (`Zones`: its rect,
whether the game has the input, the stick and jump areas, a handle under
the pointer) and draws the stick and button in Play (`Touch::paint`). The
chrome's egui settings for fingers are in `android::touch::setup`. The
tuning numbers (hold 320 ms, long press 650 ms, slop 8 pt, fling, stick
and button sizes) are `mech::TOUCH_*` with their rows in
docs/MECHANICS.md ("Touch screens") and mechanics.json. The module has no
Android dependency, so its tests run on the desktop
(`cargo test touch_input`).

### Fingers in Play: the game's touch (`touch_input::game_touch`)

The table above is the editor's reading of fingers. A game's scripts
expect a touch screen's: `UserInputService.TouchEnabled`, `PreferredInput
== Touch`, one `InputObject` per finger for its life (`UserInputType.Touch`,
`Position` in GUI coordinates, `Delta` since its last event) through
`InputBegan / Changed / Ended` and `TouchStarted / Moved / Ended`,
ContextActionService bindings on `Enum.UserInputType.Touch`, the input
action system's `TouchPosition / TouchDelta / TouchPinch` bindings with
`UIButton` / `UIModifier`, GuiObject `InputBegan` with a Touch object
and `TouchTap`, and the gestures (`TouchTap`, `TouchTapInWorld`,
`TouchLongPress`, `TouchPan`, `TouchPinch`, `TouchRotate`, `TouchSwipe`).
Both PlayerModules (the old `PlayerModule.module` and the input-action one
in `PlayerScripts.rbxp`) draw their own thumbstick and jump button from
those and nothing else, so while the game drives (Play, client, menu
closed) on a touch screen the editor's stick and jump zones are off and
fingers that land on the view go to the game raw:

1. `touch_input::Touch::filter` keeps a finger that lands on the view
   (outside `Zones::ui`: the top bar's menu and chat buttons, the hotbar
   slots and the chat box stay egui's)
   out of egui's stream and queues it as a `GameTouch` (Begin / Change /
   End / Cancel with its position); Play ending or the window losing
   focus cancels them all.
2. `viewport.rs` hands the frame's `GameTouch`es to `Gui::touch_input`
   (`crates/rustudio-engine/src/gui_input.rs`): hit-tests each against the game's ScreenGuis,
   fires the GuiObjects' `MouseEnter / Moved / Leave`, `InputBegan /
   Changed / Ended` (a `GuiArg::Touch` → the finger's InputObject),
   `MouseButton1Down / Up / Click`, `Activated` (Active buttons only; an
   `Active = false` button like the DynamicThumbstick's full-frame
   UIModifier does not take the finger: `Hit.active`) and `TouchTap`, and
   returns a `TouchOut` per finger: GUI coordinates, the delta, `sunk`
   (an Active object or a TextBox under it when it landed) and
   `began_on` (the GuiButtons under its landing point). With the game
   menu open every finger is `sunk`. A finger that lands on a
   ScrollingFrame (the top one under it with `ScrollingEnabled`) drags
   its canvas (`GuiEvent::ScrollDrag`: `CanvasPosition` follows the
   finger at once, against it, limited by `ScrollingDirection`), and
   when it lifts moving the canvas keeps its speed and slows down
   (`GuiEvent::Fling`; `play_game.rs scroll_fling` + the glide step,
   `mech::GUI_TOUCH_FLING_*`, rustudio's own curve); a finger that
   scrolled past the tap slop doesn't click the button it landed on.
   Whether the camera turns too is `Active`'s, as in Roblox: a
   ScrollingFrame with `Active = false` scrolls and lets the finger
   through to the camera (the DevForum's "camera moves while I scroll"
   threads, fixed by setting Active); most of LBG's lists are
   `Active = false`. The unit test is gui_input.rs
   `a_finger_drags_a_scrolling_frame_and_does_not_click_through_it`.
3. `play_game.rs game_viewport` runs the gestures over them
   (`crates/rustudio-player/src/touch_gestures.rs`: tap, tap in world, long press, pan, pinch,
   rotate, swipe; `mech::TOUCH_*`), makes an unprocessed tap in the world
   the game's click (the Mouse's `Button1Down / Up`, ClickDetectors,
   tools) and the latest finger the game's `Pointer` while no mouse is
   there.
4. `play.rs` dispatches each `TouchOut` to `__rs_touch_event` and each
   gesture to `__rs_touch_gesture` (`crates/rustudio-engine/src/library.luau`) after the GUI's
   events — on the gameplay worker since 0.3.3 (`play_worker.rs`: the
   presentation's `Input::take` carries the frame's fingers and gestures
   over with its keys and GUI events, `Input::apply` queues them for the
   worker's step; `GameState::touches_fed` keeps `game_viewport`, which
   runs every frame, from feeding a finger to the recognizer twice while
   the worker still holds the last step — the first phone build of the
   rebase lost every finger here, and grew the list without bound, the
   `play_worker.rs` test `fingers_reach_the_gameplay_worker` pins it;
   what the worker's client decides for the presentation,
   `SetCore("ChatActive")` and `GuiService.TopbarEnabled`, returns in
   its publication): `domains/gui_input.luau` "Fingers" assigns the finger to the
   input actions whose bindings use it (`UIButton`: landed on it;
   `TouchPosition / TouchDelta / TouchPinch`, gated by `UIModifier` when
   set, else only an unsunk finger; contexts by Priority, `Sink` stops)
   — a ViewportPosition action reads the finger's viewport position
   (+58 px: the PlayerModule adds `GetInsetArea(None).Min` back),
   `TouchDelta` the lone finger's motion as a rate, `TouchPinch` the
   first two fingers' distance change as a rate — then the library runs
   the ContextActionService bindings and fires UserInputService's events
   (`gameProcessedEvent` when the GUI or a bound action took it).

`device_prop` (`crates/rustudio-engine/src/runtime_ext.rs`) answers `TouchEnabled`,
`TouchScreenEnabled`, `PreferredInput = Touch`, `MouseEnabled =
KeyboardEnabled = false` on a touch screen and `AccelerometerEnabled /
GyroscopeEnabled` from the device's sensors (`crates/rustudio-engine/src/device_sensors.rs` over
`src/android/sensors.rs`: `GetDeviceAcceleration / Gravity / Rotation`
and the `Device*Changed` events, in g and as a CFrame, the sensors on
from the first script that asks). `UserInputService:GetLastInputType()`
answers `Touch` on a touch screen before any finger has landed (Roblox's
first answer is the device's own kind; `library.luau` `last_input`,
Keyboard on a desktop): the PlayerModule's ControlModule reads it once
as it starts to pick its controls, so the thumbstick and `TouchGui`'s
`JumpButton` exist from the first spawn — games build their own touch
buttons next to that JumpButton when the server's join-time settings
arrive (LBG's `SetSettings`: with Keyboard as the first answer the
JumpButton only appeared at the first touch, and the game's Dash / M1 /
Block buttons only with the next spawn's re-send). `RUSTUDIO_TOUCH=0` (the
`debug.rustudio.env` property) puts the editor's stick and jump back
(the keyboard-and-mouse reading of fingers) on a device; `=1` turns the
game's touch on on a desktop with a touch screen.

**ContextActionService's touch buttons** (`BindAction(name, fn, true,
...)`): Roblox's ContextActionTouch CoreScript draws one per action with
`createTouchButton` in `PlayerGui.ContextActionGui` and presses it with
`ContextActionService:CallFunction(name, state, inputObject)`
(RobloxScriptSecurity). With the CoreScripts on (Settings > rustudio >
Play > CoreScripts) that script runs here; without them
`crates/rustudio-engine/src/domains/context_action_touch.luau` does its
job, built from its source (Roblox-Client-Tracker
`scripts/CoreScripts/CoreScripts/ContextActionTouch.lua`): the same
`ContextButtonFrame` (the screen's lower-right 30 % × 50 %), the same
45 × 45 `ContextActionButton` ImageButtons (35 × 35 on a screen 320 tall
or less) at its seven places — the first free one, `SetPosition` moving
a button —, `ActionIcon` and `ActionTitle` children from `SetImage` /
`SetTitle` (SourceSansBold 18, white, stroked), its up / down images
(assets 97166444 / 97166756), `Maximum of 7 allowed touch buttons` as
its warning, the frame shown while `PreferredInput == Touch` and
`GuiService.TouchControlsEnabled`; `GetButton` gets the button through
`GetActionButtonEvent` / `FireActionButtonFoundSignal`. A finger
landing on a button runs the handler with `UserInputState.Begin` and
the finger's InputObject, its moves with `Change`, its lift on the
button with `End`, and a lift anywhere else after it dragged off with
`Cancel` (Roblox's AWAY state; heard from `UserInputService.InputEnded`,
since the button no longer has the finger) — `CallFunction` is new in
`library.luau` for both scripts (the CoreScript's buttons showed but did
nothing before, the method missing), runs the handler at the binder's
identity and reports its error as the script's. `gui_input.luau
start_touch_buttons` starts the drawer once in Play on a touch screen
(`touch_input::game_touch`) when the LocalPlayer's PlayerGui exists and
the CoreScripts aren't running (`core_scripts.scripts` empty); the
PlayerModule's own thumbstick and jump button are theirs as before. Test:
`runtime.rs touch_buttons_without_the_core_scripts` (the button, its
children, `GetButton`, a game script refused `CallFunction`, Begin / End
and Begin / Cancel from two fingers, `UnbindAction` taking it away).

**The game's canvas: View > Play Fills the Screen** (on by default on
Android; `RUSTUDIO_PLAY_FULL=1` / `0` sets it, saved with the layout).
While a game is played it alone covers the window, at the display's own
scale — egui zoom 1, so a point is a density-independent pixel, Roblox's
GUI unit on Android — and the menu bar, ribbon, status bar, dock and
panel windows come back when it ends (`src/app.rs` `play_fills_screen` /
`play_full_frame`). The way out is the **Stop button at the top right of
the game** (`play_stop_button`: one of Roblox's top-bar IconButtons, a
red stop square, where the bar keeps nothing of its own — its Roblox and
chat buttons are at the left and a game's TopbarPlus icons follow them;
a finger landing on it stays egui's, `Zones::ui`; a press let go over it
counts however long it lasted or the frame between was, egui's own click
giving up after 0.8 s), since the ribbon's Stop is hidden with the ribbon
and a phone has no Shift+F5. Checked headless: `--play --window 1280x720
--ui-click 1242,34@10` on a place ends the session (the result JSON's
`state` reads `edit`). **Beside Stop, a Studio button** (a window glyph,
`play_full_button`) puts the game back in the dock's 3D view without
ending it — the Output, Explorer, Properties, plugin widgets and the
ribbon's Stop around it, the editor's zoom back — for reading a game's
prints or using a plugin while it runs; the view's top-right **Fill the
Screen button** (four corners) takes it back over the window. Session
state only (`Editor::play_full_exit`): the next Play fills the screen
again, and View > Play Fills the Screen stays the setting. Headless:
`--ui-click 1186,34@10` docks it (the window shows the ribbon and the
dock with the game still playing) and a click on the view's corner
button fills it again. The Back
button still opens Roblox's menu, whose Leave (then its confirmation)
ends the session too, and a game that kicked the player (`Player:Kick`)
shows Studio's "Disconnected" dialog with a Leave button that ends it as
well (play_game.rs `draw_kicked`). Without it a phone's Studio, zoomed out to fit
the ribbon (the first-run zoom above, 0.8), gave a played game the dock's
3D view: a canvas about a third of the screen (833 × 316 points on a
1550 × 720 phone, 258 under the top bar) in units a fifth smaller than
Roblox's, so the PlayerModule's thumbstick and jump button and LBG's Dash
/ M1 / Block / Shiftlock buttons, laid out from the bottom up in those
units, ran up under the top bar and off the view. The same phone now
gives the game 886 × 411 units, what Roblox's app gives it: the phone
layout (`min(TouchControlFrame.AbsoluteSize) <= 500`), a 70-unit jump
button, every button on the screen.

### Text: the soft keyboard, Cut / Copy / Paste

winit never resizes an Android window for the soft keyboard (the
manifest's `adjustResize` moves NativeActivity's content rect; the surface
egui draws on stays the whole screen), so `src/android/keyboard.rs` makes
the room: while a text field has the focus, or the room is still made, it
asks the Java main thread for the keyboard's height every 100 ms
(`jni::keyboard_height_later`: `WindowInsets.getInsets(Type.ime())` on
API 30+, the system inset less the stable inset before that; the editor's
thread never waits for the answer, which comes a frame or two later) and
takes that much off the root `Ui` at the start of `App::ui` — the dock,
the 3D view and the status bar move up as an Android view would — and
gives it to egui as the bottom safe-area inset, so windows, menus and
popups keep above it too (set every frame, zero when the keyboard is
down: egui keeps the last inset it was given). When the room is made or
changes, the focused field's caret is scrolled into view
(`keyboard::reveal_cursor`, from the Properties cells and the script
editor: the field's `TextEditOutput` gives the caret's row).

A game's TextBox (Play, `gui::share().focus`) is the exception: the
panels are not moved for it. On a landscape phone the keyboard takes
half the height and the menu, toolbars and status bar most of the rest,
so the 3D view above them was a strip a few lines tall with the game's
GUI squeezed into it and the box typed into off it, under the status
bar. Instead the keyboard lies over the window, as it does over a
phone's games — and then **nothing** is moved for it: the bottom
safe-area inset stays zero as well (`Keyboard::over`), because egui lays
its panels and top-level `Ui`s inside the safe area (`content_rect`), and
an inset given while the panels were left alone had the status bar float
up to the keyboard's top edge on its own, over the view. Only
`Editor::keyboard_pan` (src/viewport.rs, the studio crate) moves: it slides the 3D view,
its GUIs with it, up by what shows the box above the keyboard (and the
text bar on it, below) — as Android pans a window to its focused field:
never past the box's own top edge leaving the view, never further than
the keyboard covers the view — and back when the keyboard goes. What
slides above the view's panel is clipped; the fingers that land there are
the toolbar's (`Zones::viewport` is the visible part). **The status bar
is not drawn while the keyboard is up**, in either mode (`RustudioApp::ui`,
`Keyboard::room` > 0): it carries nothing a typist needs, a 2026-11-25
report had its Team Create / camera strip over the text being typed, and
in the panel mode it took a line off the few a landscape phone has left
above the keyboard. **Neither are the menu bar, the test bar and the
ribbon while an editor field has the keyboard** (`Keyboard::typing_layout`:
a field of the editor's own focused and room made for the keyboard — not
a game's TextBox, which moves nothing): a landscape phone is about 410 pt
tall, the keyboard and the text bar take about 200 of them and the three
top panels about 140, which left the script editor two or three lines
("still can't see what I'm typing", 2026-11-25); without them the dock
gets the whole strip above the keyboard, eight to ten lines. They come
back with the keyboard going down (the focus released, Back, Done). The
game's scripts see the
keyboard where Roblox's client reports it: `UserInputService.
OnScreenKeyboardVisible / OnScreenKeyboardPosition / OnScreenKeyboardSize`
in the game's coordinates (`touch_input::set_soft_keyboard`, read by
`runtime_ext.rs` `device_prop`), and Enter from the touch screen fires
`TextBox.ReturnPressedFromOnScreenKeyboard` before `FocusLost` (a
runtime test drives both through the Lua side on the host:
`runtime.rs soft_keyboard_reaches_the_scripts`).

**What the keyboard types goes through an `InputConnection` of the
app's own** (`src/android/ime.rs`, `tools/android/java/dev/rustudio/studio/ImeBridge.java`).
A NativeActivity has no view that offers the keyboard one, so the
keyboard ran in the framework's fallback mode: single characters came as
key events (winit's `Event::Text`), and everything else a keyboard does —
a word composed with suggestions and committed, an auto-correction, a
swiped word, its own paste key, deleting a word — came as a
`KeyEvent.ACTION_MULTIPLE` whose characters only the Java object carries,
which native code never sees. Text went missing, keyboards that both
compose and send keys (Samsung's) delivered some of it twice or garbled,
and in the script editor, where a keyboard composes every word, nothing
much arrived at all. The fix is a Java class: `ImeBridge` is a 1 × 1
`View` added to the activity's content (`addContentView`), focusable only
while the editor wants the keyboard, whose `onCreateInputConnection` hands
the keyboard a `BaseInputConnection` over a `SpannableStringBuilder` that
mirrors the focused field — text, selection, the keyboard's composing
span — so the keyboard works as it does in an `EditText`. Every change
the keyboard makes to the mirror (one call, or the batch it bracketed with
`beginBatchEdit` / `endBatchEdit`) is found by comparing the mirror before
and after and reported to the editor as one replacement, code points
`[a, b)` becoming a string with the selection at `[selStart, selEnd)`,
through a native method (`nativeOp`, on the Java main thread, queued for
the editor's thread). The editor applies one replacement a frame from
`App::raw_input_hook`: an egui `TextEdit` gets its cursor range set to
`[a, b)` (`TextEditState`) and the text as `Event::Text` (Backspace for a
deletion, Enter for the action key), which it handles as typing over the
selection; a Roblox TextBox gets the same as a selection and an insertion
in `Gui::keyboard` (`gui::share().tb_ime`). At the end of the frame the
field's text (egui's `OutputEvent::FocusGained / ValueChanged /
TextSelectionChanged / Clicked` carry it; a TextBox's state is
`share().tb_live`) is hashed (FNV-1a over code points, the same on both
sides) and checked against what the Java side computed of its mirror; a
difference — the editor auto-indented an Enter, closed a bracket, the
user tapped the caret elsewhere, pasted, undid, a hardware key — sends the
Java side the editor's state (`setState`, with the number of the last
operation it includes, so a state behind an operation in flight is left
for the next one; the keyboard is restarted when its composition would be
stale). The editor's text is the truth throughout, so a disagreement
costs at most a dropped suggestion. The script editor's field is
`KIND_CODE`: no suggestions, auto-correction or capitals
(`TYPE_TEXT_FLAG_NO_SUGGESTIONS` plus the visible-password variation,
which is what Samsung's keyboard honours); a multi-line TextBox is
`KIND_MULTILINE` (Enter is a line); everything else `KIND_TEXT` (Enter is
the action key: the field commits, as Enter from a hardware keyboard
does). Key events the keyboard still sends — Enter, Delete, a character —
are applied to the mirror and swallowed there, so they aren't typed a
second time through the window; arrows and the rest pass through to
native code as before. The keyboard's own Select all / Cut / Copy / Paste
(`performContextMenuAction`) work on the mirror and the system clipboard.
A character that nonetheless arrives through the window as a key event
while the keyboard is up through the bridge would be the same character
again: key-event text (winit's `Event::Text`; never `Event::Key`) is held
back one frame and dropped if an operation of the keyboard's arrives
meanwhile — the key event can reach the window a frame before the
operation — else typed (`ime.rs key_text`): a hardware keyboard, whose
keys reach the window before any input method and so never come through
the bridge, types as before, a frame later. Logcat says which way a
doubled character came: `ime: op …` for the bridge's, `ime: N key-event
character(s) dropped / typed` for the window's.

The doubling a 2026-11 device had ("s" typed as "ss", a paste pasted
twice) was the bridge's own: every operation a keyboard makes outside a
`beginBatchEdit` / `endBatchEdit` pair of its own was reported twice —
`BaseInputConnection`'s `commitText`, `setComposingText`,
`finishComposingText` and `deleteSurroundingText` bracket their work with
those two calls, and the bridge's overrides of them counted the inner
pair as a batch of the keyboard's and reported it as it ended, then
reported the operation again on its own account (`ImeBridge.java
mutate`; keyboards that batch every call, as Gboard does, were reported
once, which is why the bridge looked right with one keyboard and typed
"ss" and pasted twice with another). The batch count is now held up
through the operation's run, so the inner pair reports nothing. The
editor also drops an operation that merely repeats the one before it —
the next number, the same replacement, the mirror's hash the same after
it, which a replacement made anew cannot leave (`ime.rs repeats`) — and
logs `ime: op N reports op M again … dropped`.

The class is compiled by `tools/android/build-dex.sh` (javac → d8, with
an SDK's `android.jar` and `d8`; run it when the Java changes) into
`assets/android/ImeBridge.dex`, which is committed, built into the native
library with `include_bytes!`, and loaded at start by `ime::install`
with an `InMemoryDexClassLoader` whose parent is the activity's class
loader; `nativeOp` is bound with `RegisterNatives` — the APK build needs
no Java toolchain, Gradle or SDK, and the manifest's `NativeActivity`
stays. logcat says `ime: bridge loaded (N byte dex)` and `ime: input
connection view attached`, then `ime: show for Code, N chars, selection
a..b`, `ime: op 12 replace 3..3 with 5 chars, selection 8..8`, and `ime:
op 12 left the field another text than the keyboard's mirror: the mirror
takes the editor's` when a resync happened. Should the dex fail to load
(`ime: the bridge's class couldn't be loaded (…)`), the keyboard is asked
up through the activity as before, in the fallback mode.

The keyboard is shown and hidden by `keyboard.rs` itself
(`Keyboard::end_frame`), not by egui-winit. egui-winit shows it while
egui reports an IME field (`PlatformOutput::ime`, a focused `TextEdit`:
`set_ime_allowed` → android-activity's `show_soft_input` /
`hide_soft_input`) and answers every IME interruption egui asks for with
a hide and a show — and egui counts every `request_focus` as one, the one
its own `TextEdit` makes for the tap that focuses it and for every tap
that moves its caret included (`egui-winit` 0.36 `handle_platform_output`,
`should_interrupt_composition`): the keyboard opened, closed and opened
again on each tap into a field, whatever the editor's own fields did
(they had already stopped asking for the focus every frame). So at the
end of each frame the IME output is taken out of egui's platform output
(egui-winit never asks winit for the keyboard) and the bridge's view is
asked (`ime::show` / `hide`: `InputMethodManager.showSoftInput` for the
view, with the field's kind, text and selection — or the activity
directly, as winit would, without the bridge) when what wants the
keyboard changes: a focused egui text field, or a Roblox TextBox with the
focus — the game's in Play, a plugin widget's — which egui-winit never
showed it for. Up at once
(`soft keyboard asked up for a text field` / `a TextBox` in logcat), down
once nothing has wanted it for 250 ms (`asked down`), so a field losing
the focus for a frame or the focus moving between fields keeps it up, as
Android keeps it between EditTexts. The user hiding it with Back or its
own hide key is still invisible to the app except through the height
readings, which go through hysteresis (`Keyboard::follow`): the first
height above zero is taken at once (`soft keyboard up: N px, M ms after
the focus`), a different height only after two equal readings running, a
zero while a field is focused is ignored for 600 ms — longer than the
hide and show of a focus move and than the slide out — and only then
releases the focus (`soft keyboard hidden by the user`; a TextBox gets
FocusLost): the edit ends with its text kept, as a click away ends it,
and a tap on the field starts it again. A zero with nothing focused takes
the room away at once.

The soft keyboard has no Ctrl, egui's text fields have no menu of their
own, and egui-winit has no clipboard on Android. So while the keyboard is
up for anything, a **text bar** stands in a band
right above it (`src/android/text_bar.rs`, `HEIGHT` points; part of the
room keyboard.rs makes, or of what the view slides up for a game's
TextBox — `text_bar::in_use`): Cut, Copy, Paste and Select all for an
egui field, Paste and Copy for a Roblox TextBox (a game's in Play, a
plugin widget's), fed to egui as the events the shortcuts would be
(`Event::Cut` / `Copy` / `Paste(text)` / Ctrl+A) through `raw_input_hook`
on the next frame. The bar is up with the bridge as well: the keyboard's
own paste, clipboard panel and Select all can reach the field through the
input connection (`commitText`, `performContextMenuAction`), but a
2026-11 Android 13 report had a keyboard whose panel did nothing, and a
game's TextBox has no long-press menu. The dark strip labelled "Text" /
"TextBox" over a game in Play is the price of a paste that works.
Whatever room the keyboard takes is painted the window's colour
(`Keyboard::shrink`): the band
between the panels' end and the keyboard's top was left unpainted, black,
the moment the keyboard went before the next height reading, and while
the bar's band was reserved with the bar not up. egui's `TextEdit` answers
them; a TextBox gets them in `Gui::keyboard` (crates/rustudio-engine/src/gui_input.rs): Paste
types the clipboard's text in (a single-line box gets line breaks as
spaces), Copy copies the box's text. The `Editor::shortcuts` Copy / Cut /
Paste actions (objects, in the Explorer) stand back while a TextBox has
the focus, as they do for an egui field. A long press on an egui text
field still opens the same entries as a menu (`src/android/text_menu.rs`,
which keeps the field's focus for its tap — egui's rule is a click
outside takes it — and puts back the selection egui's right click
collapsed). The bar keeps the focus too, and had to be more careful
about it: egui judges the click outside as the *field* draws, before the
bar does, so the field had lost the focus by the time the bar's turn
came, the bar (drawn for a focused field) wasn't drawn in the very frame
the tap completed in, and its button never saw the click — the bar did
nothing on a device. From a press that begins on the bar to its release,
egui's rule is set to `SurrenderFocusOn::Never` (`TextBar::before_frame`,
from the raw input), and put back after. Objects are cut, copied and
pasted from the Explorer's long-press menu as before. The keyboard's own
clipboard key pastes through the input connection as well (above), which
needs no clipboard access of the app's: the keyboard reads it as the
default IME.

The clipboard itself is `src/android/clipboard.rs` over
`ClipboardManager`, asked on the editor's thread (since API 24 the
manager a Context hands out works from any thread — the "needs a Looper
thread" condition is Android 4's — and from Android 10 it answers the app
of the focused window only: a read from elsewhere, or while the window
isn't focused, comes back empty, not as an error). A write that fails
there is handed to the Java main thread (`jni::clipboard_set`; egui's
copies are taken out of the platform output at the end of the frame). A
paste reads the clipboard at that moment (`clipboard::text_now`, one JNI
call), so what was copied in another app a second earlier is what
pastes; the text bar and the long-press menu read once when they open
(`clipboard::prime`) to enable their Paste, and the last answer is kept
(`clipboard::text`) — the editor's own copies seed it, so a paste right
after a copy works even where the system refuses the read. The main
thread is the fallback when a read fails on the editor's thread
(`clipboard::refresh`, answered a frame or two later). Each step logs
a `clipboard:` line (`N chars put on the system clipboard`, `N chars read
from …`, `the system clipboard has no text (or this window lacks the
focus)`, `put failed: …` / `read failed: …`), the text bar logs
`text bar: …`, and `adb shell dumpsys clipboard` shows whether a primary
clip exists at all.

The layer is **off on the Start Page** (`Touch::set_enabled` from
`raw_input_hook`, while `start_page_ui` would draw): that page is plain
egui — lists that scroll by drag (`ScrollArea`'s own touch rule), cards
that are taps, long presses for its menus — and has none of the editor's
reasons to tell a scroll from a drag. Switching off lets go of anything
held; a finger down at a switch stays egui's until it lifts, so no press
is left half delivered. Once a place opens, the layer is on.

### Publish on a phone

Publish / Save to Roblox copied the whole tree (`Document::save_snapshot`)
to encode it on the upload's thread, so the editor went on meanwhile. A
place of some size doubled in memory at the press of Publish and the
system killed the process on the spot — no panic, no signal, so no crash
report; `logcat` says `lowmemorykiller` / `am_kill`, which the logs file
above now carries. On Android the place is encoded on the editor's thread
from the tree itself, as Save to File does (`cloud_ui.rs start_save`,
`cloud: place encoded for the upload in 0.8s, N bytes` in logcat), and
only the bytes go to the upload's thread.

### Luau completions in the script editor (`src/luau_intel`)

The script editor's row of completions above the text (plugins'
`ScriptEditorService` items were all it had) now comes first from the
place and Roblox's API, ported from Android-rbxl's `luau_intelligence.rs`
(the "Sora" completions): globals and libraries, `game:GetService("…")`,
the services' children by name, classes' properties / methods / events,
`Enum.X.Y`, the script's locals, functions and parameters, the members a
required ModuleScript returns (instance and string requires, `./`, `../`,
`@self`) and require paths; a line above it shows the signature of the
call the caret is in with the current parameter in bold. A tap on an item
replaces what was typed of it (`apply_suggestion_at`) and puts the caret
after it. The place's index (`ProjectIndex`, every script's source with
the open tabs' unsaved text over it) is cached by a fingerprint and
rebuilt when the place changes, at most every four times its own cost
(`luau intel: place indexed in N ms` in logcat when that is over 50 ms);
the completion itself is computed when the text or the caret changes. The
API tables (`api_data.rs`, static) and the reflection questions
(`schema.rs`, over `rbx_reflection_database`) came along; the port's 47
unit tests run on the host (`cargo test --lib luau_intel`).

**A tap on a completion keeps the keyboard up** (the 2026-11-25 report:
"a suggestion tap closes the keyboard"). egui's rule is that a click
outside the focused widget surrenders its focus, and it judges the click
as the *field* draws — so the editor had no focus for the rest of the
tap's frame, the keyboard's mirror ended its session on "no field"
(ime.rs), the focus came back only through `request_focus` after the
field (the built-in items; the plugins' items never asked for it), and
the keyboard went down after `HIDE_AFTER` whenever the tap outlasted a
frame or two. Now the strips register their rects every frame
(`keyboard::keep_focus_in`) and `Keyboard::before_frame` holds egui's
`surrender_focus_on` at `Never` from a press that began inside one to
the frame of its release — the text bar's device (above), made general;
both accept paths put the caret after the word and keep the focus; the
keyboard is never asked down while a finger is on the screen
(`Keyboard::end_frame`); and the editor hands the mirror its text at the
end of every frame it draws (`ime::field_text`), since the mirror learned
the text from egui's change events only, which a completion taken
outside the field never made — the keyboard's next letter was placed by
the old text. The log line for a hide now names what has the focus.

**Enter, then a letter on the line before** (the same report). After each
of its operations the keyboard was told nothing of where the caret now
was: a TextView calls `InputMethodManager.updateSelection` (and
`updateExtractedText`, for a keyboard that asked to follow the text)
after every edit, its own included, and a keyboard keeps a model of the
caret from those calls and composes and deletes by it. `ImeBridge.java`
now does the same at the end of every operation (`Conn.finish`:
`tellSelection`, and `getExtractedText` remembers a monitor request), and
`ime::unshift` moves a reported insertion to where the mirror's caret
says it was made when a plain diff put it at the end of a run of equal
characters (Enter at the end of a line above other lines came back as a
line break *at the start of the next line*, with the caret moved back a
frame later).

**Auto-indent and Tab** (`src/code_indent.rs`, 5 host tests: `cargo test
--lib code_indent`). Studio's Auto Indent Rule, Tab Width and Indent
Using Spaces settings drive it: Enter keeps the previous line's
indentation and goes one level deeper after a line that opens a block
(`then`, `do`, `function (...)`, `repeat`, `else`, `{`, `(`); Enter right
before the block's closer (`if x then|end`, `{|}`) puts the closer on a
line of its own at the outer level; a closer typed at the start of a line
(`end`, `else`, `elseif`, `until`, `}`) steps out to its opener's
indentation (a small Luau lexer skips strings and `--` comments; typing on
past it, `endTime`, steps back in). On the desktop (and a hardware
keyboard) upstream 0.6.0's own auto-indent does it: `script_editor/indent.rs`
`handle_keys` takes the frame's Enter / typing events ahead of egui's
`TextEdit` (the port's earlier after-the-fact hook went with the 0.6.0
rebase; the two rules agree). The soft keyboard's text, though, goes through the mirror: had
the editor indented after the fact, the mirror would have been a frame
behind and the keyboard's next letter placed by the unindented text. So
`Conn.codeFixup` asks the native side *inside* the keyboard's operation
(`nativeCodeFixup` → `ime::native_code_fixup` → the same
`code_indent::on_insert`) and makes the replacement on the mirror before
the operation is reported: the one replacement the editor applies holds
the indentation, and field, mirror and keyboard agree. The soft keyboard
has no Tab key: the editor's header shows **Tab** and **Untab** while the
field is focused (one unit at the caret, or the selection's lines in and
out; `code_indent::tab` / `untab`), held in focus like the strips. The
dex was rebuilt (`tools/android/build-dex.sh`, 18 088 bytes).

## The first Play on a phone: Studio's content

Roblox's default character (the R15 body with its face and dynamic head),
its `Animate` and `Health` scripts, and the PlayerModule whose
CameraModule follows the character and whose ControlModule moves it all
come from Studio's own content, which rustudio has none of in the APK: at
its first start the editor downloads it from Roblox's CDN (about 70 MB
compressed, 14 packages plus a hundred or so retired textures fetched one
by one; `crates/rustudio-core/src/studio_content.rs`; an install from an
earlier build fetches the packages listed since at the next start) into the app's files folder
(`…/files/rustudio/studio`). That takes a few minutes on a phone. Until
it is there, Play spawns rustudio's stand-in character — plain blocks, no
face, no animation — and no camera or movement script follows it unless
the place brings its own: a place with a `PlayerModule` of its own in
StarterPlayerScripts (LBG's fork, with its shift lock, camera and touch
controls) gets the engine's `PlayerScriptsLoader` with or without the
content (`default_scripts.rs insert_own_loader`,
`PLAYER_SCRIPTS_LOADER`), so the fork runs as on Roblox; without one,
the engine's keyboard stand-in moves the character. Output
says so at the spawn (`Studio's content isn't installed yet…`), lists
each package as it comes (`Studio content: downloading …zip (3/13)…`)
and ends with `Studio content <version> installed in …` or an error. Stop
and press Play again after that line (fonts need a restart). The download
goes on where it stopped at the next start when the app is killed
meanwhile (packages already unpacked are kept in `.studio-partial`), and
retries a dropped connection a couple of times by itself. Uninstalling
the app removes the content with its data; `adb install -r` keeps it.

## Frame time on a phone

The renderer and its quality system are upstream's, unchanged on Android
(the port's own graphics system — a 1–10 quality bar with its tables, a
lite shader, a motion scale with EASU + RCAS, frame reuse, a frame-rate
cap, a perf log — was dropped when the port was rebased onto
0.3.3-alpha.2, which brought its own: `crates/rustudio-render/src/quality.rs`,
`gpu_caps.rs`, `gpu_timing.rs`, the Performance page of File > Studio
Settings > rustudio, and the MicroProfiler). What a phone gets from it:

- **Weak-GPU mode on every Android device** (`gpu_caps.rs` `weak_reason`:
  software adapters, OpenGL and old Intel graphics on a computer; a
  phone's GPU is a tiler with a tenth of a desktop card's bandwidth and a
  thermal budget). Graphics quality defaults to Automatic
  (`rustudio_settings::choice_default`) and Automatic starts at its lowest
  level (`quality::WEAK_START_LEVEL`: no SSAO, no sun or light shadows,
  no clouds, depth of field, bloom or scene probe, no grass) and climbs as
  the frame times allow (Studio's Windows profile, `quality::TARGET_MS`).
  `RUSTUDIO_LOWEND=0` turns the mode off for a run, `=1` forces it on a
  computer; `RUSTUDIO_AUTO_QUALITY_LEVEL=N` pins Automatic's level (1–21)
  and `RUSTUDIO_QUALITY_TRACE=1` logs each step.
- **The start, and the black window before the first frame**: the
  renderer is built in `App::new` (`Game::new` → `Renderer::with_caps`:
  the shader module, the post, bloom and depth-of-field passes, the sun
  and light shadow atlases) and the viewport's own pass set — some
  thirty pipelines through the driver's compiler for one sample count —
  when a frame first needs it (`render.rs` `ensure_pipelines`). Upstream
  builds a set at start (`startup_samples`: 4x MSAA, or 1 sample where
  the 4x shader can't run, as on OpenGL 4.3) and, since 0.4.0, a worker
  thread makes the 1x set meanwhile for a session likely to draw with it
  (weak-GPU mode, Automatic quality, Low detail;
  `RUSTUDIO_NO_PIPELINE_PREBUILD=1` turns the worker off); on a phone the
  quality level draws one sample a pixel (`quality.rs` `samples`, levels
  1–16), so the port builds nothing at start, takes the worker's 1x set
  at the first frame (or builds it there), and pays for the
  `probe_samples` set only when the scene probe or a ViewportFrame asks
  for it. logcat
  (`RustStdoutStderr`) times the pieces: `[startup] renderer built in N
  ms`, `[startup] viewport pipelines for 1 sample(s) a pixel built in N
  ms`, `[startup] first frame built in N ms, M ms after the engine's
  start` (`RUSTUDIO_STARTUP_TRACE=1` prints them on a desktop); the
  activity's own start to `android_main` is the system's share.
  The driver's share of those pipelines is paid once per driver now:
  the device is made with wgpu's `PIPELINE_CACHE` feature where the
  adapter has it (Vulkan), every pipeline descriptor names the
  process's cache (`crates/rustudio-render/src/pipeline_cache.rs`:
  `init` in `Game::new`, `current()` in the descriptors), and its blob
  — the `VkPipelineCache` data, the compiled shaders inside it — is
  written to `<asset cache>/pipelines/wgpu_pipeline_cache_vulkan_<vendor>_<device>.bin`
  two seconds after a burst of pipeline builds (`save` each frame; at
  once on exit and when the activity goes to the background) and
  handed back at the next start, which then skips the compiles the
  driver recognises. logcat: `[startup] pipeline cache primed with N
  KB` / `empty (first run on this driver)`, and `pipeline cache: N KB
  written after M pipeline(s)`. wgpu checks the blob's header (adapter,
  driver version) and the driver its own; a mismatch (a driver update,
  a Turnip swap) starts empty. Turnip and PanVK keep their disk shader
  cache besides (`MESA_SHADER_CACHE_DIR`, the Turnip section), so they
  gained less from this than the vendor drivers, which keep nothing
  between processes. `RUSTUDIO_NO_PIPELINE_CACHE=1` turns it off.
  The blob is never read out of the driver's cache (`get_data`) while
  another thread builds pipelines into it (`pipeline_cache::building`,
  held by the shader warm-up worker per pass set and by the
  specialised-pipeline threads; `save` skips a frame or, forced at
  exit, waits): Vulkan asks drivers to allow the two at once, but a
  stock Adreno Vulkan has aborted on it (`pthread_mutex_lock called on
  a destroyed mutex`, seen in another Android port of rustudio).
- **Shaders are compiled on the phone, never precompiled**: every shader
  is WGSL (`shader.wgsl`, `clouds.wgsl`, `postfx*.wgsl`, the inline ones)
  that this device's driver compiles at run time; the APK carries no
  compiled shaders, and the two caches above are written by the phone
  for its own adapter and driver version (a desktop's blob would fail
  wgpu's header check and start empty). What the port adds is *when* the
  compiling happens: the **shader warm-up**
  (`crates/rustudio-render/src/warmup.rs`). Upstream's worker made only
  the 1x viewport set; on Android (or with `RUSTUDIO_SHADER_WARMUP=1` on
  a desktop) it goes on to everything a frame would otherwise compile at
  its first use — the `probe_samples` (4x) viewport set the scene probe
  needs in every place's first frames, the depth post passes, the world
  GUI and highlight passes at the session's first sample count, the
  cloud and adornment passes at both, the ViewportFrame blit — handing
  each over on a channel as it is done (`Renderer::poll_warmup` each
  frame). A frame that needs a viewport set the worker is about to
  deliver waits for it (`ensure_pipelines` → `Warmup::wait_viewport`:
  the same thirty pipelines, not made twice); anything else it builds
  itself and the worker skips (`mark_local`). So the first run on a
  driver fills the caches in the background instead of on the UI
  thread, and a primed run has every pass ready a few hundred
  milliseconds in — no first-Play, first-selection or first-clouds
  compile hitch, and none when Automatic quality steps to a level that
  draws 4x (a compile there was a slow frame the controller read as
  "too slow", stepping back down). logcat: `[startup] shader warm-up: N
  pass set(s) built on the worker in T ms (S made by a frame first; P
  pipeline(s) through the cache; viewport sets at [1, 4] sample(s) a
  pixel)`. `RUSTUDIO_SHADER_WARMUP=0` (or `RUSTUDIO_NO_PIPELINE_PREBUILD`)
  turns the worker off; not on OpenGL either way (the shared context).
  **View > GPU Driver > Shaders** shows the warm-up line, the pipeline
  cache's state (primed KB, pipelines made this run, KB on disk) and
  the switch **Rebuild shader caches at the next start**
  (`src/android/shader_cache.rs`: a marker; the next start deletes
  `<cache>/rustudio/pipelines/` and Mesa's `turnip_shader_cache` /
  `panvk_shader_cache` before any driver loads, logs `[startup] Shader
  caches rebuilt this start: N file(s), K KB removed (…)`, and compiles
  everything afresh — a slower first frame once; for after a driver
  update, or when frames stutter where they didn't).
  `RUSTUDIO_SHADER_REBUILD=1` in `debug.rustudio.env` does the same (on
  a desktop it removes the pipeline cache blob, `pipeline_cache::init`).
- **Play's start** (the "game loads slowly" of the second phone report)
  is timed the same way: `[startup] Play started in N ms: runtimes made
  A, server DataModel copied B, client DataModel replicated C, physics
  built D, globals bound E, server queries warmed F, server scripts
  started G, player added H, …; K scripts compiled (X KB, C from the
  bytecode cache, M --!native) in T ms` — the "play: …" marks of
  `start_play` (`crates/rustudio-core/src/load_trace.rs` `play_step`)
  and the compiler's share (`runtime.rs compile_script` →
  `load_trace::note_compile`) — and once more eight seconds in
  (`[startup] 8 s into Play; K scripts compiled …`: modules compile as
  they are required, so most of a big game's compiling comes after the
  start line). **Since 2026-11-25 a place's scripts compile for Play in
  the background as soon as the place is open**
  (`bytecode_cache::precompile`, from `Editor::set_document`: one thread,
  every Script / LocalScript / ModuleScript source of 1 KB and more,
  into the memory and (4 KB and more) disk caches; a line `[startup] N
  of M scripts (K KB) precompiled for Play in T ms` when it is through;
  `RUSTUDIO_NO_PRECOMPILE=1` skips it) — Roblox has a place's scripts
  compiled by the time the player is in, and rustudio's first Play of a
  place compiled everything as the game started, finding the cache only
  from the second Play on. **Compiling is a small part of the start**:
  LBG on a 2-core desktop (debug build) starts Play in 1.4–1.6 s, of
  which the 209 scripts loaded at start compile in 411 ms cold and 41 ms
  from the cache; the rest is making the two Luau VMs (`runtimes made`
  ≈ 340 ms: `Runtime::new` + `install_side` each), copying the DataModel
  for the server (≈ 350 ms) and replicating it to the client (≈ 350 ms),
  the server scripts' first run (≈ 140 ms) and the player's set-up
  (≈ 120 ms). After the start line the game's own first frames are the
  wait a player feels: LBG's client runs about 2 s of script in its first
  eight frames (`ClientRenderer` 1 s, TopbarPlus' `IconController` 0.4 s,
  the camera module, the HUD scripts) and the server 1.2 s in its first
  fourteen (`LimitedShopTimer`'s DataStore session, `Bootstrap.Init`),
  which `RUSTUDIO_SCRIPT_PROFILE=live` (`--bench`'s `scripts` report,
  docs/PROFILING.md) shows per script. A phone's own numbers: its
  `[startup] Play started in …` line is in the session log. (Under
  `--fixed-dt` the same start takes 55 s on the desktop, all of it in
  `server head start`'s six frames and none of it script time (312 ms
  for the whole run): a harness artefact of the simulated clock —
  probably the synchronous decodes it asks for (`gui.rs`, `media.rs`
  `sim_clock::on()`), not checked — not the phone's.) `RUSTUDIO_LOAD_TRACE=1` (through `debug.rustudio.env`)
  prints the full `LOADTRACE` trail instead: the place read and parsed,
  every frame over 100 ms with its biggest bench sections, the asset
  queues as they change, each line with the resident size (`[rss N MB]`,
  what `lmkd` judges the app by).

  Big places and memory (2026-11-24, a 44 MB place of 996 380 instances
  that crashed on open, and a 30 MB one that froze at Play): the file's
  25.6 M property values parsed to ~4 GB, and Play held four copies of
  the place. Now (`rbx_binary` vendored, marked rustudio):
  - values equal to what `Document::prop` falls back to aren't stored
    (`doc.rs implied_default`; names read straight from a property map
    are always kept): 12.3 M of 25.6 M stored, every read the same
    (`implied_default_tests`; `RUSTUDIO_TEST_PLACE=<.rbxl>` runs the
    check over a real place). `RUSTUDIO_KEEP_DEFAULTS=1` stores them all;
  - an instance's property list isn't sized for every property its
    class has (2.2 GB for a million instances before a value was read),
    and each class's instances share their common values as soon as the
    file's chunks for that class end, not after the whole file;
  - the play worker gets Play's server copy made once
    (`start_play_threaded`); its `start_play` takes it as the server
    DataModel instead of copying it again (a fourth copy, ~1 GB and ~6 s);
  - copies size their tables once and skip the reference fix-up for
    instances without references (`rbx_dom_weak` `clone_multiple_into_external`).
  Desktop measure of that place: open peak 4.0 → 2.6 GB (19 → 10.5 s);
  Play out of memory past 6 GB → running at 5.7 GB. Left: Play's start
  still blocks the UI (~12 s there) while the main thread copies the
  place and the worker builds the client copy and starts the scripts. What is known of the cost without a trace:
  LBG is 7.3 MB of Luau in 685 scripts, 67 of them `--!native` (1.4
  MB — their machine code is no longer built as they load: they run
  interpreted for their first frames and `runtime.rs native_drain`
  builds them 3 ms a frame once Play runs, logged as `[startup] native
  code for N --!native script(s) (X KB) built in T ms over F frame(s)`;
  `RUSTUDIO_NATIVE_NOW=1` builds at the load as before, and the
  bytecode disk cache keeps the compiled form of every script over 4 KB
  across launches, so a second launch compiles next to nothing —
  `RUSTUDIO_NO_NATIVE=1`
  interprets them instead); the bytecode cache
  (`crates/rustudio-engine/src/bytecode_cache.rs`) kept bytecode on
  disk only for sources over 256 KB, so a phone compiled the 80 scripts
  between 16 KB and 256 KB (5 MB) again at every launch — on Android
  everything over 16 KB goes to disk now (`MIN_DISK`), and the memory
  cap is 128 MB instead of 768. The place's 154,682 instances are read,
  copied for the server and replicated to the client at every Play; the
  trace tells which of these a phone spends its time in.
- **3D resolution 75 % by default on Android** (the same
  `choice_default`): the first phone measurements fell well under the
  display's rate at full resolution, and at a phone's pixel density three
  quarters reads as sharp. The dialog's choice (100 / 75 / 50 / 33 / 25 %)
  is saved as on a desktop.
- **The rest of the Performance page** — Shading detail, Ambient
  occlusion, Shadow quality, Grass detail, Glow effects, Idle grass
  animation — works as on a desktop and is saved in the settings file
  (`files/config/…`). Presentation and Editor frame limit don't apply:
  both are `src/native_loop.rs`'s (the desktop window's own event loop,
  which paces Mailbox frames to the refresh or the limit), and the
  activity runs eframe's loop; the swapchain is vsync (`AutoVsync`:
  `offscreen::present_mode`, as the standalone player's) and the display
  sets the rate. Each row has an
  environment switch (`RUSTUDIO_PERF_QUALITY`, `RUSTUDIO_PERF_RESOLUTION`,
  `RUSTUDIO_PERF_DETAIL`, `RUSTUDIO_PERF_SSAO`, `RUSTUDIO_PERF_SHADOWS`,
  `RUSTUDIO_PERF_GRASS`, `RUSTUDIO_PERF_BLOOM`, `RUSTUDIO_PERF_FPS`,
  `RUSTUDIO_PERF_PRESENT`, `RUSTUDIO_PERF_IDLE`; the values are the
  dialog's ids, `rustudio_settings.rs` `OPTS`) that wins over the saved
  choice for the run.
- **View > GPU Driver** (`src/graphics_settings.rs`) names the adapter,
  backend and driver the 3D view runs on (Vulkan is the one wanted; a GL
  fallback is slow), says when weak-GPU mode is on and why, and on Android
  holds the Mesa driver switch ("Turnip driver" below).
- **Measuring**: the status bar's fps (`RustudioApp::ui`) counts the
  frames that followed the frame before at once — asked for, or brought by
  a touch — and leaves out the first after a pause (a still editor at 5
  frames a second is idle, not slow). The MicroProfiler (View >
  MicroProfiler; docs/PROFILING.md) is the same window as on a desktop and
  reads `.rsprof` captures; `RUSTUDIO_GPU_TIMING=1` adds the GPU time per
  pass group from timestamp queries where the driver has them inside
  encoders (`gpu_timing.rs`; Adreno and Mali do, through Turnip and PanVK
  too). And "debug build" in the start-up line (`rustudio … starting`) is
  the answer before any of that: the debug APK runs the editor at
  opt-level 1 with debug assertions in every crate, several times slower
  than the release APK.
- **The main thread's own per-frame work in Play** is what a profile
  capture of a big game shows first, and it is platform-independent: a
  `--profile-capture` of LBG on the desktop (debug, `--no-render`,
  2026-11-25, `--play-only --no-render --profile-capture`) had the main lane busy
  10.4 ms a frame, 6.6 of them in `vp_input` — `gui_modal::modal_shown`
  walking every descendant of the PlayerGui each frame the game held
  the mouse, to see whether a Modal button is up (LBG's GUI is tens of
  thousands of objects). It is an index kept from the change journal
  now (`gui_modal::ModalIndex`, in `Gui`), a full walk once per
  document. What is left on that lane is the game's: TopbarPlus'
  `updateTopbar` at 4–56 ms a call, the GUI walk (`gui_walk`) at 27 ms
  p95 when LBG's layout changes, `p9_collect` spikes to 300 ms while the
  game's first clones become garbage, and the camera module's occlusion
  raycasts (`popper`, 10 ms average over the first frames). The voxel
  destruction behind LBG's craters (`VoxelClient.luau`: up to 400 pooled
  debris parts with physics and a Heartbeat raycast loop,
  `MaxSplitsPerHit` 4096 anchored pieces moved with `BulkMoveTo`,
  `ClientDebrisPerHit` unlimited; the Virus Overlord's Infernal Blades
  beam voxelises a 90-stud cube) is the game's own load, cut in half by
  its Settings > Fast Mode (`S_FastMode` halves the debris) or off with
  `S_NoDestruction`; the harness probe for it is
  `tools/probes/voxel1.luau` (it needs the character on a
  `VoxelOrigin` part — the spawn is not on one).
- **"The viewport lags only while things spawn and fly" (2026-11-26,
  LBG on a phone).** Reproduced on the desktop with
  `tools/probes/burst.luau` (400 falling debris parts, 2000
  anchored parts, 10 VFX parts a Heartbeat; `--play-only --no-render
  --luau server tools/probes/burst.luau@9 --luau client
  tools/probes/fpswatch.luau@5 --profile-capture`, tools/probes/README.md).
  The render lane keeps 60 fps throughout; what drops is the gameplay
  thread's rate — Heartbeats a second 30–36 → 8–18 while ≥ 50 debris
  parts are awake (`fpswatch.luau`), single digits in the worst second
  — so every script-driven thing (VFX tweens, the camera, voxel debris
  moved by `BulkMoveTo`) moves in steps while the world itself stays
  smooth. Two causes, both in the gameplay step:
  1. *Physics over budget while the debris is awake.* ≈400 awake bodies,
     2800 contact pairs, 6400 solver rows at 240 Hz: the step waits
     6–8 ms for the busy physics pool (`pf_spawn`) and the hurried
     environment ticks (`physics_hurry_tail`) take 1–7 ms more, every
     gameplay step. That is the sustained part of the lag; on a phone
     the pool's threads also share the big cores with the render thread.
     On Android the pool now defaults to **two threads**
     (`android::export_env` sets `RUSTUDIO_PHYSICS_THREADS=2` unless
     `debug.rustudio.env` says otherwise): a phone has four fast cores at
     most and the render, gameplay and UI threads are on them already;
     with four workers one lands on a slow core and every fork-join tick
     waits for it. Other knobs: `RUSTUDIO_PHYSICS_HURRY=0`,
     `RUSTUDIO_PHYSICS_CATCHUP=0` (a hurried frame's environment ticks
     are lost, as Roblox's throttle loses them, instead of caught up
     between frames), `RUSTUDIO_SOLVER_SERIAL=1` (`mech.rs`
     `PHYSICS_HZ`, `PHYSICS_SERIAL_MAX_AWAKE`); the game's own Fast Mode
     halves the debris. The desktop harness (2 CPUs, swapping at LBG's
     1.7 GB) is too noisy to rank these — a phone capture during a
     burst is what decides them.
  2. *One-off stalls when a script mass-edits parts.* LBG's server init
     (`Bootstrap.Init.Loaders.workspace`, ≈15 s into Play) sets
     `CollisionGroup` on 6500 map parts and `CanCollide` on 2600; a
     dropped leaderboard destroys hundreds; the debris pool is
     (un)anchored in batches. Each used to cost: a 300 ms
     `render_snapshot_apply` (the viewport dropped the previous frame's
     6500-entry property map on the main thread — now deferred to the
     worker, `snapshot.rs discard`); a physics rebuild that re-created
     and woke every one of those parts (`[physics] big rebuild: 6567
     seeds` — a `CollisionGroup` write now sets the collider's groups in
     place, and the marks a publish moves aside keep their property
     names (`held_marks`) instead of becoming bare refs that rebuilt the
     part whatever the property was; the client's copy of the map, which
     its physics leaves alone until a body of its own comes near, no
     longer seeds a rebuild for each replicated write either; a rebuilt
     resting assembly also stays asleep under the rapier solver); and a
     `scene_sync` that
     re-read every part's draw (`ss_part_draw` 278 ms — non-visual
     properties no longer mark a part for re-reading,
     `scene.rs non_visual_prop`). What is left of the stall is the
     script's own run (the writes cost 1–25 µs each, the init's
     `PreloadAsync`, `Voxel:RegisterOrigin` 3600 calls, `GetDescendants`
     over the whole map).
  Script hot spots the same captures showed (`RUSTUDIO_SCRIPT_PROFILE=exact`):
  TopbarPlus (`setDropdown` 573 ms, `GetTextSize` 841 calls),
  `QueryDescendants` (Luau, 65 calls 346 ms), `GetTagged` 64 ms (the
  TagCache rebuilds), `IconController` Heartbeat (`TweenService:Create`
  2754×), the camera's Popper rays. `RUSTUDIO_SNAPSHOT_LOG=1` prints
  the mass edits as they happen (docs/PROFILING.md).

Switches for a device, which has no shell environment to launch from:

```
adb shell setprop debug.rustudio.env "RUSTUDIO_PERF_SSAO=off RUSTUDIO_PERF_RESOLUTION=50"
```

then relaunch (`setprop debug.rustudio.env ""` clears). Space-separated
`RUSTUDIO_*=value` pairs, 91 characters at most (`android::export_env`):
the `RUSTUDIO_PERF_*` rows above, `RUSTUDIO_LOWEND`,
`RUSTUDIO_AUTO_QUALITY_LEVEL`, `RUSTUDIO_QUALITY_TRACE`,
`RUSTUDIO_GPU_TIMING`, `RUSTUDIO_FRAME_LATENCY` (swapchain images less
one, 1 to 3; below), `RUSTUDIO_PRESENT_MODE` (`fifo`, `relaxed`,
`mailbox`, `immediate`, `vsync`, `novsync`; a mode the surface lacks falls
back to FIFO), `RUSTUDIO_NO_PERF_HINT` (no ADPF hint session for the
frame thread; below), `RUSTUDIO_TURNIP` (0|1|force), `RUSTUDIO_GPU_TURBO`
(1), `RUSTUDIO_TU_DEBUG` (Turnip's flags; "Turnip driver" below),
`RUSTUDIO_SYSTEM_BARS` (1: the navigation and status bars stay, instead of
immersive mode), `RUSTUDIO_PHYSICS_THREADS` (the physics pool; 2 on a
phone by default, "Frame time on a phone" above), `RUSTUDIO_PHYSICS_HURRY`,
`RUSTUDIO_PHYSICS_CATCHUP`, `RUSTUDIO_SHADER_WARMUP` (0: no startup
shader warm-up worker; on by default on a phone), `RUSTUDIO_SHADER_REBUILD`
(1: this start deletes the pipeline cache and Mesa's shader cache first;
"Shaders are compiled on the phone" above), `RUSTUDIO_PERF_LOG` (0: no
frame report; below).

### The frame report

Every five seconds the phone logs one line on where its frames went
(`crates/rustudio-player/src/perf_report.rs`; logcat tag `rustudio`,
`perf: …`; the same line under View > GPU Driver > Frame report, with
a Copy button — paste it into a bug report). It reads:

```
perf: 296 frames in 5.0 s: 16.9 ms apart (p95 21.6, worst 43); CPU 10.9 ms a frame
= app 9.8 + egui/present 1.1; GPU 4.5 ms a drawn frame (296 measured) = post 3.5
+ shadows 0.5 + scene 0.5; 3D view 845x349 px at level 3; sections (ms a frame):
viewport 6.1, vp_after 4.5, plugin_tick 2.0, gui_world_pass 1.2; frames asked by:
viewport.rs:3172 x296, widgets_ui.rs:943 x167
```

- *apart*: the time between frames (mean, 95th percentile, the worst
  one in the window). A 60 Hz phone that keeps up reads 16.7.
- *CPU*: eframe's measure of the whole frame on the UI thread, split
  into the app's own update (`app`: the editor, the 3D view's
  preparation, the game's GUI) and the rest (egui's tessellation, the
  UI's draw, the present); `slept` is the frame limit's sleep, left out.
- *GPU*: the 3D view's passes from the GPU's own timestamps (needs the
  adapter's timestamp queries; "GPU not timed" without), per drawn
  frame, the heaviest first: `scene` (the parts, terrain, sky), `post`
  (the tone map and upscale; bloom, DoF), `shadows`, `ssao`, `see-through`,
  `world gui`, `post (after gui)`. A GPU-bound phone shows the GPU total
  near or over the frame spacing; a CPU-bound one a small GPU total next
  to a large CPU one.
- *3D view*: the render target's pixels, Automatic's level (1–21) and
  the MSAA count when above 1.
- *pipeline(s) built*: shaders the driver compiled in the window (a
  hitch each, on the first frames and at a new quality tier).
- *sections*: the UI thread's `bench::scope`s, ms a frame, the ten
  heaviest at 0.3 ms or more (`viewport` holds the 3D view's frame,
  `vp_render` the GPU command encoding, `lighting` the lights and grids,
  `plugin_tick` the plugins, `dock_panels` the editor's panels with the
  viewport inside them).
- *frames asked by*: what requested egui repaints (file:line in rustudio
  or egui), the four most frequent. A frame loop that never rests shows
  its instigator here.

`RUSTUDIO_PERF_LOG=0` turns it off (`=1` turns it on on a computer, to
stderr through the log). Its cost is the GPU timestamps (a few per
frame) and one string every five seconds.

### The frame's pacing on Android

- **Three swapchain images** (`offscreen::frame_latency`,
  crates/rustudio-player; `RUSTUDIO_FRAME_LATENCY` 1 to 3 overrides):
  egui's low-latency default asks for one frame of latency, two images.
  Android's compositor keeps the image on screen until it has latched the
  next one, so with two images every frame first waits for that — a vsync
  of nothing before any drawing, half the refresh rate at best, which was
  a ~31 ms wait per frame at 60 Hz on the first phone whether the camera
  moved or not (egui notes the same halving on iOS and leaves the default
  there). With a third image the next frame starts while the last one is
  still queued, and the CPU's work overlaps the GPU's.
- **Vsync** (`offscreen::present_mode`): `AutoVsync` on Android whatever
  Performance > Presentation says — a desktop's Mailbox ("Low latency")
  is paced to the refresh by `src/native_loop.rs`, which the activity's
  eframe loop hasn't, and an unpaced Mailbox loop on a phone draws as
  fast as the GPU allows, heat and battery for frames the display never
  shows (the standalone player keeps vsync for the same reason).
  `RUSTUDIO_PRESENT_MODE=mailbox` for a measurement. The display's
  refresh rate (`Display.getRefreshRate`, `app.rs display_refresh_hz`: a
  phone that switches between 60 and 120 Hz says which it is on) is the
  governor hint's budget.
- **Post-processing once a frame** (`render.rs` `postfx_after_gui`,
  `Game::draw_world` in game_view.rs): the post chain — depth of field,
  bloom, sun rays, the tone map and the upscale to the display size —
  ran in `render` and then again after the world GUIs and effects had
  been drawn into the HDR scene (`finish_again`), so a game showing a
  BillboardGui or a particle paid it twice every frame: 2–4 ms of a
  phone's GPU at FHD+. When the GUI pass will follow (the frame's GUI
  batches, or effects drawn the frame before), `render` leaves the post
  to it; a caller that draws nothing after `render` gets it at its next
  highlight, adornment or frame. (ViewportFrames keep their own.)
- **Light grids and moving parts**: parts that only moved refresh the
  light grid (the sun and sky occupancy voxels, 4 studs) at most every
  1.5 s and the light-occlusion grid (shadowed local lights, 2–4 studs)
  every 0.5 s — each a gather over the place's parts on the UI thread, a
  worker and a 3D texture upload. In Play on a phone a move only counts
  when a voxel could see it (`game_view.rs` `moves_matter`: an opaque
  part of a quarter of a voxel's volume or more, or 256 movers at once),
  so characters' limbs, debris and bullets — moving every frame of every
  game — no longer rebuild both grids for as long as anyone walks. A
  walking character's own voxel shade stays where the last rebuild put
  it until a door, a vehicle or an edit moves the grid on. The editor
  and the desktop keep every move.
- **The frame limit: 60 on a phone** (Performance > Editor frame limit;
  `rustudio_settings::choice_default_for`, `RUSTUDIO_PERF_FPS` overrides,
  Unlimited can be picked). A 90 or 120 Hz display otherwise ran the
  game — Heartbeat and RenderStepped scripts, the physics steps, the
  render snapshot, egui — at the display's rate, up to twice the CPU
  work of Roblox's phone client, which caps at 60 unless the player opts
  in, for the battery and the heat that throttles the cores a minute in.
  The limit is first asked of the panel (`src/android/frame_rate.rs`:
  `ANativeWindow_setFrameRate`, Android 11+, looked up at run time): on
  an adaptive display setting SurfaceFlinger moves a 120 Hz panel to 60
  and vsync paces the frames with nothing slept, at half the panel's
  power (logcat `display: 60 Hz asked for the frame limit`, then
  `display: 60 Hz` once it switched; the rate is read again every few
  seconds). Where the panel stays faster (a fixed 120 Hz setting, an
  older Android), `app.rs pace_frame` sleeps the rest of the period
  before each frame — the desktop's pacing is `src/native_loop.rs`'s,
  which the activity's eframe loop hasn't (its sleep is left out of the
  governor's hint and the hint's budget becomes the limit's period).
  Nothing happens when the limit is at or above the display's rate, so a
  60 Hz phone at the default never sleeps and vsync paces it as before.
- **The CPU governor told the frame budget** (`src/android/perf_hint.rs`;
  `RUSTUDIO_NO_PERF_HINT` turns it off): Android's performance hints
  (ADPF, `PerformanceHintManager`, API 31+). A frame loop that works for
  8 ms and then waits 25 ms for the display looks idle to the governor,
  which keeps the big core at its lowest clock — the 8 ms become 15, the
  frame misses its vsync and waits for the next, and looks idler still.
  The session is opened for eframe's thread (the editor, egui and the
  renderer's encoding all run on it) with the display's period as the
  target, and every frame
  reports the last frame's CPU time (eframe's, without the wait for the
  display); the governor raises the clocks when the work runs close to the
  budget and lowers them when it has slack. This is what Android's games
  get through the Frame Pacing library and Unity / Unreal through their
  ADPF plug-ins; the vendors put it at 10–20 % off the CPU side of a frame
  and a steadier frame. The same thread is given Android's display
  priority (`Process.setThreadPriority(THREAD_PRIORITY_DISPLAY)`, nice
  −4, what an app's RenderThread runs at): `android_main` runs on a
  thread of android-activity's at the default priority, level with the
  editor's own workers (texture decoding, downloads, the content
  install), and a frame should not wait its turn behind them. The startup
  log says `frame thread N: display priority` and `perf hints: session
  for thread N at 16.7 ms a frame`, or why not (API 30 and below, or a
  vendor that returns no session).
- **No cursor commands in Play.** `mouse_lock::note_window` took the
  Android window for one that can't lock the cursor, so a game's
  LockCenter grab fell back to Confined and sent a cursor position every
  frame, which winit rejected with a warning logged each time.
- The release APK is built with thin LTO (`CARGO_PROFILE_RELEASE_LTO` in
  android.yml; the workspace profile is unchanged for desktops).

What the port's own renderer had and upstream's hasn't yet, in case a
device run asks for it back on top of `quality.rs`: a lower render scale
while the camera moves (upscaled by EASU + RCAS), the last 3D frame shown
again while nothing in it changed, a lite shader for the lowest levels
(no normal maps, no reflections, one light), smaller material texture
arrays, half-size sun shadow tiles at the low levels, one `vkQueueSubmit`
a frame through egui's paint callback, a per-frame perf line in logcat.
The measurements that motivated each are in the port's previous patch,
the one against 0.3.2 (its `crates/rustudio-render/src/graphics.rs` and
this file's "Graphics quality" section there).

## Luau native code (`--!native`): `vendor/luau0-src`, `.cargo/config.toml`

Upstream turned Luau's code generator on (mlua's `luau-jit`,
crates/rustudio-engine/Cargo.toml): a `--!native` module and an
`@native` function run as machine code, as in Roblox, which has native
code on Android and iOS too; every other chunk stays interpreted
(`luau_codegen_compile_native`, vendor mlua `state/raw.rs`).
`RUSTUDIO_NO_NATIVE=1` turns it off — on a phone `adb shell setprop
debug.rustudio.env "RUSTUDIO_NO_NATIVE=1"`, then relaunch. The phone
build keeps it, which took three things a Rust-linked Android library
doesn't get by itself. None of them shows on a desktop, and `cargo
check` of the Android target is blind to all three (it links nothing):

- **`__clear_cache`.** After writing machine code, Luau's `CodeAllocator`
  flushes the instruction cache with `__builtin___clear_cache`, which on
  AArch64 is a call into the compiler's runtime library (compiler-rt's
  builtins: `__clear_cache`). rustc links with `-nodefaultlibs`, and the
  NDK sysroot xbuild builds against has no
  `libclang_rt.builtins-aarch64-android.a`, so the reference stayed
  undefined — a shared library links anyway — and the first APK of the
  rebased port died in `NativeActivity.onCreate`: `dlopen failed: cannot
  locate symbol "__clear_cache" referenced by librustudio_android.so`.
  `CodeAllocator.cpp flushInstructionCache` now does the maintenance
  inline on Android (compiler-rt's own sequence for AArch64: `dc cvau`
  over the data cache lines, `ic ivau` over the instruction cache lines,
  the sizes from `CTR_EL0`, the barriers between).
- **One unwinder.** A Luau error is a C++ exception (`luaD_throw`; mlua
  builds Luau without `LUA_USE_LONGJMP`), and a native frame can only be
  unwound with the unwind table Luau registers for it
  (`CodeBlockUnwind.cpp`: `__register_frame`). Since NDK r23 every
  library carries its own copy of libunwind with hidden symbols,
  libc++_shared.so included (`llvm-readelf --dyn-syms` shows no
  `_Unwind_*` there), so with the shared libc++ an exception thrown by
  its `__cxa_throw` is unwound by *its* copy, which has never heard of
  the tables Luau handed to the copy inside librustudio_android.so: the
  first `error()` under a `--!native` function would have ended in
  `std::terminate`. The Android targets therefore link libc++ statically
  (`.cargo/config.toml` `[env]`: `CXXSTDLIB_aarch64_linux_android =
  "c++_static"`, which the vendored luau0-src completes with libc++abi):
  thrower, unwinder (the libunwind.a Rust's std links in) and Luau's
  registration are one library's — the arrangement Luau itself runs
  under on macOS, where libc++ and libunwind are the system's single
  copies. The editor's own library no longer lists libc++_shared.so in
  NEEDED; xbuild still copies it into the APK for any bundled library
  that does (fetch-turnip.sh's check lets the name through for that).
- **A strong `__register_frame`.** Luau declares it weak so a platform
  without one runs without native code, and a weak reference pulls
  nothing out of a static archive — native code would silently stay off
  if nothing else dragged that member of libunwind.a in. The Android
  build defines `LUAU_ENABLE_REGISTER_FRAME` (luau0-src `lib.rs`), and
  `crates/rustudio-android/build.rs` links the library with
  `-Wl,--no-undefined`: a symbol that no NEEDED library provides is a
  build error now, not a logcat line after an install.

On a device, a `--!native` Script with a hot loop should run visibly
faster than the same code without the directive (the Script Profiler, or
`os.clock()` around it), and `pcall(function() error("x") end)` inside a
`--!native` function must return `false, "x"` — an abort there would
mean the unwinder above isn't the one that threw. Without a device:
`llvm-nm -D --undefined-only librustudio_android.so` lists what dlopen
will look up (nothing named `__clear_cache`), `llvm-readelf -d … | grep
NEEDED` the libraries it may look in (no libc++_shared.so), and `llvm-nm
librustudio_android.so | grep -E ' (__register_frame|__cxa_throw|_Unwind_RaiseException)$'`
must show all three defined (`t`/`T`) inside the library.

### Luau compiler level and fast flags: `RUSTUDIO_LUAU_OPT`, `RUSTUDIO_LUAU_FLAGS`

Studio compiles scripts at Luau optimization level 1; the desktop build
keeps that (a debugger's view of locals, upvalues and call sites matches
the source). A phone build compiles at level 2 by default — inlining
and loop unrolling, the level Roblox's own clients use — because a
big game's interpreted scripts are most of a phone's frame
(`bytecode_cache.rs optimization_level`). `RUSTUDIO_LUAU_OPT=0|1|2` sets
it anywhere (`adb shell setprop debug.rustudio.env "RUSTUDIO_LUAU_OPT=1"`,
relaunch); the bytecode disk cache keys on it (`salt`), so switching
never reuses the other level's bytecode. `RUSTUDIO_LUAU_FLAGS=
"LuauFlagA,LuauFlagB=false"` sets Luau fast flags (a name alone turns it
on) before the first VM is made, for trying a flag the vendored Luau
has (`runtime.rs luau_flags`). Neither changes what scripts see: a
level-2 chunk behaves as a level-1 one, only `debug.info` line
and name details of inlined calls differ.

(Script speed is judged on the release APK — "Install the release APK
to judge speed", above: the debug one runs the Luau VM and the engine
with assertions and overflow checks, several times slower.)

## Turnip driver (Snapdragon) and PanVK (Mali): `src/android/turnip.rs`

The arm64 APK carries Mesa's open-source Adreno Vulkan driver, **Turnip**
(`libvulkan_freedreno.so`, [nihui's Android
builds](https://github.com/nihui/mesa-turnip-android-driver), MIT), Mesa's
Mali driver **PanVK** (`libvulkan_panfrost.so`, [JimVulkan's
mali-panvk](https://github.com/JimVulkan/mali-panvk) build on the stock
Arm kbase kernel driver — no root, no custom kernel — MIT; optional, 18 MB), and
**libadrenotools** ([TouseefX's
libadrenotools-native](https://github.com/TouseefX/libadrenotools-native),
a fork of bylaws/libadrenotools for apps without Java of their own,
BSD-2-Clause; the shadowhook / bytehook libraries inside are ByteDance's,
MIT). On a Snapdragon phone the editor can swap Turnip in for the
vendor's driver at start, on a Mali phone PanVK (same path, other file:
"PanVK" below) — **off by default since 0.6.0-port: View > GPU Driver
turns it on** (`turnip::wanted`, a marker file `files/turnip/on`; the
swap was on by default for every eligible phone until some crashed at
start or in their first minutes with it, which the failed-start marker
only catches when the crash comes before the first frames; a choice of
"off" made under the old default is kept). The device's own driver spent most of the 3D view's
frame waiting (the "Frame time on a phone" measurements: ~60 ms of GPU
time for a baseplate of 12 triangles, a wait rather than drawing); Turnip
is the driver the emulators on Android reach for because its submit and
present paths don't.

**Where the files come from.** The pinned downloads (SHA-256 checked by
the script; `android.yml`'s `TURNIP_ZIP_URL` / `ADRENOTOOLS_ZIP_URL` /
`PANVK_ZIP_URL` and the manual run's `turnip_zip` / `adrenotools_zip` /
`panvk_zip` inputs name the same three):

- libadrenotools: <https://nightly.link/TouseefX/libadrenotools-native/actions/runs/44679007919/libadrenotools-arm64-v8a.zip>
  (`libadrenotools.so`, `libhook_impl.so`, `libmain_hook.so`,
  `libfile_redirect_hook.so`, `libgsl_alloc_hook.so`, `libshadowhook.so`,
  `libshadowhook_nothing.so`, `libbytehook.so`) — the fork's CI build of
  commit 8c7d9f5 (2026-11-20 05:08, workflow run 44679007919; SHA-256
  `94aacd0a59fa3b23c361ad33a4809018374bcbb0d71564d2f5afc277b114c34c`).
  Over the build before it (ca1ad33, run 44383713561) its constructor's
  per-generation Turnip defaults (`applyTurnipOptimizations`; the
  generation is the GPU model after "Adreno" in kgsl's `gpu_model`,
  three digits for a 6xx, so an SoC number such as SM8250 can't pass
  for an 8xx) give an Adreno 6xx `TU_DEBUG=noconform,noubwc,nolrz` —
  the UBWC whole-device restarts, "Turnip's own switches" below — when
  nothing set `TU_DEBUG` before it ran; rustudio does, so it switches
  UBWC off there through the switches file itself, and takes the build
  for parity. It has
  the `driver.cpp` rewrite of 4d4d482 (2026-11-17), which looks for the system's Vulkan loader
  under the vendor paths too (`/system/lib64`, `/system/vendor/lib64`,
  `/vendor/lib64`, `/odm/lib64`, `/product/lib64`, `/system_ext/lib64`:
  what HyperOS devices need), lets its own initialisation be switched
  off (`ADRENOTOOLS_DISABLE=1`, which rustudio sets before loading it;
  with it on, the shim hooks only `vkGetInstanceProcAddr` /
  `vkGetDeviceProcAddr` of the system loader and sends the system's own
  callers — hwui, WebView, anything under `/system`, `/vendor`, `/apex`
  — on to the vendor's driver), carries a working PanVK (Mali) path of
  its own (`init_panvk_driver`, which opens the same
  `libvulkan_panfrost.so` name and shares Turnip's init; rustudio opens
  the file through `adrenotools_open_libvulkan` itself, "PanVK" below),
  tells a Mali the way rustudio does (`/dev/kgsl-3d0` first, then
  `/dev/mali0`, `ro.hardware.vulkan` / `egl`, the kbase sysfs, the
  device tree) and defaults Mesa's own WSI to mailbox presentation
  (`MESA_VK_WSI_PRESENT_MODE`; the Android swapchain is the system
  loader's, so the editor's vsync is unchanged). Its other switches
  (`ADRENOTOOLS_FORCE_DRIVER=turnip|panvk`, `ADRENOTOOLS_DRIVER_DIR`,
  `ADRENOTOOLS_DRIVER_NAME`, `ADRENOTOOLS_HOOK_DIR`,
  `ADRENOTOOLS_HOOK_SYSTEM`) only matter when its own initialisation
  runs, which here it doesn't. On top of that, the fork's fixes of
  2026-11-18 (commits 95ddbd5, 3360339) for whole-device reboots with
  Turnip — a freeze, the sound stuttering, a green screen, a restart:
  the GSL free hook zeroed `gpuaddr` before `IOCTL_KGSL_GPUMEM_GET_INFO`
  and then sent `IOCTL_KGSL_GPUOBJ_FREE` a garbage id, freeing a GPU
  object still in use (a GPU fault in the kernel, a KGSL panic, a
  reboot); the `adrenotools_set_turbo(false)` its init did on every
  start, a `KGSL_PROP_PWRCTRL` ioctl that some Adreno kernels crash on,
  is gone (turbo is only on request now, `RUSTUDIO_GPU_TURBO` below);
  the GSL alloc / free hooks and the pending GPU mapping are under a
  mutex (Vulkan calls them from several threads); handle leaks and an
  unaligned `MAP_FIXED` are fixed. `libadrenotools.so` and
  `libhook_impl.so` of this build carry a Rust layer (cxx) for the
  pending-mapping bookkeeping, the Mali mailbox and the dispatch state,
  and come with Rust std's debug info — 8 MB each; the script strips
  that with `llvm-strip --strip-debug` when it finds one (PATH or the
  NDK named by `ANDROID_NDK_HOME`; the workflow's `llvm` package), to
  3.4 MB each — the symbol tables stay, so crash logs still name
  functions; `STRIP=0` keeps it. GitHub keeps workflow artifacts for 90
  days; after that, point `ADRENOTOOLS_ZIP` at a release of the fork
  (V0.7,
  <https://github.com/TouseefX/libadrenotools-native/releases/download/V0.7/libadrenotools-arm64-v8a.zip>,
  is the last one; with it HyperOS keeps the device's driver, as below,
  and its own initialisation, which has no off switch, is run by
  rustudio and not relied on). The app reads which build it has off the
  file (`turnip::Shim`: the `/vendor/lib64/libvulkan.so` and
  `ADRENOTOOLS_DISABLE` strings) and logs it: `turnip: libadrenotools in
  this APK: vendor loader paths yes, own initialisation switchable off
  (ADRENOTOOLS_DISABLE)`.
- Turnip driver: <https://github.com/nihui/mesa-turnip-android-driver/releases/download/26.3.1/mesa-turnip-android-26.3.1.zip>
  (`libvulkan_freedreno.so`, Mesa 26.3.1-lto of 2026-11-19, Adreno
  6xx/7xx/8xx; SHA-256
  `9f38529b5f5f6640c1d600a398cc5218cdf7706066e68e8d2fbc7cbde644a8c0`;
  17.6 MB, of which 1.2 MB is debug info the script strips the same
  way, to 16.5 MB)
- PanVK driver: <https://github.com/JimVulkan/mali-panvk/releases/download/v1.6.0/mali-panvk-27.0.0-devel-r10.zip>
  (release r10 of 2026-11-04, SHA-256
  `7e97db4d79d81ec9461d1e0676b62a865561cb56e21c151693aa25ffbe749f1c`:
  `libvulkan_panfrost.so`, 18 MB, with a `meta.json` and the notices —
  only the `.so` goes in. Mesa 27.0.0-devel's PanVK (git 58f2c55) built
  for Android as a Vulkan HAL module — it exports `HMI`, which is how
  the loader's copy opens it — on the **stock Arm kbase** kernel driver
  (`/dev/mali0`), the one Android's Mali devices ship: no root and no
  custom kernel, where the earlier panvk-kbase-android build needed one.
  Vulkan 1.4 on the Bifrost Mali-G31/G51/G52/G71/G72/G76, the Valhall
  G57/G68/G77/G78 and the command-stream G610/G615/G710/G715/G720/
  Immortalis-G925/Mali-G1 (1.3 on cores with fewer than 8 colour
  attachments, the G71 among them; G610/G710/G715 untested by its
  author); Android 12+). `PANVK=0` (the workflow's `panvk` input off)
  leaves it out: an 18 MB smaller APK on which Mali devices keep their
  driver.

Another build goes in with `TURNIP_ZIP=<url or file>` /
`ADRENOTOOLS_ZIP=<url or file>` / `PANVK_ZIP=<url or file, zip or bare .so>`
(a `*_SHA256` beside it is checked when given), or by dropping the `.so` files into
`crates/rustudio-android/libs/arm64-v8a/` by hand (kept unless `FORCE=1`).

**How the swap works.** `tools/android/fetch-turnip.sh` (run by the
workflow for the arm64 APK; a manual run can point `turnip_zip` /
`adrenotools_zip` at other zips or turn `turnip` off) puts the libraries
into `crates/rustudio-android/libs/arm64-v8a/`, which `manifest.yaml`
lists as xbuild `runtime_libs`, so they land in the APK's `lib/arm64-v8a`
and, since xbuild stores libraries compressed and the manifest doesn't
say `extractNativeLibs=false`, the installer unpacks them into the app's
native library folder (`ApplicationInfo.nativeLibraryDir`) as real
files. That folder is where libadrenotools must find its hook libraries
and the driver. (xbuild checks the bundled libraries' NEEDED names
against its NDK sysroot before packing; PanVK's `libhardware.so` isn't
there, so the script marks it — "xbuild quirks worth knowing" above.)
At start, in `android_main` before eframe exists — while the launch
screen is up — `turnip::activate()`:

1. Decides whether the device qualifies and which driver it gets
   (`turnip::Mesa`: PanVK when `ro.hardware.vulkan` / `ro.hardware.egl`
   say "mali" or `/dev/mali0` exists, Turnip otherwise). For Turnip —
   Adreno only, 650 and up, not an old HyperOS: arm64-v8a;
   `ro.hardware.vulkan` / `ro.hardware.egl` not
   naming another maker's GPU (a PowerVR, Xclipse, … is out at
   once: Turnip drives Adreno and PanVK Mali, nothing else); `/dev/kgsl-3d0` present
   (Qualcomm's GPU kernel driver, what Turnip's KGSL backend opens) or
   `ro.soc.manufacturer` / `ro.hardware` naming Qualcomm; an Adreno 650
   or newer (`turnip::ADRENO_MIN`) per `/sys/class/kgsl/kgsl-3d0/gpu_model`,
   or per `ro.soc.model` for the Snapdragons listed in
   `Device::adreno_model` when that node isn't readable — an unknown
   model is out, as are the 610–644 and everything older; Xiaomi's
   HyperOS (`ro.mi.os.version.name` / `.code`) only with a
   libadrenotools build that looks for the system's Vulkan loader under
   the vendor paths HyperOS keeps it at (`/vendor/lib64/libvulkan.so`,
   …): `turnip::Shim` reads that off the shim file in the APK (the path
   list is a plain string in the binary) —
   with an older build those Snapdragons keep their driver, since its
   hook found nothing under `/system/lib64` (MIUI proper isn't excluded;
   the log names both); Android 12+ (API 31, the fork's floor); the
   files in the APK. Anything else keeps the device's driver, with one
   `turnip: not used — …` line in logcat (`turnip: device: …` before it
   lists everything the decision read), and the window doesn't offer the
   switch. `RUSTUDIO_TURNIP=force` skips the GPU, model, HyperOS and
   Android-version checks for a run (the driver is still the device's
   kind: PanVK on a Mali, Turnip elsewhere). For PanVK the checks are
   the Mali ones under "PanVK" below, then the same HyperOS, Android 12
   and files-in-the-APK ones.
2. Sets `ADRENOTOOLS_DISABLE=1` (and, for PanVK,
   `PAN_I_WANT_A_BROKEN_VULKAN_DRIVER=1`) and `dlopen`s
   `libadrenotools.so` from the native library folder. The shim's
   constructor puts Mesa's environment in (`MESA_VK_WSI_PRESENT_MODE`,
   `TU_*` defaults on an Adreno, `PAN_*` on a Mali, …; `setenv` without
   overwrite, so the app's own `TU_DEBUG` stays) and, because of the
   switch, does nothing else: no copy of the loader of its own, no hooks
   on the system loader, no JavaVM wanted. `MESA_SHADER_CACHE_DIR` is set
   here, under the app's cache folder (`turnip_shader_cache` /
   `panvk_shader_cache`). With an older shim that has no such switch
   (the fork's V0.7 or 4a084a5), its `JNI_OnLoad` is called with the
   process's JavaVM right there, so its own initialisation — Mesa's
   environment, a private copy of the system's `libvulkan.so` with the
   driver behind it, its hooks on the system loader — is done when the
   call returns (a `call_once` inside makes it and the fork's fallback
   thread one); see below for why those hooks aren't relied on.
3. Opens a Turnip-backed loader of its own through the fork's exported
   `adrenotools_open_libvulkan` (`RTLD_NOW | RTLD_LOCAL`,
   `ADRENOTOOLS_DRIVER_CUSTOM`, the native library folder as hook and
   driver folder, `libvulkan_freedreno.so`): a fresh copy of
   `/system/lib64/libvulkan.so` in a linker namespace of its own, whose
   `android_load_sphal_library` is overridden to load the Turnip driver
   instead of `vulkan.adreno.so`. Its `vkGetInstanceProcAddr` is kept
   (`turnip::entry()`).
4. Probes that loader: a Vulkan instance, the first physical device's
   name and `VK_KHR_driver_properties` (`turnip: probe: Turnip Adreno
   (TM) 650 — turnip Mesa driver Mesa 26.3.0-lto (Vulkan 1.3.363)`),
   then destroys it. `turnip: ACTIVE — …` follows when it is Turnip.

Then `wgpu_setup` (src/lib.rs, through `offscreen::wgpu_options_with`) asks `turnip::wgpu_setup()` for the
renderer's objects: ash's `Entry::from_static_fn` on that
`vkGetInstanceProcAddr`, a `VkInstance` made exactly as wgpu-hal's own
`Instance::init` makes one (its `desired_extensions`, application info,
API version), `wgpu_hal::vulkan::Instance::from_raw`,
`wgpu::Instance::from_hal`, the first adapter, and a device from the
same device descriptor the ordinary start uses — handed to eframe as
`egui_wgpu::WgpuSetup::Existing`. The log line is `turnip: wgpu: Turnip
Adreno (TM) 650 — turnip Mesa driver …`; egui_wgpu's "Using wgpu
adapter" line says the same, as does View > GPU Driver ("Renderer on:
…"). wgpu itself is
unchanged; window surfaces and swapchains come from the loader copy's
own swapchain code (Mesa's Android builds leave them to the loader, as
the vendor driver does).

*Why not the fork's patch of the system loader.* Left to itself,
libadrenotools-native inline-hooks the system `libvulkan.so`'s
`vkGetInstanceProcAddr` / `vkGetDeviceProcAddr` (shadowhook in shared
mode) to answer from a private copy of the loader with the driver behind
it, so that a plain `dlopen("libvulkan.so")` — wgpu-hal's
`ash::Entry::load()` — would land on Turnip without any of the above.
Its 4a084a5 build didn't hold: the first device run's probe instance was
Turnip's, wgpu's instance made right after it on the same thread was
the Adreno driver's (`AdrenoVK-0: Driver Path : /vendor/lib64/hw/vulkan.adreno.so`
appeared between the two) — its proxies (`hooked_vkGetInstanceProcAddr`
→ `g_turnip_gipa(...)`) returned without `SHADOWHOOK_POP_STACK()`, so
the per-thread frame shadowhook pushes for each call was never popped,
and shadowhook's hub (`sh_hub_push_stack`: a frame with the same
`orig_addr` already on the thread's stack = recursive call) sent every
later call of the function on that thread to the original. The 4d4d482
rewrite scopes the frames (`SHADOWHOOK_STACK_SCOPE()`), hooks these two
functions only (hooking everything Vulkan was its earlier way and
unsafe) and routes callers from the system's own libraries and UI
threads back to the vendor's driver; but it needs the JavaVM, the
Application context and a thread of its own to settle, and whether it
is in place when wgpu loads is a race rustudio would have to wait out.
The loader handle of rustudio's own and the pre-built wgpu objects need
none of it, so the patch is switched off (`ADRENOTOOLS_DISABLE=1`) where
the shim honours that, and with an older shim run but not relied on
(its leaked frame is settled by `settle_system_loader` when the editor
falls back to the device's driver, so that wgpu's own loading sees the
vendor's loader uniformly).

**Switches.** View > GPU Driver has the checkbox (off by default; the
marker files `files/turnip/on` / `off`; takes effect at the next start).
From adb: `RUSTUDIO_TURNIP=0` keeps the device's driver, `=1` ignores the window's
choice and the failed-start marker, `=force` also skips the device checks
(for a Snapdragon the properties don't name). `RUSTUDIO_GPU_TURBO=1`
asks libadrenotools for the GPU's top clocks (`adrenotools_set_turbo`, a
KGSL property; thermal limits still apply; works with either driver) —
with care: the `KGSL_PROP_PWRCTRL` ioctl behind it crashes some Adreno
kernels (the fork stopped sending it at init for that reason: a crash
there takes the whole device down), so leave it off on a device that
has rebooted with Turnip.

*Turnip's own switches* (Mesa's `TU_DEBUG` options) are under the same
heading while Turnip is active, for when something draws wrong with it —
the emulator panels' "driver settings": **LRZ off** (`nolrz`; on by
default, below), **LRZ fast clear off** (`nolrzfc`), **Binning off**
(`nobin`), **Tiled rendering off** (`sysmem`), **Flush caches after
every draw** (`flushall`), and, at the next start, **Passes through
dynamic rendering** (`dynamic`, on by default: libadrenotools' own
setting) and **Bandwidth compression off** (`noubwc`; on by default on
an Adreno 6xx — 650, 660 —, below). They are kept in
`files/turnip/debug` (comma-separated flag names), which is handed to
the driver as `TU_DEBUG_FILE`: Mesa reads it at start-up and watches it
(inotify), so a flag of its runtime set (`tu_runtime_debug_flags` in
`src/freedreno/vulkan/tu_util.cc`: the five live ones above, also
`gmem`, `forcebin`, `syncdraw`, …) changes the running driver the moment
the box is ticked — the renderer needs no restart, and a chunk that was
missing comes back (or doesn't) on the spot. The file persists, so a fix
found this way stays; "Default" writes `dynamic,nolrz` back (on an
Adreno 6xx `dynamic,nolrz,noubwc`).

*UBWC and the whole-device restarts.* With UBWC (the GPU's bandwidth
compression of textures and render targets, Turnip's default) on,
Turnip's first frame took a Galaxy S20 FE (Snapdragon 865, Adreno 650)
down to a restart of the whole phone: a fatal bus error in the chip,
`TZBSP_ERR_FATAL_NOC_ERROR`, the finding of
[WindWakerHD-Android-Plus v1.7](https://github.com/Nycolazs/WindWakerHD-Android-Plus/releases/tag/v1.7)
with Mesa 26.0 to 26.3 ("above 1x resolution"; it turns UBWC off for
Turnip on Adreno 6xx by itself, at no measurable cost there: 18.2
against 19.5 ms of GPU time per frame). A Snapdragon 865 device here
restarted the same way with the libadrenotools build that had the
fork's reboot fixes of 2026-11-18 (the GSL free hook, the PWRCTRL
ioctl) in, so those were not the whole story. rustudio puts `noubwc`
in the switches file on an Adreno 6xx (`turnip::default_switches`:
the file's first write, and once for a file written by an earlier
build — `files/turnip/default-noubwc` beside it remembers, so unticking
it afterwards holds); Mesa reads the file's start-time flags at
`tu_env_init_once` (`start_debug |= file_flags & ~tu_runtime_debug_flags`),
so it is in force from the first frame of the next start. Adreno 7xx
and up keep compression: no such report, and it is worth bandwidth
there. libadrenotools-native 8c7d9f5's constructor defaults
`TU_DEBUG=noconform,noubwc,nolrz` on a 6xx as well, but only when
nothing set `TU_DEBUG` before it ran, which rustudio does
(`TU_DEBUG_BASE`); the switch here does not depend on it. The logcat line: `turnip: TU_DEBUG=…;
switches (…/files/turnip/debug): dynamic,nolrz,noubwc` (and, the first
time, `turnip: noubwc joins the switches on by default on this GPU
(Adreno 650)`). A 6xx that has proved it can take UBWC — a long session
at the full 3D render scale — can have it back by unticking the box.

**LRZ is off by default** (`turnip::DEFAULT_SWITCHES`). Found on the
S20 Ultra with Mesa 26.3.0: a flat terrain drew, then whole chunks of it
went missing while their neighbours stayed, each chunk one draw of the
viewport pass; ticking LRZ off brought them back on the spot, and the
baseplate runs at the same 120 fps without LRZ. The renderer's side was
checked against Turnip's own LRZ rules (`tu_lrz.cc`
`tu6_calculate_lrz_state`): every scene pipeline tests GREATER /
GREATER_OR_EQUAL (reverse Z, one direction for the whole pass), nothing
writes depth with ALWAYS or NOT_EQUAL or with blending (the sky and the
lines on top test ALWAYS without writing, which only skips LRZ for those
draws), the decal-depth draws are depth-only with `discard` (LRZ write
off for them and, as they come after colour, for the rest of the pass —
conservative, not wrong), and the terrain draws themselves are the
plainest kind: depth test GREATER, depth write, no discard, no blending,
after opaque parts drawn the same way. A draw like that can only vanish
in whole if the low-resolution depth buffer holds nearer depth than was
ever drawn — the driver's (the binning pass's position-only vertex
shader variant, LRZ fast clear and the on-GPU direction tracking of
Adreno 650 are the moving parts), not the renderer's. To narrow it on a
device: untick LRZ off, tick LRZ fast clear off — chunks staying means
`nolrzfc` alone would do, which keeps most of LRZ's early rejection. To
try LRZ with a newer Turnip build (`TURNIP_ZIP_URL`): untick it. The start-time `TU_DEBUG` is `turnip::TU_DEBUG_BASE`
(the fork's list less `dynamic`; the names Mesa doesn't know are
harmless), and `RUSTUDIO_TU_DEBUG=<flags>` replaces it for a run (the
file's flags still add to it). A flag in `TU_DEBUG` can't be turned off
at runtime (Mesa warns so in logcat). The fork's constructor sets more
of Mesa's environment than `TU_DEBUG` (its `global_atomic_init`): among
it `MESA_VK_WSI_PRESENT_MODE=mailbox`, which makes Mesa's WSI present in
mailbox mode whatever the swapchain asks for — so under Turnip
`RUSTUDIO_PRESENT_MODE` and wgpu's FIFO default don't apply, and frames
are rendered unthrottled and dropped rather than paced (the 120 fps
measured below is the panel's rate); `tu_override_uncached_as_cache_coherent=true`
(a driconf option: write-combined host-visible memory is served from the
cached-coherent type where the KGSL kernel grants IO-coherent memory, a
no-op elsewhere); `MESA_SHADER_CACHE_MAX_SIZE=4G`; and
`TU_OVERRIDE_HEAP_SIZE` at 40 % of RAM. Left as they are for now.

**When it goes wrong.** A `pending` marker is written before the swap
and removed after the editor's second frame (`turnip::mark_healthy`); a
start that dies in between leaves it, and the next start turns it into
`failed`, keeps the device's driver and says so in logcat and in the
window ("Try again" clears it; so does `RUSTUDIO_TURNIP=1`). When
libadrenotools' initialisation fails without crashing (the driver file
missing, the hook not taking: its own lines are under the logcat tag
`AdrenoToolsPatch`), `adrenotools_open_libvulkan` returns nothing or the
probe finds the Adreno driver answering through the opened loader, which
is logged as `turnip: not active — …` / `not used — …`, the marker is
removed and the start goes on with the device's driver. The same when
`wgpu_setup` fails after a good probe (`turnip: wgpu on the Turnip-backed
loader failed …`): wgpu makes its ordinary instance. OpenGL ES remains
wgpu's fallback in every case. Known limits: Turnip builds come per Mesa
release and a device's Vulkan loader must accept the driver's
`VK_ANDROID_native_buffer` version (Android 14–16 take any; nihui's
builds target API 26+); Adreno 5xx and older have no Turnip; a 16 KB
page-size device (Android 15+, some Pixels) needs libraries aligned for
it, which these prebuilt ones may not be — the loader then refuses them
and the device's driver stays.

`adb shell setprop debug.rustudio.env "RUSTUDIO_TURNIP=0"` is the quick
A/B against the vendor's driver; egui_wgpu's "Using wgpu adapter" line
and View > GPU Driver name the adapter.

*Something draws wrong* (geometry missing, flickering, a pass's result
stale) — in this order, each a minute: (1) is it the driver? logcat
(`adb logcat -s rustudio AdrenoToolsPatch mesa`) with
`RUSTUDIO_QUALITY_TRACE=1` for `render quality N -> M` lines while it
happens — Automatic quality stepping down shortens the draw distance
(500–1200 studs at levels 1–3, `quality.rs` `DRAW_DISTANCE`), and parts
and terrain chunks wholly beyond it aren't drawn on either driver;
Graphics quality Manual in the Performance settings, or
`RUSTUDIO_AUTO_QUALITY_LEVEL=21`, tells. (2) `RUSTUDIO_TURNIP=0`: the same on
the vendor's driver means the renderer, not Turnip. (3) Turnip's
switches, top down, one at a time, watching the scene (LRZ off is already
on by default; with a Turnip build that has it off, LRZ off fixing it
points at the depth pre-pass, then `nolrzfc` alone, to narrow it),
Binning off at the visibility stream, Tiled rendering off at tile loads /
stores / resolves (a `LoadOp::Load` on an attachment that was
`StoreOp::Discard`ed shows here), Flush caches at a barrier or cache
hazard. (4) `dynamic` off and a restart: wgpu-hal 30 drives classic
`VkRenderPass`es, which the fork's `dynamic` routes through Mesa's
dynamic-rendering emulation (`vk_common_CmdBeginRenderPass2`), a path
Turnip's own render-pass code doesn't take. Report the flag that fixed
it: a switch is a workaround, the renderer side (pass order, a depth
state, a buffer written while in flight) is the fix.

**Measured.** Galaxy S20 Ultra (Snapdragon 865, Adreno 650, Android 12 /
API 31, 120 Hz panel), release APK, the baseplate: the vendor's Vulkan
driver (build of 10/2021) held ~30 fps with 35–49 ms of each frame spent
in the present wait; Turnip (Mesa 26.3.0) on the same build runs it at a
steady 120 fps. The first run with the `from_raw` hand-over logged
`turnip: wgpu: Turnip Adreno (TM) 650 — turnip Mesa driver …` and no
`AdrenoVK-0` line, as expected. Second round, same device, LRZ off: the
baseplate holds 120 fps at the port's then top quality level (4× MSAA,
four shadow cascades, SSAO, bloom, depth of field, clouds); a flat
terrain at that level runs at 20 fps — the terrain shader, its shadow
cascades and the full-resolution post passes, not the driver — and in
Automatic mode
(level 3 to start) the chunks beyond the level's view distance are
expected to go (the "Something draws wrong" ladder above).

**PanVK (Mali).** The same module, shim, loader, probe, markers and
window switch with the other file: on a device whose GPU is a Mali
(`ro.hardware.vulkan` / `ro.hardware.egl` "mali", or kbase's device
node `/dev/mali0`) `turnip::Mesa::PanVk` is chosen and
`adrenotools_open_libvulkan` is given `libvulkan_panfrost.so` (the name
the fork's own `init_panvk_driver` opens). The checks before it:
`/dev/mali0` must exist (the build here is PanVK on ARM's **kbase**
kernel driver, the one Android's Mali devices have, not the mainline
panfrost DRM driver; without the node PanVK has nothing to open); the
core per kbase's sysfs `gpuinfo` (`/sys/devices/platform/<addr>.mali/gpuinfo`,
"Mali-G52 MP2 r1p0 0x7402"; `turnip::mali_gpuinfo` looks under the
usual nodes) must be a Bifrost or newer one (`Mali-G…`): the Midgard
`Mali-T…` and Utgard `Mali-4xx` have no PanVK code and keep their
driver, an unreadable `gpuinfo` lets the probe decide. With the pinned
shim (4d4d482) that is all: its own initialisation is switched off
(`ADRENOTOOLS_DISABLE=1`), its constructor sets PanVK's environment
(`PAN_I_WANT_A_BROKEN_VULKAN_DRIVER=1`, which the driver wants before it
enumerates a device — rustudio sets it too, in case the shim's Mali
test and its own differ — `PAN_MESA_DEBUG=kbase` for Mesa's GL side,
which nothing here loads, `PAN_VK_DEBUG=sync`, …), and rustudio's own
PanVK-backed loader is what wgpu gets. With an older shim that has no
off switch (V0.7, 4a084a5) one more check: the device tree's model
string (`/proc/device-tree/model`) must not say "Mali"
(`Device::shim_would_misfire`, applied only then) — that build's own
initialisation (`perform_init`, run at the shim's load whether or not
`JNI_OnLoad` is called, by its native-load fallback thread) tells a
Mali from the KGSL node's model, which a Mali device hasn't, and falls
back to that string; a Mali it recognises goes down its PanVK path,
whose hooks of the system loader call through its Turnip function
pointer, null then — a crash at the first `vkGetInstanceProcAddr` the
system loader answers; on every other Mali device it runs its Turnip
initialisation (a hooked loader copy with Turnip behind it, which finds
no Adreno: zero devices, no crash — and the one call per thread its
leaked shadowhook frame lets through to it is spent by
`settle_system_loader` when the editor falls back, as on Adreno). Then
the HyperOS, Android 12 and files-in-the-APK checks as for Turnip. No
`TU_DEBUG` is set for PanVK (the switches section of the window is
Turnip's; PanVK's own knobs are `PANVK_DEBUG` and this build's
`PANVK_KBASE_*` / `PANVK_BC_*` — BC texture formats emulated, which a
Mali hasn't), and the shader cache folder is `panvk_shader_cache`. The
probe counts PanVK as answering when
`VK_KHR_driver_properties` says `MESA_PANVK` or the names say "panvk" /
"Panfrost" (`turnip: probe: Mali-G610 (PanVK) — panvk Mesa …`); the
`pending` / `failed` markers and View > GPU Driver ("PanVK
(Mesa) Vulkan driver") work as for Turnip. No Mali device was at hand
for this: it is the untested half, gated to fail safe — a start that
doesn't reach its second frame turns it off, and the device's driver is
what the next start uses.

## Device-run notes (first APKs)

Found on devices with the phase-1 APKs; the fixes are in the table above
and here. What a report from a device should carry: `adb logcat -s
rustudio` filtered to `soft keyboard` (each height change, the focus
release), `clipboard` (each copy put on the system clipboard, each read
and its length, or "no clip"), `text menu` (open, cut, copy, paste,
select all), `render quality` (each Automatic level change, with
`RUSTUDIO_QUALITY_TRACE=1`) and `memory:` (the resident size each 256 MB
it grows, with the place being opened, followed by what the editor holds
— the DataModel's instances, the renderer's meshes, slabs, textures and
items, and the GPU allocator's account (`memory::gpu_line`, wgpu's
`generate_allocator_report`: MB allocated and reserved, and the largest
allocations by label — on a phone the GPU's memory is system memory,
Turnip's heap is host memory, so this says how much of the resident size
is the GPU's; a run of 2026-11-25 was lost to `Out of memory` at 5.3 GB
resident with 173 MB of Lua heaps and 48 MB of meshes, and nothing said
where the rest was) — and, during Play, the worker's own line every 15 s:
the two Lua heaps, the two DataModels' sizes, and how many publications
replaced the viewport's whole document instead of sending a delta — one a
frame would be the viewport rebuilding its scene every frame:
`src/android/memory.rs`, `play_worker.rs memory_log`;
`RUSTUDIO_MEMORY_LOG=1` turns the worker's line on on a desktop). **All of
these are in the session log** (`<cache>/rustudio/logs/session.log`,
`src/android/logcat.rs`: the app's lines at Info and the graphics stack's
warnings, 6 MB then rotated to `session-prev.log`; a new process rotates
too, and **three previous runs are kept** (`session-prev.log`,
`session-prev2.log`, `session-prev3.log`, `logcat::PREVIOUS`) — with one,
the run that crashed was gone once its automatic restart had itself been
left and the app started again before Help > Copy Logs: the 2026-11-25
"the session log was not saved". The file is unbuffered (a killed process
keeps every line) and synced to storage every 1.5 s and at a panic, a
lost GPU device and the loop's end (`sync_session`), for the crash that
takes the phone down with it)
plus everything written to stdout and stderr (`capture_stdio`: the
`[startup]` timings, `LOADTRACE`, Luau prints — the app's own pipe over
android-activity's logcat forwarder, the lines still reach logcat under
`RustStdoutStderr`), and the logs export (Help > Copy Logs to Download,
and the start after a crash) carries the end of all of them, oldest first — logcat's own buffer held 20 s of the app
on the Note20 whose export of 2026-11-25 had none of the `memory:` lines
that said when and into what the memory had grown. For a
crash: `adb logcat -b crash -d` (debuggerd's tombstone: the
signal, an abort's message — `memory allocation of N bytes failed` for
one out of memory — and every thread's stack) and `adb shell dumpsys
activity exit-info dev.rustudio.studio` (the reason and the memory at the
end: `reason=2 (SIGNALED) status=6` is SIGABRT, `status=11` SIGSEGV,
`reason=3 (LOW_MEMORY)` the system's killer). `crates/rustudio-core/src/crash.rs` no longer
captures a backtrace in the fatal-signal handler on Android: it writes
the note and hands the signal to debuggerd at once. The backtrace ran
under a 5 s alarm and, in a process at several GB, the alarm won — the
runs that opened a large place ended as `status=14` (SIGALRM) with no
tombstone and the signal that ended them unknown. Last run's reports are
also echoed to logcat at the next start.

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
  builds. A first run now renders the view at 75% resolution in weak-GPU
  mode (Performance settings; "Frame time on a phone"), and `x build
  --release` (or the `profiling` profile) is the build to judge speed by;
  `RUSTUDIO_PERF_RESOLUTION` overrides.
- **Clipboard.** egui-winit has no Android clipboard, and the soft
  keyboard has no Ctrl: the long-press menu on text fields
  (`android/text_menu.rs`) and the mirror in `android/clipboard.rs`.
- **Typing behind the keyboard.** The window never shrank for the soft
  keyboard; `android/keyboard.rs` (see "Text" under "Fingers").
- **The keyboard came back as soon as it was hidden** while a Properties
  value or an Explorer name was being edited: the field asked for the
  focus every frame, which egui turns into an IME hide-and-show each
  time (see "Text" under "Fingers").
- **Ten seconds of black before the first frame; 30+ ms of GPU a frame
  for an empty baseplate.** Every pipeline compiled before the first
  frame, and every pass of the desktop renderer run at full cost: the
  graphics levels and the lazy pipelines ("Frame time on a phone").
- **Browser sign-in "always fails".** Closing the account popup (a tap
  outside it, Escape, Android's Back turned into Escape) cancelled the
  waiting sign-in; it now goes on behind the closed popup, every step is
  logged (`[Info] Roblox: browser sign-in: …`, the failure as
  `[Error] Roblox: …`), and a session Roblox accepted is kept for the run
  even when the Keystore-backed store can't save it
  (`Client::sign_in_with`). On Android every Output line goes to logcat
  (`RustStdoutStderr`), not only warnings and errors: the panel is small
  on a phone and a report needs the Info lines.
- **No character in Test Play.** Run (not Play) has no character by
  design; the other reasons a spawn is skipped now say so in the Output
  (`No character: …`), and a spawn logs `Character: R15|R6 (from the …)`
  and what the body is made of (`Character body: N parts, M of them
  MeshParts`).
- **Browser sign-in never opened the code page.** Two causes: eframe's
  `links` feature (and so `ctx.open_url`) isn't in the Android feature
  set, and `keyring::Entry::new` failed on Android so the session check
  errored before the code showed. `platform::open_url` and the keyring
  shim.
- **Browser sign-in lost when the editor came back.** The code page took
  the screen, and while it was in front the system stopped the editor's
  process (a place open is 1–2 GB; `dumpsys activity exit-info` showed
  it force-stopped by the system's own app manager and by memory
  pressure) — the polling job that would have traded the confirmed code
  for a session lived in that process, and the restart came up signed
  out with nothing to go on with. Two changes. The pending code is kept
  on disk (`QuickLogin::remember`: `quick-login.json` next to
  `account.toml`, in the app's private files, readable by this user
  only, removed when the sign-in ends either way) and the next start
  takes it up before the usual session check (`AccountUi::restore` →
  `QuickLogin::pending`, within ten minutes of the start of the
  sign-in: it polls the same code, so a confirmation given while the
  editor was gone signs it in, an expired one says so, and the account
  panel opens by itself to show which; logcat: `taking up code … again
  after a restart`). And the page opens as a **Custom Tab** in the
  editor's own task (`jni::open_url`: the `EXTRA_SESSION` extra with no
  binder, no support library needed; a browser without Custom Tabs
  shows the page as usual, and no `FLAG_ACTIVITY_NEW_TASK`, which would
  put the tab in a task of its own), with the default browser's cookies —
  the roblox.com sign-in Quick Sign-in needs — and Back or the tab's close
  button landing straight on the editor instead of on the browser's
  own task. The cookie field no longer takes the focus (and brings the
  soft keyboard up over the code) while a browser sign-in is showing it.
  (What would truly keep the process alive, a foreground service, needs
  a Java service class and a `<service>` element, neither of which an
  xbuild manifest can carry.)
- **Opening a file asked for All-files access.** NativeActivity gets no
  `onActivityResult`, so the system document picker can't hand a choice
  back; the in-app picker lists the app's own folder without any
  permission and offers a button for shared storage (All files access on
  API 30+, the runtime permission below). "Open with rustudio" from a
  Files app needs nothing.
- **Panels didn't scroll, no camera on touch.** `crates/rustudio-engine/src/touch_input.rs`.
- **Start Page lists wouldn't scroll / taps misfired.** The finger layer
  ran there too, with no viewport to publish its zones, over a page that
  is plain egui; it is off until a place opens (see "Fingers").
- **No soft keyboard, Cut / Copy / Paste or system clipboard on the Start
  Page.** `RustudioApp::frame` returned for the Start Page before the
  end-of-frame text handling that the editor's and a full-screen game's
  frames reach (`keyboard.end_frame`, the bar, the long-press menu,
  `clipboard::after_frame`), so its search box, a new place's name and
  the sign-in fields took no typing and copied nowhere. Every way out
  of the frame now passes `text_end_frame`.
- **Lag on a Baseplate with a release APK.** The chrome redrew the 3D
  view every frame it drew itself; see "Frame time on a phone".
- **The 3D view lags while the camera moves, the chrome less so.** The
  first round's measurements showed ~31 ms of wait a frame with the
  camera still, so the frame itself, not the 3D pass, was the floor: two
  swapchain images under Android's compositor (see "Frame time on a
  phone", three images now).
- **~30 fps on the baseplate whatever the settings (Snapdragon 865).**
  The vendor's Vulkan driver, not the renderer: 35–49 ms of each frame in
  the present wait. The Turnip driver ("Turnip driver (Snapdragon)"
  above) runs the same build at 120 fps.
- **Terrain chunks vanishing under Turnip.** A flat terrain drew, then
  whole chunks went missing while their neighbours stayed. Turnip's LRZ
  (its low-resolution depth pre-pass) culling draws it shouldn't;
  "LRZ off" in View > GPU Driver > Turnip switches brought them back on
  the spot and is the default now (the "LRZ is off by default" paragraph
  above has the renderer-side check). Not to be confused with Automatic
  quality stepping down and shortening the draw distance, which also
  drops far chunks — on either driver, logged as `render quality N -> M`
  with `RUSTUDIO_QUALITY_TRACE=1`.

- **Dead taps on GUI buttons, the keyboard flapping.** Not the Luau VM:
  the lift's `PointerGone` in the same frame as the release (see
  "Fingers"), and egui-winit's IME toggle (see "Text").
- **Killed by the system (`reason=3 LOW_MEMORY`) with a big place open;
  `memory allocation of N bytes failed`.** The resident size grew with
  each place opened and never came back: a glibc-only `malloc_trim` in
  `heap::trim`. On bionic it is `mallopt(M_PURGE)` now, run at the same
  moments (a place closed, the materials rebuilt) and at two new ones:
  the activity's `onLowMemory` (hooked in `lifecycle.rs`; `memory.rs`
  warns in the Output with the resident size and what the device has
  left) and the save when the app goes to the background. The first
  `memory:` line of a run carries the device's totals
  (`ActivityManager.MemoryInfo`: total, available, the low-memory
  threshold) so a report tells a 4 GB phone from an 8 GB one.
- **Nothing in a game answered a finger (its own thumbstick, jump button
  and touch buttons did nothing).** The game saw a mouse and keyboard
  (see "Fingers in Play": fingers reach the game as touch now).
- **"Crashes" that `dumpsys activity exit-info` lists as `reason=1
  (EXIT_SELF) status=0` in the foreground (`importance=100`), several
  in a few minutes, after a place was opened from a Files app.** Two
  things, both of which made a process end by itself with a clean
  status:
  1. *A second activity.* The editor's activity had xbuild's default
     `launchMode` (`singleTop`); a VIEW intent from a file manager while
     rustudio ran (its task in the background, or the intent without
     `FLAG_ACTIVITY_NEW_TASK`) made Android create a second instance of
     the activity in the same process. `NativeActivity` ran the
     library's entry point for it, the glue started a second
     `android_main`, and `eframe::run_native` failed there (one winit
     event loop per process), after which `android::run` left the whole
     process — editor, place and all. `src/android/intent.rs` now: an
     activity after the first never reaches the glue; its file is
     copied in and queued for the editor's next frame
     (`storage::take_opened`, `PendingAction::Open`, the same path as
     macOS's Finder hand-over), the editor's task is moved to the front
     (`ActivityManager.moveTaskToFront`, REORDER_TASKS in
     manifest.yaml) and the activity finishes inside its `onCreate`,
     unseen. `launchMode` is `standard` on purpose: a new instance per
     intent is the one way a VIEW intent reaches native code without a
     Java class (`onNewIntent` isn't passed on, and `singleTop` /
     `singleTask` would swallow the intent in it). `android::run` also
     refuses to run twice (returns, which finishes that activity only).
  2. *The same place at every launch.* The activity's VIEW intent is
     delivered again whenever its task comes back after the process
     died (a tap on the icon brings the task back; Android recreates
     the activity with its original intent), so a place that brought
     the app down came back at once, and the Imported folder got a
     "name (2)", "name (3)" copy each time. `storage::launch_file`
     writes `state/opening` (the file) before an automatic open and
     `storage::settle` removes it 20 s later; at a start that finds the
     marker naming the same file, the file is left alone and Output
     says so (File ▸ Open Recent opens it on purpose). Imports are
     compared byte for byte with the copies already there and reuse
     them.
  A Rust panic on the editor's thread read the same way: android-
  activity's glue catches a panic out of `android_main`, logs it
  (logcat, not under the rustudio tag) and finishes the Activity
  gracefully, and the destroy hook then left the process with status 0.
  `android::run` catches the panic itself now: the panic message and
  the crash report's path go to logcat under `rustudio` at error level,
  and the process leaves with status 101 (an eframe error: status 2).
  The report (`crash::install`'s hook: message and backtrace) is echoed
  to Output and logcat at the next start. So, after any exit the
  exit-info calls `EXIT_SELF`: relaunch, then `adb logcat -d -s rustudio
  RustStdoutStderr RustPanic | grep -B2 -A40 -iE "panic|second
  activity|leaving the process"`.
- **Status 101 a few minutes into a large place** (S20 Ultra, 11-17,
  four times at ~500 MB pss, no memory pressure). Two things:
  1. *Material textures.* The renderer keeps every material's three
     maps in texture arrays (`render.rs MaterialArrays`: albedo,
     surface, emission; `capacity` layers each) and grows them when a
     place brings more materials than fit — the new arrays are made,
     the old layers copied, the pending uploads written. Under the
     phone's texture budget (`max_sampled_textures_per_shader_stage` 24
     keeps the arrays few and the layer count small) a place with many
     textured materials crossed the growth point while uploads for the
     new layers were already queued: a write at a layer the array
     didn't have yet, which is a wgpu validation error. Fixed: the
     growth is made before a layer beyond the capacity is handed out,
     the copy and the pending uploads are clamped to the capacity of
     both arrays, and an upload at a layer the array hasn't drops with
     a line in the log instead of reaching wgpu. A test grows the
     arrays through their bounds on the host's GPU
     (`material_arrays_grow_within_bounds_on_this_gpu`).
  2. *wgpu's answer to such an error.* A validation error nobody
     catches is a panic on the calling thread — the editor loop's, so
     exit 101 for a frame that could have gone on without the one
     write. On Android the device now has an uncaptured-error handler
     (`android::install_gpu_error_handler`): the error's text goes to
     logcat under `rustudio` (the first 20 in full, a count after that),
     and the frame goes on. Desktop keeps wgpu's default (the panic names the
     bug while developing).
- **Status 101 again (S20 Ultra, 11-18 06:07, pss 1.7 GB, LBG).** With
  the GPU error handler in, a 101 is a Rust panic proper (an index,
  an unwrap, a slice) on the editor's thread; which one, only its
  report says, and none came with that run. The report is now written
  where a PC can read it — `Android/data/dev.rustudio.studio/files/
  crash/<timestamp>-<pid>.txt` (`crash::dir`), not the private cache
  folder — and the next start names it in Output and logcat
  (`rustudio crashed last time: panic in thread 'main' at
  src/x.rs:123 — message`). To report one: `adb logcat -d -s rustudio
  RustPanic RustStdoutStderr` right after (the panic line and the
  report's path at error level), or relaunch and copy the Output line,
  or pull the file over USB.
- **The LBG place on a phone: PlayerModule, menu, wrapped lists.** Three
  of the first report's four points weren't bugs of the port or had an
  upstream gap behind them: (1) `PlayerModule` sits where Roblox's
  client puts it — `Player.PlayerScripts.PlayerModule` (next to
  `PlayerScriptsLoader` and `RbxCharacterSounds`) for a place whose
  `Workspace.PlayerScriptsUseInputActionSystem` is Default or Disabled,
  every place without the opt-in while Roblox's rollout is in phase 1,
  and `StarterPlayer.PlayerModule` only with the flag Enabled (LBG,
  Default, with its own fork of `PlayerModule` in StarterPlayerScripts:
  the fork kept, the engine's `PlayerScriptsLoader` added;
  `crates/rustudio-core/src/default_scripts.rs`, docs/ENGINE-GAPS.md "A new place got the
  Input Action System PlayerScripts"); (2) no menu or chat button over the game was upstream's state
  on every platform (Esc and `/` only) — the top bar above; (3) the
  shop rows and player list running off their frames were
  `UIListLayout.Wraps` (90 of the place's 250 lists), ignored upstream —
  `crates/rustudio-engine/src/gui_walk.rs list_lines`; the glows behind its frames are
  `UIShadow` (138), now drawn (`crates/rustudio-engine/src/gui_fx.rs shadow`).
- **Dead at start after the rebase onto v0.3.3-alpha.2 (11-20):
  `java.lang.UnsatisfiedLinkError: … dlopen failed: cannot locate symbol
  "__clear_cache" referenced by librustudio_android.so` in
  `NativeActivity.onCreate`, twice (the relaunch too).** Upstream's new
  Luau code generator (`luau-jit`): its instruction-cache flush is a
  call into compiler-rt's builtins on AArch64, which a Rust link has no
  copy of; the desktop builds and the arm64 `cargo check` of CI can't
  see an undefined C symbol. Fixed in `vendor/luau0-src` and locked out
  by `-Wl,--no-undefined` — "Luau native code" above, with the two
  further things native code needed in this library.
- **The first build that started (11-21): no thumbstick or camera in
  Play, TopbarPlus's icons over the Roblox button a fifth of the view
  down, a slower start, and a "crash" on a baseplate with nothing in
  the log.** Four findings. (1) 0.3.3's gameplay worker (`play_worker.rs`)
  took `step_play` off the main thread, and its `Input` hand-off didn't
  know the port's fingers: none reached `__rs_touch_event`, and the
  presentation, which nothing drained any more, fed every finger it had
  ever seen to the gesture recognizer again each frame — a list and a
  frame time growing with every touch event, the likely "crash" (a
  freeze the system ends leaves no rustudio log; the exit-info says
  `LOW_MEMORY` or an ANR). "Fingers in Play" step 4 has the fix and its
  test. (2) `ScreenInsets.TopbarSafeInsets` was laid out as
  CoreUISafeInsets; it is the top bar's free area now (`mech::
  gui_topbar_left`, the row in the table above), and `TopbarInset` read
  before the GUI module's first step answered for a chat button that was
  on (`G.core_gui_on`). (3) Two viewport pipeline sets before the first
  frame — "Frame time on a phone", with the `[startup]` lines that now
  time a start. (4) The player-scripts policy was checked against the
  DevForum (`crates/rustudio-core/src/default_scripts.rs`): a fork of
  the old character `Sound` script now keeps Roblox's
  `RbxCharacterSounds` out, the rest already matched.
- **The second report on that build (11-21): the baseplate "crash" gone,
  LBG plays, its moves run, shift lock and VFX work, TopbarPlus sits
  right; left were a slow load, a finger over a list that turned the
  camera instead of scrolling, and three script errors Roblox doesn't
  give.** (1) `Run Service is not a valid member of DataModel "Game"`:
  Roblox's RunService instance is *named* "Run Service", and LBG's
  character modules (`CharacterStorage.*.M1` / `FrontDash`, 15 of them)
  read `game["Run Service"]`; the module failed to load, so
  `Controls:880` indexed nil with `FrontDash`. rustudio names services
  by class (`doc.rs default_instance_name`, the table rustudio-net
  shares), so the DataModel's child lookup takes the Roblox name as an
  alias (`mech::service_class_for_name`, `runtime.rs find_named_child`:
  `game["Run Service"]`, `FindFirstChild`, `WaitForChild`). (2) `Unable
  to assign property CustomPhysicalProperties. PhysicalProperties
  expected, got nil`: `part.CustomPhysicalProperties = nil` is how a
  script goes back to the material's physics on Roblox; `lua_to_variant`
  takes nil as `PhysicalProperties::Default` now (the property reads nil
  again). Both are in the runtime test
  `run_service_by_its_roblox_name_and_physical_properties_reset`. (3)
  `Savage Fangs:419: attempt to index nil with 'GetAttribute'` is in a
  script newer than the place on file (the lines don't match); the
  candidate is `Player:GetAttribute('SelectedCharacter')` in the hit's
  `task.spawn`, where `Player` is nil after the move's own "dummy
  check" (`Player:IsA('Model')`) — open until the script or the place
  is at hand. (4) ScrollingFrames had no touch scrolling at all (only
  the wheel): "Fingers in Play" step 2 has it now, with Roblox's
  `Active` rule for the camera. (5) The load: no log was sent; the
  `[startup] Play started …` line and the compile summary ("Frame time
  on a phone", "Play's start") exist for the next one, and the bytecode
  disk cache covers every script over 16 KB on Android.

## Later steps (in order)

1. ~~Touch for the game~~ ("Fingers in Play"). Left: GuiObject's own
   `TouchLongPress / Pan / Pinch / Rotate / Swipe`, `TouchDrag`.
2. ~~Device properties and sensors~~ (`device_prop()`,
   `crates/rustudio-engine/src/device_sensors.rs`).
3. Haptics (`HapticService`, `HapticEffect`) through `jni::vibrate*`.
4. ~~Soft keyboard for a game's TextBoxes~~ (`OnScreenKeyboardVisible /
   Position / Size`, `ReturnPressedFromOnScreenKeyboard`, the view
   sliding up to the box: "Text: the soft keyboard"). Left: the device
   insets (`ScreenInsets` from the system bars and cutout; `TopbarInset`
   is done), orientation; a selection
   in a game's TextBox (the text bar's Copy takes the whole text, its
   Cut and Select all are for egui fields).
5. ~~`ContextActionService` touch buttons (`createTouchButton`) without
   the CoreScripts~~ (`domains/context_action_touch.luau`, Roblox's
   ContextActionTouch rebuilt; `ContextActionService:CallFunction` for
   both; the "Audit: Roblox services on a device" row).
6. ~~The display cutout: the window under the camera hole
   (`layoutInDisplayCutoutMode` shortEdges), the chrome padded by the
   cutout inset~~ (`src/android/cutout.rs`; the "What is where" row).
   Left: the 3D view under the hole with only the GUIs inset, which
   wants `ScreenInsets.DeviceSafeInsets` in the engine first.

Each step closes rows in docs/ENGINE-GAPS.md; the Creator Hub reference
pages are the source of truth for every member name, argument and enum.
