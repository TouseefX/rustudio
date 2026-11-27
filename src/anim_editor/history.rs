//! Undo/Redo history stack operations for `AnimationClipEditor`
//! (derived from `Reducers/History` and `Thunks/History/{AddWaypoint,Undo,Redo}`).

use crate::anim_editor::selection::prune_invalid_selection;
use crate::anim_editor::state::{EditorState, HistoryWaypoint};

/// Pushes a deep clone of the current `animation_data` and `tracks` onto
/// `history.past` (newest at index 0), caps the stack at `history.max_waypoint_size`
/// (default 20), clears `history.future`, and marks the animation dirty.
pub fn add_waypoint(state: &mut EditorState) {
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    state
        .history
        .push_waypoint(anim.clone(), state.tracks.clone());
    state.is_dirty = true;
}

/// Undoes the most recent edit by popping `history.past[0]`, pushing the current
/// state onto `history.future[0]` (capped at `max_waypoint_size`), restoring
/// `animation_data` and `tracks`, pruning invalid selection, and marking dirty.
///
/// Returns `true` if a waypoint was restored, or `false` if the undo stack was empty.
pub fn undo(state: &mut EditorState) -> bool {
    if state.history.past.is_empty() {
        return false;
    }

    state.in_review_state = false;

    if let Some(current_anim) = state.animation_data.clone() {
        state.history.future.insert(
            0,
            HistoryWaypoint {
                animation_data: current_anim,
                tracks: state.tracks.clone(),
            },
        );
        state
            .history
            .future
            .truncate(state.history.max_waypoint_size);
    }

    let waypoint = state.history.past.remove(0);
    restore_waypoint(state, waypoint);
    state.is_dirty = true;
    true
}

/// Redoes the most recently undone edit by popping `history.future[0]`, pushing
/// the current state onto `history.past[0]` (capped at `max_waypoint_size`),
/// restoring `animation_data` and `tracks`, pruning invalid selection, and marking dirty.
///
/// Returns `true` if a waypoint was restored, or `false` if the redo stack was empty.
pub fn redo(state: &mut EditorState) -> bool {
    if state.history.future.is_empty() {
        return false;
    }

    if let Some(current_anim) = state.animation_data.clone() {
        state.history.past.insert(
            0,
            HistoryWaypoint {
                animation_data: current_anim,
                tracks: state.tracks.clone(),
            },
        );
        state.history.past.truncate(state.history.max_waypoint_size);
    }

    let waypoint = state.history.future.remove(0);
    restore_waypoint(state, waypoint);
    state.is_dirty = true;
    true
}

/// Clears both undo (`past`) and redo (`future`) history stacks.
pub fn clear_history(state: &mut EditorState) {
    state.history.clear();
}

fn restore_waypoint(state: &mut EditorState, mut waypoint: HistoryWaypoint) {
    waypoint.animation_data.set_end_tick();
    waypoint.animation_data.validate_keyframe_names();

    let start = waypoint.animation_data.metadata.start_tick;
    let end = waypoint.animation_data.metadata.end_tick;
    if start != end {
        state.playback.playhead = state.playback.playhead.clamp(start, end);
    }

    state.animation_data = Some(waypoint.animation_data);
    state.tracks = waypoint.tracks;
    state.selected_keyframes.clear();
    state.selected_events.clear();
    state.drag_context = None;
    prune_invalid_selection(state);
}
