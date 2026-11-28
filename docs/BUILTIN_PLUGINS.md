# RusTudio Built-in Plugins Roadmap & Inventory

Status of RusTudio's modular built-in plugin suite (**Luau / Vide** in [`plugins/`](../plugins/) and **Native Rust** in [`src/plugin_host/builtin_suite.rs`](../src/plugin_host/builtin_suite.rs)) compared against Roblox Studio's built-in plugins cataloged in [`src/plugin_host/builtin_lazy.rs`](../src/plugin_host/builtin_lazy.rs).

---

## Implemented Built-in Suite (27)

### Luau / Vide (`plugins/`) — 17
- [x] **9-Slice Editor** (`plugins/9SliceEditor`) — Interactive `SliceCenter` visual editor for `ImageLabel` / `ImageButton` with checkerboard preview, 4 draggable Left/Right/Top/Bottom guide lines, pixel offset inputs, presets, Revert, `GuiService.Open9SliceEditor`, and Properties `SliceCenter` `…` trigger.
- [x] **Alignment Tool** (`plugins/AlignmentTool`) — Min/Center/Max alignment across X/Y/Z in World/Local space, Selection Bounds or Active Object.
- [x] **Audio Discovery** (`plugins/AudioDiscovery`) — Place-wide `Sound` / `AudioPlayer` scanner, filter, instance/path table, and live `SoundService` preview.
- [x] **Collision Groups** (`plugins/CollisionGroups`) — `PhysicsService` collision group list + 2D matrix table, inline rename/add/delete, and selection assignment.
- [x] **Event Emulator** (`plugins/EventEmulator`) — 1:1 RE of Studio's `InputPane` (`Current Event` selector for `Theme`, `RBXEvent`, `MemStorage`, `NameBox`, `Save`/`Send` buttons) + `RepopulatableHistory` clickable history list.
- [x] **Find In Place** (`plugins/FindInPlace`) — Multi-script search & Replace All with Case Sensitive (`Aa`), Whole Word (`Ab`), and Pattern/Regex (`.*`).
- [x] **Force Visualization Control** (`plugins/ForceVisualizationControl`) — 1:1 RE of Studio's draggable 220px `CoreGui` viewport overlay (`Constraints`, `Contacts`, `Gravity`, `Fluid forces`, `Net force`, `Scaling`, `Smoothing`) with exact RGB vector legend swatches and `PhysicsSettings` bindings.
- [x] **Localization Tools** (`plugins/LocalizationTools`) — Automatic Text Capture toggle, Cloud status, and embedded `LocalizationTable` management.
- [x] **Material Manager** (`plugins/MaterialManager`) — Category browser, Grid & List views with single-`ViewportFrame` PBR sphere snapshots, `Use2022Materials`, and `MaterialVariant` creation.
- [x] **Object Browser** (`plugins/ObjectBrowser`) — 3-pane Roblox class + live Enum browser with `StudioService:GetClassIcon` icons, members, inheritance, and summaries.
- [x] **Path2D Editor** (`plugins/PathEditor`) — Visual Bézier `Path2D` editor with Studio's 4 `PluginAction`s (`MoveTool`, `AddPointMode`, `AddTangentMode`, `DoneEditing`), top-center `PathToolbar` HUD, draggable control points & tangent handles, presets, and live length readout.
- [x] **Pivot Editor** (`plugins/PivotEditor`) — Floating Offset/Orientation editor, snap presets, and one-click Reset Pivot.
- [x] **Player Emulator** (`plugins/PlayerEmulator`) — `PlayerEmulatorService` emulation toggle, Country/Region, Locale, Pseudolocalization + Text Elongation, and Custom `PolicyService` flags.
- [x] **Style Editor** (`plugins/StyleEditor`) — Onboarding page + 2-pane `StyleSheet` / `StyleRule` / `StyleLink` editor.
- [x] **Tag Editor** (`plugins/TagEditor`) — `CollectionService` tag list, color swatches, group prefixes, and live `SelectionBox` World View adornments.
- [x] **Texture Viewer** (`plugins/TextureViewer`) — Place-wide texture asset auditor scanning all 25 Studio texture properties, with live previews, search/selection filter, usage counts, Select Instances, and undoable bulk Replace Asset ID.
- [x] **Wind Control** (`plugins/WindControl`) — Translucent viewport HUD with live 3D `windhose.rbxm` windsock, compass drag, speed/yaw/pitch controls, and `Workspace.GlobalWind` sync.

### Native Rust (`src/plugin_host/builtin_suite.rs` & `src/device_sim/`) — 10
- [x] **Animation Editor** (`rustudio_AnimationEditor`)
- [x] **Command Bar** (`rustudio_CommandBar`)
- [x] **Device Simulator & Controller Emulator** (`src/device_sim/`, contributed by Str8Anonymous in PR #52) — Studio 0.748 43-device presets, safe-area insets, frames, touch/pinch/rotate/swipe emulation, and virtual Gamepad1 controller.
- [x] **Toolbox** (`rustudio_Toolbox`)
- [x] **Terrain Editor** (`rustudio_TerrainEditor`)
- [x] **Asset Manager** (`rustudio_AssetManager`)
- [x] **Character / Rig Builder** (`rustudio_RigBuilder`)
- [x] **View Selector** (`rustudio_ViewSelector`)
- [x] **Material Picker** (`rustudio_MaterialPicker`)
- [x] **Viewport Draggers** (`rustudio_Draggers`)

*(Also implemented natively inside the editor shell: Experience/Game Settings, Publish to Roblox, Collaborate, Constraint insertion, and 3D Asset Import.)*

---

## Remaining Secondary / Specialized Studio Built-ins

### 1. UI & 2D *(Completed)*
- [x] **9-Slice Editor** (`9SliceEditor`) — Interactive `SliceCenter` visual editor for `ImageLabel` / `ImageButton` with draggable Left/Right/Top/Bottom guide lines, pixel offset inputs, Revert, and `GuiService.Open9SliceEditor` + Properties `...` trigger.
- [x] **Path2D Editor** (`PathEditor`) — Visual Bézier `Path2D` editor with Studio's 4 `PluginAction`s (`MoveTool`, `AddPointMode`, `AddTangentMode`, `DoneEditing`), top-center `PathToolbar` HUD, draggable control points & tangent handles, presets, and live length/bounds readout.
- [x] **Texture Viewer** (`TextureViewer`) — Place-wide texture asset auditor scanning all 25 Studio texture properties, with live previews, search/selection filter, usage counts, Select Instances, Copy ID, and bulk Replace Asset ID.

### 2. Testing & Emulation *(Completed)*
- [x] **Device Simulator & Controller Emulator** (`src/device_sim/`, PR #52 by `Str8Anonymous`) — Studio 0.748 43-device presets, safe-area insets, frames, touch/pinch/rotate/swipe emulation, and virtual Gamepad1 Controller Emulator.
- [x] **Player Emulator** (`plugins/PlayerEmulator`) — 1:1 RE of Studio's flat `MainView` (`Test in Studio`, `Language`, `Pseudolocalization` + `Text Elongation (%)`, `Country/Region`, `Custom Policies`, and expandable `AllowedExternalLinkReferences`) bound to `PlayerEmulatorService`.
- [x] **Event Emulator** (`plugins/EventEmulator`) — 1:1 RE of Studio's `InputPane` (`Theme` / `RBXEvent` / `MemStorage` views, `NameBox`, `Save` / `Send`) + `RepopulatableHistory` clickable reload list.
- [x] **Force Visualization Control** (`plugins/ForceVisualizationControl`) — 1:1 RE of Studio's compact `CoreGui` viewport overlay with RGB vector legend swatches, scale sliders, and smoothing controls bound to `settings().Physics`.

### 3. Avatar, Lighting & World
- [ ] **Light Guides** (`LightGuides`) — Selection-triggered 3D viewport range/cone/surface gizmos for `PointLight`, `SpotLight`, and `SurfaceLight`.
- [ ] **Layered Clothing Editor** (`LayeredClothingEditor`) — Cage mesh fitting, inner/outer cage visualization, and wrap deformer preview for `WrapLayer` / `WrapTarget`.
- [ ] **Avatar Compatibility Previewer** (`AvatarCompatibilityPreviewer`) — Preview custom rigs/accessories across standard animations, clothing layers, and body scales.
- [ ] **Terrain Palette Editor** (`TerrainPaletteEditor`) — Customize `Terrain` material colors (`MaterialColors`) and material overrides in a dedicated swatch editor.
- [ ] **Drafts Widget** (`DraftsWidget`) — Team Create uncommitted script drafts diff/commit/discard panel.

### 4. Roblox Cloud / AI-Specific
- [ ] **Material Generator** (`MaterialGenerator`) — Prompt-driven PBR texture generation into `MaterialVariant` (requires cloud generation backend or local provider).
- [ ] **Texture Generator** (`TextureGenerator`) — Prompt-driven 3D mesh texture synthesis.
- [ ] **Convert To Package** (`ConvertToPackage`) — Package publishing/linking workflow for reusable Roblox cloud models.
- [ ] **VR Status** (`VRStatus`) — VR headset connection and stereo viewport status indicator.
