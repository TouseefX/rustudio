//! Core editing operations & thunks for `AnimationClipEditor`
//! (derived from `Thunks/*` and `Thunks/Selection/*`).

use std::collections::{BTreeMap, BTreeSet};

use glam::{Quat, Vec3};

use crate::anim_editor::constants::{
    MAX_ANIMATION_LENGTH, PROP_POSITION, PROP_ROTATION, PROP_X, PROP_Y, PROP_Z,
    SNAP_TO_KEYFRAME_THRESHOLD, TICK_FREQUENCY,
};
use crate::anim_editor::events::refresh_available_events;
use crate::anim_editor::history::{add_waypoint, clear_history};
use crate::anim_editor::model::{
    clamp_tick, nearest_frame, unwrap_euler, AnimationData, KeyInterpolationMode, Keyframe,
    KeyframePatch, KeyframeValue, RotationOrder, Tick, Track, TrackType,
};
use crate::anim_editor::selection::{
    clear_selection, collect_selected_leaf_ticks, prune_invalid_selection, rebuild_track_list,
};
use crate::anim_editor::state::{
    is_selection_empty, Clipboard, ClipboardKeyframeEntry, ClipboardType, DragContext, DragMode,
    EditorMode, EditorState, HistoryWaypoint, RightClickContextInfo,
    SelectionNode, SnapMode, TangentSide,
};

// ============================================================================
// ValueChanged & Keyframe Creation
// ============================================================================

/// Core `ValueChanged` thunk (`Thunks/ValueChanged` + `EditorController.onValueChanged`).
///
/// - Auto-creates the top-level track if missing (and inserts a default keyframe at
///   tick `0` whenever creating a first keyframe at `tick != 0`).
/// - In Channel/Curve mode, decomposes composite `CFrame`, `Position`, and `EulerAngles`
///   values into their leaf channels (`Position.{X,Y,Z}`, `Rotation.{X,Y,Z}` or `Quaternion`).
/// - Applies `Linear`/`In` for CFrame pose mode and `Cubic` for Curve/Channel mode.
/// - Updates `end_tick` and `editing_length` if `tick > end_tick`.
pub fn value_changed(
    state: &mut EditorState,
    instance: &str,
    path: &[String],
    track_type: TrackType,
    tick: Tick,
    value: KeyframeValue,
    add_waypoint_now: bool,
) {
    if state.read_only || path.is_empty() || state.animation_data.is_none() {
        return;
    }

    let tick = clamp_tick(tick);
    if add_waypoint_now {
        add_waypoint(state);
    }

    let is_channel = state.is_channel_animation();
    let default_rot_type = state.settings.default_rotation_type;
    let default_euler_order = state.settings.default_euler_angles_order;

    let mut track_created = false;

    if !is_channel {
        track_created |= apply_leaf_value(
            state,
            instance,
            path,
            track_type,
            default_rot_type,
            default_euler_order,
            tick,
            value,
        );
    } else {
        // Resolve rotation type and Euler order from existing top-level track if present.
        let (rot_type, euler_order) = state
            .animation_data
            .as_ref()
            .and_then(|anim| anim.get_track(instance, &path[..1]))
            .map(|top| {
                (
                    top.get_rotation_type().unwrap_or(default_rot_type),
                    top.get_euler_angles_order().unwrap_or(default_euler_order),
                )
            })
            .unwrap_or((default_rot_type, default_euler_order));

        match (track_type, value) {
            (TrackType::CFrame, KeyframeValue::CFrame(xf)) => {
                let axes = [(PROP_X, xf.pos.x), (PROP_Y, xf.pos.y), (PROP_Z, xf.pos.z)];
                for (axis, coord) in axes {
                    let mut p = path.to_vec();
                    p.push(PROP_POSITION.to_string());
                    p.push(axis.to_string());
                    track_created |= apply_leaf_value(
                        state,
                        instance,
                        &p,
                        TrackType::Number,
                        rot_type,
                        euler_order,
                        tick,
                        KeyframeValue::Number(coord),
                    );
                }

                if rot_type == TrackType::Quaternion {
                    let mut p = path.to_vec();
                    p.push(PROP_ROTATION.to_string());
                    let q = Quat::from_mat3(&xf.rot).normalize();
                    track_created |= apply_leaf_value(
                        state,
                        instance,
                        &p,
                        TrackType::Quaternion,
                        rot_type,
                        euler_order,
                        tick,
                        KeyframeValue::Quaternion(q),
                    );
                } else {
                    let raw_euler = euler_order.to_euler_angles(xf.rot);
                    let prev_euler = find_prev_euler_angles(state, instance, path, tick);
                    let euler = unwrap_euler(raw_euler, prev_euler, euler_order);
                    let rot_axes = [(PROP_X, euler.x), (PROP_Y, euler.y), (PROP_Z, euler.z)];
                    for (axis, angle) in rot_axes {
                        let mut p = path.to_vec();
                        p.push(PROP_ROTATION.to_string());
                        p.push(axis.to_string());
                        track_created |= apply_leaf_value(
                            state,
                            instance,
                            &p,
                            TrackType::Angle,
                            rot_type,
                            euler_order,
                            tick,
                            KeyframeValue::Angle(angle),
                        );
                    }
                }
            }
            (TrackType::Position, KeyframeValue::Position(v)) => {
                for (axis, coord) in [(PROP_X, v.x), (PROP_Y, v.y), (PROP_Z, v.z)] {
                    let mut p = path.to_vec();
                    p.push(axis.to_string());
                    track_created |= apply_leaf_value(
                        state,
                        instance,
                        &p,
                        TrackType::Number,
                        rot_type,
                        euler_order,
                        tick,
                        KeyframeValue::Number(coord),
                    );
                }
            }
            (TrackType::EulerAngles, KeyframeValue::Position(v)) => {
                for (axis, angle) in [(PROP_X, v.x), (PROP_Y, v.y), (PROP_Z, v.z)] {
                    let mut p = path.to_vec();
                    p.push(axis.to_string());
                    track_created |= apply_leaf_value(
                        state,
                        instance,
                        &p,
                        TrackType::Angle,
                        rot_type,
                        euler_order,
                        tick,
                        KeyframeValue::Angle(angle),
                    );
                }
            }
            _ => {
                track_created |= apply_leaf_value(
                    state,
                    instance,
                    path,
                    track_type,
                    rot_type,
                    euler_order,
                    tick,
                    value,
                );
            }
        }
    }

    finalize_animation_data_update(state);
    if track_created {
        rebuild_track_list(state);
    }
}

/// Adds a new top-level track to `instance`, committing an undo waypoint and rebuilding `tracks`.
pub fn add_track(state: &mut EditorState, instance: &str, name: &str, track_type: TrackType) {
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    if anim
        .instances
        .get(instance)
        .is_some_and(|inst| inst.tracks.contains_key(name))
    {
        return;
    }

    add_waypoint(state);
    let rot_type = state.settings.default_rotation_type;
    let euler_order = state.settings.default_euler_angles_order;

    if let Some(anim) = state.animation_data.as_mut() {
        anim.add_track(instance, name, track_type, rot_type, euler_order);
    }
    rebuild_track_list(state);
}

/// Deletes a top-level track from `instance`, committing an undo waypoint and pruning selection.
pub fn delete_track(state: &mut EditorState, instance: &str, name: &str) {
    let exists = state
        .animation_data
        .as_ref()
        .and_then(|anim| anim.instances.get(instance))
        .is_some_and(|inst| inst.tracks.contains_key(name));
    if !exists {
        return;
    }

    add_waypoint(state);
    if let Some(anim) = state.animation_data.as_mut() {
        anim.remove_track(instance, name);
    }
    finalize_animation_data_update(state);
    prune_invalid_selection(state);
    rebuild_track_list(state);
}

/// Clears all keyframes on the track (and its components) at `(instance, path)`.
pub fn clear_track(state: &mut EditorState, instance: &str, path: &[String]) {
    let exists = state
        .animation_data
        .as_ref()
        .is_some_and(|anim| anim.get_track(instance, path).is_some());
    if !exists {
        return;
    }

    add_waypoint(state);
    if let Some(anim) = state.animation_data.as_mut() {
        anim.clear_track(instance, path);
    }
    finalize_animation_data_update(state);
    prune_invalid_selection(state);
}

/// Inserts keyframes at `state.playback.playhead` on every leaf track under `(instance, path)`
/// that does not already have a keyframe at that tick.
pub fn add_keyframe_at_playhead(state: &mut EditorState, instance: &str, path: &[String]) {
    if state.read_only || path.is_empty() || state.animation_data.is_none() {
        return;
    }
    let tick = clamp_tick(state.playback.playhead);

    let has_track = state
        .animation_data
        .as_ref()
        .is_some_and(|anim| anim.get_track(instance, path).is_some());
    if !has_track {
        return;
    }

    add_waypoint(state);
    if let Some(anim) = state.animation_data.as_mut() {
        if let Some(track) = anim.get_track_mut(instance, path) {
            let mut base = path.to_vec();
            track.traverse_leaves_mut(&mut base, &mut |_leaf_path, leaf| {
                if !leaf.data.contains_key(&tick) {
                    let val = sample_leaf_linear(leaf, tick);
                    let kf = if leaf.is_curve_track {
                        Keyframe::new_curve(val)
                    } else {
                        Keyframe::new_pose(val)
                    };
                    leaf.add_keyframe(tick, kf);
                    if tick != 0 && !leaf.data.contains_key(&0) {
                        leaf.add_default_keyframe(0);
                    }
                }
            });
        }
    }
    finalize_animation_data_update(state);
}

// ============================================================================
// Keyframe Dragging & Scaling (`DragContext`, `MoveSelectedKeyframes`, `ScaleSelectedKeyframes`)
// ============================================================================

/// Captures a `DragContext` snapshot at the start of a keyframe move/scale or tangent drag.
pub fn begin_keyframe_drag(
    state: &mut EditorState,
    pivot_tick: Tick,
    pivot_value: Option<f32>,
    mode: DragMode,
) {
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    match mode {
        DragMode::Keyframe => {
            if is_selection_empty(&state.selected_keyframes) {
                return;
            }
            state.drag_context = Some(DragContext::new_keyframes(
                anim.clone(),
                state.selected_keyframes.clone(),
                pivot_tick,
                pivot_value,
            ));
        }
        DragMode::Tangent => {
            let instance = state
                .right_click_context
                .instance_name
                .clone()
                .unwrap_or_else(|| "Root".to_string());
            let path = state
                .right_click_context
                .path
                .clone()
                .or_else(|| state.last_selected_path.clone())
                .unwrap_or_default();
            let side = state
                .right_click_context
                .tangent_side
                .unwrap_or(TangentSide::Right);
            if let Some(track) = anim.get_track(&instance, &path) {
                if let Some(kf) = track.data.get(&pivot_tick) {
                    let val = kf.value.as_f32().unwrap_or(0.0);
                    state.drag_context = Some(DragContext::new_tangent(
                        instance,
                        path,
                        pivot_tick,
                        val,
                        side,
                        track.track_type,
                    ));
                }
            }
        }
    }
}

/// Moves all selected keyframes cleanly from the pre-drag `DragContext::Keyframes` snapshot
/// (`Thunks/Selection/MoveSelectedKeyframes`), with zero compounding drift.
pub fn update_keyframe_drag(state: &mut EditorState, target_tick: Tick, delta_value: Option<f32>) {
    let Some(DragContext::Keyframes {
        animation_data: orig_anim,
        selected_keyframes: orig_selected,
        pivot_tick,
        new_tick,
        pivot_value,
        new_value,
        earliest_tick,
        latest_tick,
        has_waypoint,
        ..
    }) = state.drag_context.as_mut()
    else {
        return;
    };

    *new_tick = target_tick;
    if let (Some(pv), Some(dv)) = (*pivot_value, delta_value) {
        *new_value = Some(pv + dv);
    }

    let moved_time = target_tick != *pivot_tick;
    let moved_val = delta_value.is_some_and(|dv| dv.abs() > 0.0);
    if (moved_time || moved_val) && !*has_waypoint {
        state.history.past.insert(
            0,
            HistoryWaypoint {
                animation_data: orig_anim.clone(),
                tracks: state.tracks.clone(),
            },
        );
        state.history.past.truncate(state.history.max_waypoint_size);
        state.history.future.clear();
        state.is_dirty = true;
        *has_waypoint = true;
    }

    let move_right = *pivot_tick < target_tick;
    let pivot = *pivot_tick;
    let earliest = *earliest_tick;
    let latest = *latest_tick;
    let snap_mode = state.settings.snap_mode;
    let frame_rate = state.frame_rate;

    let mut new_anim = orig_anim.clone();
    let mut new_selected = orig_selected.clone();
    let mut moved_named: Vec<(Tick, Tick)> = Vec::new();

    for (inst_name, sel_tracks) in &mut new_selected {
        let Some(inst) = new_anim.instances.get_mut(inst_name) else {
            continue;
        };
        for (track_name, sel_node) in sel_tracks {
            let Some(data_track) = inst.tracks.get_mut(track_name) else {
                continue;
            };
            move_selection_node_recursive(
                sel_node,
                data_track,
                pivot,
                target_tick,
                earliest,
                latest,
                move_right,
                snap_mode,
                frame_rate,
                delta_value,
                &mut moved_named,
            );
        }
    }

    for (old_t, new_t) in moved_named {
        new_anim.move_named_keyframe(old_t, new_t);
    }

    state.animation_data = Some(new_anim);
    state.selected_keyframes = new_selected;
    finalize_animation_data_update(state);
}

/// Scales all selected keyframes around `pivot_tick` from the pre-drag `DragContext::Keyframes`
/// snapshot using exact two-pass anti-overwrite ordering (`ScaleSelectedKeyframes`).
pub fn update_keyframe_scale(state: &mut EditorState, new_tick: Tick) {
    let Some(ctx) = state.drag_context.as_mut() else {
        return;
    };
    ctx.scale_keyframes(new_tick);

    let DragContext::Keyframes {
        animation_data: orig_anim,
        selected_keyframes: orig_selected,
        pivot_tick,
        scale,
        has_waypoint,
        ..
    } = ctx
    else {
        return;
    };

    if new_tick != *pivot_tick && !*has_waypoint {
        state.history.past.insert(
            0,
            HistoryWaypoint {
                animation_data: orig_anim.clone(),
                tracks: state.tracks.clone(),
            },
        );
        state.history.past.truncate(state.history.max_waypoint_size);
        state.history.future.clear();
        state.is_dirty = true;
        *has_waypoint = true;
    }

    let pivot = *pivot_tick;
    let scale_factor = *scale;
    let snap_mode = state.settings.snap_mode;
    let frame_rate = state.frame_rate;

    let mut new_anim = orig_anim.clone();
    let mut new_selected = orig_selected.clone();
    let mut moved_named: Vec<(Tick, Tick)> = Vec::new();

    for (inst_name, sel_tracks) in &mut new_selected {
        let Some(inst) = new_anim.instances.get_mut(inst_name) else {
            continue;
        };
        for (track_name, sel_node) in sel_tracks {
            let Some(data_track) = inst.tracks.get_mut(track_name) else {
                continue;
            };
            scale_selection_node_recursive(
                sel_node,
                data_track,
                pivot,
                scale_factor,
                snap_mode,
                frame_rate,
                &mut moved_named,
            );
        }
    }

    for (old_t, new_t) in moved_named {
        new_anim.move_named_keyframe(old_t, new_t);
    }

    state.animation_data = Some(new_anim);
    state.selected_keyframes = new_selected;
    finalize_animation_data_update(state);
}

/// Finishes an active drag operation. If `commit` is `false`, restores the pre-drag
/// snapshot and removes any drag waypoint that was pushed.
pub fn end_drag(state: &mut EditorState, commit: bool) {
    let Some(ctx) = state.drag_context.take() else {
        return;
    };

    if !commit {
        match ctx {
            DragContext::Keyframes {
                animation_data,
                selected_keyframes,
                has_waypoint,
                ..
            } => {
                state.animation_data = Some(animation_data);
                state.selected_keyframes = selected_keyframes;
                if has_waypoint && !state.history.past.is_empty() {
                    state.history.past.remove(0);
                }
            }
            DragContext::Events {
                animation_data,
                selected_events,
                has_waypoint,
                ..
            } => {
                state.animation_data = Some(animation_data);
                state.selected_events = selected_events;
                if has_waypoint && !state.history.past.is_empty() {
                    state.history.past.remove(0);
                }
            }
            DragContext::Tangent { has_waypoint, .. } => {
                if has_waypoint {
                    crate::anim_editor::history::undo(state);
                    state.history.future.clear();
                }
            }
        }
    }

    finalize_animation_data_update(state);
}

// ============================================================================
// Bulk Selection Editing & Clipboard
// ============================================================================

/// Deletes all selected keyframes (`DeleteSelectedKeyframes`).
/// Any top-level track that becomes completely empty after deletion is removed.
pub fn delete_selected_keyframes(state: &mut EditorState) {
    if is_selection_empty(&state.selected_keyframes) || state.animation_data.is_none() {
        return;
    }
    add_waypoint(state);

    let leaves = {
        let anim = state.animation_data.as_ref().unwrap();
        collect_selected_leaf_ticks(&state.selected_keyframes, anim)
    };

    let mut touched_top_tracks: BTreeSet<(String, String)> = BTreeSet::new();
    if let Some(anim) = state.animation_data.as_mut() {
        for (inst_name, path, ticks) in leaves {
            if let Some(top_name) = path.first() {
                touched_top_tracks.insert((inst_name.clone(), top_name.clone()));
            }
            if let Some(track) = anim.get_track_mut(&inst_name, &path) {
                for tick in ticks {
                    track.delete_keyframe(tick);
                }
            }
        }

        // Remove any touched top-level track whose remaining data is completely empty.
        for (inst_name, top_name) in touched_top_tracks {
            let should_remove = anim
                .instances
                .get(&inst_name)
                .and_then(|inst| inst.tracks.get(&top_name))
                .is_some_and(Track::is_empty);
            if should_remove {
                anim.remove_track(&inst_name, &top_name);
            }
        }
    }

    state.selected_keyframes.clear();
    finalize_animation_data_update(state);
    prune_invalid_selection(state);
    rebuild_track_list(state);
}

/// Resets the value of every selected keyframe to its track type's default value
/// (`ResetSelectedKeyframes`), keeping the current selection intact.
pub fn reset_selected_keyframes(state: &mut EditorState) {
    if is_selection_empty(&state.selected_keyframes) || state.animation_data.is_none() {
        return;
    }
    add_waypoint(state);

    let leaves = {
        let anim = state.animation_data.as_ref().unwrap();
        collect_selected_leaf_ticks(&state.selected_keyframes, anim)
    };

    if let Some(anim) = state.animation_data.as_mut() {
        for (inst_name, path, ticks) in leaves {
            if let Some(track) = anim.get_track_mut(&inst_name, &path) {
                let default_val = KeyframeValue::default_for(track.track_type);
                for tick in ticks {
                    if let Some(kf) = track.data.get_mut(&tick) {
                        kf.value = default_val;
                    }
                }
            }
        }
    }
}

/// Copies all selected keyframes into `state.clipboard` (`CopySelectedKeyframes`).
pub fn copy_selected_keyframes(state: &mut EditorState) {
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    if is_selection_empty(&state.selected_keyframes) {
        return;
    }

    let mut copied: BTreeMap<String, Vec<ClipboardKeyframeEntry>> = BTreeMap::new();
    let leaves = collect_selected_leaf_ticks(&state.selected_keyframes, anim);

    for (inst_name, path, ticks) in leaves {
        let Some(top_name) = path.first() else {
            continue;
        };
        let Some(top_track) = anim.get_track(&inst_name, &path[..1]) else {
            continue;
        };
        let Some(leaf_track) = anim.get_track(&inst_name, &path) else {
            continue;
        };

        let mut data = BTreeMap::new();
        for tick in ticks {
            if let Some(&kf) = leaf_track.data.get(&tick) {
                data.insert(tick, kf);
            }
        }
        if data.is_empty() {
            continue;
        }

        copied
            .entry(inst_name)
            .or_default()
            .push(ClipboardKeyframeEntry {
                top_track_name: top_name.clone(),
                top_track_type: top_track.track_type,
                rel_path: path[1..].to_vec(),
                track_type: leaf_track.track_type,
                rotation_type: top_track.get_rotation_type(),
                euler_angles_order: top_track.get_euler_angles_order(),
                data,
            });
    }

    if !copied.is_empty() {
        if anim.is_channel_animation() {
            state.clipboard = Clipboard::Channels(copied);
            state.clipboard_type = Some(ClipboardType::Channels);
        } else {
            state.clipboard = Clipboard::Keyframes(copied);
            state.clipboard_type = Some(ClipboardType::Keyframes);
        }
    }
}

/// Pastes copied keyframes starting at `target_tick` (`PasteKeyframes`).
///
/// Offsets all copied keyframes relative to the global earliest copied tick,
/// auto-creates missing top-level tracks, overwrites existing keyframes at the
/// destination tick, and sets `notifications.cannot_paste_error = true` on type mismatch.
pub fn paste_keyframes(state: &mut EditorState, target_tick: Tick) {
    let copied = match &state.clipboard {
        Clipboard::Keyframes(m) | Clipboard::Channels(m) if !m.is_empty() => m.clone(),
        _ => return,
    };
    if state.animation_data.is_none() {
        return;
    }

    let mut earliest_tick: Option<Tick> = None;
    for entries in copied.values() {
        for entry in entries {
            if let Some(&first) = entry.data.keys().next() {
                earliest_tick = Some(match earliest_tick {
                    Some(cur) => cur.min(first),
                    None => first,
                });
            }
        }
    }
    let Some(earliest) = earliest_tick else {
        return;
    };

    add_waypoint(state);
    let default_rot = state.settings.default_rotation_type;
    let default_order = state.settings.default_euler_angles_order;
    let mut paste_mismatch = false;

    if let Some(anim) = state.animation_data.as_mut() {
        for (inst_name, entries) in copied {
            for entry in entries {
                let top_exists = anim
                    .instances
                    .get(&inst_name)
                    .is_some_and(|inst| inst.tracks.contains_key(&entry.top_track_name));
                if !top_exists {
                    anim.add_track(
                        &inst_name,
                        entry.top_track_name.clone(),
                        entry.top_track_type,
                        entry.rotation_type.unwrap_or(default_rot),
                        entry.euler_angles_order.unwrap_or(default_order),
                    );
                }

                let mut full_path = vec![entry.top_track_name.clone()];
                full_path.extend(entry.rel_path.iter().cloned());

                if let Some(track) = anim.get_track_mut(&inst_name, &full_path) {
                    if track.track_type == entry.track_type {
                        for (orig_tick, kf) in entry.data {
                            let insert_tick = clamp_tick(target_tick + (orig_tick - earliest));
                            track.add_keyframe(insert_tick, kf);
                        }
                    } else {
                        paste_mismatch = true;
                    }
                } else {
                    paste_mismatch = true;
                }
            }
        }
    }

    if paste_mismatch {
        state.notifications.cannot_paste_error = true;
    }

    finalize_animation_data_update(state);
    rebuild_track_list(state);
}

/// Applies `patch` to every selected keyframe (`SetSelectedKeyframeData`).
pub fn set_selected_keyframe_data(state: &mut EditorState, patch: KeyframePatch) {
    if is_selection_empty(&state.selected_keyframes) || state.animation_data.is_none() {
        return;
    }
    add_waypoint(state);

    let leaves = {
        let anim = state.animation_data.as_ref().unwrap();
        collect_selected_leaf_ticks(&state.selected_keyframes, anim)
    };

    if let Some(anim) = state.animation_data.as_mut() {
        for (inst_name, path, ticks) in leaves {
            if let Some(track) = anim.get_track_mut(&inst_name, &path) {
                for tick in ticks {
                    track.set_keyframe_data(tick, &patch);
                }
            }
        }
    }
}

/// Sets or clears (`None`) a cubic tangent slope on `(instance, path, tick)`
/// (`Thunks/SetKeyframeTangent`).
pub fn set_keyframe_tangent(
    state: &mut EditorState,
    instance: &str,
    path: &[String],
    tick: Tick,
    side: TangentSide,
    slope: Option<f32>,
) {
    let exists = state
        .animation_data
        .as_ref()
        .and_then(|anim| anim.get_track(instance, path))
        .is_some_and(|t| t.data.contains_key(&tick));
    if !exists {
        return;
    }

    add_waypoint(state);
    if let Some(anim) = state.animation_data.as_mut() {
        if let Some(track) = anim.get_track_mut(instance, path) {
            match side {
                TangentSide::Left => {
                    // Force the preceding keyframe to Cubic so the left tangent applies.
                    if let Ok(idx) = track.keyframes.binary_search(&tick) {
                        if idx > 0 {
                            let prev_tick = track.keyframes[idx - 1];
                            if let Some(prev_kf) = track.data.get_mut(&prev_tick) {
                                prev_kf.interpolation_mode = Some(KeyInterpolationMode::Cubic);
                            }
                        }
                    }
                    if let Some(kf) = track.data.get_mut(&tick) {
                        kf.left_slope = slope;
                    }
                }
                TangentSide::Right => {
                    if let Some(kf) = track.data.get_mut(&tick) {
                        kf.interpolation_mode = Some(KeyInterpolationMode::Cubic);
                        kf.right_slope = slope;
                    }
                }
            }
        }
    }
}

/// Renames the summary keyframe at `tick` (`Thunks/RenameKeyframe`).
pub fn rename_keyframe(state: &mut EditorState, tick: Tick, new_name: String) {
    if state.animation_data.is_none() {
        return;
    }
    add_waypoint(state);
    if let Some(anim) = state.animation_data.as_mut() {
        anim.rename_keyframe(tick, new_name);
    }
}

// ============================================================================
// Curve Promotion, Rotation Conversion, Splitting & Sanitization
// ============================================================================

/// Promotes a `KeyframeSequence`-style animation to a Channel/Curve animation using
/// `state.settings.default_rotation_type` and `state.settings.default_euler_angles_order`.
pub fn promote_to_curves(state: &mut EditorState) {
    let should_promote = state
        .animation_data
        .as_ref()
        .is_some_and(|anim| !anim.is_channel_animation());
    if !should_promote {
        return;
    }

    add_waypoint(state);
    let rot_type = state.settings.default_rotation_type;
    let euler_order = state.settings.default_euler_angles_order;

    if let Some(anim) = state.animation_data.as_mut() {
        anim.promote_to_curves(rot_type, euler_order);
    }

    state.selected_keyframes.clear();
    state.editor_mode = EditorMode::Curves;
    finalize_animation_data_update(state);
    prune_invalid_selection(state);
    rebuild_track_list(state);
    crate::anim_editor::ui_curve_editor::fit_curve_view(state);
}

/// Converts a track's rotation representation between `Quaternion` and `EulerAngles`
/// (`Thunks/ConvertTrack`).
pub fn convert_track_rotation(
    state: &mut EditorState,
    instance: &str,
    path: &[String],
    to_euler: bool,
) {
    if path.is_empty() || state.animation_data.is_none() {
        return;
    }

    // Determine the rotation track path (either `path` itself or `path/Rotation` for a CFrame track).
    let rot_path = {
        let anim = state.animation_data.as_ref().unwrap();
        let Some(track) = anim.get_track(instance, path) else {
            return;
        };
        if track.track_type == TrackType::CFrame {
            let mut p = path.to_vec();
            p.push(PROP_ROTATION.to_string());
            p
        } else {
            path.to_vec()
        }
    };

    let needs_convert = state
        .animation_data
        .as_ref()
        .and_then(|anim| anim.get_track(instance, &rot_path))
        .is_some_and(|t| {
            if to_euler {
                t.track_type == TrackType::Quaternion
            } else {
                t.track_type == TrackType::EulerAngles
            }
        });
    if !needs_convert {
        return;
    }

    add_waypoint(state);
    let order = state.settings.default_euler_angles_order;
    if let Some(anim) = state.animation_data.as_mut() {
        if let Some(rot_track) = anim.get_track_mut(instance, &rot_path) {
            if to_euler {
                *rot_track =
                    crate::anim_editor::curves::convert_track_to_euler_angles(rot_track, order);
            } else {
                *rot_track = crate::anim_editor::curves::convert_track_to_quaternion(rot_track);
            }
        }
    }

    finalize_animation_data_update(state);
    prune_invalid_selection(state);
    rebuild_track_list(state);
}

/// Sets the `RotationOrder` on an `EulerAngles` rotation track (`SetTrackEulerAnglesOrder`).
pub fn set_track_euler_angles_order(
    state: &mut EditorState,
    instance: &str,
    path: &[String],
    order: RotationOrder,
) {
    if path.is_empty() || state.animation_data.is_none() {
        return;
    }

    let already_same = state
        .animation_data
        .as_ref()
        .and_then(|anim| anim.get_track(instance, path))
        .is_some_and(|t| t.get_euler_angles_order() == Some(order));
    if already_same {
        return;
    }

    add_waypoint(state);
    if let Some(anim) = state.animation_data.as_mut() {
        let top_exists = anim
            .instances
            .get(instance)
            .is_some_and(|inst| inst.tracks.contains_key(&path[0]));
        if !top_exists {
            anim.add_track(
                instance,
                path[0].clone(),
                TrackType::CFrame,
                TrackType::EulerAngles,
                order,
            );
        }

        if let Some(track) = anim.get_track_mut(instance, path) {
            if track.track_type == TrackType::CFrame {
                if let Some(rot) = track.components.get_mut(PROP_ROTATION) {
                    rot.euler_angles_order = Some(order);
                }
            } else {
                track.euler_angles_order = Some(order);
            }
        }
    }

    rebuild_track_list(state);
}

/// Inserts a curve-preserving split keyframe at `tick` on `(instance, path)`
/// (`Thunks/SplitTrack`).
pub fn split_track_at_tick(state: &mut EditorState, instance: &str, path: &[String], tick: Tick) {
    let exists = state
        .animation_data
        .as_ref()
        .is_some_and(|anim| anim.get_track(instance, path).is_some());
    if !exists {
        return;
    }

    let tick = clamp_tick(tick);
    add_waypoint(state);
    if let Some(anim) = state.animation_data.as_mut() {
        if let Some(track) = anim.get_track_mut(instance, path) {
            crate::anim_editor::curves::split_track(track, tick);
        }
    }
    finalize_animation_data_update(state);
}

/// Sanitizes the active animation: clamps ticks to `[0, MAX_ANIMATION_LENGTH]`,
/// strips non-finite (`NaN`/`Inf`) keyframes, repairs out-of-order `keyframes` arrays,
/// and removes empty top-level tracks.
pub fn sanitize_animation(state: &mut EditorState) {
    if state.animation_data.is_none() {
        return;
    }

    add_waypoint(state);
    if let Some(anim) = state.animation_data.as_mut() {
        for inst in anim.instances.values_mut() {
            for track in inst.tracks.values_mut() {
                sanitize_track_recursive(track);
            }
            inst.tracks.retain(|_, track| !track.is_empty());
        }

        // Sanitize event ticks and keep sorted/deduplicated.
        let mut clean_events_data = BTreeMap::new();
        for (tick, map) in std::mem::take(&mut anim.events.data) {
            if !map.is_empty() {
                let clamped = clamp_tick(tick);
                clean_events_data.insert(clamped, map);
            }
        }
        anim.events.keyframes = clean_events_data.keys().copied().collect();
        anim.events.data = clean_events_data;
    }

    finalize_animation_data_update(state);
    prune_invalid_selection(state);
    rebuild_track_list(state);
}

// ============================================================================
// Snapping, Length & Lifecycle Thunks
// ============================================================================

/// Snaps `tick` to the nearest frame boundary at `frame_rate`.
#[inline]
pub fn snap_to_nearest_frame(tick: Tick, frame_rate: u32) -> Tick {
    nearest_frame(tick, frame_rate)
}

/// Snaps `tick` to the nearest keyframe in `data` if within `SNAP_TO_KEYFRAME_THRESHOLD`
/// (30 px at `px_per_tick`), otherwise falls back to `snap_to_nearest_frame`.
pub fn snap_to_nearest_keyframe(data: &AnimationData, tick: Tick, px_per_tick: f32) -> Tick {
    let summary = data.get_summary_ticks();
    if !summary.is_empty() && px_per_tick > 0.0 {
        let closest = match summary.binary_search(&tick) {
            Ok(idx) => summary[idx],
            Err(idx) => {
                let prev = idx.checked_sub(1).map(|i| summary[i]);
                let next = summary.get(idx).copied();
                match (prev, next) {
                    (Some(p), Some(n)) => {
                        if (tick - p).abs() <= (n - tick).abs() {
                            p
                        } else {
                            n
                        }
                    }
                    (Some(p), None) => p,
                    (None, Some(n)) => n,
                    (None, None) => tick,
                }
            }
        };
        let px_dist = ((closest - tick).abs() as f32) * px_per_tick;
        if px_dist < SNAP_TO_KEYFRAME_THRESHOLD {
            return closest;
        }
    }
    snap_to_nearest_frame(tick, data.metadata.frame_rate)
}

/// Updates `state.playback.editing_length` (at least `end_tick` and `TICK_FREQUENCY`)
/// and resets horizontal scroll/zoom (`Thunks/UpdateEditingLength`).
pub fn update_editing_length(state: &mut EditorState, new_length: Tick) {
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    let clamped = new_length
        .max(anim.metadata.end_tick)
        .max(TICK_FREQUENCY)
        .min(MAX_ANIMATION_LENGTH);
    state.playback.editing_length = clamped;
    state.curve_editor.view.horizontal_scroll = 0.0;
    state.curve_editor.view.horizontal_zoom = 0.0;
}

/// Creates a brand new rig animation and loads it cleanly (`CreateNewAnimation`).
pub fn create_new_animation(state: &mut EditorState, name: String) {
    let data = AnimationData::new_rig_animation(name);
    load_animation_data(state, data);
    state.is_dirty = false;
    state.is_legacy_anim_saves = false;
}

/// Loads `data` into the editor, resetting history, selection, scroll, and track list
/// (`LoadAnimationData`).
pub fn load_animation_data(state: &mut EditorState, mut data: AnimationData) {
    clear_history(state);
    clear_selection(state);
    state.selected_tracks.clear();
    state.last_selected_path = None;
    state.right_click_context = RightClickContextInfo::default();
    state.drag_context = None;
    state.editor_mode = EditorMode::DopeSheet;
    state.frame_rate = data.metadata.frame_rate.max(1);

    let clipped = data.remove_extra_keyframes();
    if clipped {
        state.notifications.clipped_warning = true;
    }
    data.set_end_tick();
    data.validate_keyframe_names();

    let end_tick = data.metadata.end_tick;
    let has_events = !data.events.keyframes.is_empty();

    state.animation_data = Some(data);
    state.top_track_index = 1;
    state.playback.playhead = 0;
    state.is_dirty = true;
    state.settings.show_events = has_events;

    update_editing_length(state, end_tick);
    rebuild_track_list(state);
    refresh_available_events(state);
}

// ============================================================================
// Internal Helpers
// ============================================================================

fn apply_leaf_value(
    state: &mut EditorState,
    instance: &str,
    path: &[String],
    track_type: TrackType,
    rotation_type: TrackType,
    euler_order: RotationOrder,
    tick: Tick,
    value: KeyframeValue,
) -> bool {
    let Some(anim) = state.animation_data.as_mut() else {
        return false;
    };

    let mut created_track = false;
    if anim.get_track(instance, path).is_none() {
        let top_type = if track_type == TrackType::Facs {
            TrackType::Facs
        } else {
            TrackType::CFrame
        };
        if anim.get_track(instance, &path[..1]).is_none() {
            anim.add_track(
                instance,
                path[0].clone(),
                top_type,
                rotation_type,
                euler_order,
            );
            created_track = true;
        }
    }

    let Some(track) = anim.get_track_mut(instance, path) else {
        return created_track;
    };

    if let Some(existing) = track.data.get_mut(&tick) {
        existing.value = value;
    } else {
        let kf = if track.is_curve_track {
            Keyframe::new_curve(value)
        } else {
            Keyframe::new_pose(value)
        };
        track.add_keyframe(tick, kf);
        if tick != 0 && !track.data.contains_key(&0) {
            track.add_default_keyframe(0);
        }
    }

    created_track
}

fn find_prev_euler_angles(
    state: &EditorState,
    instance: &str,
    cframe_path: &[String],
    tick: Tick,
) -> Option<Vec3> {
    let anim = state.animation_data.as_ref()?;
    let mut rot_path = cframe_path.to_vec();
    rot_path.push(PROP_ROTATION.to_string());
    let rot_track = anim.get_track(instance, &rot_path)?;
    let rx = rot_track.components.get(PROP_X)?;
    let ry = rot_track.components.get(PROP_Y)?;
    let rz = rot_track.components.get(PROP_Z)?;

    let prev_x = prev_scalar_before(rx, tick)?;
    let prev_y = prev_scalar_before(ry, tick)?;
    let prev_z = prev_scalar_before(rz, tick)?;
    Some(Vec3::new(prev_x, prev_y, prev_z))
}

fn prev_scalar_before(track: &Track, tick: Tick) -> Option<f32> {
    let idx = match track.keyframes.binary_search(&tick) {
        Ok(i) => i.checked_sub(1)?,
        Err(i) => i.checked_sub(1)?,
    };
    let prev_tick = track.keyframes[idx];
    track.data.get(&prev_tick)?.value.as_f32()
}

fn sample_leaf_linear(track: &Track, tick: Tick) -> KeyframeValue {
    if track.keyframes.is_empty() {
        return KeyframeValue::default_for(track.track_type);
    }
    match track.keyframes.binary_search(&tick) {
        Ok(i) => track
            .data
            .get(&track.keyframes[i])
            .map(|k| k.value)
            .unwrap_or_else(|| KeyframeValue::default_for(track.track_type)),
        Err(0) => track
            .data
            .get(&track.keyframes[0])
            .map(|k| k.value)
            .unwrap_or_else(|| KeyframeValue::default_for(track.track_type)),
        Err(i) if i >= track.keyframes.len() => track
            .data
            .get(&track.keyframes[track.keyframes.len() - 1])
            .map(|k| k.value)
            .unwrap_or_else(|| KeyframeValue::default_for(track.track_type)),
        Err(i) => {
            let t0 = track.keyframes[i - 1];
            let t1 = track.keyframes[i];
            let v0 = track
                .data
                .get(&t0)
                .map(|k| k.value)
                .unwrap_or_else(|| KeyframeValue::default_for(track.track_type));
            let v1 = track
                .data
                .get(&t1)
                .map(|k| k.value)
                .unwrap_or_else(|| KeyframeValue::default_for(track.track_type));
            let alpha = if t1 > t0 {
                (tick - t0) as f32 / (t1 - t0) as f32
            } else {
                0.0
            };
            v0.lerp(v1, alpha)
        }
    }
}

fn finalize_animation_data_update(state: &mut EditorState) {
    let Some(anim) = state.animation_data.as_mut() else {
        return;
    };
    if anim.remove_extra_keyframes() {
        state.notifications.clipped_warning = true;
    }
    anim.set_end_tick();
    anim.validate_keyframe_names();

    let start = anim.metadata.start_tick;
    let end = anim.metadata.end_tick;
    if start != end {
        state.playback.playhead = state.playback.playhead.clamp(start, end);
    }
    if end - start > state.playback.editing_length {
        state.playback.editing_length = end.max(TICK_FREQUENCY).min(MAX_ANIMATION_LENGTH);
    }
}

#[allow(clippy::too_many_arguments)]
fn move_selection_node_recursive(
    sel_node: &mut SelectionNode,
    data_track: &mut Track,
    pivot_tick: Tick,
    new_tick: Tick,
    earliest_tick: Tick,
    latest_tick: Tick,
    move_right: bool,
    snap_mode: SnapMode,
    frame_rate: u32,
    delta_value: Option<f32>,
    moved_named: &mut Vec<(Tick, Tick)>,
) {
    if !sel_node.selection.is_empty() {
        let ticks: Vec<Tick> = sel_node.selection.iter().copied().collect();
        let ordered: Vec<Tick> = if move_right {
            ticks.into_iter().rev().collect()
        } else {
            ticks
        };

        let mut updated_selection = BTreeSet::new();
        for old_tick in ordered {
            let mut target = new_tick + (old_tick - pivot_tick);
            if snap_mode != SnapMode::None {
                target = nearest_frame(target, frame_rate);
            }
            let min_allowed = old_tick - earliest_tick;
            let max_allowed = MAX_ANIMATION_LENGTH - (latest_tick - old_tick);
            target = target.clamp(min_allowed, max_allowed);

            if data_track.is_leaf() {
                if old_tick != target && data_track.move_keyframe(old_tick, target) {
                    moved_named.push((old_tick, target));
                }
                if let Some(dv) = delta_value {
                    if data_track.track_type != TrackType::Quaternion {
                        if let Some(kf) = data_track.data.get_mut(&target) {
                            match kf.value {
                                KeyframeValue::Number(v) => {
                                    kf.value = KeyframeValue::Number(v + dv);
                                }
                                KeyframeValue::Angle(v) => {
                                    kf.value = KeyframeValue::Angle(v + dv);
                                }
                                KeyframeValue::Facs(v) => {
                                    kf.value = KeyframeValue::Facs((v + dv).clamp(0.0, 1.0));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            updated_selection.insert(target);
        }
        sel_node.selection = updated_selection;
    }

    for (comp_name, child_sel) in &mut sel_node.components {
        if let Some(child_data) = data_track.components.get_mut(comp_name) {
            move_selection_node_recursive(
                child_sel,
                child_data,
                pivot_tick,
                new_tick,
                earliest_tick,
                latest_tick,
                move_right,
                snap_mode,
                frame_rate,
                delta_value,
                moved_named,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn scale_selection_node_recursive(
    sel_node: &mut SelectionNode,
    data_track: &mut Track,
    pivot_tick: Tick,
    scale: f64,
    snap_mode: SnapMode,
    frame_rate: u32,
    moved_named: &mut Vec<(Tick, Tick)>,
) {
    if !sel_node.selection.is_empty() {
        let ticks: Vec<Tick> = sel_node.selection.iter().copied().collect();
        let compute_target = |old_tick: Tick| -> Tick {
            let mut t = ((pivot_tick as f64) + ((old_tick - pivot_tick) as f64) * scale + 0.5)
                .floor() as Tick;
            if snap_mode != SnapMode::None {
                t = nearest_frame(t, frame_rate);
            }
            clamp_tick(t)
        };

        let mut updated_selection = BTreeSet::new();

        let mut apply_one = |old_tick: Tick, target: Tick| {
            if data_track.is_leaf()
                && old_tick != target
                && data_track.move_keyframe(old_tick, target)
            {
                moved_named.push((old_tick, target));
            }
            updated_selection.insert(target);
        };

        if scale > 1.0 {
            // Expanding outward away from pivot:
            // Pass 1: left-to-right for ticks mapping strictly left of pivot.
            for &old_tick in &ticks {
                let target = compute_target(old_tick);
                if target >= pivot_tick {
                    break;
                }
                apply_one(old_tick, target);
            }
            // Pass 2: right-to-left for ticks mapping strictly right of pivot.
            for &old_tick in ticks.iter().rev() {
                let target = compute_target(old_tick);
                if target <= pivot_tick {
                    break;
                }
                apply_one(old_tick, target);
            }
            // Any ticks staying at pivot_tick remain selected.
            for &old_tick in &ticks {
                let target = compute_target(old_tick);
                if target == pivot_tick {
                    updated_selection.insert(target);
                }
            }
        } else {
            // Shrinking inward toward pivot:
            let split = ticks.partition_point(|&t| t <= pivot_tick);
            // Pass 1: from closest-to-pivot moving leftward.
            for &old_tick in ticks[..split].iter().rev() {
                let target = compute_target(old_tick);
                apply_one(old_tick, target);
            }
            // Pass 2: from closest-to-pivot moving rightward.
            for &old_tick in &ticks[split..] {
                let target = compute_target(old_tick);
                apply_one(old_tick, target);
            }
        }

        sel_node.selection = updated_selection;
    }

    for (comp_name, child_sel) in &mut sel_node.components {
        if let Some(child_data) = data_track.components.get_mut(comp_name) {
            scale_selection_node_recursive(
                child_sel,
                child_data,
                pivot_tick,
                scale,
                snap_mode,
                frame_rate,
                moved_named,
            );
        }
    }
}

fn is_keyframe_value_finite(v: &KeyframeValue) -> bool {
    match v {
        KeyframeValue::CFrame(xf) => {
            xf.pos.is_finite()
                && xf.rot.x_axis.is_finite()
                && xf.rot.y_axis.is_finite()
                && xf.rot.z_axis.is_finite()
        }
        KeyframeValue::Position(p) => p.is_finite(),
        KeyframeValue::Quaternion(q) => q.is_finite(),
        KeyframeValue::Number(x) | KeyframeValue::Angle(x) | KeyframeValue::Facs(x) => {
            x.is_finite()
        }
    }
}

fn sanitize_track_recursive(track: &mut Track) {
    if !track.components.is_empty() {
        track.keyframes.clear();
        track.data.clear();
        for comp in track.components.values_mut() {
            sanitize_track_recursive(comp);
        }
        return;
    }

    let mut clean_data = BTreeMap::new();
    for (tick, mut kf) in std::mem::take(&mut track.data) {
        if !is_keyframe_value_finite(&kf.value) {
            continue;
        }
        if kf.left_slope.is_some_and(|s| !s.is_finite()) {
            kf.left_slope = None;
        }
        if kf.right_slope.is_some_and(|s| !s.is_finite()) {
            kf.right_slope = None;
        }
        if let KeyframeValue::Facs(v) = kf.value {
            kf.value = KeyframeValue::Facs(v.clamp(0.0, 1.0));
        }
        clean_data.insert(clamp_tick(tick), kf);
    }

    track.keyframes = clean_data.keys().copied().collect();
    track.data = clean_data;
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim_editor::history::{redo, undo};
    use crate::anim_editor::selection::{
        deselect_keyframe, is_keyframe_selected, select_all_keyframes, select_keyframe,
    };
    use glam::Mat3;
    use rustudio_core::math::Xform;

    fn make_state_with_anim() -> EditorState {
        let mut state = EditorState::new();
        state.settings.snap_mode = SnapMode::None;
        create_new_animation(&mut state, "TestAnim".to_string());
        state
    }

    #[test]
    fn value_changed_auto_creates_track_and_tick_zero_anchor() {
        let mut state = make_state_with_anim();
        let xf = Xform {
            pos: Vec3::new(1.0, 2.0, 3.0),
            rot: Mat3::IDENTITY,
        };
        value_changed(
            &mut state,
            "Root",
            &["UpperTorso".to_string()],
            TrackType::CFrame,
            2400,
            KeyframeValue::CFrame(xf),
            true,
        );

        let anim = state.animation_data.as_ref().unwrap();
        let track = anim
            .get_track("Root", &["UpperTorso".to_string()])
            .expect("track auto-created");
        assert_eq!(track.keyframes, vec![0, 2400]);
        assert_eq!(
            track.data[&0].value,
            KeyframeValue::default_for(TrackType::CFrame)
        );
        assert_eq!(track.data[&2400].value, KeyframeValue::CFrame(xf));
        assert_eq!(anim.metadata.end_tick, 2400);
    }

    #[test]
    fn undo_redo_restores_animation_data_and_respects_cap() {
        let mut state = make_state_with_anim();
        state.history.max_waypoint_size = 3;

        for i in 1..=5 {
            value_changed(
                &mut state,
                "Root",
                &["Head".to_string()],
                TrackType::CFrame,
                i * 80,
                KeyframeValue::CFrame(Xform {
                    pos: Vec3::new(i as f32, 0.0, 0.0),
                    rot: Mat3::IDENTITY,
                }),
                true,
            );
        }

        // Past stack is capped at 3 waypoints.
        assert_eq!(state.history.past.len(), 3);
        assert!(undo(&mut state));
        assert_eq!(state.history.future.len(), 1);
        assert_eq!(
            state
                .animation_data
                .as_ref()
                .unwrap()
                .get_track("Root", &["Head".to_string()])
                .unwrap()
                .keyframes
                .len(),
            5 // 0, 80, 160, 240, 320
        );

        assert!(redo(&mut state));
        assert_eq!(
            state
                .animation_data
                .as_ref()
                .unwrap()
                .get_track("Root", &["Head".to_string()])
                .unwrap()
                .keyframes
                .len(),
            6 // 0, 80, 160, 240, 320, 400
        );
    }

    #[test]
    fn selection_three_pass_deselect_clears_ancestors_and_prunes() {
        let mut state = make_state_with_anim();
        let xf = Xform {
            pos: Vec3::new(1.0, 2.0, 3.0),
            rot: Mat3::IDENTITY,
        };
        value_changed(
            &mut state,
            "Root",
            &["UpperTorso".to_string()],
            TrackType::CFrame,
            1200,
            KeyframeValue::CFrame(xf),
            true,
        );
        promote_to_curves(&mut state);
        select_all_keyframes(&mut state);

        let top_path = vec!["UpperTorso".to_string()];
        let pos_path = vec!["UpperTorso".to_string(), "Position".to_string()];
        let x_path = vec![
            "UpperTorso".to_string(),
            "Position".to_string(),
            "X".to_string(),
        ];
        let y_path = vec![
            "UpperTorso".to_string(),
            "Position".to_string(),
            "Y".to_string(),
        ];

        assert!(is_keyframe_selected(
            &state.selected_keyframes,
            "Root",
            &top_path,
            1200
        ));
        assert!(is_keyframe_selected(
            &state.selected_keyframes,
            "Root",
            &x_path,
            1200
        ));

        // Deselecting Position.X at 1200 clears 1200 on Position and UpperTorso ancestors,
        // while keeping Position.Y at 1200 selected.
        deselect_keyframe(&mut state, "Root", &x_path, 1200);
        assert!(!is_keyframe_selected(
            &state.selected_keyframes,
            "Root",
            &x_path,
            1200
        ));
        assert!(!is_keyframe_selected(
            &state.selected_keyframes,
            "Root",
            &pos_path,
            1200
        ));
        assert!(!is_keyframe_selected(
            &state.selected_keyframes,
            "Root",
            &top_path,
            1200
        ));
        assert!(is_keyframe_selected(
            &state.selected_keyframes,
            "Root",
            &y_path,
            1200
        ));
    }

    #[test]
    fn drag_and_scale_anti_overwrite_preserves_distinct_keyframes() {
        let mut state = make_state_with_anim();
        for (i, t) in [0, 80, 160].iter().enumerate() {
            value_changed(
                &mut state,
                "Root",
                &["Head".to_string()],
                TrackType::CFrame,
                *t,
                KeyframeValue::CFrame(Xform {
                    pos: Vec3::new(i as f32, 0.0, 0.0),
                    rot: Mat3::IDENTITY,
                }),
                false,
            );
        }

        select_all_keyframes(&mut state);
        // Drag right by +80 ticks (overlapping old ticks 80 & 160).
        begin_keyframe_drag(&mut state, 80, None, DragMode::Keyframe);
        update_keyframe_drag(&mut state, 160, None);
        end_drag(&mut state, true);

        let track = state
            .animation_data
            .as_ref()
            .unwrap()
            .get_track("Root", &["Head".to_string()])
            .unwrap();
        assert_eq!(track.keyframes, vec![80, 160, 240]);
        assert_eq!(track.data[&80].value.as_position().unwrap().x, 0.0);
        assert_eq!(track.data[&160].value.as_position().unwrap().x, 1.0);
        assert_eq!(track.data[&240].value.as_position().unwrap().x, 2.0);

        // Scale outward by 2x around pivot 80 (latest 240 -> 400).
        begin_keyframe_drag(&mut state, 80, None, DragMode::Keyframe);
        update_keyframe_scale(&mut state, 400);
        end_drag(&mut state, true);

        let track = state
            .animation_data
            .as_ref()
            .unwrap()
            .get_track("Root", &["Head".to_string()])
            .unwrap();
        assert_eq!(track.keyframes, vec![80, 240, 400]);
        assert_eq!(track.data[&80].value.as_position().unwrap().x, 0.0);
        assert_eq!(track.data[&240].value.as_position().unwrap().x, 1.0);
        assert_eq!(track.data[&400].value.as_position().unwrap().x, 2.0);
    }

    #[test]
    fn copy_paste_offsets_from_earliest_copied_tick() {
        let mut state = make_state_with_anim();
        let p1 = Xform {
            pos: Vec3::new(1.0, 0.0, 0.0),
            rot: Mat3::IDENTITY,
        };
        let p2 = Xform {
            pos: Vec3::new(2.0, 0.0, 0.0),
            rot: Mat3::IDENTITY,
        };
        value_changed(
            &mut state,
            "Root",
            &["Head".to_string()],
            TrackType::CFrame,
            100,
            KeyframeValue::CFrame(p1),
            false,
        );
        value_changed(
            &mut state,
            "Root",
            &["Head".to_string()],
            TrackType::CFrame,
            300,
            KeyframeValue::CFrame(p2),
            false,
        );

        select_keyframe(&mut state, "Root", &["Head".to_string()], 100, false);
        select_keyframe(&mut state, "Root", &["Head".to_string()], 300, true);
        copy_selected_keyframes(&mut state);
        paste_keyframes(&mut state, 1000);

        let track = state
            .animation_data
            .as_ref()
            .unwrap()
            .get_track("Root", &["Head".to_string()])
            .unwrap();
        assert_eq!(track.data[&1000].value, KeyframeValue::CFrame(p1));
        assert_eq!(track.data[&1200].value, KeyframeValue::CFrame(p2));
    }

    #[test]
    fn promote_to_curves_splits_cframe_into_position_and_euler_channels() {
        let mut state = make_state_with_anim();
        state.settings.default_rotation_type = TrackType::EulerAngles;
        state.settings.default_euler_angles_order = RotationOrder::XYZ;

        value_changed(
            &mut state,
            "Root",
            &["Head".to_string()],
            TrackType::CFrame,
            1200,
            KeyframeValue::CFrame(Xform {
                pos: Vec3::new(3.0, 4.0, 5.0),
                rot: Mat3::IDENTITY,
            }),
            true,
        );

        promote_to_curves(&mut state);

        let anim = state.animation_data.as_ref().unwrap();
        assert!(anim.is_channel_animation());
        assert!(anim.metadata.name.ends_with(" [CHANNELS]"));

        let pos_x = anim
            .get_track(
                "Root",
                &["Head".to_string(), "Position".to_string(), "X".to_string()],
            )
            .expect("Position.X created");
        assert!(pos_x.is_curve_track);
        assert_eq!(pos_x.data[&1200].value, KeyframeValue::Number(3.0));
    }
}
