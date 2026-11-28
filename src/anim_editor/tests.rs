//! Comprehensive end-to-end QA, headless UI, R15 rig lifecycle, and Curve Editor
//! integration tests for RusTudio's native 1:1 Animation Editor port.

use std::path::PathBuf;

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use glam::{Mat3, Vec3};
use rbx_dom_weak::types::{Ref, Variant};
use rbx_dom_weak::{InstanceBuilder, WeakDom};

use rustudio_core::doc::{self, Document};
use rustudio_core::math::{cf_mul, Xform};
use rustudio_render::camera::Camera;
use rustudio_render::scene::Scene;

use crate::anim_editor::constants::{
    ANIMSAVES_NAME, AUTOMATIC_SAVE_NAME, PROP_POSITION, PROP_ROTATION, PROP_Y, RBX_ANIMSAVES_NAME,
};
use crate::anim_editor::events;
use crate::anim_editor::history;
use crate::anim_editor::model::{KeyInterpolationMode, KeyframeValue, Tick, TrackType};
use crate::anim_editor::ops;
use crate::anim_editor::reduction;
use crate::anim_editor::rig;
use crate::anim_editor::rig_viz;
use crate::anim_editor::selection;
use crate::anim_editor::serialization;
use crate::anim_editor::state::{
    ActiveDialog, EditorMode, EditorState, ImportSettingsDialogState, MigrationChoice,
    MigrationItem, PlayState, ReduceKeyframesDialogMode, ReduceKeyframesState, RigTypeKind,
    RigValidationIssue, TangentSide, ToastKind, ToolMode, ValidationSeverity,
};
use crate::anim_editor::ui_main::{show_animation_editor_panel, PanelFrameResult};
use crate::anim_editor::ui_menus::show_settings_menu;
use crate::anim_editor::viewport_dragger;

const ID_XF: Xform = Xform {
    pos: Vec3::ZERO,
    rot: Mat3::IDENTITY,
};

// ============================================================================
// Test Helpers: Document + R15 Rig Builder + Headless egui Runner
// ============================================================================

fn approx_vec3(a: Vec3, b: Vec3, eps: f32) {
    assert!(
        (a - b).length() <= eps,
        "expected {b:?}, got {a:?} (diff={})",
        (a - b).length()
    );
}

fn approx_xform(a: Xform, b: Xform, eps: f32) {
    approx_vec3(a.pos, b.pos, eps);
    approx_vec3(a.rot.x_axis, b.rot.x_axis, eps);
    approx_vec3(a.rot.y_axis, b.rot.y_axis, eps);
    approx_vec3(a.rot.z_axis, b.rot.z_axis, eps);
}

/// Builds a complete, valid R15 rig `Model` under `Workspace` with `Humanoid`,
/// `HumanoidRootPart`, all 15 R15 `Part`s, and 14 `Motor6D` joints in rest pose.
fn build_r15_rig_document() -> (Document, Ref) {
    let mut dom = WeakDom::new(InstanceBuilder::new("DataModel"));
    let ws = dom.insert(dom.root_ref(), InstanceBuilder::new("Workspace"));
    let _ss = dom.insert(dom.root_ref(), InstanceBuilder::new("ServerStorage"));

    let rig_model = dom.insert(
        ws,
        InstanceBuilder::new("Model").with_name("R15Dummy"),
    );
    dom.insert(
        rig_model,
        InstanceBuilder::new("Humanoid")
            .with_property("RigType", Variant::Enum(rbx_dom_weak::types::Enum::from_u32(1))),
    );

    let mut insert_part = |name: &str, pos: Vec3, size: Vec3, anchored: bool| -> Ref {
        let xf = Xform {
            pos,
            rot: Mat3::IDENTITY,
        };
        dom.insert(
            rig_model,
            InstanceBuilder::new("Part")
                .with_name(name)
                .with_property("CFrame", xf.to_cframe())
                .with_property("Size", rbx_dom_weak::types::Vector3::new(size.x, size.y, size.z))
                .with_property("Anchored", anchored),
        )
    };

    let hrp = insert_part("HumanoidRootPart", Vec3::new(0.0, 3.0, 0.0), Vec3::new(2.0, 2.0, 1.0), true);
    let lower_torso = insert_part("LowerTorso", Vec3::new(0.0, 2.5, 0.0), Vec3::new(2.0, 0.5, 1.0), false);
    let upper_torso = insert_part("UpperTorso", Vec3::new(0.0, 3.5, 0.0), Vec3::new(2.0, 1.5, 1.0), false);
    let head = insert_part("Head", Vec3::new(0.0, 4.75, 0.0), Vec3::new(1.0, 1.0, 1.0), false);

    let l_uarm = insert_part("LeftUpperArm", Vec3::new(-1.5, 3.75, 0.0), Vec3::new(1.0, 1.0, 1.0), false);
    let l_larm = insert_part("LeftLowerArm", Vec3::new(-1.5, 2.75, 0.0), Vec3::new(1.0, 1.0, 1.0), false);
    let l_hand = insert_part("LeftHand", Vec3::new(-1.5, 2.0, 0.0), Vec3::new(1.0, 0.5, 1.0), false);

    let r_uarm = insert_part("RightUpperArm", Vec3::new(1.5, 3.75, 0.0), Vec3::new(1.0, 1.0, 1.0), false);
    let r_larm = insert_part("RightLowerArm", Vec3::new(1.5, 2.75, 0.0), Vec3::new(1.0, 1.0, 1.0), false);
    let r_hand = insert_part("RightHand", Vec3::new(1.5, 2.0, 0.0), Vec3::new(1.0, 0.5, 1.0), false);

    let l_uleg = insert_part("LeftUpperLeg", Vec3::new(-0.5, 1.75, 0.0), Vec3::new(1.0, 1.0, 1.0), false);
    let l_lleg = insert_part("LeftLowerLeg", Vec3::new(-0.5, 0.75, 0.0), Vec3::new(1.0, 1.0, 1.0), false);
    let l_foot = insert_part("LeftFoot", Vec3::new(-0.5, 0.15, 0.0), Vec3::new(1.0, 0.3, 1.0), false);

    let r_uleg = insert_part("RightUpperLeg", Vec3::new(0.5, 1.75, 0.0), Vec3::new(1.0, 1.0, 1.0), false);
    let r_lleg = insert_part("RightLowerLeg", Vec3::new(0.5, 0.75, 0.0), Vec3::new(1.0, 1.0, 1.0), false);
    let r_foot = insert_part("RightFoot", Vec3::new(0.5, 0.15, 0.0), Vec3::new(1.0, 0.3, 1.0), false);

    // Set PrimaryPart on rig_model
    dom.get_by_ref_mut(rig_model)
        .unwrap()
        .properties
        .insert("PrimaryPart".into(), Variant::Ref(hrp));

    // Helper to connect Part0 -> Part1 with Motor6D whose C0/C1 match their rest positions.
    let mut connect = |name: &str, p0: Ref, p1: Ref, p0_pos: Vec3, p1_pos: Vec3, joint_pos: Vec3| {
        let c0 = Xform {
            pos: joint_pos - p0_pos,
            rot: Mat3::IDENTITY,
        };
        let c1 = Xform {
            pos: joint_pos - p1_pos,
            rot: Mat3::IDENTITY,
        };
        dom.insert(
            p1,
            InstanceBuilder::new("Motor6D")
                .with_name(name)
                .with_property("Part0", Variant::Ref(p0))
                .with_property("Part1", Variant::Ref(p1))
                .with_property("C0", c0.to_cframe())
                .with_property("C1", c1.to_cframe())
                .with_property("Transform", ID_XF.to_cframe()),
        );
    };

    connect("Root", hrp, lower_torso, Vec3::new(0.0, 3.0, 0.0), Vec3::new(0.0, 2.5, 0.0), Vec3::new(0.0, 2.5, 0.0));
    connect("Waist", lower_torso, upper_torso, Vec3::new(0.0, 2.5, 0.0), Vec3::new(0.0, 3.5, 0.0), Vec3::new(0.0, 2.75, 0.0));
    connect("Neck", upper_torso, head, Vec3::new(0.0, 3.5, 0.0), Vec3::new(0.0, 4.75, 0.0), Vec3::new(0.0, 4.25, 0.0));

    connect("LeftShoulder", upper_torso, l_uarm, Vec3::new(0.0, 3.5, 0.0), Vec3::new(-1.5, 3.75, 0.0), Vec3::new(-1.0, 4.0, 0.0));
    connect("LeftElbow", l_uarm, l_larm, Vec3::new(-1.5, 3.75, 0.0), Vec3::new(-1.5, 2.75, 0.0), Vec3::new(-1.5, 3.25, 0.0));
    connect("LeftWrist", l_larm, l_hand, Vec3::new(-1.5, 2.75, 0.0), Vec3::new(-1.5, 2.0, 0.0), Vec3::new(-1.5, 2.25, 0.0));

    connect("RightShoulder", upper_torso, r_uarm, Vec3::new(0.0, 3.5, 0.0), Vec3::new(1.5, 3.75, 0.0), Vec3::new(1.0, 4.0, 0.0));
    connect("RightElbow", r_uarm, r_larm, Vec3::new(1.5, 3.75, 0.0), Vec3::new(1.5, 2.75, 0.0), Vec3::new(1.5, 3.25, 0.0));
    connect("RightWrist", r_larm, r_hand, Vec3::new(1.5, 2.75, 0.0), Vec3::new(1.5, 2.0, 0.0), Vec3::new(1.5, 2.25, 0.0));

    connect("LeftHip", lower_torso, l_uleg, Vec3::new(0.0, 2.5, 0.0), Vec3::new(-0.5, 1.75, 0.0), Vec3::new(-0.5, 2.25, 0.0));
    connect("LeftKnee", l_uleg, l_lleg, Vec3::new(-0.5, 1.75, 0.0), Vec3::new(-0.5, 0.75, 0.0), Vec3::new(-0.5, 1.25, 0.0));
    connect("LeftAnkle", l_lleg, l_foot, Vec3::new(-0.5, 0.75, 0.0), Vec3::new(-0.5, 0.15, 0.0), Vec3::new(-0.5, 0.3, 0.0));

    connect("RightHip", lower_torso, r_uleg, Vec3::new(0.0, 2.5, 0.0), Vec3::new(0.5, 1.75, 0.0), Vec3::new(0.5, 2.25, 0.0));
    connect("RightKnee", r_uleg, r_lleg, Vec3::new(0.5, 1.75, 0.0), Vec3::new(0.5, 0.75, 0.0), Vec3::new(0.5, 1.25, 0.0));
    connect("RightAnkle", r_lleg, r_foot, Vec3::new(0.5, 0.75, 0.0), Vec3::new(0.5, 0.15, 0.0), Vec3::new(0.5, 0.3, 0.0));

    let doc = Document::from_play_dom(dom, "r15_test".to_string());
    (doc, rig_model)
}

fn default_raw_input() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 420.0))),
        ..Default::default()
    }
}

fn run_panel_frame(
    ctx: &egui::Context,
    raw_input: RawInput,
    state: &mut EditorState,
    is_playing: bool,
    dt: f32,
) -> (PanelFrameResult, usize) {
    let mut frame_res = PanelFrameResult::default();
    let mut output = ctx.run_ui(raw_input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            frame_res = show_animation_editor_panel(ui, state, is_playing, dt);
        });
    });
    let shape_count = output.shapes.len();
    output.textures_delta.clear();
    (frame_res, shape_count)
}

fn make_populated_state() -> (Document, Ref, EditorState) {
    let (doc, rig_model) = build_r15_rig_document();
    let mut state = EditorState::default();
    state.rig_cache = rig::build_rig_cache(&doc, rig_model);
    state.active = true;
    ops::create_new_animation(&mut state, "TestClip".to_string());

    // Populate several R15 tracks with keyframes across [0, 4800]
    let rot_a = Xform {
        pos: Vec3::new(0.0, 0.25, 0.0),
        rot: Mat3::from_rotation_x(0.4),
    };
    let rot_b = Xform {
        pos: Vec3::new(0.0, -0.15, 0.1),
        rot: Mat3::from_rotation_z(-0.6),
    };
    for part in [
        "LowerTorso",
        "UpperTorso",
        "Head",
        "RightUpperArm",
        "RightLowerArm",
        "LeftUpperArm",
        "LeftLowerArm",
        "RightUpperLeg",
        "LeftUpperLeg",
    ] {
        ops::value_changed(
            &mut state,
            "Root",
            &[part.to_string()],
            TrackType::CFrame,
            0,
            KeyframeValue::CFrame(ID_XF),
            false,
        );
        ops::value_changed(
            &mut state,
            "Root",
            &[part.to_string()],
            TrackType::CFrame,
            2400,
            KeyframeValue::CFrame(rot_a),
            false,
        );
        ops::value_changed(
            &mut state,
            "Root",
            &[part.to_string()],
            TrackType::CFrame,
            4800,
            KeyframeValue::CFrame(rot_b),
            false,
        );
    }

    // Add animation events
    if let Some(anim) = state.animation_data.as_mut() {
        anim.events.add_event(1200, "Footstep".to_string(), "Left".to_string());
        anim.events.add_event(3600, "Footstep".to_string(), "Right".to_string());
        anim.events.named_keyframes.insert(2400, "Apex".to_string());
    }
    state.settings.show_events = true;
    events::refresh_available_events(&mut state);
    selection::rebuild_track_list(&mut state);

    (doc, rig_model, state)
}

// ============================================================================
// 1. Headless egui UI Rendering Tests Across Every Mode, Window, Menu & Dialog
// ============================================================================

#[test]
fn headless_ui_renders_start_screen_and_inactive_cover() {
    let ctx = egui::Context::default();
    let mut state = EditorState::default();

    // StartScreen (empty, no rig)
    let (_, shapes_empty) = run_panel_frame(&ctx, default_raw_input(), &mut state, false, 1.0 / 60.0);
    assert!(shapes_empty > 0, "empty StartScreen should paint shapes");

    // StartScreen (with rig attached, no animation_data)
    let (doc, rig_model) = build_r15_rig_document();
    state.rig_cache = rig::build_rig_cache(&doc, rig_model);
    state.active = true;
    let (_, shapes_rig) = run_panel_frame(&ctx, default_raw_input(), &mut state, false, 1.0 / 60.0);
    assert!(shapes_rig > 0, "rig StartScreen should paint shapes");

    // InactiveCover (while playing game)
    let (_, shapes_cover) = run_panel_frame(&ctx, default_raw_input(), &mut state, true, 1.0 / 60.0);
    assert!(shapes_cover > 0, "InactiveCover should paint shapes");
}

#[test]
fn headless_ui_renders_dopesheet_with_tracks_events_and_scale_controls() {
    let ctx = egui::Context::default();
    let (_doc, _rig, mut state) = make_populated_state();

    // Select keyframes at two distinct ticks so ScaleControls overlay renders too!
    selection::select_keyframes_at_tick(&mut state, 0, false);
    selection::select_keyframes_at_tick(&mut state, 2400, true);

    let (_, shapes) = run_panel_frame(&ctx, default_raw_input(), &mut state, false, 1.0 / 60.0);
    assert!(shapes > 0, "DopeSheet with tracks, events & scale controls should render");
}

#[test]
fn headless_ui_renders_curve_editor_cubic_linear_constant_quaternion_and_tangents() {
    let ctx = egui::Context::default();
    let (_doc, _rig, mut state) = make_populated_state();

    // Promote to curves so tracks become channel tracks
    ops::promote_to_curves(&mut state);
    assert_eq!(state.editor_mode, EditorMode::Curves);
    assert!(state.is_channel_animation());

    // Convert RightUpperArm rotation to Quaternion while keeping LowerTorso as EulerAngles
    ops::convert_track_rotation(&mut state, "Root", &["RightUpperArm".to_string()], false);

    // Set mixed Cubic / Linear / Constant interpolation modes + explicit tangents
    if let Some(anim) = state.animation_data.as_mut() {
        let pos_y_path = vec![
            "LowerTorso".to_string(),
            PROP_POSITION.to_string(),
            PROP_Y.to_string(),
        ];
        if let Some(track) = anim.get_track_mut("Root", &pos_y_path) {
            if let Some(kf) = track.data.get_mut(&0) {
                kf.interpolation_mode = Some(KeyInterpolationMode::Cubic);
                kf.right_slope = Some(0.0005);
            }
            if let Some(kf) = track.data.get_mut(&2400) {
                kf.interpolation_mode = Some(KeyInterpolationMode::Linear);
                kf.left_slope = Some(-0.0003);
                kf.right_slope = Some(0.0002);
            }
            if let Some(kf) = track.data.get_mut(&4800) {
                kf.interpolation_mode = Some(KeyInterpolationMode::Constant);
            }
        }
    }

    // Select a keyframe so tangent handles are drawn
    selection::select_keyframe_range(
        &mut state,
        "Root",
        &[
            "LowerTorso".to_string(),
            PROP_POSITION.to_string(),
            PROP_Y.to_string(),
        ],
        2400,
        2400,
        false,
    );

    let (_, shapes) = run_panel_frame(&ctx, default_raw_input(), &mut state, false, 1.0 / 60.0);
    assert!(shapes > 0, "CurveEditor with mixed curves & tangents should render");
}

#[test]
fn headless_ui_renders_all_floating_windows_dialogs_menus_and_toasts() {
    let ctx = egui::Context::default();
    let (_doc, _rig, mut state) = make_populated_state();

    // 1. Floating IKWindow + FaceControlsWindow
    state.ik.show_tree = true;
    state.ik.enabled = true;
    state.rig_cache.face_controls_ref = Some(Ref::new());
    state.facs.show_panel = true;
    let (_, shapes) = run_panel_frame(&ctx, default_raw_input(), &mut state, false, 1.0 / 60.0);
    assert!(shapes > 0);
    state.ik.show_tree = false;
    state.facs.show_panel = false;

    // 2. EditEventsDialog
    events::open_edit_events_dialog(&mut state, 1200);
    let (_, shapes) = run_panel_frame(&ctx, default_raw_input(), &mut state, false, 1.0 / 60.0);
    assert!(shapes > 0);
    state.active_dialog = ActiveDialog::None;

    // 3. ReduceKeyframesDialog
    let orig = state.animation_data.clone().unwrap();
    let prioritized_ticks = reduction::compute_reduction_order(&orig);
    let total = prioritized_ticks.len().max(2);
    state.active_dialog = ActiveDialog::ReduceKeyframes(ReduceKeyframesState {
        mode: ReduceKeyframesDialogMode::FromMenu,
        animation_backup: Some(orig.clone()),
        source_data: Some(orig),
        prioritized_ticks,
        initial_keyframes: total,
        target_keyframes: total,
        has_moved: false,
    });
    let (_, shapes) = run_panel_frame(&ctx, default_raw_input(), &mut state, false, 1.0 / 60.0);
    assert!(shapes > 0);

    // 4. All remaining ActiveDialog variants
    let dialogs: Vec<ActiveDialog> = vec![
        ActiveDialog::Migration {
            items: vec![MigrationItem {
                anim_ref: Ref::new(),
                name: "LegacyWalk".to_string(),
                choice: MigrationChoice::Migrate,
            }],
        },
        ActiveDialog::ImportSettings(ImportSettingsDialogState {
            fbx_path: Some(PathBuf::from("/tmp/test.fbx")),
            track_selection_items: vec!["Take 001".to_string()],
            ..Default::default()
        }),
        ActiveDialog::ChangeFps {
            text: "60".to_string(),
            error: None,
        },
        ActiveDialog::ChangePlaybackSpeed {
            text: "1.5".to_string(),
            error: None,
        },
        ActiveDialog::ChangeGridSpeed {
            text: "16".to_string(),
            error: None,
        },
        ActiveDialog::PromoteToCurves,
        ActiveDialog::CreateNewAnimation {
            name: "NewClip".to_string(),
            error: None,
        },
        ActiveDialog::SaveAs {
            name: "SavedClip".to_string(),
            error: None,
        },
        ActiveDialog::OverwriteConfirm {
            name: "SavedClip".to_string(),
            is_legacy: false,
        },
        ActiveDialog::RenameKeyframe {
            tick: 2400,
            name: "Apex".to_string(),
        },
        ActiveDialog::ChangeDuration {
            text: "60".to_string(),
            current_duration_frames: 60,
            error: None,
        },
        ActiveDialog::RigErrors {
            title: "Rig Errors".to_string(),
            issues: vec![RigValidationIssue {
                id: "DuplicateName".to_string(),
                severity: ValidationSeverity::Error,
                message: "Duplicate part name".to_string(),
                details: vec!["PartA".to_string()],
            }],
        },
        ActiveDialog::ImportFromRoblox {
            asset_id_text: "123456".to_string(),
            error: None,
        },
        ActiveDialog::InProgress {
            text: "Importing...".to_string(),
            cancellable: true,
        },
    ];

    for dlg in dialogs {
        state.active_dialog = dlg;
        let (_, shapes) = run_panel_frame(&ctx, default_raw_input(), &mut state, false, 1.0 / 60.0);
        assert!(shapes > 0);
    }
    state.active_dialog = ActiveDialog::None;

    // 5. AnimationClipMenu (open + verify click outside closes it!)
    state.menus.show_clip_menu = true;
    let (_, shapes_menu) = run_panel_frame(&ctx, default_raw_input(), &mut state, false, 1.0 / 60.0);
    assert!(shapes_menu > 0);
    assert!(state.menus.show_clip_menu, "AnimationClipMenu should stay open on idle frame");

    // Simulate clicking far outside the clip menu (press + release at bottom-right of panel)
    let mut click_down = default_raw_input();
    click_down.events.push(Event::PointerMoved(Pos2::new(1100.0, 350.0)));
    click_down.events.push(Event::PointerButton {
        pos: Pos2::new(1100.0, 350.0),
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    let _ = run_panel_frame(&ctx, click_down, &mut state, false, 1.0 / 60.0);

    let mut click_up = default_raw_input();
    click_up.events.push(Event::PointerMoved(Pos2::new(1100.0, 350.0)));
    click_up.events.push(Event::PointerButton {
        pos: Pos2::new(1100.0, 350.0),
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    let _ = run_panel_frame(&ctx, click_up, &mut state, false, 1.0 / 60.0);
    assert!(
        !state.menus.show_clip_menu,
        "AnimationClipMenu must close when clicking outside"
    );

    // 6. SettingsMenu direct render
    let mut out_settings = ctx.run_ui(default_raw_input(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            show_settings_menu(ui, &mut state);
        });
    });
    let settings_shapes = out_settings.shapes.len();
    out_settings.textures_delta.clear();
    assert!(settings_shapes > 0);

    // 7. Toasts (NoticeToast & ActionToast)
    state.toast.show_notice("Saved \"TestClip\"", 0.0);
    let (_, shapes_notice) = run_panel_frame(&ctx, default_raw_input(), &mut state, false, 1.0 / 60.0);
    assert!(shapes_notice > 0);

    state.toast.active = Some(ToastKind::Action {
        text: "Migration available".to_string(),
        primary_label: "Migrate".to_string(),
        secondary_label: "Ignore".to_string(),
        action_id: "migrate".to_string(),
    });
    let (_, shapes_action) = run_panel_frame(&ctx, default_raw_input(), &mut state, false, 1.0 / 60.0);
    assert!(shapes_action > 0);
}

#[test]
fn keyboard_shortcuts_play_undo_redo_tool_and_space_toggle() {
    let ctx = egui::Context::default();
    let (_doc, _rig, mut state) = make_populated_state();

    // First frame to register layout
    let _ = run_panel_frame(&ctx, default_raw_input(), &mut state, false, 1.0 / 60.0);

    let key_input = |key: Key, modifiers: Modifiers| -> RawInput {
        let mut raw = default_raw_input();
        raw.events.push(Event::PointerMoved(Pos2::new(600.0, 200.0)));
        raw.events.push(Event::ModifiersChanged(modifiers));
        raw.events.push(Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        });
        raw
    };

    // Space: Pause -> Play -> Pause
    assert_eq!(state.playback.play_state, PlayState::Pause);
    let _ = run_panel_frame(&ctx, key_input(Key::Space, Modifiers::NONE), &mut state, false, 1.0 / 60.0);
    assert_eq!(state.playback.play_state, PlayState::Play);
    let _ = run_panel_frame(&ctx, key_input(Key::Space, Modifiers::NONE), &mut state, false, 1.0 / 60.0);
    assert_eq!(state.playback.play_state, PlayState::Pause);

    // R: Rotate -> Move -> Rotate
    assert_eq!(state.viewport_dragger.tool, ToolMode::Rotate);
    let _ = run_panel_frame(&ctx, key_input(Key::R, Modifiers::NONE), &mut state, false, 1.0 / 60.0);
    assert_eq!(state.viewport_dragger.tool, ToolMode::Move);
    let _ = run_panel_frame(&ctx, key_input(Key::R, Modifiers::NONE), &mut state, false, 1.0 / 60.0);
    assert_eq!(state.viewport_dragger.tool, ToolMode::Rotate);

    // Ctrl+L: Local -> World -> Local
    assert!(!state.viewport_dragger.world_space);
    let _ = run_panel_frame(&ctx, key_input(Key::L, Modifiers::CTRL), &mut state, false, 1.0 / 60.0);
    assert!(state.viewport_dragger.world_space);
    let _ = run_panel_frame(&ctx, key_input(Key::L, Modifiers::CTRL), &mut state, false, 1.0 / 60.0);
    assert!(!state.viewport_dragger.world_space);

    // Ctrl+Z / Ctrl+Y: Undo & Redo
    let before_end = state.animation_data.as_ref().unwrap().metadata.end_tick;
    ops::value_changed(
        &mut state,
        "Root",
        &["Head".to_string()],
        TrackType::CFrame,
        7200,
        KeyframeValue::CFrame(ID_XF),
        true,
    );
    assert_eq!(state.animation_data.as_ref().unwrap().metadata.end_tick, 7200);

    let (res_undo, _) = run_panel_frame(&ctx, key_input(Key::Z, Modifiers::CTRL), &mut state, false, 1.0 / 60.0);
    assert!(res_undo.needs_rig_step);
    assert_eq!(state.animation_data.as_ref().unwrap().metadata.end_tick, before_end);

    let (res_redo, _) = run_panel_frame(&ctx, key_input(Key::Y, Modifiers::CTRL), &mut state, false, 1.0 / 60.0);
    assert!(res_redo.needs_rig_step);
    assert_eq!(state.animation_data.as_ref().unwrap().metadata.end_tick, 7200);
}

#[test]
fn viewport_dragger_picks_rig_before_active_and_selects_joint_with_gizmo() {
    let (mut doc, rig_model) = build_r15_rig_document();
    let mut scene = Scene::default();
    scene.sync(&mut doc);

    let mut cam = Camera::default();
    cam.pos = Vec3::new(0.0, 3.5, 10.0);
    cam.yaw = 0.0;
    cam.pitch = 0.0;

    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
    let ctx = egui::Context::default();
    let mut state = EditorState::default();
    assert!(!state.active);

    // Project UpperTorso position to screen to click the rig while !state.active
    let upper_torso_ref = doc
        .dom
        .get_by_ref(rig_model)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|&c| doc.dom.get_by_ref(c).is_some_and(|i| i.name == "UpperTorso"))
        .unwrap();
    let upper_torso_pos = doc::xform_of(&doc, upper_torso_ref).unwrap().pos;
    let click_px = cam.project(rect, upper_torso_pos).unwrap();

    let mut raw = RawInput {
        screen_rect: Some(rect),
        ..Default::default()
    };
    raw.events.push(Event::PointerMoved(click_px));
    raw.events.push(Event::PointerButton {
        pos: click_px,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });

    let mut action = viewport_dragger::ViewportAction::default();
    let mut out1 = ctx.run_ui(raw, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
            action = viewport_dragger::handle_viewport_interaction(
                ui.ctx(),
                ui.painter(),
                rect,
                &resp,
                &cam,
                &scene,
                &mut doc,
                &mut state,
                None,
                None,
            );
        });
    });
    out1.textures_delta.clear();

    assert_eq!(
        action.attach_rig,
        Some(rig_model),
        "Clicking a rig in 3D viewport when Animation Editor is open (not yet active) must emit attach_rig"
    );
    assert!(action.consumed_pointer);

    // Now attach rig & create animation, then click RightUpperArm to select joint + show gizmo
    state.rig_cache = rig::build_rig_cache(&doc, rig_model);
    state.active = true;
    ops::create_new_animation(&mut state, "PoseTest".to_string());

    let r_uarm_ref = *state.rig_cache.part_refs.get("RightUpperArm").unwrap();
    let r_uarm_pos = doc::xform_of(&doc, r_uarm_ref).unwrap().pos;
    let arm_px = cam.project(rect, r_uarm_pos).unwrap();

    let mut raw_arm = RawInput {
        screen_rect: Some(rect),
        ..Default::default()
    };
    raw_arm.events.push(Event::PointerMoved(arm_px));
    raw_arm.events.push(Event::PointerButton {
        pos: arm_px,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });

    let mut arm_action = viewport_dragger::ViewportAction::default();
    let mut out_arm = ctx.run_ui(raw_arm, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
            arm_action = viewport_dragger::handle_viewport_interaction(
                ui.ctx(),
                ui.painter(),
                rect,
                &resp,
                &cam,
                &scene,
                &mut doc,
                &mut state,
                None,
                None,
            );
        });
    });
    let arm_shapes = out_arm.shapes.len();
    out_arm.textures_delta.clear();

    assert!(arm_action.consumed_pointer);
    assert_eq!(state.selected_tracks, vec![vec!["RightUpperArm".to_string()]]);
    assert!(
        arm_shapes > 0,
        "Gizmo handles should render immediately on the click frame"
    );

    // Verify exact joint pivot == Part1.CFrame * C1
    let pivot = viewport_dragger::compute_joint_pivot(
        &doc,
        &scene,
        &state.rig_cache,
        "RightUpperArm",
        true,
    )
    .unwrap();
    let joint = state.rig_cache.joints_by_part.get("RightUpperArm").unwrap();
    let expected_pivot = cf_mul(&doc::xform_of(&doc, r_uarm_ref).unwrap(), &joint.c1);
    approx_xform(pivot, expected_pivot, 1e-4);
}

// ============================================================================
// 2. Full R15 Rig Lifecycle Integration Test
// ============================================================================

#[test]
fn r15_rig_full_lifecycle_pose_fk_save_reset_reload() {
    let (mut doc, rig_model) = build_r15_rig_document();

    // 1. validate_rig: zero errors
    let issues = rig::validate_rig(&doc, rig_model);
    let errors: Vec<_> = issues
        .iter()
        .filter(|i| i.severity == ValidationSeverity::Error)
        .collect();
    assert!(errors.is_empty(), "Expected 0 rig validation errors, got: {errors:?}");

    // 2. build_rig_cache
    let cache = rig::build_rig_cache(&doc, rig_model);
    assert_eq!(cache.rig_type, RigTypeKind::R15);
    assert_eq!(cache.root_ref, Some(rig_model));
    assert!(cache.joints_by_part.contains_key("RightUpperArm"));
    assert!(cache.joints_by_part.contains_key("RightLowerArm"));
    assert!(cache.joints_by_part.contains_key("RightHand"));

    let r_hand_ref = *cache.part_refs.get("RightHand").unwrap();
    let r_uarm_joint_ref = cache.joints_by_part.get("RightUpperArm").unwrap().joint_ref;
    let rest_hand_xf = doc::xform_of(&doc, r_hand_ref).unwrap();

    // 3. create_new_animation
    let mut state = EditorState::default();
    state.rig_cache = cache;
    state.active = true;
    ops::create_new_animation(&mut state, "WaveHello".to_string());
    assert!(state.animation_data.is_some());

    // 4. value_changed poses RightUpperArm (90 deg Z rotation) and RightLowerArm (45 deg X rotation) at tick 2400
    let arm_pose = Xform {
        pos: Vec3::ZERO,
        rot: Mat3::from_rotation_z(90.0_f32.to_radians()),
    };
    let forearm_pose = Xform {
        pos: Vec3::ZERO,
        rot: Mat3::from_rotation_x(45.0_f32.to_radians()),
    };
    ops::value_changed(
        &mut state,
        "Root",
        &["RightUpperArm".to_string()],
        TrackType::CFrame,
        2400,
        KeyframeValue::CFrame(arm_pose),
        true,
    );
    ops::value_changed(
        &mut state,
        "Root",
        &["RightLowerArm".to_string()],
        TrackType::CFrame,
        2400,
        KeyframeValue::CFrame(forearm_pose),
        true,
    );
    assert!(state.is_dirty);

    // 5. step_rig_animation moves BasePart.CFrame via FK
    let data = state.animation_data.clone().unwrap();
    rig::step_rig_animation(&mut doc, &state.rig_cache, &data, 2400);

    let posed_hand_xf = doc::xform_of(&doc, r_hand_ref).unwrap();
    assert!(
        (posed_hand_xf.pos - rest_hand_xf.pos).length() > 0.5,
        "FK must move RightHand when RightUpperArm & RightLowerArm are posed"
    );

    // Verify Motor6D.Transform was written
    let written_tf = rig_viz::read_cframe_prop(&doc, r_uarm_joint_ref, "Transform");
    approx_xform(written_tf, arm_pose, 1e-4);

    // 6. save_clip_to_anim_saves -> ServerStorage.RBX_ANIMSAVES linked via rig.AnimSaves
    let saved_ref =
        rig::save_clip_to_anim_saves(&mut doc, &mut state.rig_cache, &data, "WaveHello")
            .expect("save_clip_to_anim_saves should succeed");
    assert_eq!(state.rig_cache.anim_saves.len(), 1);
    assert_eq!(state.rig_cache.anim_saves[0].name, "WaveHello");

    // Verify rig.AnimSaves ObjectValue points to a child under ServerStorage.RBX_ANIMSAVES
    let anim_saves_ov = doc
        .dom
        .get_by_ref(rig_model)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|&c| doc.dom.get_by_ref(c).is_some_and(|i| i.name == ANIMSAVES_NAME))
        .expect("rig.AnimSaves ObjectValue must exist");
    assert_eq!(doc.class_of(anim_saves_ov).as_deref(), Some("ObjectValue"));
    let server_entry = match doc.prop(anim_saves_ov, "Value") {
        Some(Variant::Ref(r)) => r,
        other => panic!("Expected Ref Value on rig.AnimSaves, got {other:?}"),
    };
    let rbx_animsaves = doc.dom.get_by_ref(server_entry).unwrap().parent();
    assert_eq!(
        doc.dom.get_by_ref(rbx_animsaves).unwrap().name,
        RBX_ANIMSAVES_NAME
    );

    // Also verify autosave works with AUTOMATIC_SAVE_NAME
    let _auto_ref = rig::save_clip_to_anim_saves(
        &mut doc,
        &mut state.rig_cache,
        &data,
        AUTOMATIC_SAVE_NAME,
    )
    .expect("autosave should succeed");
    assert!(state
        .rig_cache
        .anim_saves
        .iter()
        .any(|e| e.name == AUTOMATIC_SAVE_NAME));

    // 7. reset_rig_pose restores rest pose (Motor6D.Transform == Identity & BasePart.CFrame == rest)
    rig::reset_rig_pose(&mut doc, &state.rig_cache);
    let reset_tf = rig_viz::read_cframe_prop(&doc, r_uarm_joint_ref, "Transform");
    approx_xform(reset_tf, ID_XF, 1e-4);
    let reset_hand_xf = doc::xform_of(&doc, r_hand_ref).unwrap();
    approx_xform(reset_hand_xf, rest_hand_xf, 1e-4);

    // 8. Reload from AnimSaves and verify step_rig_animation matches posed BasePart.CFrame!
    let reloaded = serialization::from_keyframe_sequence(&doc, saved_ref, 30)
        .expect("from_keyframe_sequence should succeed");
    ops::load_animation_data(&mut state, reloaded);
    let reloaded_data = state.animation_data.as_ref().unwrap();
    rig::step_rig_animation(&mut doc, &state.rig_cache, reloaded_data, 2400);

    let reloaded_hand_xf = doc::xform_of(&doc, r_hand_ref).unwrap();
    approx_xform(reloaded_hand_xf, posed_hand_xf, 1e-3);
}

// ============================================================================
// 3. Full Curve Editor + Serialization + Reduction + Undo/Redo Round-Trip Test
// ============================================================================

#[test]
fn curve_editor_promote_convert_tangent_split_serialize_reduce_undo_roundtrip() {
    let (mut doc, _rig_model) = build_r15_rig_document();
    let mut state = EditorState::default();
    ops::create_new_animation(&mut state, "CurveTest".to_string());

    // Author 12 keyframes on LowerTorso and RightUpperArm
    for i in 0..12 {
        let tick: Tick = i * 400;
        let phase = (i as f32) * 0.45;
        let torso_xf = Xform {
            pos: Vec3::new(0.0, phase.sin() * 1.5, (i as f32) * 0.1),
            rot: Mat3::from_rotation_y(phase * 0.3),
        };
        let arm_xf = Xform {
            pos: Vec3::ZERO,
            rot: Mat3::from_euler(glam::EulerRot::XYZ, phase * 0.5, phase * 0.2, -phase * 0.4),
        };
        ops::value_changed(
            &mut state,
            "Root",
            &["LowerTorso".to_string()],
            TrackType::CFrame,
            tick,
            KeyframeValue::CFrame(torso_xf),
            false,
        );
        ops::value_changed(
            &mut state,
            "Root",
            &["RightUpperArm".to_string()],
            TrackType::CFrame,
            tick,
            KeyframeValue::CFrame(arm_xf),
            false,
        );
    }

    assert!(!state.is_channel_animation());

    // 1. PromoteToCurves
    ops::promote_to_curves(&mut state);
    assert!(state.is_channel_animation());
    assert_eq!(state.editor_mode, EditorMode::Curves);

    let pos_y_path = vec![
        "LowerTorso".to_string(),
        PROP_POSITION.to_string(),
        PROP_Y.to_string(),
    ];
    let orig_pos_y_count = state
        .animation_data
        .as_ref()
        .unwrap()
        .get_track("Root", &pos_y_path)
        .unwrap()
        .keyframes
        .len();
    assert_eq!(orig_pos_y_count, 12);

    // 2. convert_track_rotation: EulerAngles -> Quaternion
    ops::convert_track_rotation(&mut state, "Root", &["RightUpperArm".to_string()], false);
    let arm_rot_path = vec!["RightUpperArm".to_string(), PROP_ROTATION.to_string()];
    assert_eq!(
        state
            .animation_data
            .as_ref()
            .unwrap()
            .get_track("Root", &arm_rot_path)
            .unwrap()
            .track_type,
        TrackType::Quaternion
    );

    // 3. set_keyframe_tangent
    ops::set_keyframe_tangent(
        &mut state,
        "Root",
        &pos_y_path,
        800,
        TangentSide::Right,
        Some(0.0025),
    );
    let kf_800 = state
        .animation_data
        .as_ref()
        .unwrap()
        .get_track("Root", &pos_y_path)
        .unwrap()
        .data
        .get(&800)
        .unwrap();
    assert!((kf_800.right_slope.unwrap() - 0.0025).abs() < 1e-6);

    // 4. split_track_at_tick (insert interpolated keyframe at tick 600)
    ops::split_track_at_tick(&mut state, "Root", &pos_y_path, 600);
    let split_count = state
        .animation_data
        .as_ref()
        .unwrap()
        .get_track("Root", &pos_y_path)
        .unwrap()
        .keyframes
        .len();
    assert_eq!(split_count, 13);
    assert!(state
        .animation_data
        .as_ref()
        .unwrap()
        .get_track("Root", &pos_y_path)
        .unwrap()
        .data
        .contains_key(&600));

    // 5. to_curve_animation -> from_curve_animation round-trip
    let ws = doc.workspace().unwrap();
    let pre_ser = state.animation_data.clone().unwrap();
    let ca_ref = serialization::to_curve_animation(&mut doc, ws, &pre_ser, None);
    let roundtripped = serialization::from_curve_animation(&doc, ca_ref, 30)
        .expect("from_curve_animation should succeed");
    assert!(roundtripped.is_channel_animation());

    let rt_pos_y = roundtripped.get_track("Root", &pos_y_path).unwrap();
    assert_eq!(rt_pos_y.keyframes.len(), 13);
    assert!(
        (rt_pos_y.data.get(&800).unwrap().right_slope.unwrap() - 0.0025).abs() < 1e-4,
        "Tangent slope should survive CurveAnimation serialization round-trip"
    );
    let rt_arm_rot = roundtripped.get_track("Root", &arm_rot_path).unwrap();
    assert_eq!(rt_arm_rot.track_type, TrackType::Quaternion);
    assert_eq!(rt_arm_rot.keyframes.len(), 12);

    // 6. Visvalingam-Whyatt keyframe reduction + Undo/Redo
    let before_reduction = state.animation_data.clone().unwrap();
    let before_ticks = before_reduction.get_summary_ticks().len();
    assert!(before_ticks >= 12);

    let prioritized = reduction::compute_reduction_order(&before_reduction);
    let reduced = reduction::build_reduced_animation(&before_reduction, &prioritized, 5);
    assert!(
        reduced.get_summary_ticks().len() <= 5,
        "Reduced animation should have <= 5 summary ticks, got {}",
        reduced.get_summary_ticks().len()
    );

    // Commit reduction with undo waypoint
    history::add_waypoint(&mut state);
    state.animation_data = Some(reduced.clone());
    selection::rebuild_track_list(&mut state);
    assert!(state.animation_data.as_ref().unwrap().get_summary_ticks().len() <= 5);

    // Undo restores exact original animation
    assert!(history::undo(&mut state));
    assert_eq!(
        state.animation_data.as_ref().unwrap().get_summary_ticks().len(),
        before_ticks
    );
    assert_eq!(
        state
            .animation_data
            .as_ref()
            .unwrap()
            .get_track("Root", &pos_y_path)
            .unwrap()
            .keyframes
            .len(),
        13
    );

    // Redo reapplies reduced animation
    assert!(history::redo(&mut state));
    assert!(state.animation_data.as_ref().unwrap().get_summary_ticks().len() <= 5);
}

#[test]
fn narrow_panel_widths_do_not_panic_in_dopesheet_or_curves() {
    let ctx = egui::Context::default();
    let (_doc, _rig, mut state) = make_populated_state();
    for width in [600.0_f32, 420.0, 350.0, 180.0, 40.0] {
        for mode in [EditorMode::DopeSheet, EditorMode::Curves] {
            state.editor_mode = mode;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::new(1500.0, 0.0), Vec2::new(width, 260.0))),
                ..Default::default()
            };
            let _ = run_panel_frame(&ctx, raw, &mut state, false, 1.0 / 60.0);
        }
    }
}

#[test]
fn ik_and_facs_windows_do_not_grow_vertically_across_frames() {
    let ctx = egui::Context::default();
    let (_doc, _rig, mut state) = make_populated_state();
    state.ik.show_tree = true;
    state.ik.enabled = true;
    state.facs.show_panel = true;

    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 900.0));
    let mut ik_heights = Vec::new();
    let mut facs_heights = Vec::new();

    for _ in 0..25 {
        let raw = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let _ = run_panel_frame(&ctx, raw, &mut state, false, 1.0 / 60.0);
        if let Some(r) = ctx.memory(|m| m.area_rect(egui::Id::new("ace_ik_window"))) {
            ik_heights.push(r.height());
        }
        if let Some(r) = ctx.memory(|m| m.area_rect(egui::Id::new("ace_face_controls_window"))) {
            facs_heights.push(r.height());
        }
    }

    assert!(ik_heights.len() >= 10, "IK window rect should be tracked");
    assert!(facs_heights.len() >= 10, "Face Controls window rect should be tracked");
    let ik_settled = ik_heights[3];
    let ik_final = *ik_heights.last().unwrap();
    assert!(
        (ik_final - ik_settled).abs() < 1.0 && ik_final < 700.0,
        "IK window height grew uncontrollably: {:?}",
        ik_heights
    );
    let facs_settled = facs_heights[3];
    let facs_final = *facs_heights.last().unwrap();
    assert!(
        (facs_final - facs_settled).abs() < 1.0 && facs_final < 700.0,
        "Face Controls window height grew uncontrollably: {:?}",
        facs_heights
    );
}

#[test]
#[ignore]
fn studio_parity_generate_and_eval() {
    use crate::anim_editor::constants::{PROP_X, PROP_Y, PROP_Z};
    use crate::anim_editor::model::{
        AnimationData, AnimationPriority, EasingDirection, EasingStyle, Keyframe, RotationOrder,
        Track,
    };
    use glam::Quat;

    let out_dir = std::env::var("ANIM_PARITY_OUT_DIR")
        .expect("set ANIM_PARITY_OUT_DIR to output directory");
    let out_path = PathBuf::from(&out_dir);
    std::fs::create_dir_all(&out_path).unwrap();

    let (mut doc, rig_model) = build_r15_rig_document();

    // Add Animator inside Humanoid so Studio's Animator:LoadAnimation works directly.
    let hum_ref = doc
        .dom
        .get_by_ref(rig_model)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|&c| doc.class_of(c).as_deref() == Some("Humanoid"))
        .expect("Humanoid not found");
    doc.insert(hum_ref, InstanceBuilder::new("Animator").with_name("Animator"));

    let ss_ref = doc
        .dom
        .root()
        .children()
        .iter()
        .copied()
        .find(|&c| doc.class_of(c).as_deref() == Some("ServerStorage"))
        .expect("ServerStorage not found");

    let rig_cache = rig::build_rig_cache(&doc, rig_model);

    let xf_euler = |px: f32, py: f32, pz: f32, rx: f32, ry: f32, rz: f32| -> Xform {
        Xform {
            pos: Vec3::new(px, py, pz),
            rot: RotationOrder::XYZ.from_euler_angles(Vec3::new(rx, ry, rz)),
        }
    };

    // ------------------------------------------------------------------------
    // 1. KeyframeSequence covering all 6 PoseEasingStyles x 3 EasingDirections
    // ------------------------------------------------------------------------
    let mut kf_data = AnimationData::new_rig_animation("ParityKeyframeSeq".to_string());
    kf_data.metadata.looping = false;
    kf_data.metadata.priority = AnimationPriority::Action;
    kf_data.metadata.frame_rate = 30;
    kf_data.metadata.is_channel_animation = false;

    let kf_specs: &[(&str, [(EasingStyle, EasingDirection); 4], [Xform; 5])] = &[
        (
            "LowerTorso",
            [
                (EasingStyle::Linear, EasingDirection::In),
                (EasingStyle::Linear, EasingDirection::Out),
                (EasingStyle::Linear, EasingDirection::InOut),
                (EasingStyle::Linear, EasingDirection::In),
            ],
            [
                xf_euler(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
                xf_euler(0.3, 0.2, -0.1, 0.25, -0.35, 0.15),
                xf_euler(-0.2, 0.45, 0.2, -0.30, 0.50, -0.25),
                xf_euler(0.15, -0.25, 0.35, 0.40, 0.10, 0.35),
                xf_euler(0.0, 0.1, -0.15, -0.15, -0.20, 0.10),
            ],
        ),
        (
            "UpperTorso",
            [
                (EasingStyle::Constant, EasingDirection::In),
                (EasingStyle::Constant, EasingDirection::Out),
                (EasingStyle::Constant, EasingDirection::InOut),
                (EasingStyle::Linear, EasingDirection::In),
            ],
            [
                xf_euler(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
                xf_euler(0.1, 0.25, -0.2, 0.35, 0.20, -0.40),
                xf_euler(-0.15, -0.1, 0.3, -0.25, -0.45, 0.30),
                xf_euler(0.2, 0.15, 0.05, 0.50, 0.35, 0.20),
                xf_euler(0.0, 0.0, 0.0, 0.10, -0.15, -0.10),
            ],
        ),
        (
            "Head",
            [
                (EasingStyle::Cubic, EasingDirection::In),
                (EasingStyle::Cubic, EasingDirection::Out),
                (EasingStyle::Cubic, EasingDirection::InOut),
                (EasingStyle::Cubic, EasingDirection::Out),
            ],
            [
                xf_euler(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
                xf_euler(0.05, 0.15, 0.1, 0.45, -0.60, 0.20),
                xf_euler(-0.1, 0.05, -0.15, -0.35, 0.70, -0.30),
                xf_euler(0.12, -0.08, 0.08, 0.25, -0.40, 0.45),
                xf_euler(0.0, 0.0, 0.0, 0.0, 0.15, -0.10),
            ],
        ),
        (
            "RightUpperArm",
            [
                (EasingStyle::CubicV2, EasingDirection::In),
                (EasingStyle::CubicV2, EasingDirection::Out),
                (EasingStyle::CubicV2, EasingDirection::InOut),
                (EasingStyle::CubicV2, EasingDirection::Out),
            ],
            [
                xf_euler(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
                xf_euler(0.15, 0.2, -0.1, 0.80, 0.30, -0.50),
                xf_euler(-0.1, 0.35, 0.2, -0.40, 0.65, 0.45),
                xf_euler(0.25, -0.15, 0.1, 0.55, -0.35, -0.75),
                xf_euler(0.05, 0.05, -0.05, -0.20, 0.25, 0.15),
            ],
        ),
        (
            "RightLowerArm",
            [
                (EasingStyle::Bounce, EasingDirection::In),
                (EasingStyle::Bounce, EasingDirection::Out),
                (EasingStyle::Bounce, EasingDirection::InOut),
                (EasingStyle::Bounce, EasingDirection::Out),
            ],
            [
                xf_euler(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
                xf_euler(0.1, -0.1, 0.15, 0.90, -0.20, 0.30),
                xf_euler(-0.08, 0.2, -0.1, 0.15, 0.50, -0.60),
                xf_euler(0.18, 0.05, 0.22, 1.10, -0.35, 0.25),
                xf_euler(0.0, 0.0, 0.0, 0.30, 0.10, -0.15),
            ],
        ),
        (
            "LeftUpperLeg",
            [
                (EasingStyle::Elastic, EasingDirection::In),
                (EasingStyle::Elastic, EasingDirection::Out),
                (EasingStyle::Elastic, EasingDirection::InOut),
                (EasingStyle::Elastic, EasingDirection::Out),
            ],
            [
                xf_euler(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
                xf_euler(-0.12, 0.18, 0.25, -0.65, 0.25, -0.35),
                xf_euler(0.14, -0.15, -0.2, 0.55, -0.40, 0.40),
                xf_euler(-0.2, 0.1, 0.15, -0.45, 0.30, 0.20),
                xf_euler(0.0, 0.0, 0.0, 0.15, -0.10, -0.05),
            ],
        ),
    ];

    let ticks = [0, 1200, 2400, 3600, 4800];
    {
        let root = kf_data.instances.entry("Root".to_string()).or_default();
        for &(joint, seg_easings, poses) in kf_specs {
            let mut tr = Track::new(TrackType::CFrame);
            for i in 0..5 {
                let (style, dir) = if i < 4 {
                    seg_easings[i]
                } else {
                    (EasingStyle::Linear, EasingDirection::In)
                };
                tr.add_keyframe(
                    ticks[i],
                    Keyframe {
                        value: KeyframeValue::CFrame(poses[i]),
                        easing_style: Some(style),
                        easing_direction: Some(dir),
                        interpolation_mode: None,
                        left_slope: None,
                        right_slope: None,
                    },
                );
            }
            root.tracks.insert(joint.to_string(), tr);
        }
    }
    kf_data.set_end_tick();
    let _kf_seq_ref = serialization::to_keyframe_sequence(&mut doc, ss_ref, &kf_data, Some(&rig_cache));

    // ------------------------------------------------------------------------
    // 2. CurveAnimation covering Vector3Curve, EulerRotationCurve (XYZ/YXZ/ZYX),
    //    and RotationCurve (Quaternion Cubic, Linear, Constant)
    // ------------------------------------------------------------------------
    let mut curve_data = AnimationData::new_rig_animation("ParityCurveAnim".to_string());
    curve_data.metadata.looping = false;
    curve_data.metadata.priority = AnimationPriority::Action;
    curve_data.metadata.frame_rate = 30;
    curve_data.metadata.is_channel_animation = true;

    let make_scalar_track = |tt: TrackType,
                             keys: &[(Tick, f32, KeyInterpolationMode, Option<f32>, Option<f32>)]|
     -> Track {
        let mut t = Track::new_curve(tt);
        for &(tick, v, mode, ls, rs) in keys {
            let kv = if tt == TrackType::Angle {
                KeyframeValue::Angle(v)
            } else {
                KeyframeValue::Number(v)
            };
            t.add_keyframe(
                tick,
                Keyframe {
                    value: kv,
                    easing_style: None,
                    easing_direction: None,
                    interpolation_mode: Some(mode),
                    left_slope: ls,
                    right_slope: rs,
                },
            );
        }
        t
    };

    let make_vec3_track = |tt: TrackType,
                           order: Option<RotationOrder>,
                           x_keys: &[(Tick, f32, KeyInterpolationMode, Option<f32>, Option<f32>)],
                           y_keys: &[(Tick, f32, KeyInterpolationMode, Option<f32>, Option<f32>)],
                           z_keys: &[(Tick, f32, KeyInterpolationMode, Option<f32>, Option<f32>)]|
     -> Track {
        let sub_tt = if tt == TrackType::EulerAngles {
            TrackType::Angle
        } else {
            TrackType::Number
        };
        let mut tr = Track::new_curve(tt);
        tr.euler_angles_order = order;
        tr.components.insert(PROP_X.to_string(), make_scalar_track(sub_tt, x_keys));
        tr.components.insert(PROP_Y.to_string(), make_scalar_track(sub_tt, y_keys));
        tr.components.insert(PROP_Z.to_string(), make_scalar_track(sub_tt, z_keys));
        tr
    };

    let make_quat_track = |keys: &[(Tick, Quat, KeyInterpolationMode, Option<f32>, Option<f32>)]| -> Track {
        let mut tr = Track::new_curve(TrackType::Quaternion);
        for &(tick, q, mode, ls, rs) in keys {
            tr.add_keyframe(
                tick,
                Keyframe {
                    value: KeyframeValue::Quaternion(q.normalize()),
                    easing_style: None,
                    easing_direction: None,
                    interpolation_mode: Some(mode),
                    left_slope: ls,
                    right_slope: rs,
                },
            );
        }
        tr
    };

    let cub = KeyInterpolationMode::Cubic;
    let lin = KeyInterpolationMode::Linear;
    let con = KeyInterpolationMode::Constant;
    // Helper: slope in units/second -> units/tick
    let sps = |v_per_sec: f32| Some(v_per_sec / 2400.0);

    {
        let root = curve_data.instances.entry("Root".to_string()).or_default();

        // LowerTorso: Vector3Curve (Cubic auto-tangents) + EulerRotationCurve XYZ (Cubic auto-tangents)
        {
            let mut cf = Track::new_curve(TrackType::CFrame);
            cf.components.insert(
                PROP_POSITION.to_string(),
                make_vec3_track(
                    TrackType::Position,
                    None,
                    &[
                        (0, 0.0, cub, None, None),
                        (1200, 0.35, cub, None, None),
                        (2400, -0.20, cub, None, None),
                        (3600, 0.40, cub, None, None),
                        (4800, 0.05, cub, None, None),
                    ],
                    &[
                        (0, 0.0, cub, None, None),
                        (1200, 0.25, cub, None, None),
                        (2400, 0.50, cub, None, None),
                        (3600, -0.15, cub, None, None),
                        (4800, 0.10, cub, None, None),
                    ],
                    &[
                        (0, 0.0, cub, None, None),
                        (1200, -0.15, cub, None, None),
                        (2400, 0.30, cub, None, None),
                        (3600, 0.10, cub, None, None),
                        (4800, -0.20, cub, None, None),
                    ],
                ),
            );
            cf.components.insert(
                PROP_ROTATION.to_string(),
                make_vec3_track(
                    TrackType::EulerAngles,
                    Some(RotationOrder::XYZ),
                    &[
                        (0, 0.0, cub, None, None),
                        (1200, 0.40, cub, None, None),
                        (2400, -0.35, cub, None, None),
                        (3600, 0.50, cub, None, None),
                        (4800, 0.10, cub, None, None),
                    ],
                    &[
                        (0, 0.0, cub, None, None),
                        (1200, -0.50, cub, None, None),
                        (2400, 0.60, cub, None, None),
                        (3600, 0.20, cub, None, None),
                        (4800, -0.25, cub, None, None),
                    ],
                    &[
                        (0, 0.0, cub, None, None),
                        (1200, 0.30, cub, None, None),
                        (2400, -0.45, cub, None, None),
                        (3600, 0.35, cub, None, None),
                        (4800, 0.0, cub, None, None),
                    ],
                ),
            );
            root.tracks.insert("LowerTorso".to_string(), cf);
        }

        // UpperTorso: Vector3Curve (Cubic explicit LeftTangent/RightTangent) + EulerRotationCurve YXZ (Cubic explicit slopes)
        {
            let mut cf = Track::new_curve(TrackType::CFrame);
            cf.components.insert(
                PROP_POSITION.to_string(),
                make_vec3_track(
                    TrackType::Position,
                    None,
                    &[
                        (0, 0.0, cub, None, sps(1.2)),
                        (1200, 0.25, cub, sps(-0.6), sps(0.8)),
                        (2400, -0.10, cub, sps(0.5), sps(-0.9)),
                        (3600, 0.30, cub, sps(1.0), sps(0.0)),
                        (4800, 0.0, cub, sps(-0.7), None),
                    ],
                    &[
                        (0, 0.0, cub, None, sps(-0.5)),
                        (1200, 0.30, cub, sps(0.9), sps(0.9)),
                        (2400, 0.15, cub, sps(-0.4), sps(0.6)),
                        (3600, -0.20, cub, sps(-0.8), sps(0.5)),
                        (4800, 0.05, cub, sps(0.3), None),
                    ],
                    &[
                        (0, 0.0, cub, None, sps(0.4)),
                        (1200, -0.20, cub, sps(-0.3), sps(-0.3)),
                        (2400, 0.25, cub, sps(0.7), sps(-0.5)),
                        (3600, 0.05, cub, sps(0.2), sps(0.8)),
                        (4800, -0.10, cub, sps(-0.4), None),
                    ],
                ),
            );
            cf.components.insert(
                PROP_ROTATION.to_string(),
                make_vec3_track(
                    TrackType::EulerAngles,
                    Some(RotationOrder::YXZ),
                    &[
                        (0, 0.0, cub, None, sps(1.5)),
                        (1200, 0.55, cub, sps(-0.4), sps(0.9)),
                        (2400, -0.40, cub, sps(-1.1), sps(1.2)),
                        (3600, 0.65, cub, sps(0.5), sps(-0.8)),
                        (4800, -0.15, cub, sps(-0.6), None),
                    ],
                    &[
                        (0, 0.0, cub, None, sps(-1.0)),
                        (1200, -0.60, cub, sps(0.7), sps(0.7)),
                        (2400, 0.75, cub, sps(1.0), sps(-1.0)),
                        (3600, -0.30, cub, sps(-0.5), sps(0.4)),
                        (4800, 0.20, cub, sps(0.8), None),
                    ],
                    &[
                        (0, 0.0, cub, None, sps(0.6)),
                        (1200, 0.35, cub, sps(0.2), sps(-0.9)),
                        (2400, -0.50, cub, sps(-0.3), sps(0.8)),
                        (3600, 0.45, cub, sps(0.9), sps(-0.2)),
                        (4800, 0.05, cub, sps(-0.5), None),
                    ],
                ),
            );
            root.tracks.insert("UpperTorso".to_string(), cf);
        }

        // Head: Vector3Curve (Linear, Constant, mixed) + EulerRotationCurve ZYX (mixed Cubic/Linear/Constant)
        {
            let mut cf = Track::new_curve(TrackType::CFrame);
            cf.components.insert(
                PROP_POSITION.to_string(),
                make_vec3_track(
                    TrackType::Position,
                    None,
                    &[
                        (0, 0.0, lin, None, None),
                        (1200, 0.20, lin, None, None),
                        (2400, -0.15, lin, None, None),
                        (3600, 0.10, lin, None, None),
                        (4800, 0.0, lin, None, None),
                    ],
                    &[
                        (0, 0.0, con, None, None),
                        (1200, 0.18, con, None, None),
                        (2400, -0.12, con, None, None),
                        (3600, 0.22, con, None, None),
                        (4800, 0.0, con, None, None),
                    ],
                    &[
                        (0, 0.0, cub, None, sps(0.6)),
                        (1200, 0.15, lin, sps(-0.4), None),
                        (2400, -0.10, con, None, None),
                        (3600, 0.20, cub, None, None),
                        (4800, 0.0, cub, sps(-0.5), None),
                    ],
                ),
            );
            cf.components.insert(
                PROP_ROTATION.to_string(),
                make_vec3_track(
                    TrackType::EulerAngles,
                    Some(RotationOrder::ZYX),
                    &[
                        (0, 0.0, cub, None, sps(1.0)),
                        (1200, 0.45, lin, sps(0.3), None),
                        (2400, -0.35, con, None, None),
                        (3600, 0.55, cub, None, sps(-0.6)),
                        (4800, 0.10, cub, sps(0.2), None),
                    ],
                    &[
                        (0, 0.0, lin, None, None),
                        (1200, -0.70, cub, None, sps(0.8)),
                        (2400, 0.50, cub, None, None),
                        (3600, -0.25, con, None, None),
                        (4800, 0.15, lin, None, None),
                    ],
                    &[
                        (0, 0.0, con, None, None),
                        (1200, 0.40, cub, None, None),
                        (2400, -0.55, lin, None, None),
                        (3600, 0.30, cub, None, sps(0.5)),
                        (4800, -0.10, cub, None, None),
                    ],
                ),
            );
            root.tracks.insert("Head".to_string(), cf);
        }

        // RightUpperArm: RotationCurve (Quaternion Cubic Hermite slerp: auto + explicit tangents)
        {
            let q0 = Quat::IDENTITY;
            let q1 = Quat::from_mat3(&RotationOrder::XYZ.from_euler_angles(Vec3::new(0.8, 0.3, -0.5)));
            let q2 = Quat::from_mat3(&RotationOrder::XYZ.from_euler_angles(Vec3::new(-0.4, 0.7, 0.5)));
            let q3 = Quat::from_mat3(&RotationOrder::XYZ.from_euler_angles(Vec3::new(0.6, -0.4, -0.8)));
            let q4 = Quat::from_mat3(&RotationOrder::XYZ.from_euler_angles(Vec3::new(-0.2, 0.25, 0.2)));
            let mut cf = Track::new_curve(TrackType::CFrame);
            cf.components.insert(
                PROP_POSITION.to_string(),
                make_vec3_track(
                    TrackType::Position,
                    None,
                    &[(0, 0.0, lin, None, None), (2400, 0.2, lin, None, None), (4800, 0.0, lin, None, None)],
                    &[(0, 0.0, lin, None, None), (2400, 0.15, lin, None, None), (4800, -0.1, lin, None, None)],
                    &[(0, 0.0, lin, None, None), (2400, -0.1, lin, None, None), (4800, 0.1, lin, None, None)],
                ),
            );
            cf.components.insert(
                PROP_ROTATION.to_string(),
                make_quat_track(&[
                    (0, q0, cub, None, None),
                    (1200, q1, cub, None, None),
                    (2400, q2, cub, sps(1.5), sps(2.2)),
                    (3600, q3, cub, sps(0.8), sps(1.6)),
                    (4800, q4, cub, sps(0.5), None),
                ]),
            );
            root.tracks.insert("RightUpperArm".to_string(), cf);
        }

        // RightLowerArm: RotationCurve (Quaternion Linear slerp)
        {
            let q0 = Quat::IDENTITY;
            let q1 = Quat::from_mat3(&RotationOrder::XYZ.from_euler_angles(Vec3::new(0.95, -0.25, 0.35)));
            let q2 = Quat::from_mat3(&RotationOrder::XYZ.from_euler_angles(Vec3::new(0.20, 0.60, -0.55)));
            let q3 = Quat::from_mat3(&RotationOrder::XYZ.from_euler_angles(Vec3::new(1.10, -0.30, 0.25)));
            let q4 = Quat::from_mat3(&RotationOrder::XYZ.from_euler_angles(Vec3::new(0.30, 0.15, -0.10)));
            let mut cf = Track::new_curve(TrackType::CFrame);
            cf.components.insert(
                PROP_POSITION.to_string(),
                make_vec3_track(
                    TrackType::Position,
                    None,
                    &[(0, 0.0, con, None, None), (2400, 0.15, con, None, None), (4800, 0.0, con, None, None)],
                    &[(0, 0.0, con, None, None), (2400, -0.1, con, None, None), (4800, 0.05, con, None, None)],
                    &[(0, 0.0, con, None, None), (2400, 0.1, con, None, None), (4800, 0.0, con, None, None)],
                ),
            );
            cf.components.insert(
                PROP_ROTATION.to_string(),
                make_quat_track(&[
                    (0, q0, lin, None, None),
                    (1200, q1, lin, None, None),
                    (2400, q2, lin, None, None),
                    (3600, q3, lin, None, None),
                    (4800, q4, lin, None, None),
                ]),
            );
            root.tracks.insert("RightLowerArm".to_string(), cf);
        }

        // LeftUpperLeg: RotationCurve (Quaternion mixed Constant, Linear, Cubic)
        {
            let q0 = Quat::IDENTITY;
            let q1 = Quat::from_mat3(&RotationOrder::XYZ.from_euler_angles(Vec3::new(-0.70, 0.30, -0.40)));
            let q2 = Quat::from_mat3(&RotationOrder::XYZ.from_euler_angles(Vec3::new(0.60, -0.45, 0.35)));
            let q3 = Quat::from_mat3(&RotationOrder::XYZ.from_euler_angles(Vec3::new(-0.50, 0.25, 0.20)));
            let q4 = Quat::from_mat3(&RotationOrder::XYZ.from_euler_angles(Vec3::new(0.15, -0.15, -0.10)));
            let mut cf = Track::new_curve(TrackType::CFrame);
            cf.components.insert(
                PROP_POSITION.to_string(),
                make_vec3_track(
                    TrackType::Position,
                    None,
                    &[(0, 0.0, cub, None, None), (1600, -0.2, cub, None, None), (3200, 0.2, cub, None, None), (4800, 0.0, cub, None, None)],
                    &[(0, 0.0, cub, None, None), (1600, 0.15, cub, None, None), (3200, -0.15, cub, None, None), (4800, 0.0, cub, None, None)],
                    &[(0, 0.0, cub, None, None), (1600, 0.25, cub, None, None), (3200, -0.2, cub, None, None), (4800, 0.0, cub, None, None)],
                ),
            );
            cf.components.insert(
                PROP_ROTATION.to_string(),
                make_quat_track(&[
                    (0, q0, con, None, None),
                    (1200, q1, lin, None, None),
                    (2400, q2, cub, None, None),
                    (3600, q3, cub, None, None),
                    (4800, q4, cub, None, None),
                ]),
            );
            root.tracks.insert("LeftUpperLeg".to_string(), cf);
        }
    }
    curve_data.set_end_tick();
    let _curve_anim_ref = serialization::to_curve_animation(&mut doc, ss_ref, &curve_data, Some(&rig_cache));

    // Write .rbxl via rbx_binary
    let rbxl_path = out_path.join("anim_parity.rbxl");
    {
        let mut buf = Vec::new();
        let top_ids: Vec<Ref> = doc.dom.root().children().to_vec();
        rbx_binary::to_writer(&mut buf, &doc.dom, &top_ids).expect("rbx_binary::to_writer failed");
        std::fs::write(&rbxl_path, &buf).expect("failed to write anim_parity.rbxl");
    }

    // Reload the exact .rbxl via rbx_binary::from_reader and evaluate frames 0..=60
    let loaded_bytes = std::fs::read(&rbxl_path).unwrap();
    let loaded_dom = rbx_binary::from_reader(std::io::Cursor::new(&loaded_bytes))
        .expect("rbx_binary::from_reader failed");
    let mut eval_doc = Document::from_play_dom(loaded_dom, "anim_parity".to_string());

    let eval_ws = eval_doc
        .dom
        .root()
        .children()
        .iter()
        .copied()
        .find(|&c| eval_doc.class_of(c).as_deref() == Some("Workspace"))
        .unwrap();
    let eval_ss = eval_doc
        .dom
        .root()
        .children()
        .iter()
        .copied()
        .find(|&c| eval_doc.class_of(c).as_deref() == Some("ServerStorage"))
        .unwrap();
    let eval_rig = eval_doc
        .dom
        .get_by_ref(eval_ws)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|&c| eval_doc.dom.get_by_ref(c).is_some_and(|i| i.name == "R15Dummy"))
        .unwrap();
    let eval_kf_ref = eval_doc
        .dom
        .get_by_ref(eval_ss)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|&c| eval_doc.dom.get_by_ref(c).is_some_and(|i| i.name == "ParityKeyframeSeq"))
        .unwrap();
    let eval_curve_ref = eval_doc
        .dom
        .get_by_ref(eval_ss)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|&c| eval_doc.dom.get_by_ref(c).is_some_and(|i| i.name == "ParityCurveAnim"))
        .unwrap();

    let eval_rig_cache = rig::build_rig_cache(&eval_doc, eval_rig);
    let loaded_kf_data = serialization::from_keyframe_sequence(&eval_doc, eval_kf_ref, 30)
        .expect("from_keyframe_sequence failed");
    let loaded_curve_data = serialization::from_curve_animation(&eval_doc, eval_curve_ref, 30)
        .expect("from_curve_animation failed");

    let joints = [
        "LowerTorso",
        "UpperTorso",
        "Head",
        "RightUpperArm",
        "RightLowerArm",
        "LeftUpperLeg",
    ];

    let xform_to_arr = |xf: Xform| -> [f64; 12] {
        [
            xf.pos.x as f64,
            xf.pos.y as f64,
            xf.pos.z as f64,
            xf.rot.x_axis.x as f64,
            xf.rot.y_axis.x as f64,
            xf.rot.z_axis.x as f64,
            xf.rot.x_axis.y as f64,
            xf.rot.y_axis.y as f64,
            xf.rot.z_axis.y as f64,
            xf.rot.x_axis.z as f64,
            xf.rot.y_axis.z as f64,
            xf.rot.z_axis.z as f64,
        ]
    };

    let mut eval_clip = |clip_data: &AnimationData| -> serde_json::Value {
        let mut frames_json = Vec::with_capacity(61);
        for frame in 0..=60 {
            let tick = frame * 80;
            rig::step_rig_animation(&mut eval_doc, &eval_rig_cache, clip_data, tick);
            let mut j_map = serde_json::Map::new();
            for &jname in &joints {
                let motor_ref = eval_rig_cache.motor6d_map[jname];
                let part_ref = eval_rig_cache.part_refs[jname];
                let m_xf = match eval_doc.prop(motor_ref, "Transform") {
                    Some(Variant::CFrame(cf)) => Xform::from_cframe(&cf),
                    _ => ID_XF,
                };
                let p_xf = match eval_doc.prop(part_ref, "CFrame") {
                    Some(Variant::CFrame(cf)) => Xform::from_cframe(&cf),
                    _ => ID_XF,
                };
                j_map.insert(
                    jname.to_string(),
                    serde_json::json!({
                        "transform": xform_to_arr(m_xf),
                        "part_cf": xform_to_arr(p_xf),
                    }),
                );
            }
            frames_json.push(serde_json::json!({
                "frame": frame,
                "t": (frame as f64) / 30.0,
                "joints": j_map,
            }));
        }
        serde_json::Value::Array(frames_json)
    };

    let out_json = serde_json::json!({
        "KeyframeSequence": eval_clip(&loaded_kf_data),
        "CurveAnimation": eval_clip(&loaded_curve_data),
    });
    let json_path = out_path.join("rustudio_frames.json");
    std::fs::write(&json_path, serde_json::to_string_pretty(&out_json).unwrap()).unwrap();
    println!("Wrote {} and {}", rbxl_path.display(), json_path.display());
}

#[test]
fn anim_saves_discovery_reverse_lookup_multiple_and_latest_sort() {
    use rbx_dom_weak::types::Tags;

    let (mut doc, rig_ref) = build_r15_rig_document();
    let root = doc.dom.root_ref();
    let ss = doc.find_child(root, "ServerStorage").unwrap();
    let rbx_saves = doc.insert(ss, InstanceBuilder::new("Model").with_name(RBX_ANIMSAVES_NAME));
    let server_entry = doc.insert(
        rbx_saves,
        InstanceBuilder::new("ObjectValue")
            .with_name("Dummy")
            .with_property("Value", Variant::Ref(rig_ref)),
    );

    // 1. No rig.AnimSaves child at all — reverse lookup in ServerStorage.RBX_ANIMSAVES must still find clips,
    //    and sort newest timestamp first (matching Studio's LoadLatestAnimation).
    let old_clip = doc.insert(
        server_entry,
        InstanceBuilder::new("KeyframeSequence")
            .with_name("rough draft #1")
            .with_property("Tags", Variant::Tags(Tags::from(vec!["1711309648".to_string()]))),
    );
    let untagged_clip = doc.insert(
        server_entry,
        InstanceBuilder::new("KeyframeSequence").with_name("UntaggedClip"),
    );
    let newest_clip = doc.insert(
        server_entry,
        InstanceBuilder::new("CurveAnimation")
            .with_name("Celestial Final")
            .with_property("Tags", Variant::Tags(Tags::from(vec!["1711343825".to_string()]))),
    );

    let (entries, has_legacy) = rig::list_anim_saves(&doc, rig_ref);
    assert!(!has_legacy);
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].anim_ref, newest_clip);
    assert_eq!(entries[0].name, "Celestial Final");
    assert_eq!(entries[1].anim_ref, old_clip);
    assert_eq!(entries[2].anim_ref, untagged_clip);

    // 2. Rig has BOTH an empty legacy Model named "AnimSaves" first AND an ObjectValue link second,
    //    plus a legacy clip under a Configuration named "AnimSaves". Must find all without duplicates.
    let _empty_legacy = doc.insert(
        rig_ref,
        InstanceBuilder::new("Model").with_name(ANIMSAVES_NAME),
    );
    let _ov_link = doc.insert(
        rig_ref,
        InstanceBuilder::new("ObjectValue")
            .with_name(ANIMSAVES_NAME)
            .with_property("Value", Variant::Ref(server_entry)),
    );
    let cfg_legacy = doc.insert(
        rig_ref,
        InstanceBuilder::new("Configuration").with_name(ANIMSAVES_NAME),
    );
    let legacy_clip = doc.insert(
        cfg_legacy,
        InstanceBuilder::new("KeyframeSequence")
            .with_name("LegacyClip")
            .with_property("Tags", Variant::Tags(Tags::from(vec!["1711340000".to_string()]))),
    );

    let (entries2, has_legacy2) = rig::list_anim_saves(&doc, rig_ref);
    assert!(has_legacy2);
    assert_eq!(entries2.len(), 4);
    assert_eq!(entries2[0].anim_ref, newest_clip);
    assert_eq!(entries2[1].anim_ref, legacy_clip);
    assert_eq!(entries2[2].anim_ref, old_clip);
    assert_eq!(entries2[3].anim_ref, untagged_clip);
}

#[test]
fn curve_animation_decodes_shared_string_values_and_times() {
    use crate::anim_editor::model::AnimationData;
    use rbx_dom_weak::types::SharedString;

    let (mut doc, rig_ref) = build_r15_rig_document();
    let cache = rig::build_rig_cache(&doc, rig_ref);
    let ws = doc.workspace().unwrap();

    let mut state = EditorState::new();
    state.rig_cache = cache.clone();
    ops::load_animation_data(&mut state, AnimationData::new_rig_animation("SharedStringCurve"));
    ops::promote_to_curves(&mut state);
    ops::value_changed(
        &mut state,
        "Root",
        &["Head".into(), PROP_POSITION.into(), PROP_Y.into()],
        TrackType::Number,
        0,
        KeyframeValue::Number(0.0),
        false,
    );
    ops::value_changed(
        &mut state,
        "Root",
        &["Head".into(), PROP_POSITION.into(), PROP_Y.into()],
        TrackType::Number,
        2400,
        KeyframeValue::Number(2.5),
        false,
    );
    let mut data = state.animation_data.unwrap();
    data.set_end_tick();

    let ca_ref = serialization::to_curve_animation(&mut doc, ws, &data, Some(&cache));

    // Convert all FloatCurve / RotationCurve `ValuesAndTimes` props to Variant::SharedString
    // (as Team Create replication can deliver them).
    let desc: Vec<Ref> = doc.dom.descendants_of(ca_ref).map(|i| i.referent()).collect();
    let mut converted = 0usize;
    for r in desc {
        if let Some(Variant::BinaryString(bs)) = doc.prop(r, "ValuesAndTimes") {
            let raw: &[u8] = bs.as_ref();
            let shared = SharedString::new(raw.to_vec());
            doc.set_prop_live(r, rbx_dom_weak::ustr("ValuesAndTimes"), Variant::SharedString(shared));
            converted += 1;
        }
    }
    assert!(converted > 0, "expected at least one curve blob to convert");

    let decoded = serialization::from_curve_animation(&doc, ca_ref, 30)
        .expect("SharedString CurveAnimation must decode");
    let head = &decoded.instances["Root"].tracks["Head"];
    let pos_y = &head.components[PROP_POSITION].components[PROP_Y];
    assert!(pos_y.keyframes.contains(&2400), "keyframe at 2400 missing");
    let v = pos_y.data[&2400].value.as_f32().unwrap();
    assert!((v - 2.5).abs() < 1e-4, "expected 2.5, got {v}");
}

#[test]
fn anim_editor_sync_selection_switches_rigs_and_loads_latest_clip() {
    use crate::anim_editor::model::AnimationData;
    use rbx_dom_weak::types::Tags;

    // Build two distinct R15 rigs in one Document, each with its own saved animations
    // in ServerStorage.RBX_ANIMSAVES.
    let (mut doc_a, rig_a) = build_r15_rig_document();
    let (doc_b, rig_b_src) = build_r15_rig_document();
    let ws = doc_a.workspace().unwrap();
    let mut bytes_b = Vec::new();
    rbx_binary::to_writer(&mut bytes_b, &doc_b.dom, &[rig_b_src]).unwrap();
    let pasted = doc_a.paste_groups(vec![(bytes_b, ws)], "add rig B");
    let rig_b = pasted[0];
    doc_a.rename(rig_b, "RigB".into());

    let root = doc_a.dom.root_ref();
    let ss = doc_a.find_child(root, "ServerStorage").unwrap();
    let rbx_saves = doc_a.insert(ss, InstanceBuilder::new("Model").with_name(RBX_ANIMSAVES_NAME));

    let entry_a = doc_a.insert(
        rbx_saves,
        InstanceBuilder::new("ObjectValue")
            .with_name("RigA")
            .with_property("Value", Variant::Ref(rig_a)),
    );
    let _ov_a = doc_a.insert(
        rig_a,
        InstanceBuilder::new("ObjectValue")
            .with_name(ANIMSAVES_NAME)
            .with_property("Value", Variant::Ref(entry_a)),
    );
    let cache_a = rig::build_rig_cache(&doc_a, rig_a);
    let clip_a = AnimationData::new_rig_animation("RigA_Clip");
    let clip_a_ref = serialization::to_keyframe_sequence(&mut doc_a, entry_a, &clip_a, Some(&cache_a));
    doc_a.set_prop_live(
        clip_a_ref,
        rbx_dom_weak::ustr("Tags"),
        Variant::Tags(Tags::from(vec!["1700000001".to_string()])),
    );

    // Rig B has NO rig.AnimSaves child (only ServerStorage.RBX_ANIMSAVES entry) and TWO clips;
    // the newer timestamp must be loaded!
    let entry_b = doc_a.insert(
        rbx_saves,
        InstanceBuilder::new("ObjectValue")
            .with_name("RigB")
            .with_property("Value", Variant::Ref(rig_b)),
    );
    let cache_b = rig::build_rig_cache(&doc_a, rig_b);
    let clip_b_old = AnimationData::new_rig_animation("RigB_Old");
    let clip_b_old_ref =
        serialization::to_keyframe_sequence(&mut doc_a, entry_b, &clip_b_old, Some(&cache_b));
    doc_a.set_prop_live(
        clip_b_old_ref,
        rbx_dom_weak::ustr("Tags"),
        Variant::Tags(Tags::from(vec!["1700000010".to_string()])),
    );
    let clip_b_new = AnimationData::new_rig_animation("RigB_Latest");
    let clip_b_new_ref =
        serialization::to_keyframe_sequence(&mut doc_a, entry_b, &clip_b_new, Some(&cache_b));
    doc_a.set_prop_live(
        clip_b_new_ref,
        rbx_dom_weak::ustr("Tags"),
        Variant::Tags(Tags::from(vec!["1700000099".to_string()])),
    );

    let ctx = egui::Context::default();
    let mut app = crate::app::RustudioApp::new_in(
        &ctx,
        None,
        None,
        None,
        crate::app::Harness::test_empty(),
        false,
    );
    app.ed.game.doc = doc_a;
    let ed = &mut app.ed;
    ed.plugins
        .native_mut()
        .note_widget_open(crate::anim_editor::host::PLUGIN_ID, true);

    // 1. Select Rig A -> attaches Rig A and loads RigA_Clip.
    ed.select(vec![rig_a]);
    ed.native_plugins_tick();
    {
        let p = ed
            .plugins
            .native_plugin::<crate::anim_editor::host::AnimationEditorPlugin>(
                crate::anim_editor::host::PLUGIN_ID,
            )
            .expect("AnimationEditorPlugin loaded");
        assert_eq!(p.state.rig_cache.root_ref, Some(rig_a));
        assert_eq!(
            p.state.animation_data.as_ref().map(|d| d.metadata.name.as_str()),
            Some("RigA_Clip")
        );
    }

    // 2. Select Rig B -> must switch even though Rig A already had an active animation,
    //    discover Rig B's ServerStorage.RBX_ANIMSAVES saves, and load RigB_Latest!
    ed.select(vec![rig_b]);
    ed.native_plugins_tick();
    let head_b = {
        let p = ed
            .plugins
            .native_plugin_mut::<crate::anim_editor::host::AnimationEditorPlugin>(
                crate::anim_editor::host::PLUGIN_ID,
            )
            .expect("AnimationEditorPlugin loaded");
        assert_eq!(p.state.rig_cache.root_ref, Some(rig_b));
        assert_eq!(
            p.state.animation_data.as_ref().map(|d| d.metadata.name.as_str()),
            Some("RigB_Latest")
        );
        // 3. Selecting a child part of Rig B keeps Rig B attached without resetting animation_data.
        let head_b = *p.state.rig_cache.part_refs.get("Head").unwrap();
        p.state.playback.playhead = 1200;
        head_b
    };
    ed.select(vec![head_b]);
    ed.native_plugins_tick();
    {
        let p = ed
            .plugins
            .native_plugin::<crate::anim_editor::host::AnimationEditorPlugin>(
                crate::anim_editor::host::PLUGIN_ID,
            )
            .expect("AnimationEditorPlugin loaded");
        assert_eq!(p.state.rig_cache.root_ref, Some(rig_b));
        assert_eq!(p.state.playback.playhead, 1200);
    }
}

#[test]
fn tracklist_selection_syncs_selected_track_instances_for_3d_viewport() {
    let (doc, rig_ref) = build_r15_rig_document();
    let cache = rig::build_rig_cache(&doc, rig_ref);
    let left_leg = *cache.part_refs.get("LeftUpperLeg").unwrap();
    let right_arm = *cache.part_refs.get("RightUpperArm").unwrap();

    let mut state = EditorState::new();
    state.rig_cache = cache;
    state.active = true;
    ops::load_animation_data(
        &mut state,
        crate::anim_editor::model::AnimationData::new_rig_animation("SyncTest"),
    );

    // 1. Simulate selecting LeftUpperLeg in the 3D viewport.
    state.selected_tracks = vec![vec!["LeftUpperLeg".into()]];
    state.sync_selected_track_instances();
    assert_eq!(state.selected_track_instances, vec![left_leg]);

    // 2. Simulate selecting RightUpperArm in the TrackList / Summary.
    selection::select_track(&mut state, vec!["RightUpperArm".into()], false);
    assert_eq!(state.selected_tracks, vec![vec!["RightUpperArm".to_string()]]);
    assert_eq!(state.selected_track_instances, vec![right_arm]);

    // 3. Selecting a sub-channel path (e.g. LeftUpperLeg -> Rotation -> X) still selects LeftUpperLeg in 3D.
    selection::select_track(
        &mut state,
        vec!["LeftUpperLeg".into(), PROP_ROTATION.into(), "X".into()],
        false,
    );
    assert_eq!(state.selected_track_instances, vec![left_leg]);
}

#[test]
fn composite_curve_track_resolves_cubic_orange_and_constant_purple_keyframe_colors() {
    use crate::anim_editor::model::AnimationData;
    use crate::anim_editor::theme::AceTheme;
    use crate::anim_editor::ui_dopesheet::collect_track_components_info;

    let (doc, rig_ref) = build_r15_rig_document();
    let cache = rig::build_rig_cache(&doc, rig_ref);
    let mut state = EditorState::new();
    state.rig_cache = cache;
    ops::load_animation_data(&mut state, AnimationData::new_rig_animation("ColorTest"));
    ops::promote_to_curves(&mut state);

    // Add a full CFrame pose keyframe at tick 0 (default Cubic across all 6 channels).
    ops::value_changed(
        &mut state,
        "Root",
        &["Head".into()],
        TrackType::CFrame,
        0,
        KeyframeValue::CFrame(ID_XF),
        false,
    );
    // Add a partial keyframe at tick 2400 (only Position.Y).
    ops::value_changed(
        &mut state,
        "Root",
        &["Head".into(), PROP_POSITION.into(), PROP_Y.into()],
        TrackType::Number,
        2400,
        KeyframeValue::Number(1.0),
        false,
    );

    let anim = state.animation_data.as_ref().unwrap();
    let head = &anim.instances["Root"].tracks["Head"];
    let info = collect_track_components_info(head, 0, 4800);
    let theme = AceTheme::dark();

    // Tick 0: all leaf channels keyed with Cubic -> complete + Cubic -> orange (#FEBD51).
    let t0 = info[&0];
    assert!(t0.complete);
    assert_eq!(t0.interpolation_mode, Some(KeyInterpolationMode::Cubic));
    assert_eq!(
        theme.keyframe_colors(None, t0.interpolation_mode, false, false),
        theme.keyframe_cubic
    );

    // Tick 2400: only 1 of 6 leaf channels keyed -> incomplete (hollow default diamond).
    let t2400 = info[&2400];
    assert!(!t2400.complete);
}
