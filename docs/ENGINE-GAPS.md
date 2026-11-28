# Engine gaps (measured)

What real games touch that rustudio doesn't implement, counted by the gap
telemetry (`RUSTUDIO_GAPS=FILE`, crates/rustudio-core/src/telemetry.rs) while playing the
fidelity corpus: **Place A** (lobby, START GAME, 60 s of a Map 1 match with
walking and firing), the **demo** scene and the **control** place (Play,
walking, jumping, clicking). Every member is checked against Studio's own API
dump (app/api-dump.json), so only real Roblox members are counted.

Gap kinds:

| Kind | Meaning |
|---|---|
| `missing` | a real member the runtime doesn't know (indexing it errors) |
| `call-unimplemented` | a known method with no implementation behind it |
| `default-only` | a property Roblox works out at run time (not saved in places) that rustudio only answered with its default |
| `write-ignored` | such a property written with nothing in rustudio acting on it |
| `event-unwired` | an event scripts connect to that nothing in rustudio fires (not named by any fire path in the engine's sources, and not fired during the run) |

## EditableMesh / EditableImage / Content (Place A's skinned guns)

Were silent stubs (AddVertex returned 0, Size (0, 0), ReadPixelsBuffer nil,
`Content.fromObject` missing): Place A's WeaponModels / CamoFx / CamoLayers fell
back inside their pcalls and every camo'd gun drew plain grey. Now
(crates/rustudio-engine/src/editable.rs, a runtime submodule):

- **Content** is a datatype (typeof "Content"; `SourceType`, `Uri`,
  `Object`): `Content.none`, `fromUri`, `fromAssetId`, `fromObject(EditableImage | EditableMesh)`.
  Every `*Content` property reads as one and takes one (or a string).
- **EditableMesh**: vertices, normals, UVs, colours and triangles under typed
  ids; Add*/Get*/Set*, Get/SetFace{Vertices,Normals,UVs,Colors},
  RemoveFace / RemoveUnused / Clear, GetVertexFaces, GetAdjacentVertices,
  GetCenter / GetSize, RaycastLocal, FindClosestVertex,
  FindVerticesWithinSphere, BatchAdd / BatchSetValues / BatchGetValues.
  AssetService:CreateEditableMesh(Async) (an asset URI or another
  EditableMesh; FixedSize kept). CreateMeshPartAsync(Content.fromObject(em))
  snapshots it as a generated 2.00 mesh (`rbxtemp://rustudio/editable/mesh/…`),
  so the MeshPart is drawn, ray-tested and collided as any MeshPart; Size /
  MeshSize = the mesh bounds.
- **EditableImage**: RGBA8 pixels at its Size; Read/WritePixelsBuffer,
  DrawRectangle / DrawCircle / DrawLine / DrawTriangle / DrawImage /
  DrawImageTransformed (BlendSourceOver, Overwrite, Add, Multiply,
  Subtract). Shown wherever a Content points at it (ImageLabel/ImageButton
  ImageContent, Decal/Texture TextureContent, SurfaceAppearance maps via
  AssetService:CreateSurfaceAppearanceAsync, clones included); each write
  re-uploads into the same GPU / egui texture slot (no growth per frame).
- **Any stub function**, on its first call, now prints
  `rustudio: Class.Member is a stub (it does nothing here)` to Output (once
  per member), so a test can't pass on one silently.

Still open: edits to an EditableMesh after CreateMeshPartAsync don't reach
the MeshPart (it holds a snapshot; CamoLayers' animated shell UVs stay at
their first frame); bones / FACS, DrawImageProjected / SampleImageProjected,
MergeVertices / Triangulate and the *WithAttribute queries are stubs; no
Editable* memory budget (Create* never returns nil); EditableImage /
EditableMesh are Instances here (typeof "Instance", Roblox: "Object").

## InputBinding.UIButton (buttons bound to input actions)

InputBinding.UIButton and UIModifier were read and written but nothing
acted on them (`write-ignored`): a game whose GuiButtons drive InputActions
through a binding's UIButton got no Pressed / Released, so its buttons did
nothing when clicked. Now Play sends each GuiButton's MouseButton1Down / Up
to the input action system (crates/rustudio-engine/src/domains/gui_input.luau):
a Bool action is held from the button's MouseButton1Down to its
MouseButton1Up, as the Roblox docs describe ("a GuiButton press/release
through the binding's UIButton property"). Estimates (the docs don't say):
UIModifier is a second button held with UIButton, context Priority and Sink
apply as for keys, and a press that never gets its MouseButton1Up ends when
the left mouse button comes up or the button leaves the game.

## Device Simulator (the device's input and screen)

Scripts always saw a desktop: UserInputService's TouchEnabled / GamepadEnabled
were false, PreferredInput KeyboardAndMouse, IsTenFootInterface false,
CurrentScreenOrientation LandscapeLeft, ViewportDisplaySize Medium, and the
ScreenInsets only knew the top bar. With Test > Device Simulator (or
`--device ID`) they now answer for the simulated device, with the numbers
measured in Studio's Play for each of its 43 presets (docs/MECHANICS.md,
Device Simulator):

| Member | Status |
|---|---|
| UserInputService TouchEnabled, KeyboardEnabled, MouseEnabled, GamepadEnabled, PreferredInput | **done**: the device's own flags; Changed fires on a switch mid-Play, in Studio's order |
| GuiService:IsTenFootInterface() | **done**: true on Xbox / PlayStation |
| PlayerGui.CurrentScreenOrientation | **done**: Portrait / LandscapeRight on a device, a frame after the view turns |
| GuiService.ViewportDisplaySize | **done**: the device's own |
| ScreenInsets None / DeviceSafeInsets, GuiService:GetInsetArea, Camera.ViewportSize | **done**: the safe area, and the whole screen under the notch for None |
| Camera:ViewportPointToRay / WorldToViewportPoint on a device | **done**: projected over the whole screen, from the safe area's corner |
| UserInputService.GamepadConnected / GamepadDisconnected for the device's own pad | **done**: consoles, headsets, the handhelds and the TV hold Gamepad1 |
| Edit-mode ScreenGui insets and view size on a device | estimate: the edit view's GUIs keep covering the safe area; flags are the device's (measured) |
| UserInputService TouchStarted / TouchMoved / TouchEnded, TouchTap, TouchTapInWorld, TouchLongPress, TouchPan, TouchSwipe, TouchPinch, TouchRotate; InputBegan / Changed / Ended with UserInputType Touch (UserInputService, bound actions and GuiObjects) | **done** (mouse-as-touch on a simulated phone or tablet): one InputObject per finger; Roblox's PlayerModule thumbstick and jump button work from it |
| Input action system KeyCode TouchPosition (with a UIModifier), TouchDelta, TouchPinch | **done**: Roblox's Input Action System player scripts (Workspace.PlayerScriptsUseInputActionSystem) get their DynamicThumbstick, touch camera and pinch zoom from fingers; which fingers count and the pinch's units are estimates |
| Virtual controller (Controller Emulator): gamepad InputBegan / Changed / Ended from keys, GamepadConnected / Disconnected, GamepadEnabled, PreferredInput Gamepad | **done**: Studio's key map and events; Roblox's PlayerModule walks and jumps from it |
| GuiObject.TouchTap | open: not fired yet (GuiObjects get the Touch InputBegan / InputEnded and their buttons' MouseButton1Click / Activated) |

## Round 19: the whole corpus, and what was wrong rather than missing

Ranked by `RUSTUDIO_GAPS` calls summed over the six fidelity places (Place A's
lobby, Place A mid-match, Place C, Place B, demo, control; each place's
`checks/compare/<place>/rustudio/gaps.json`). Only two stubbed members are
still called anywhere in the corpus; the rest of the round's engine work is
members that existed but behaved wrongly, found by the diff against Studio:

| # | Calls | Member | Kind | Places | Status |
|---|---|---|---|---|---|
| 1 | 15 | Terrain:SetMaterialColor (and GetMaterialColor) | stub-call | Place C (Main: every biome's colours) | **done**: writes / reads Terrain.MaterialColors (23 RGB slots), replicated; the drawn terrain follows |
| 2 | 1 (+ every landing) | Terrain:FillBlock, with FillBall / FillCylinder / FillWedge / FillRegion / Clear / ReplaceMaterial | call-unimplemented | Place B (water blocks); Place C builds every planet surface with ~700 FillBlocks on landing (not reached in the 40 s run) | **done**: voxels edited and written back to SmoothGrid once a step (drawn, collided, replicated). WriteVoxels / ReadVoxels / SetCell still open (no corpus caller) |
| – | every terrain ray | WorldRoot:Raycast, FindPartOnRay… | wrong: never hit terrain | any place with terrain | **done**: hits the voxels' surface with its material and normal (water not yet) |
| – | every frame | CFrame `toAxisAngle` (and `lerp`, `toEulerAngles*`, `toOrientation`, Color3 `lerp`) | missing lowercase aliases | Place B's Poppercam | **done** (Studio's list, probed) |
| – | once per PlayerModule | UserSettings():IsUserFeatureEnabled | wrong: errored on 16 names Studio answers | Place B, Place C, Place A | **done**: Studio's values; Place B's camera now pulls in like Studio's |
| – | every top-down frame | CFrame.lookAt / CFrame.new(pos, at) near vertical | wrong: turned 180° | the calibration scenes; any top-down camera | **done** |
| – | every frame | LinearVelocity (Vector) / AlignOrientation (soft) on a HumanoidRootPart | wrong: ignored | Place C's ship | **done**: the ship holds the player in space (camera matches Studio's exactly) |
| – | once | Humanoid.PlatformStanding on the client | event not fired on the client | Place C | **done** |
| – | every frame | parts welded to a character | wrong: simulated loose | Place C's ship | **done**: carried with the character |
| – | every landing | Humanoid.FloorMaterial on terrain | wrong: Plastic | any terrain place | **done**: the voxel's material |

Render fixes from the same places: Neon bloom (rings → Studio's smooth
two-Gaussian halo, self-test `bloom-smooth`), see-through parts drawn from
inside (Place B's white wash), Technology ShadowMap → Soft, a script
camera's roll. Measurements and sources: MECHANICS.md, round 19.

Open, by how much they block the corpus: Realistic local lights close to
the light (`lightcap-real`: Studio 2–4× brighter within a few studs);
water in terrain rays; WriteVoxels / ReadVoxels; Place C's `animation`
area reads 0 from an offline-Studio artifact (MECHANICS.md, round 19), and
Place B's walk / jump facts differ because offline Studio's meshes around its
spawn never load and collide as boxes.

## Round 12: ranked by real call count

Counts are touches across the three places (each place started once, the
pre-fix build). "Scripts" are the top callers. Behaviour is Roblox's
documented one (create.roblox.com/docs/reference/engine/classes/…).

| # | Count | Class.member | Kind | Studio-documented behaviour | Scripts | Status |
|---|---|---|---|---|---|---|
| 1 | 17 | Players.PlayerRemoving | event-unwired | Fires on the server and clients just before a Player leaves the game (also when a Studio test stops), with the Player | PlayerModule.CameraModule, ZoomController.Popper, RbxCharacterSounds, Place A LowGrav / OtherPlayers | **done**: fired on both sides when Play stops, with a dispatch so handlers run |
| 2 | 12 | Player.CharacterRemoving | event-unwired | Fires just before a player's Character is removed (respawn, LoadCharacter, leaving), with the old character | PlayerModule.CameraModule, Popper, ControlModule | **done**: fired on both sides before respawn / LoadCharacter removes it |
| 3 | 6 | UserInputService.TextBoxFocusReleased | event-unwired | Fires when a TextBox loses focus, with that TextBox | CameraInput, ControlModule.Keyboard | **done**: fired with each TextBox.FocusLost |
| 4 | 6 | UserInputService.WindowFocused | event-unwired | Fires when the Roblox window gains focus | CameraInput, ControlModule.Keyboard | **done**: from the OS window's focus each frame; UIS:IsWindowFocused() too |
| 5 | 5 | Humanoid.Seated | event-unwired | Fires when the humanoid sits in or leaves a Seat / VehicleSeat: (active, seatPart) | ControlModule, Animate | **done** (round 13): touching an enabled empty Seat / VehicleSeat or Seat:Sit sits the humanoid (SeatWeld built as Studio's does: C0 (0, Size.Y × 0.5, 0)·Angles(−π/2,0,0), C1 per rig type from the rig attachments; Seat.Occupant, Humanoid.SeatPart / Sit, state Seated); jumping, Sit = false, a removed weld or a disabled / gone seat gets it up. Proved in Play on the control place: Seated(true, TestSeat), root 2.00 over the seat's centre, jump → Seated(false, nil) + Jumping, touch re-sits after the 1 s cooldown |
| 6 | 3 | GuiService.MenuOpened | event-unwired | Fires when the player opens the Roblox Esc menu | CameraInput | **done** (round 13): Esc in Play opens a game menu (Resume / Reset Character / Leave); MenuOpened / MenuClosed fire on the client and GuiService.MenuIsOpen follows. Proved on the control place: MenuOpened(true) MenuClosed(false) ×2; Reset killed the character (Died on server and client) |
| 7 | 3 | UserInputService.TextBoxFocused | event-unwired | Fires when a TextBox gains focus, with that TextBox | ControlModule.Keyboard | **done**: fired with each TextBox.Focused |
| 8 | 3 | UserInputService.WindowFocusReleased | event-unwired | Fires when the Roblox window loses focus | CameraInput | **done** (as WindowFocused) |
| 9 | 2 | DataModel.PlaceId | default-only | The place's id; 0 for an unpublished place opened from a file in Studio | Place A PlayerSettings, Progression | **done**: 0, with GameId / CreatorId / PlaceVersion 0 and JobId / PrivateServerId "" |
| 10 | 2 | Humanoid.Climbing | event-unwired | Fires while climbing (a TrussPart or ladder) with the climb speed | Animate | **done** (round 13): walking into a TrussPart climbs at the move speed × 0.7, down when moving away from it, standing still holds on, jumping leaves away from it. Proved: Humanoid:MoveTo into a 24-stud truss rose 21.1 studs, Climbing(11.2) |
| 11 | 2 | Humanoid.FallingDown | event-unwired | Fires on entering / leaving the FallingDown state (active) | Animate | **done** (round 13): Humanoid:ChangeState(FallingDown) holds the humanoid 3 s, then GettingUp until upright, then Running; FallingDown(true/false) fire. Proved: StateChanged FallingDown@0.0 → GettingUp@3.0 → Running |
| 12 | 2 | Humanoid.GettingUp | event-unwired | Fires on entering / leaving the GettingUp state (active) | Animate | **done** (round 13): as FallingDown (GettingUp(true) on entering, (false) on leaving) |
| 13 | 2 | Humanoid.PlatformStanding | event-unwired | Fires when PlatformStand changes (active) | Animate | **done**: fired when PlatformStand changes |
| 14 | 2 | Humanoid.Swimming | event-unwired | Fires while swimming in terrain water, with the speed | Animate | **done** (round 13): the root in terrain water (occupancy > 0.5) swims: no gravity, Jump rises, a submerged idle humanoid floats up; Swimming(speed) on change. Proved by unit test `physics::water_and_floating` (no corpus place has water; Terrain editing from scripts is still unimplemented) |
| 15 | 2 | Instance.Archivable | default-only | Whether the instance is saved and cloned: Clone() returns nil for a non-archivable instance and leaves out non-archivable descendants | Place A Match | **done**: Clone() leaves non-archivable descendants out (the root case already returned nil) |
| 16 | 2 | ScriptContext.Error | event-unwired | Fires when a script errors: (message, stackTrace, script) | Place A DevErrors, Main | **done**: fired for every script error on that side, with the script when its name resolves |
| 17 | 2 | StudioTestService.GetTestArgs | missing | The arguments a test run was started with (ExecuteRunModeAsync / ExecutePlayModeAsync); nil for a plain Play | Place A DevTest, Progression | **done**: nil |
| 18 | 1 | UserInputService.GamepadDisconnected | event-unwired | Fires when a gamepad is unplugged, with its UserInputType | Place A GameInput | open: no gamepad input yet |
| 19 | 1 | UserInputService.LastInputTypeChanged | event-unwired | Fires when the kind of input the player last used changes (keyboard, mouse movement, a button…), with the new UserInputType | Place A GameInput | **done**: from the input stream; GetLastInputType() follows it, and before any input answers the device's kind (Touch on a touch screen, Keyboard on a desktop) as Roblox does — the PlayerModule picks its touch controls from that first answer |

After the fixes the same runs touch: Place A 3 open gaps (GuiService.MenuOpened,
Humanoid.Seated, UserInputService.GamepadDisconnected), demo and control 6
each (Humanoid.Seated / Climbing / FallingDown / GettingUp / Swimming,
GuiService.MenuOpened). Output stays free of errors in all three.

Telemetry fixes found on the way: the first Place A run counted 115,176
`write-ignored` writes to Motor6D.Transform (the engine does read it, through
`properties.get`), and `default-only` reads of Workspace.Terrain /
DataModel.Workspace (they resolve to children); the build-time scan of the
engine's sources (build.rs) now covers both.

## Round 14: every script-reachable member exists

**Stubs.** `tools/gen_stubs.py` writes `crates/rustudio-core/src/api_stubs.rs` from the dump:
4646 members a game script may reach (Security None; properties by read
security), each with a typed default (`false`, `0`, `""`, `{}`, the enum's
item 0, `Vector3.new()`…, a signal). `crates/rustudio-engine/src/stubs.rs` answers from it only
after the runtime's own lookups fail: reads give the default (or what a
script wrote; read-only ones refuse with Studio's message), writes are kept
per instance and fire Changed, calls return the typed empty value, events
are connectable signals nothing fires, callbacks can be assigned. Each use
is logged in the gap telemetry (`stub`, `stub-call`, `stub-write`). Left
out: NotScriptable members and the plugin / Studio-only classes (debugger,
dragger, Selection, ChangeHistoryService…). Lowercase deprecated spellings
(`sound:play`) still reach the real capitalised member first; a
reflection-known property without a reflection default (TextFits,
InterpolationThrottling) now reads its typed default instead of nil.
build.rs skips the generated file when scanning for the events the engine
fires (otherwise every event would look wired).

**Ranked by real calls.** Place A's match, the newer Place A build
(`place-a/build/place-a.rbxl`), Place B (og.rbxl), Place B'
laser tag, Godelz RPG, demo and control, each played (walk, jump, clicks,
E / F), with `RUSTUDIO_GAPS` (`../checks/coverage/gaps-*-r14.json`):

| # | Count | Member | Kind | Places | Status |
|---|---|---|---|---|---|
| 1 | (every read) | string attributes (BinaryString) | wrong value | laser tag (`viewModel`, `fireMode`) | **fixed**: rbx-dom reads string attributes from binary places as BinaryString and the runtime turned them into nil; the laser-tag blasters errored on every equip. Now strings; 0 Output errors there |
| 2 | 18 | TweenBase.PlaybackState | default-only | Place B | **done**: Begin / Delayed / Playing / Paused / Completed / Cancelled from the tween's run |
| 3 | 7 | Model.WorldPivot (write) | write-ignored | newer Place A build (Bo3Map) | **done**: with a PrimaryPart, sets its PivotOffset = PrimaryPart.CFrame⁻¹ · pivot; else the model's WorldPivotData; GetPivot / WorldPivot read it back |
| 4 | 2 | UIGridStyleLayout.AbsoluteContentSize | default-only | Godelz RPG | **done**: the extent of the visible GuiObjects it lays out, as drawn last frame (UDim2 rects before) |
| 5 | 2 | UserInputService.GamepadDisconnected | event-unwired | Place A, newer Place A | **done**: Play scans the joystick devices (/dev/input/by-id/*-event-joystick; Linux only, none on macOS or Windows yet) about once a second and fires GamepadConnected / GamepadDisconnected with Gamepad1…8; GetConnectedGamepads / GetNavigationGamepads / GetGamepadConnected answer from it. Since then host pads are read through gilrs on every platform, buttons and sticks included (crates/rustudio-player/src/gamepad.rs) |
| 6 | 2 | Mouse.Icon | stub default | PlayerModule (shift-lock) | **done**: UserInputService.MouseIcon, else the PlayerMouse's Icon, is drawn centred on the pointer over the game view in place of the system arrow |

After these, a stub-hit count from the noise of unset object properties
(Player.Character, Humanoid.Torso read nil, which is Roblox's value, not a
stub) was removed from the telemetry. The remaining Output errors in those
places are Studio's too for an unpublished place: DataStore access, a free
model's `require(assetId)`, and the leaderstats those scripts then never
make.

Coverage (round 14, `../checks/coverage/round-14.json`), game scripts:
functions exist 1569 / real 342 of 1626; properties 2801 / 2279 of 2802;
events 398 / 172 of 410; callbacks 13 / 3 of 13 (OnInvoke,
OnServerInvoke, OnClientInvoke are real).

## Round 13

Every open row above is done except GamepadDisconnected (no gamepad input:
needs a gamepad library, 1 call). The same three runs on the round-13 build
touch one open gap in all: Place A `UserInputService.GamepadDisconnected`
(demo and control: none). Output free of errors in all three.

Also implemented while working the list: Humanoid:SetStateEnabled /
GetStateEnabled (a disabled state is never entered), ChangeState beyond
Jumping (Seated, Dead, FallingDown, GettingUp, Ragdoll, Physics, the
walking states), Seat:Sit / VehicleSeat:Sit, client-side HealthChanged /
Died from a replicated Health, Lighting.ClockTime / TimeOfDay /
SetMinutesAfterMidnight as one value.

### Place A as a game: why Round read nil and no one scored

Diagnosed with probes on both sides (round 13):

1. **Collision groups lived in each script VM.** Place A's server registers
   WeaponRay / PlayerClip / ZombieClip and sets them uncollidable; the
   client never saw that, so its weapon rays (`CollisionGroup = "WeaponRay"`)
   stopped on the window PlayerClip brushes and no hit was ever reported
   (0 `ZHit` remotes). With no kills the player was downed at ~88 s, the
   session ended and its folder (with Round / RoundState) was deleted:
   that is the nil the probes read. **Fixed**: one shared table per Play
   session (crates/rustudio-core/src/collision_groups.rs) used by both runtimes' queries and by
   the physics (rapier interaction groups; the character collides as its
   root part's group).
2. **SetAttribute fired on an unchanged value** (2236 AttributeChanged for
   ZombiesLeft in a minute). **Fixed**: no change, no event.
3. **Key taps within one frame were lost** (R to reload, F to buy).
   **Fixed**: they reach InputBegan / InputEnded.
4. The harness fired at a wall: the client probe now aims at the nearest
   enemy a WeaponRay reaches, as a player would.

Result (`place_a_match.py`, gate now requires ≥ 1 kill and points over 500):
round 1 active after 36 s, 6 kills, 1040 points, ZombiesLeft 6 → 1 and the
round ending into intermission within the 60 s watched, not downed, 0
error lines. Wall buys and doors (hold F), auto and R reload and the HUD
(points, weapon, clip / reserve, buy hints) work. Noted, not an engine
issue: `place-a.rbxl` is older than the Place A source (built 2026-10-26).

## What else came out of making Place A playable

Not API gaps but engine bugs the Place A match exposed (enemies never spawned):

- **Mesh raycasts reported the box face's normal** instead of the hit
  triangle's. Map 1's outdoor ground is terrain baked into MeshParts, so
  every downward hit there read Normal.Y = 0; Place A's nav-grid builder needs
  Normal.Y > 0.7, found no floor outside, and no spawn spot connected to a
  window. Now the triangle's normal (inverse-transpose of the mesh scale).
- **WorldRoot:GetPartsInPart compared bounding boxes**; Roblox tests the real
  shapes (Place A's headroom probes rely on it: a turned brush's or a terrain
  chunk's box covers far more than the part). Now exact: box vs box by
  separating axes, box vs mesh triangles (crates/rustudio-physics/src/query_shapes.rs).
- **A layout inside a Folder laid out nothing**: `gui_walk.rs` flattened a
  Folder's GuiObjects into its parent's children and looked for a
  UIListLayout / UIGridLayout / UIPageLayout among the parent's own
  children only, so LBG's skill bar — `Skills.Container.Buttons`, a
  Folder holding the UIListLayout and the skill buttons cloned into it as
  skills are added — drew every button at the Container's top-left, one on
  top of the other. Now each group of GuiObjects (the parent's own, each
  Folder's) is placed by the layout beside it, inside the parent's
  content, as Roblox does (`children`: `place_children` once for the
  parent and once per Folder; test `list_layout_in_a_folder`). Not done:
  an AutomaticSize parent measuring a Folder-held list's lines.
- **An AutomaticSize list of RelativeXX cards measured three times its
  height** (LBG's limited emotes: a column growing on Y of
  `{1, 0}, {0.932, 0}` SizeConstraint RelativeXX cards, each holding a
  0.48 UIAspectRatioConstraint frame): `gui_walk.rs` measured a child with
  a Y Scale as sized by the growing axis whatever the Scale read, and
  counted it by its content; down in the content the aspect frame, told
  its height was unknown, derived 500 px of it from its width, and the
  300 px list came out at 900 — its "LIMITED TIME" header and "Leaving
  in" footer centred off the screen, the cards under the top edge. Now
  `scale_axes` says which parent axis each Scale reads (both the width
  under RelativeXX, both the height under RelativeYY) and a card on a
  fixed axis counts by its Size (`auto_child_size`, `own_size_in_auto`).
  Test `auto_size_counts_relative_xx_children_by_size`.
- **A TextScaled label growing on X alone was 8 px wide**: the
  `{0, 0}, {1, 0}` name tags of a horizontal list (LBG's "Accessory" /
  "Animation" pills) were measured at their TextSize wrapped in no width
  — one glyph wide — and the painted text then scaled down to 2 px in
  that box. Roblox sizes such text by the box's height and widens the box
  to the line (`scaled_text_size`: the largest whole size whose line fits
  the height, at most 100; `content_size_in`; `FaceText` for a script's
  read between walks). Test `text_scaled_auto_x_sizes_text_by_height`.
  Not done: TextScaled growing on Y or XY still measures at TextSize.
- **A short Team Create join played as if whole**: `rustudio_net`'s
  `Ready` is 1.5 s without an instance batch, not the server's end of
  transfer, and a child parked for a parent that never arrived left no
  trace. The Output now counts the parked instances at Ready (a few
  named) and asks to reopen the place, or open a saved .rbxl, before
  playing (`cloud_ui.rs` on `model.rs`'s `parked_count` /
  `parked_sample`). (The LBG `BeanWind.Mesh` failure once blamed on this was
  the constructor-name gap, now closed upstream: the saved file holds
  154,682 instances, not the 160,078 claimed, so the join had been complete.) Not done: the
  protocol's own end-of-join signal; a join that stalls under QUIC flow
  control while the phone is busy applying batches can still pass for
  quiet.
- **A server script's `Player.Character = rig` left the player with no
  controls.** Roblox treats the write as a spawn (the class page; staff on
  the DevForum, 2024: the old character is removed "the way things have
  worked since the beginning"): the old character's `CharacterRemoving`,
  then it goes, `Changed("Character")`, `CharacterAdded` with the rig on
  the server and on each client, the camera's `CameraSubject` on its
  Humanoid, the PlayerModule rebinding its controls to it — the custom-rig
  pattern games use (`CharacterAutoLoads = false`, a rig cloned from
  storage into Workspace, `player.Character = rig`). rustudio stored the
  property like any other: no event, so the camera and controls stayed on
  the old model (which the game then destroyed) and the rig stood in
  Workspace as an NPC, dead to input. Now the property's setter
  (crates/rustudio-engine/src/runtime.rs `newindex_routed`, Play's server)
  fires the old character's `CharacterRemoving` and `Changed` and hands
  the pair to the session, which makes the rig the character from where
  it stands, facing where it faces (crates/rustudio-player/src/play.rs
  `adopt_character`): out of the NPCs, Studio's limb collisions on its
  standard parts, the controller on it, the server's `CharacterAdded`,
  the old model out of Workspace and destroyed after it (a respawn's
  order), then the client's half once the rig has replicated
  (`character_client_half`: the property, the camera's `CameraSubject`,
  `CharacterAdded`, the rig's own LocalScripts started after it, as after
  a spawn). `Character = nil` leaves the player without one, the old
  character gone the same way. As on Roblox, nothing is cloned into the
  rig (no StarterCharacterScripts, Animate or Health — a custom rig brings
  its own) and the Backpack and PlayerGui stay: that is `LoadCharacter`'s
  work. Writing the model that is already the character changes nothing.
  Test `server_set_character_is_the_player_character` (rig from
  ReplicatedStorage, events on both sides, camera, the rig's LocalScript
  once, `Player:Move` walks it, then nil).

## Coverage of Studio's API

Measured by `rustudio --api-coverage OUT [GAPS…]` (src/coverage.rs): one
instance of every class in the dump (services at the root, the rest in a
folder, so even classes a script can't create are covered; abstract classes
on a concrete subclass; AnimationTrack / Tween / InputObject / Mouse made the
way scripts get them), every property read and every function and event
indexed in pcall through the real runtime. A member is implemented when that
worked and the telemetry recorded no gap (`missing`, `default-only`), minus
`call-unimplemented` seen in the real runs. Functions are indexed, not
called.

| | Round 12 start | Round 12 end | Of which game scripts may use (Security None) |
|---|---|---|---|
| Functions | 314 / 3402 | 344 / 3402 | 340 / 1626 |
| Properties | 2833 / 3806 | 2873 / 3806 | 2274 / 2802 |
| Events | 172 / 849 | 172 / 849 | 172 / 410 |

Round 13 end: functions **346 / 3402** (game scripts 342 / 1626),
properties **2875 / 3806** (2276 / 2802), events 172 / 849 (172 / 410)
(`../checks/coverage/round-13.json`, gap files `gaps-*-r13.json`).

Implemented this round from the list and the next most-used families:
WorldRoot:Spherecast / Blockcast / Shapecast; the collision-group functions on
WorldRoot (and PhysicsService:GetMaxCollisionGroups / GetCollisionGroupName /
CollisionGroupContainsPart); GuiObject:TweenPosition / TweenSize /
TweenSizeAndPosition; Humanoid:GetLimb / GetBodyPartR15 / GetMoveVelocity;
Instance:GetActor; BasePart:GetClosestPointOnSurface / GetJoints /
GetNoCollisionConstraints / GetRenderCFrame / ApplyImpulseAtPosition and
ExtentsCFrame / ExtentsSize / CenterOfMass / AssemblyCenterOfMass;
Lighting:GetSunDirection / GetMoonDirection / GetMoonPhase; Model.WorldPivot
(PrimaryPart's pivot, else WorldPivotData, else the bounds' centre);
Mouse.hit / target / TargetSurface; Tween.Instance / TweenInfo; Player
run-time properties (DisplayName, MembershipType, AccountAge, LocaleId,
CanLoadCharacterAppearance, …); Players.NumPlayers / localPlayer /
players(); DataModel JobId / PrivateServerId / CreatorType / Genre /
MatchmakingType; Attachment.Rotation / WorldRotation.

The measurements are kept in `../checks/coverage/` (`round-12-start.json`,
`round-12.json`, and the before / after gap files of each place).

## Render divergence (after Place A plays cleanly)

Round 12 fidelity scores (render, settled shots): Place A 49.5, demo 69.3
(was 64.2), control 68.5 (was 67.8); overall 62.4.

- **Bloom**: done. Studio glows over-bright pixels (Neon) with no
  BloomEffect in the place; rustudio's bloom pass now runs with BloomEffect's
  defaults (Intensity 1, Size 24, Threshold 2) when there isn't one. The
  control shot's halo around the Neon bar matches within ~8 levels.
- **Shadows**: the sun shadow map's darkness was already within 2 levels of
  Studio (self-test `lighting-day`); in the aligned control frames Studio's
  character shadow is softer and lighter at its edge. Not changed this round.
- **Glass**: open. A Transparency-0.5 Glass pane is almost invisible in
  Studio and a visible light-blue pane in rustudio (demo and control); needs
  a Studio calibration scene (transparency steps over light and dark).
- **Place A chapel brightness** (Soft spot lights) and the **skybox / stud scale**
  remain open.

Round 13 fidelity (`fidelity.py run --round 13`): render Place A 49.5, demo
69.3, control 68.5 (unchanged), all gates pass (MAE 32.4 / 21.7 / 23.1).
Place A camera_controls read 75: the menu camera looked 2.18° off against this
run's Studio capture (tolerance 2°); the round-12 build measures the same
2.18° against it, so it comes from the Studio reference's timing, not from
this round. A night calibration scene (Place A's chapel lighting in a plain
room, `tools/fidelity/corpus/night.rbxlx`) measured MAE 20.4 and located
the chapel gap in Soft spot lights; the spawn ForceField bubble Studio
draws at 10 s is the other visible gap in demo and control.

Harness determinism fixed on the way (not loosened): Studio spawns the
character at a random point on the SpawnLocation, so both engines now get a
prelude that puts it on the centre (demo settled MAE 40 → 21.7; thresholds
lowered to demo 24, control 25); Place A's menu camera sways on `os.clock()`, so
rustudio's `os.clock` now counts like Studio's (from the process start) and
the harness passes Studio's reading at Play (`RUSTUDIO_CLOCK_BASE`): Place A
camera 75 → 100 with the 2° tolerance unchanged.

## Next (open, by how much they block real games)

1. **Soft spot lights** (render): Studio spreads a SpotLight's light well
   outside its cone in LightingStyle Soft (night calibration, MECHANICS.md
   round 13); the Place A chapel's darkness comes mostly from this.
   **ForceField visual**: Studio draws the spawn bubble at 10 s in demo and
   control; rustudio draws none. Terrain editing from scripts (FillBlock …).
2. ~~PathfindingService:CreatePath / Path:ComputeAsync / GetWaypoints~~
   (`domains/geometry_nav.rs`: a heightfield navmesh over the collidable
   parts and terrain, A* with Costs, jump links, PathfindingModifier /
   PathfindingLink, Blocked / Unblocked, the deprecated
   ComputeSmoothPathAsync / FindPathAsync; `geometry_tests.rs`).
3. **TextService:GetTextSize / GetTextBoundsAsync** — UI layout code.
4. ~~GuiService.MenuOpened / MenuClosed with an in-game Esc menu~~ (round 13).
5. ~~Gamepad input (GamepadConnected / Disconnected, gamepad KeyCodes)~~ (crates/rustudio-player/src/gamepad.rs).
6. The rest of `missing_game_members_by_class` in the coverage report.
7. ~~Touch for games~~ (Android, docs/ANDROID.md "Fingers in Play"): on a
   touch screen `UserInputService.TouchEnabled` / `TouchScreenEnabled` /
   `PreferredInput == Touch`, one InputObject per finger through
   `InputBegan / Changed / Ended` and `TouchStarted / Moved / Ended`,
   ContextActionService `UserInputType.Touch` bindings and the
   touch-button CoreScript's feed (`BoundActionAdded / Removed / Changed`,
   `GetActionButtonEvent`, `FireActionButtonFoundSignal`, and
   `CallFunction` for the press: ContextActionTouch draws
   `createTouchButton` buttons when the CoreScripts run, and
   `domains/context_action_touch.luau` draws the same ones — Roblox's
   ContextActionGui, places, sizes and children — when they don't), the input action
   system's `TouchPosition / TouchDelta / TouchPinch` with `UIButton` /
   `UIModifier`, GuiObject `InputBegan(Touch)` / `TouchTap`, the gestures
   (`TouchTap`, `TouchTapInWorld`, `TouchLongPress`, `TouchPan`,
   `TouchPinch`, `TouchRotate`, `TouchSwipe`) and the motion sensors
   (`AccelerometerEnabled` / `GyroscopeEnabled`, `GetDeviceAcceleration /
   Gravity / Rotation`, `Device*Changed`). Still open: GuiObject
   `TouchLongPress / Pan / Pinch / Rotate / Swipe` (only
   UserInputService's fire), `UserInputService.TouchDrag`, and the
   gestures on a desktop (no finger reaches the game there: the editor's
   mouse path stays). The soft keyboard over a game's TextBox:
   `OnScreenKeyboardVisible / Position / Size` in the game's
   coordinates and `TextBox.ReturnPressedFromOnScreenKeyboard`
   (src/viewport.rs `keyboard_pan`, crates/rustudio-engine/src/runtime_ext.rs `device_prop`;
   zero and never on a desktop, as in Studio).
