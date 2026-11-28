# Developing rustudio

rustudio builds for Linux, Windows, macOS and Android from one workspace.
Everything below is about the desktop binary unless it says otherwise; the
Android app (the same `rustudio` library inside a `NativeActivity`, built
into an APK with xbuild, driven by touch, tested on a device over `adb`)
has its own guide: [ANDROID.md](ANDROID.md) — building, what differs on a
phone (`src/android/`, `crates/rustudio-android/`), the touch layer, the
soft keyboard, graphics on a phone's GPU, and the device-run notes.

## Command-line options

Mostly for automated testing and benchmarking.

```sh
rustudio --screenshot out.png    # draw ~120 frames, save the window, exit
rustudio --self-test out.json    # drive the editor with scripted input, exit
rustudio place.rbxl --bench out.json [--play] [--seconds 10]   # frame times (avg, 1% low, slowest frames)
rustudio --stats place.rbxl      # what's in a place (counts by class)
rustudio --camera X,Y,Z,YAW,PITCH ...   # start the camera somewhere (degrees)
# Play a place and look at it like Studio's 863×675 view: click at 40 s, run
# Luau on the client at 45 s, screenshot (plus FILE.view.png) and Output at 50 s
rustudio place.rbxl --play --window 1148x1072 --click 110,143@40 \
  --luau client probe.luau@45 --screenshot shot.png --shot-after 50 --log out.txt
# Several shots timed from the start of Play: shot-2s.png, shot-10s.png, ...
# (each with .view.png, the viewport alone) and shot.frames.json (frame times)
rustudio place.rbxl --play --from-play --shots 2,5,10,20,40 --screenshot shot.png
# A fractional time keeps its decimals (--shots 2.5 -> shot-2.5s.png, 6 or 6.0
# -> shot-6s.png); every file written is printed as "[shot] /abs/path.png" on
# stdout and at the end of the --log file.
# Click a TextBox, type into it, press Enter; turn the wheel at a view position
# (over a ScrollingFrame) instead of the centre:
rustudio place.rbxl --play --from-play --shots 5 --screenshot t.png \
  --click 120,95@2 --type "hello"@2.5 --key Enter@3 --wheel -3@4:400,250
# The Device Simulator on one of Studio's devices (ids in
# crates/rustudio-core/src/devices.rs), upright; with a device, --click and
# --ldrag positions are the device screen's points; --action switches it
# mid-run (device:ID, device:none, device_rotate, device_simulator,
# device_form:mobile|desktop|console|headset, device_scaling:fit|actual|physical,
# device_popup:devices|scaling|lan|quality|touch, device_calibrate, device_manage,
# device_back, device_controller: the virtual controller, keys as a gamepad,
# device_pad_layout:generic|xbox); on a phone or tablet the left button is a finger, and
# --ldrag alt+X1,Y1>X2,Y2@S / alt+shift+… pinches / pans with two fingers:
rustudio place.rbxl --play-only --window 1914x718 --device iphone_14 --portrait \
  --action device:xbox@8 --screenshot d.png --shot-after 10
rustudio --write-corpus tools/fidelity/corpus   # regenerate the demo / control places
cargo test --workspace           # unit tests (undo/redo, files, Luau runtime, physics, meshes, FBX, GUI, PKCE)
```
A `--luau` chunk runs at the command bar's identity, so (as in Studio) its
`require` gets separate copies of the place's ModuleScripts: a probe that reads
a game module's state (a target cache, a stats table) has to create a `Script`
or `LocalScript` with that code as `Source` and parent it (instance signals and
remotes are shared either way).

`RUSTUDIO_GUI_DEBUG=1` logs GUI clicks and remote calls to Output;
`RUSTUDIO_NO_GUI=1` hides every GUI (to tell GUI from 3D in a picture).

The `--bench` report (round 20) has, besides avg / 1%-low fps and the
section timers per frame (`play_tick`, `p6b_lua`, `gui_walk`, `gui_mesh`,
`ray_parts`, … and `between_frames`, the time outside the frame's own work),
`lua`: the slowest Lua by connection ("Chunk:line Event"), by resumed
thread ("(resume) Chunk:line" where it waits) and by script start, and
`api`: the slowest engine members called from Lua ("call
FindPartOnRayWithIgnoreList", "index Position") with their call counts.
`RUSTUDIO_HITCH=1` prints every Lua resume over 10 ms (with the engine
calls inside one over 300 ms) and every engine call over 5 ms, to find
hitches. `RUSTUDIO_MEMORY_LOG=1` has the Play worker log, every 15 s, its
two Lua heaps, the two DataModels' sizes and how many of its publications
replaced the viewport's whole document rather than sending a delta (on by
default on Android: `docs/ANDROID.md`), to place a memory growth.

On macOS there is no gamescope: `--screenshot`, `--self-test` and `--bench`
open a normal window (keep it in front; macOS slows down hidden windows).
On Linux, `--screenshot`, `--self-test` and `--bench` run inside a headless gamescope,
so they never open a window on your screen (`RUSTUDIO_VISIBLE_TESTS=1` shows
them). gamescope's own lines (and its Xwayland's and Vulkan WSI layer's) stay
out of the console: rustudio's stdout / stderr come through, gamescope's are
shown only when the run fails. The self-test's last step opens your place
(set `RUSTUDIO_PERF_PLACE` to a local place file) read-only,
plays it for 5 s and records the average and 1%-low frame times.

## Which crate do I edit?

| Package | Edit here for |
|---|---|
| `rustudio` (root) | Studio UI, editing tools, plugins, and project workflows |
| `rustudio-core` | Documents, undo, file I/O, math, assets, meshes, and terrain data |
| `rustudio-physics` | Rigid bodies, collisions, queries, and humanoid physics |
| `rustudio-render` | Rendering, shaders, lighting, skinning, and world-space GUIs |
| `rustudio-engine` | Luau APIs, simulation, replication, GUIs, avatar, and audio |
| `rustudio-player` | Play and Run, game input, the game menu, and the standalone window |
| `rustudio-net` | Team Create connections and live synchronization |
| `rustudio-account` | Sign-in, avatars, asset access, and publishing |

Build the standalone player with `cargo build -p rustudio-player`.
Use `cargo check --workspace --all-targets` and `cargo test --workspace` to check the full workspace.

## Source paths

| Path | What it does |
|---|---|
| `crates/rustudio-core/src/doc.rs` | the document: DOM, undo/redo, copy/paste, group, file I/O |
| `crates/rustudio-render/src/render.rs`, `crates/rustudio-render/src/shader.wgsl` | wgpu renderer (offscreen MSAA texture shown in egui) |
| `crates/rustudio-render/src/scene.rs` | turns parts and Lighting into draw data |
| `src/viewport.rs` | camera input, picking, selection boxes, gizmos |
| `src/explorer.rs`, `src/properties.rs` | the Explorer and Properties panels |
| `src/app.rs` | menus, toolbar, shortcuts, docking, screenshot harness |
| `src/account_ui.rs` | the account window (background threads) |
| `src/selftest.rs` | `--self-test` scripted input driver |
| `crates/rustudio-engine/src/runtime.rs`, `crates/rustudio-engine/src/ltypes.rs` | Luau runtime: Instance API, signals, scheduler; Roblox datatypes |
| `crates/rustudio-player/src/play.rs`, `crates/rustudio-physics/src/physics.rs` | Play / Run / Stop, character, rapier3d physics; `physics::move_humanoid` is Studio's Humanoid controller (floor rays, hip spring, body box; docs/MECHANICS.md "Round 22") |
| `src/script_editor.rs` | script tabs: highlighting, line numbers, syntax check |
| `crates/rustudio-engine/src/replicate.rs` | server → client copy and per-frame replication |
| `crates/rustudio-engine/src/rig.rs`, `crates/rustudio-engine/src/avatar.rs` | R15 / R6 rigs, posing from Motor6D.Transform, states; avatar loading and dressing |
| `crates/rustudio-core/src/default_scripts.rs` | Roblox's default player / character scripts, read from the Studio install at Play |
| `crates/rustudio-core/src/mesh.rs`, `crates/rustudio-core/src/texture.rs`, `crates/rustudio-core/src/assets.rs` | mesh parsing (Roblox mesh 1–7, OBJ), PNG/JPEG/DDS, asset download and cache, clothing |
| `crates/rustudio-engine/src/runtime_ext.rs`, `crates/rustudio-engine/src/library.luau` | engine members beyond the core API (ClickDetector, Sound, Mouse, tags, raycasts…) and services written in Luau (TweenService, Debris, DataStores…) |
| `crates/rustudio-player/src/play_game.rs` | in-game interaction in Play: clicks, chat, tools and hotbar, explosions, sounds, NPC MoveTo |
| `crates/rustudio-core/src/terrain.rs`, `crates/rustudio-engine/src/audio.rs` | voxel terrain (SmoothGrid) decoding and meshing; sound playback (rodio) |
| `crates/rustudio-core/src/bench.rs` | frame-time measurement and the section timers |
| `crates/rustudio-engine/src/gui.rs`, `crates/rustudio-engine/src/gui_walk.rs`, `crates/rustudio-engine/src/gui_input.rs` | GUIs: fonts, images, rich text; layout and drawing of one GUI; placing GUIs on screen / surfaces / billboards and the mouse and keyboard |
| `crates/rustudio-core/src/fbx.rs` | binary FBX reader and the Roblox-style import for `LoadAsset` |
| `crates/rustudio-core/src/devices.rs`, `crates/rustudio-player/src/device_view.rs`, `crates/rustudio-player/src/touch.rs`, `crates/rustudio-player/src/virtual_pad.rs`, `src/device_sim/` | the Device Simulator: Studio's devices and their input, where the device screen sits in the view, the mouse as a finger, the virtual controller's key map, the editor's simulator state and panels |
| `crates/rustudio-account` | the Roblox session (cookie, keyring, CSRF), profile and avatar, experiences, downloads, publishing |
| `src/lib.rs`, `src/main.rs` | the studio crate is a library with two front ends: the desktop binary (`run`) and the Android activity (`android::run`) |
| `src/android/`, `crates/rustudio-android`, `crates/rustudio-core/src/android_dirs.rs` | the Android app: logcat, APK assets, JNI (Vibrator, orientation, insets, connectivity, content URIs, clipboard, browser, storage permission), sensors, the file picker, the system clipboard, the activity's end and the Back button (`lifecycle.rs`); the cdylib `NativeActivity` loads and its `manifest.yaml`; the app's folders, which core's `paths.rs` reads — see docs/ANDROID.md |
| `crates/rustudio-engine/src/touch_input.rs`, `crates/rustudio-player/src/touch_gestures.rs`, `crates/rustudio-engine/src/device_sensors.rs` | fingers → scroll / tap / hold-to-drag / camera look, the Play movement stick and jump button; the game's touch gestures; the motion sensors behind UserInputService (platform-neutral; the tests run on the desktop) |
| `src/graphics_settings.rs` | View > GPU Driver: the adapter, backend and driver the 3D view runs on, weak-GPU mode, and on Android the Mesa (Turnip / PanVK) driver switch |

