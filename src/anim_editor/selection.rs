//! Hierarchical keyframe selection tree and track-list management for `AnimationClipEditor`
//! (derived from `Util/SelectionUtils`, `Util/TrackSelectionUtils`,
//! `Thunks/Selection/{SelectKeyframeRange,SelectKeyframesAtTick,SelectAllKeyframes,
//! DeselectKeyframe,DeselectKeyframesAtTick}`, and `Thunks/SortAndSetTracks`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::anim_editor::constants::FACS_CONTROLS;
use crate::anim_editor::model::{AnimationData, Tick, Track, TrackPath, TrackType};
use crate::anim_editor::state::{EditorState, SelectedKeyframes, SelectionNode, TrackListEntry};

// ============================================================================
// Keyframe Selection Operations
// ============================================================================

/// Selects a single keyframe at `tick` on `(instance, path)`.
///
/// Matches `DopeSheetController`, which routes single-keyframe selection through
/// `SelectKeyframeRange(instance, path, tick, tick, multiSelect)`.
pub fn select_keyframe(
    state: &mut EditorState,
    instance: &str,
    path: &[String],
    tick: Tick,
    multi_select: bool,
) {
    select_keyframe_range(state, instance, path, tick, tick, multi_select);
}

/// Selects all keyframes in `[min_tick, max_tick]` on `(instance, path)` and its
/// descendant components (`Thunks/Selection/SelectKeyframeRange`).
pub fn select_keyframe_range(
    state: &mut EditorState,
    instance: &str,
    path: &[String],
    min_tick: Tick,
    max_tick: Tick,
    multi_select: bool,
) {
    if path.is_empty() {
        return;
    }
    let (lo, hi) = if min_tick <= max_tick {
        (min_tick, max_tick)
    } else {
        (max_tick, min_tick)
    };

    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    let Some(track) = anim.get_track(instance, path) else {
        return;
    };
    if !has_keyframes_in_range(track, lo, hi) {
        return;
    }

    if !multi_select {
        state.selected_keyframes.clear();
    }
    // Selecting keyframes clears event selection (mutual exclusivity).
    state.selected_events.clear();

    let inst_map = state
        .selected_keyframes
        .entry(instance.to_string())
        .or_default();
    let mut node = inst_map.entry(path[0].clone()).or_default();
    for seg in &path[1..] {
        node = node.components.entry(seg.clone()).or_default();
    }

    select_in_range_recursive(track, node, lo, hi);
}

/// Selects all keyframes across all instances and tracks at `tick` (SummaryTrack click).
pub fn select_keyframes_at_tick(state: &mut EditorState, tick: Tick, multi_select: bool) {
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    if !multi_select {
        state.selected_keyframes.clear();
    }
    state.selected_events.clear();

    let targets: Vec<(String, String)> = anim
        .instances
        .iter()
        .flat_map(|(inst_name, inst)| {
            inst.tracks
                .keys()
                .map(move |track_name| (inst_name.clone(), track_name.clone()))
        })
        .collect();

    for (inst_name, track_name) in targets {
        select_keyframe_range(state, &inst_name, &[track_name], tick, tick, true);
    }
}

/// Deselects a keyframe at `tick` on `(instance, path)` using the exact 3-pass
/// traversal from `Thunks/Selection/DeselectKeyframe`:
/// 1. Clears `tick` from the target node and all its descendant components.
/// 2. Walks the top-level track tree against `AnimationData`; whenever a node has
///    a real keyframe at `tick` in `AnimationData` and `tick` is no longer in its
///    `selection`, clears `tick` from all of its ancestor `SelectionNode`s.
/// 3. Prunes empty `SelectionNode`s, empty top-level tracks, and empty instances.
pub fn deselect_keyframe(state: &mut EditorState, instance: &str, path: &[String], tick: Tick) {
    if path.is_empty() {
        return;
    }
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    let Some(top_data_track) = anim.get_track(instance, &path[..1]) else {
        return;
    };

    // Verify target node exists and actually has `tick` selected.
    {
        let Some(mut cur) = state
            .selected_keyframes
            .get(instance)
            .and_then(|tracks| tracks.get(&path[0]))
        else {
            return;
        };
        for seg in &path[1..] {
            let Some(next) = cur.components.get(seg) else {
                return;
            };
            cur = next;
        }
        if !cur.selection.contains(&tick) {
            return;
        }
    }

    let Some(inst_map) = state.selected_keyframes.get_mut(instance) else {
        return;
    };
    let Some(top_sel) = inst_map.get_mut(&path[0]) else {
        return;
    };

    // Pass 1: Clear `tick` on target node and all descendants.
    if let Some(target_node) = get_selection_node_mut(top_sel, &path[1..]) {
        clear_tick_descendants(target_node, tick);
    }

    // Pass 2: Find every node that has a real keyframe at `tick` in `AnimationData`
    // yet no longer has `tick` selected, and clear `tick` from all of its ancestors.
    let mut deselected_real_paths: Vec<TrackPath> = Vec::new();
    let mut cur_path = TrackPath::new();
    collect_deselected_real_paths(
        top_sel,
        Some(top_data_track),
        tick,
        &mut cur_path,
        &mut deselected_real_paths,
    );
    for rel_path in &deselected_real_paths {
        clear_tick_ancestors(top_sel, rel_path, tick);
    }

    // Pass 3: Prune empty component nodes, top-level track, and instance map.
    top_sel.prune_empty();
    if top_sel.is_empty() {
        inst_map.remove(&path[0]);
    }
    if inst_map.is_empty() {
        state.selected_keyframes.remove(instance);
    }
}

/// Deselects all keyframes across all instances and tracks at `tick`.
pub fn deselect_keyframes_at_tick(state: &mut EditorState, tick: Tick) {
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    let targets: Vec<(String, String)> = anim
        .instances
        .iter()
        .flat_map(|(inst_name, inst)| {
            inst.tracks
                .keys()
                .map(move |track_name| (inst_name.clone(), track_name.clone()))
        })
        .collect();

    for (inst_name, track_name) in targets {
        deselect_keyframe(state, &inst_name, &[track_name], tick);
    }
}

/// Selects every keyframe across all instances and tracks (`SelectAllKeyframes`).
pub fn select_all_keyframes(state: &mut EditorState) {
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };

    let mut new_sel = SelectedKeyframes::new();
    for (inst_name, inst) in &anim.instances {
        let mut inst_sel = BTreeMap::new();
        for (track_name, track) in &inst.tracks {
            let mut node = SelectionNode::default();
            select_all_recursive(track, &mut node);
            if !node.is_empty() {
                inst_sel.insert(track_name.clone(), node);
            }
        }
        if !inst_sel.is_empty() {
            new_sel.insert(inst_name.clone(), inst_sel);
        }
    }

    state.selected_keyframes = new_sel;
    state.selected_events.clear();
}

/// Clears all selected keyframes and selected events.
pub fn clear_selection(state: &mut EditorState) {
    state.selected_keyframes.clear();
    state.selected_events.clear();
}

/// Returns `true` if `tick` is selected at `(instance, path)`.
pub fn is_keyframe_selected(
    sel: &SelectedKeyframes,
    instance: &str,
    path: &[String],
    tick: Tick,
) -> bool {
    if path.is_empty() {
        return false;
    }
    let Some(mut node) = sel.get(instance).and_then(|tracks| tracks.get(&path[0])) else {
        return false;
    };
    for seg in &path[1..] {
        let Some(next) = node.components.get(seg) else {
            return false;
        };
        node = next;
    }
    node.contains_tick(tick)
}

/// Collects every selected leaf track and its sorted list of selected ticks that
/// correspond to existing keyframes in `data`.
pub fn collect_selected_leaf_ticks(
    sel: &SelectedKeyframes,
    data: &AnimationData,
) -> Vec<(String, TrackPath, Vec<Tick>)> {
    let mut out = Vec::new();
    for (inst_name, tracks) in sel {
        let Some(inst) = data.instances.get(inst_name) else {
            continue;
        };
        for (track_name, sel_node) in tracks {
            let Some(data_track) = inst.tracks.get(track_name) else {
                continue;
            };
            let mut path = vec![track_name.clone()];
            collect_leaf_ticks_recursive(inst_name, &mut path, sel_node, data_track, &mut out);
        }
    }
    out
}

/// Removes references to deleted tracks, components, keyframes, or events from
/// `selected_keyframes`, `selected_tracks`, `last_selected_path`, and `selected_events`.
pub fn prune_invalid_selection(state: &mut EditorState) {
    let Some(anim) = state.animation_data.as_ref() else {
        state.selected_keyframes.clear();
        state.selected_tracks.clear();
        state.last_selected_path = None;
        state.selected_events.clear();
        sync_track_list_selection_flags(&mut state.tracks, &state.selected_tracks);
    state.sync_selected_track_instances();
        return;
    };

    // Prune SelectedTracks (`TrackSelectionUtils.PruneSelectedTracks`).
    state.selected_tracks.retain(|path| {
        if anim.get_track("Root", path).is_some() {
            return true;
        }
        anim.instances
            .keys()
            .any(|inst| anim.get_track(inst, path).is_some())
    });

    if let Some(last) = &state.last_selected_path {
        let exists = anim.get_track("Root", last).is_some()
            || anim
                .instances
                .keys()
                .any(|inst| anim.get_track(inst, last).is_some());
        if !exists {
            state.last_selected_path = state.selected_tracks.last().cloned();
        }
    }

    // Prune SelectedKeyframes against existing tracks and keyframes.
    state.selected_keyframes.retain(|inst_name, tracks| {
        let Some(inst) = anim.instances.get(inst_name) else {
            return false;
        };
        tracks.retain(|track_name, sel_node| {
            let Some(data_track) = inst.tracks.get(track_name) else {
                return false;
            };
            prune_selection_node(sel_node, data_track);
            !sel_node.is_empty()
        });
        !tracks.is_empty()
    });

    // Prune SelectedEvents against existing event ticks.
    state
        .selected_events
        .retain(|tick| anim.events.data.contains_key(tick));

    sync_track_list_selection_flags(&mut state.tracks, &state.selected_tracks);
    state.sync_selected_track_instances();
}

// ============================================================================
// Track List Hierarchy & Selection
// ============================================================================

/// Rebuilds `state.tracks`, `state.unused_tracks`, and `state.unused_facs` from
/// `animation_data` and `rig_cache`, preserving expansion states and syncing
/// track selection flags.
pub fn rebuild_track_list(state: &mut EditorState) {
    let mut expanded_map: HashMap<TrackPath, bool> = HashMap::new();
    for entry in &state.tracks {
        let mut path = vec![entry.name.clone()];
        capture_expanded_flags(entry, &mut path, &mut expanded_map);
    }

    let Some(anim) = state.animation_data.as_ref() else {
        state.tracks.clear();
        state.unused_tracks.clear();
        state.unused_facs.clear();
        return;
    };

    // Map rig part name -> hierarchy index for deterministic ordering.
    let hierarchy_index: HashMap<&str, usize> = state
        .rig_cache
        .hierarchy_order
        .iter()
        .enumerate()
        .map(|(idx, name)| (name.as_str(), idx))
        .collect();

    // Collect (instance_name, track_name, &Track) across all instances.
    let mut entries_raw: Vec<(&str, &str, &Track)> = Vec::new();
    for (inst_name, inst) in &anim.instances {
        for (track_name, track) in &inst.tracks {
            entries_raw.push((inst_name.as_str(), track_name.as_str(), track));
        }
    }

    // Sort: non-FACS before FACS; within non-FACS, rig hierarchy order first, then alphabetical.
    entries_raw.sort_by(|(inst_a, name_a, track_a), (inst_b, name_b, track_b)| {
        let is_facs_a = track_a.track_type == TrackType::Facs;
        let is_facs_b = track_b.track_type == TrackType::Facs;
        is_facs_a
            .cmp(&is_facs_b)
            .then_with(|| {
                let ord_a = hierarchy_index.get(*name_a).copied().unwrap_or(usize::MAX);
                let ord_b = hierarchy_index.get(*name_b).copied().unwrap_or(usize::MAX);
                ord_a.cmp(&ord_b)
            })
            .then_with(|| name_a.cmp(name_b))
            .then_with(|| inst_a.cmp(inst_b))
    });

    let mut new_tracks = Vec::with_capacity(entries_raw.len());
    for (inst_name, track_name, track) in entries_raw {
        let depth = compute_rig_depth(track_name, &state.rig_cache.parent_map);
        let mut path = vec![track_name.to_string()];
        let entry = build_track_list_entry(
            track_name,
            inst_name,
            track,
            depth,
            &mut path,
            &expanded_map,
            &state.selected_tracks,
        );
        new_tracks.push(entry);
    }

    state.tracks = new_tracks;

    // Update unused body tracks and unused FACS controls.
    let root_tracks = anim.instances.get("Root").map(|i| &i.tracks);
    state.unused_tracks = state
        .rig_cache
        .hierarchy_order
        .iter()
        .filter(|name| root_tracks.is_none_or(|t| !t.contains_key(name.as_str())))
        .cloned()
        .collect();

    if state.rig_cache.has_face_controls() {
        state.unused_facs = FACS_CONTROLS
            .iter()
            .map(|&(name, _)| name)
            .filter(|name| root_tracks.is_none_or(|t| !t.contains_key(*name)))
            .map(str::to_string)
            .collect();
    } else {
        state.unused_facs.clear();
    }
}

/// Sets the expanded state of the track list entry identified by `path`.
pub fn set_track_expanded(state: &mut EditorState, path: &[String], expanded: bool) {
    if path.is_empty() {
        return;
    }
    let Some(mut entry) = state.tracks.iter_mut().find(|e| e.name == path[0]) else {
        return;
    };
    for seg in &path[1..] {
        let Some(child) = entry.components.get_mut(seg) else {
            return;
        };
        entry = child;
    }
    entry.expanded = expanded;
}

/// Selects (or toggles when `multi` is true) the track at `path` and updates
/// `last_selected_path` and `TrackListEntry::selected` flags.
pub fn select_track(state: &mut EditorState, path: TrackPath, multi: bool) {
    if path.is_empty() {
        return;
    }
    if multi {
        if let Some(idx) = state.selected_tracks.iter().position(|p| p == &path) {
            state.selected_tracks.remove(idx);
        } else {
            state.selected_tracks.push(path.clone());
        }
    } else {
        state.selected_tracks.clear();
        state.selected_tracks.push(path.clone());
    }
    state.last_selected_path = Some(path);
    sync_track_list_selection_flags(&mut state.tracks, &state.selected_tracks);
    state.sync_selected_track_instances();
}

/// Moves the active track selection up (`delta < 0`) or down (`delta > 0`) across
/// currently visible rows in `state.tracks`.
pub fn move_selected_track(state: &mut EditorState, delta: i32) {
    let mut visible_paths: Vec<TrackPath> = Vec::new();
    for entry in &state.tracks {
        let mut path = vec![entry.name.clone()];
        collect_visible_track_paths(entry, &mut path, &mut visible_paths);
    }
    if visible_paths.is_empty() {
        return;
    }

    let current_path = state
        .last_selected_path
        .as_ref()
        .or_else(|| state.selected_tracks.first());

    let current_idx = current_path
        .and_then(|cur| visible_paths.iter().position(|p| p == cur))
        .unwrap_or(0);

    let max_idx = (visible_paths.len() - 1) as isize;
    let next_idx = (current_idx as isize + delta as isize).clamp(0, max_idx) as usize;
    select_track(state, visible_paths[next_idx].clone(), false);
}

// ============================================================================
// Internal Helpers
// ============================================================================

fn has_keyframes_in_range(track: &Track, min_tick: Tick, max_tick: Tick) -> bool {
    if track
        .keyframes
        .iter()
        .any(|&t| t >= min_tick && t <= max_tick)
    {
        return true;
    }
    track
        .components
        .values()
        .any(|child| has_keyframes_in_range(child, min_tick, max_tick))
}

fn select_in_range_recursive(
    data_track: &Track,
    sel_node: &mut SelectionNode,
    min_tick: Tick,
    max_tick: Tick,
) -> BTreeSet<Tick> {
    let mut ticks = BTreeSet::new();
    for &t in &data_track.keyframes {
        if t >= min_tick && t <= max_tick {
            ticks.insert(t);
        }
    }
    for (comp_name, child_track) in &data_track.components {
        if has_keyframes_in_range(child_track, min_tick, max_tick) {
            let child_sel = sel_node.components.entry(comp_name.clone()).or_default();
            let child_ticks = select_in_range_recursive(child_track, child_sel, min_tick, max_tick);
            ticks.extend(child_ticks);
        }
    }
    sel_node.selection.extend(ticks.iter().copied());
    ticks
}

fn select_all_recursive(data_track: &Track, sel_node: &mut SelectionNode) -> BTreeSet<Tick> {
    let mut ticks: BTreeSet<Tick> = data_track.data.keys().copied().collect();
    for (comp_name, child_track) in &data_track.components {
        let mut child_node = SelectionNode::default();
        let child_ticks = select_all_recursive(child_track, &mut child_node);
        if !child_node.is_empty() {
            ticks.extend(child_ticks);
            sel_node.components.insert(comp_name.clone(), child_node);
        }
    }
    sel_node.selection = ticks.clone();
    ticks
}

fn get_selection_node_mut<'a>(
    root: &'a mut SelectionNode,
    rel_path: &[String],
) -> Option<&'a mut SelectionNode> {
    let mut cur = root;
    for seg in rel_path {
        cur = cur.components.get_mut(seg)?;
    }
    Some(cur)
}

fn clear_tick_descendants(node: &mut SelectionNode, tick: Tick) {
    node.selection.remove(&tick);
    for child in node.components.values_mut() {
        clear_tick_descendants(child, tick);
    }
}

fn collect_deselected_real_paths(
    sel_node: &SelectionNode,
    data_track: Option<&Track>,
    tick: Tick,
    cur_path: &mut TrackPath,
    out: &mut Vec<TrackPath>,
) {
    if let Some(dt) = data_track {
        if dt.data.contains_key(&tick) && !sel_node.selection.contains(&tick) {
            out.push(cur_path.clone());
        }
    }
    for (comp_name, child_sel) in &sel_node.components {
        let child_data = data_track.and_then(|dt| dt.components.get(comp_name));
        cur_path.push(comp_name.clone());
        collect_deselected_real_paths(child_sel, child_data, tick, cur_path, out);
        cur_path.pop();
    }
}

fn clear_tick_ancestors(top_sel: &mut SelectionNode, rel_path: &[String], tick: Tick) {
    let mut cur = top_sel;
    for seg in rel_path {
        cur.selection.remove(&tick);
        let Some(next) = cur.components.get_mut(seg) else {
            break;
        };
        cur = next;
    }
}

fn collect_leaf_ticks_recursive(
    inst_name: &str,
    path: &mut TrackPath,
    sel_node: &SelectionNode,
    data_track: &Track,
    out: &mut Vec<(String, TrackPath, Vec<Tick>)>,
) {
    if data_track.is_leaf() {
        let ticks: Vec<Tick> = sel_node
            .selection
            .iter()
            .copied()
            .filter(|t| data_track.data.contains_key(t))
            .collect();
        if !ticks.is_empty() {
            out.push((inst_name.to_string(), path.clone(), ticks));
        }
        return;
    }

    for (comp_name, child_sel) in &sel_node.components {
        if let Some(child_data) = data_track.components.get(comp_name) {
            path.push(comp_name.clone());
            collect_leaf_ticks_recursive(inst_name, path, child_sel, child_data, out);
            path.pop();
        }
    }
}

fn prune_selection_node(sel_node: &mut SelectionNode, data_track: &Track) -> BTreeSet<Tick> {
    if data_track.is_leaf() {
        sel_node.components.clear();
        sel_node
            .selection
            .retain(|t| data_track.data.contains_key(t));
        return sel_node.selection.clone();
    }

    let mut valid_descendant_ticks = BTreeSet::new();
    sel_node.components.retain(|comp_name, child_sel| {
        let Some(child_data) = data_track.components.get(comp_name) else {
            return false;
        };
        let child_ticks = prune_selection_node(child_sel, child_data);
        valid_descendant_ticks.extend(child_ticks);
        !child_sel.is_empty()
    });
    sel_node
        .selection
        .retain(|t| valid_descendant_ticks.contains(t));
    sel_node.selection.clone()
}

fn capture_expanded_flags(
    entry: &TrackListEntry,
    path: &mut TrackPath,
    out: &mut HashMap<TrackPath, bool>,
) {
    out.insert(path.clone(), entry.expanded);
    for (comp_name, child) in &entry.components {
        path.push(comp_name.clone());
        capture_expanded_flags(child, path, out);
        path.pop();
    }
}

fn compute_rig_depth(part_name: &str, parent_map: &HashMap<String, String>) -> usize {
    let mut depth = 0usize;
    let mut cur = part_name;
    let mut guard = 0usize;
    while let Some(parent) = parent_map.get(cur) {
        depth += 1;
        cur = parent.as_str();
        guard += 1;
        if guard > 64 {
            break;
        }
    }
    depth
}

fn build_track_list_entry(
    name: &str,
    instance: &str,
    track: &Track,
    depth: usize,
    path: &mut TrackPath,
    expanded_map: &HashMap<TrackPath, bool>,
    selected_tracks: &[TrackPath],
) -> TrackListEntry {
    let mut entry = TrackListEntry::new(name, instance, track.track_type, depth);
    entry.euler_angles_order = track.euler_angles_order;
    entry.expanded = expanded_map.get(path).copied().unwrap_or(false);
    entry.selected = selected_tracks.iter().any(|p| p == path);

    for (comp_name, child_track) in &track.components {
        path.push(comp_name.clone());
        let child_entry = build_track_list_entry(
            comp_name,
            instance,
            child_track,
            depth + 1,
            path,
            expanded_map,
            selected_tracks,
        );
        entry.components.insert(comp_name.clone(), child_entry);
        path.pop();
    }

    entry
}

fn sync_track_list_selection_flags(tracks: &mut [TrackListEntry], selected_tracks: &[TrackPath]) {
    for entry in tracks {
        let mut path = vec![entry.name.clone()];
        sync_entry_selection_flags(entry, &mut path, selected_tracks);
    }
}

fn sync_entry_selection_flags(
    entry: &mut TrackListEntry,
    path: &mut TrackPath,
    selected_tracks: &[TrackPath],
) {
    entry.selected = selected_tracks.iter().any(|p| p == path);
    for (comp_name, child) in &mut entry.components {
        path.push(comp_name.clone());
        sync_entry_selection_flags(child, path, selected_tracks);
        path.pop();
    }
}

fn collect_visible_track_paths(
    entry: &TrackListEntry,
    path: &mut TrackPath,
    out: &mut Vec<TrackPath>,
) {
    out.push(path.clone());
    if !entry.expanded || entry.components.is_empty() {
        return;
    }

    // Visit ordered standard components first, then optional components, then any extras.
    let mut visited = BTreeSet::new();
    for &key in entry
        .track_type
        .component_order()
        .iter()
        .chain(entry.track_type.optional_component_order().iter())
    {
        if let Some(child) = entry.components.get(key) {
            visited.insert(key);
            path.push(key.to_string());
            collect_visible_track_paths(child, path, out);
            path.pop();
        }
    }
    for (key, child) in &entry.components {
        if !visited.contains(key.as_str()) {
            path.push(key.clone());
            collect_visible_track_paths(child, path, out);
            path.pop();
        }
    }
}
