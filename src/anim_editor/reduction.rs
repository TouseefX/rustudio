//! 1:1 N-dimensional Visvalingam-Whyatt keyframe reduction for RusTudio's native
//! `AnimationClipEditor` (derived from `Util/KeyframeReduction/MinHeap`,
//! `Util/KeyframeReduction/Triangle`,
//! `Util/KeyframeReduction/VisvalingamWhyatt`, and
//! `Components/ReduceKeyframesDialog`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use glam::{Mat3, Quat, Vec3};

use crate::anim_editor::constants::R15_LINKS;
use crate::anim_editor::curves::{evaluate_cframe_track, evaluate_number_track};
use crate::anim_editor::model::{AnimationData, Tick, Track, TrackType};

// ============================================================================
// N-Dimensional Triangle (`Util/KeyframeReduction/Triangle`)
// ============================================================================

/// Squared Euclidean distance in $N$ dimensions: $\sum_i (p_i - q_i)^2$.
#[inline]
pub fn squared_distance(p: &[f64], q: &[f64]) -> f64 {
    p.iter()
        .zip(q.iter())
        .map(|(&a, &b)| {
            let d = a - b;
            d * d
        })
        .sum()
}

/// Computes $16 \cdot \text{Area}(\triangle abc)^2 = 4 \cdot ab \cdot ac - (ab + ac - bc)^2$
/// in $N$ dimensions (Heron's formula without `sqrt`, preserving strict area ordering).
#[inline]
pub fn triangle_area_sq16(a: &[f64], b: &[f64], c: &[f64]) -> f64 {
    let ab = squared_distance(a, b);
    let ac = squared_distance(a, c);
    let bc = squared_distance(b, c);
    let s = ab + ac - bc;
    (4.0 * ab * ac - s * s).max(0.0)
}

/// Doubly-linked N-dimensional triangle node stored in an arena and tracked
/// intrusively by [`MinHeap`] via `order` (1-indexed heap slot, `0` if absent).
#[derive(Debug, Clone)]
pub struct Triangle {
    pub a: usize,
    pub b: usize,
    pub c: usize,
    pub area: f64,
    pub prev: Option<usize>,
    pub next: Option<usize>,
    /// 1-indexed position in `MinHeap::array` (`0` when not in the heap).
    pub order: usize,
}

impl Triangle {
    pub fn new(a: usize, b: usize, c: usize) -> Self {
        Self {
            a,
            b,
            c,
            area: 0.0,
            prev: None,
            next: None,
            order: 0,
        }
    }

    /// Recomputes `self.area` using the squared Heron formula matching `Triangle:updateArea()`.
    pub fn update_area(&mut self, points: &[Vec<f64>]) {
        self.area = triangle_area_sq16(&points[self.a], &points[self.b], &points[self.c]);
    }
}

// ============================================================================
// Intrusive 1-Indexed MinHeap (`Util/KeyframeReduction/MinHeap`)
// ============================================================================

/// 1-indexed binary min-heap over arena [`Triangle`] indices with intrusive
/// `Triangle::order` tracking for $O(\log n)$ arbitrary removal.
#[derive(Debug, Clone)]
pub struct MinHeap {
    /// Index `0` is a dummy sentinel so valid heap slots are `1..=self.len()`.
    array: Vec<usize>,
}

impl Default for MinHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl MinHeap {
    pub fn new() -> Self {
        Self {
            array: vec![usize::MAX],
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        let mut array = Vec::with_capacity(cap + 1);
        array.push(usize::MAX);
        Self { array }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.array.len() - 1
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    fn compare(a_id: usize, b_id: usize, arena: &[Triangle]) -> f64 {
        arena[a_id].area - arena[b_id].area
    }

    /// Appends `tri_id` to the heap, records its 1-based `order`, sifts up,
    /// and returns the new heap length.
    pub fn push(&mut self, tri_id: usize, arena: &mut [Triangle]) -> usize {
        self.array.push(tri_id);
        let idx = self.len();
        arena[tri_id].order = idx;
        self.up(idx, arena);
        self.len()
    }

    /// Removes and returns the minimum-area triangle index at root slot `1`.
    pub fn pop(&mut self, arena: &mut [Triangle]) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        let root = self.array[1];
        arena[root].order = 0;

        let last = self.array.pop().unwrap();
        if !self.is_empty() {
            self.array[1] = last;
            arena[last].order = 1;
            self.down(1, arena);
        }
        Some(root)
    }

    /// Removes an arbitrary `tri_id` in $O(\log n)$ using its intrusive `order`.
    pub fn remove(&mut self, tri_id: usize, arena: &mut [Triangle]) {
        let idx = arena[tri_id].order;
        if idx == 0 || idx > self.len() {
            return;
        }
        arena[tri_id].order = 0;

        let last = self.array.pop().unwrap();
        if idx <= self.len() {
            self.array[idx] = last;
            arena[last].order = idx;
            if Self::compare(last, tri_id, arena) < 0.0 {
                self.up(idx, arena);
            } else {
                self.down(idx, arena);
            }
        }
    }

    fn up(&mut self, mut i: usize, arena: &mut [Triangle]) {
        let item = self.array[i];
        while i > 1 {
            let parent_idx = i / 2;
            let parent = self.array[parent_idx];
            if Self::compare(item, parent, arena) < 0.0 {
                self.array[i] = parent;
                arena[parent].order = i;
                i = parent_idx;
            } else {
                break;
            }
        }
        self.array[i] = item;
        arena[item].order = i;
    }

    fn down(&mut self, mut i: usize, arena: &mut [Triangle]) {
        let n = self.len();
        let item = self.array[i];
        loop {
            let left_idx = 2 * i;
            if left_idx > n {
                break;
            }
            let right_idx = left_idx + 1;
            let smallest_idx = if right_idx <= n
                && Self::compare(self.array[right_idx], self.array[left_idx], arena) < 0.0
            {
                right_idx
            } else {
                left_idx
            };

            let smallest = self.array[smallest_idx];
            if Self::compare(smallest, item, arena) < 0.0 {
                self.array[i] = smallest;
                arena[smallest].order = i;
                i = smallest_idx;
            } else {
                break;
            }
        }
        self.array[i] = item;
        arena[item].order = i;
    }
}

// ============================================================================
// Visvalingam-Whyatt Reducer (`Util/KeyframeReduction/VisvalingamWhyatt`)
// ============================================================================

/// N-dimensional Visvalingam-Whyatt keyframe reduction engine.
pub struct VisvalingamWhyatt;

impl VisvalingamWhyatt {
    /// Reduces a sequence of `(tick, feature_vector)` samples and returns the
    /// ticks ordered from **most important** (endpoints first) to **least important**.
    ///
    /// Taking `result[..k]` yields the optimal `k`-keyframe subset.
    pub fn reduce(ticks: &[Tick], points: &[Vec<f64>]) -> Vec<Tick> {
        debug_assert_eq!(ticks.len(), points.len());
        let n = points.len();
        if n < 3 {
            return ticks.to_vec();
        }

        let num_tris = n - 2;
        let mut arena: Vec<Triangle> = Vec::with_capacity(num_tris);
        let mut heap = MinHeap::with_capacity(num_tris);

        for i in 0..num_tris {
            let mut tri = Triangle::new(i, i + 1, i + 2);
            tri.prev = if i > 0 { Some(i - 1) } else { None };
            tri.next = if i + 1 < num_tris { Some(i + 1) } else { None };
            tri.update_area(points);
            arena.push(tri);
            heap.push(i, &mut arena);
        }

        let mut result = vec![0; n];
        result[0] = ticks[0];
        result[1] = ticks[n - 1];
        let mut write_idx = n - 1;

        while let Some(tri_id) = heap.pop(&mut arena) {
            let b_idx = arena[tri_id].b;
            let a_idx = arena[tri_id].a;
            let c_idx = arena[tri_id].c;
            let prev_id = arena[tri_id].prev;
            let next_id = arena[tri_id].next;

            result[write_idx] = ticks[b_idx];
            if write_idx > 2 {
                write_idx -= 1;
            }

            if let Some(pid) = prev_id {
                arena[pid].next = next_id;
                arena[pid].c = c_idx;
                heap.remove(pid, &mut arena);
                arena[pid].update_area(points);
                heap.push(pid, &mut arena);
            }
            if let Some(nid) = next_id {
                arena[nid].prev = prev_id;
                arena[nid].a = a_idx;
                heap.remove(nid, &mut arena);
                arena[nid].update_area(points);
                heap.push(nid, &mut arena);
            }
        }

        result
    }
}

// ============================================================================
// Animation Feature Extraction & Reduced Animation Builder
// (`Components/ReduceKeyframesDialog`)
// ============================================================================

/// Exact trace-to-angle formula from `ReduceKeyframesDialog` (`PROTO_5`).
#[inline]
fn rotation_matrix_angle(rot: Mat3) -> f64 {
    let tr = (rot.x_axis.x + rot.y_axis.y + rot.z_axis.z) as f64;
    if tr >= 3.0 {
        std::f64::consts::FRAC_PI_2
    } else if tr <= -1.0 {
        -std::f64::consts::FRAC_PI_2
    } else {
        ((tr - 1.0) * 0.5).clamp(-1.0, 1.0).acos()
    }
}

/// Orthonormalizes a 3x3 rotation matrix via unit quaternion conversion.
#[inline]
fn orthonormalize(rot: Mat3) -> Mat3 {
    Mat3::from_quat(Quat::from_mat3(&rot).normalize())
}

/// Recursively computes world-space orthonormalized rotation `R_world(part) = R_parent * R_local`.
fn world_rotation(
    part: &str,
    tracks: &BTreeMap<String, Track>,
    parent_map: &HashMap<&str, &str>,
    tick: Tick,
    cache: &mut HashMap<String, Mat3>,
    visiting: &mut BTreeSet<String>,
) -> Mat3 {
    if let Some(&cached) = cache.get(part) {
        return cached;
    }
    if !visiting.insert(part.to_string()) {
        return Mat3::IDENTITY;
    }

    let local_rot = tracks
        .get(part)
        .and_then(|t| evaluate_cframe_track(t, tick))
        .map(|xf| orthonormalize(xf.rot))
        .unwrap_or(Mat3::IDENTITY);

    let world = if let Some(&parent) = parent_map.get(part) {
        if tracks.contains_key(parent) {
            let parent_rot = world_rotation(parent, tracks, parent_map, tick, cache, visiting);
            orthonormalize(parent_rot * local_rot)
        } else {
            local_rot
        }
    } else {
        local_rot
    };

    visiting.remove(part);
    cache.insert(part.to_string(), world);
    world
}

/// Computes the Visvalingam-Whyatt keyframe importance ordering for `data`.
///
/// Builds N-dimensional samples per summary tick:
/// `[tick_norm, root_pos_xyz, rel_bone_rot_angles..., facs_values...]`
/// and returns ticks ordered from most important (first & last endpoints first)
/// to least important, so taking `ordered_ticks[..k]` yields the optimal
/// `k`-keyframe reduced animation.
pub fn compute_reduction_order(data: &AnimationData) -> Vec<Tick> {
    let ticks = data.get_summary_ticks();
    if ticks.len() < 3 {
        return ticks;
    }

    let parent_map: HashMap<&str, &str> = R15_LINKS.iter().copied().collect();

    // Collect tracks across instances (preferring "Root", then any remaining instances).
    let root_tracks = data.instances.get("Root").map(|inst| &inst.tracks);

    // Identify root part track for position & relative rotation reference.
    let root_part_name: Option<String> = root_tracks.and_then(|tracks| {
        if tracks.contains_key("HumanoidRootPart") {
            Some("HumanoidRootPart".to_string())
        } else if tracks.contains_key("LowerTorso") {
            Some("LowerTorso".to_string())
        } else {
            tracks
                .iter()
                .find(|(name, t)| {
                    t.track_type == TrackType::CFrame && !parent_map.contains_key(name.as_str())
                })
                .or_else(|| {
                    tracks
                        .iter()
                        .find(|(_, t)| t.track_type == TrackType::CFrame)
                })
                .map(|(name, _)| name.clone())
        }
    });

    let t_min = ticks[0] as f64;
    let t_span = ((ticks[ticks.len() - 1] - ticks[0]) as f64).max(1.0);

    let mut points: Vec<Vec<f64>> = Vec::with_capacity(ticks.len());

    for &t in &ticks {
        let tick_norm = (t as f64 - t_min) / t_span;
        let mut sample = Vec::new();
        sample.push(tick_norm);

        if let Some(tracks) = root_tracks {
            let mut rot_cache: HashMap<String, Mat3> = HashMap::new();
            let mut visiting: BTreeSet<String> = BTreeSet::new();

            // Root position XYZ (if a root CFrame track exists).
            let root_rot_inv = if let Some(ref root_name) = root_part_name {
                let pos = tracks
                    .get(root_name)
                    .and_then(|tr| evaluate_cframe_track(tr, t))
                    .map(|xf| xf.pos)
                    .unwrap_or(Vec3::ZERO);
                sample.push(pos.x as f64);
                sample.push(pos.y as f64);
                sample.push(pos.z as f64);

                let root_world = world_rotation(
                    root_name,
                    tracks,
                    &parent_map,
                    t,
                    &mut rot_cache,
                    &mut visiting,
                );
                Some(root_world.transpose())
            } else {
                None
            };

            // Relative bone rotation angles & FACS/scalar values in deterministic BTreeMap order.
            for (name, track) in tracks {
                match track.track_type {
                    TrackType::CFrame => {
                        let part_world = world_rotation(
                            name,
                            tracks,
                            &parent_map,
                            t,
                            &mut rot_cache,
                            &mut visiting,
                        );
                        let rel = match root_rot_inv {
                            Some(inv) => part_world * inv,
                            None => part_world,
                        };
                        sample.push(rotation_matrix_angle(rel));
                    }
                    TrackType::Facs | TrackType::Number | TrackType::Angle => {
                        sample.push(evaluate_number_track(track, t).unwrap_or(0.0) as f64);
                    }
                    TrackType::Position | TrackType::EulerAngles => {
                        if let Some(v) = crate::anim_editor::curves::evaluate_track_value(track, t)
                            .and_then(|kv| kv.as_position())
                        {
                            sample.push(v.x as f64);
                            sample.push(v.y as f64);
                            sample.push(v.z as f64);
                        } else {
                            sample.extend_from_slice(&[0.0, 0.0, 0.0]);
                        }
                    }
                    TrackType::Quaternion => {
                        let rot = crate::anim_editor::curves::evaluate_quaternion_track(track, t)
                            .map(Mat3::from_quat)
                            .unwrap_or(Mat3::IDENTITY);
                        sample.push(rotation_matrix_angle(rot));
                    }
                    _ => {}
                }
            }
        }

        // Also include any non-Root instance tracks if present.
        for (inst_name, inst) in &data.instances {
            if inst_name == "Root" {
                continue;
            }
            for track in inst.tracks.values() {
                match track.track_type {
                    TrackType::CFrame => {
                        if let Some(xf) = evaluate_cframe_track(track, t) {
                            sample.push(xf.pos.x as f64);
                            sample.push(xf.pos.y as f64);
                            sample.push(xf.pos.z as f64);
                            sample.push(rotation_matrix_angle(orthonormalize(xf.rot)));
                        } else {
                            sample.extend_from_slice(&[0.0, 0.0, 0.0, 0.0]);
                        }
                    }
                    TrackType::Facs | TrackType::Number | TrackType::Angle => {
                        sample.push(evaluate_number_track(track, t).unwrap_or(0.0) as f64);
                    }
                    _ => {}
                }
            }
        }

        points.push(sample);
    }

    VisvalingamWhyatt::reduce(&ticks, &points)
}

/// Builds a reduced copy of `original` keeping only the first `keep_count`
/// prioritized ticks from `ordered_ticks` (`ReduceKeyframesDialog.previewVisvalingamWhyatt`).
pub fn build_reduced_animation(
    original: &AnimationData,
    ordered_ticks: &[Tick],
    keep_count: usize,
) -> AnimationData {
    let k = keep_count.min(ordered_ticks.len());
    let keep_set: BTreeSet<Tick> = ordered_ticks[..k].iter().copied().collect();

    let mut reduced = original.clone();
    for inst in reduced.instances.values_mut() {
        for track in inst.tracks.values_mut() {
            let mut path = Vec::new();
            track.traverse_leaves_mut(&mut path, &mut |_p, leaf| {
                leaf.keyframes.retain(|t| keep_set.contains(t));
                leaf.data.retain(|t, _| keep_set.contains(t));
                for kf in leaf.data.values_mut() {
                    kf.left_slope = None;
                    kf.right_slope = None;
                }
            });
        }
    }

    reduced.validate_keyframe_names();
    reduced.set_end_tick();
    reduced
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim_editor::model::{Keyframe, KeyframeValue, RotationOrder};
    use rustudio_core::math::Xform;

    #[test]
    fn test_min_heap_push_pop_and_intrusive_remove() {
        let mut arena = vec![
            Triangle::new(0, 1, 2),
            Triangle::new(1, 2, 3),
            Triangle::new(2, 3, 4),
            Triangle::new(3, 4, 5),
            Triangle::new(4, 5, 6),
        ];
        arena[0].area = 50.0;
        arena[1].area = 10.0;
        arena[2].area = 30.0;
        arena[3].area = 20.0;
        arena[4].area = 40.0;

        let mut heap = MinHeap::new();
        for i in 0..arena.len() {
            heap.push(i, &mut arena);
        }
        assert_eq!(heap.len(), 5);

        // Verify intrusive order invariant for every element.
        for (i, tri) in arena.iter().enumerate() {
            assert_eq!(heap.array[tri.order], i);
        }

        // Remove middle element (area 20.0, id 3) and update its area to 5.0 then re-push.
        heap.remove(3, &mut arena);
        assert_eq!(arena[3].order, 0);
        assert_eq!(heap.len(), 4);
        arena[3].area = 5.0;
        heap.push(3, &mut arena);

        // Pop all elements and verify strict ascending area order.
        let mut popped_areas = Vec::new();
        while let Some(id) = heap.pop(&mut arena) {
            assert_eq!(arena[id].order, 0);
            popped_areas.push(arena[id].area);
        }
        assert_eq!(popped_areas, vec![5.0, 10.0, 30.0, 40.0, 50.0]);
        assert!(heap.is_empty());
    }

    #[test]
    fn test_triangle_area_sq16() {
        // Right triangle in 2D: (0,0), (3,0), (0,4) -> Area = 6, 16 * Area^2 = 576.
        let a = vec![0.0, 0.0];
        let b = vec![3.0, 0.0];
        let c = vec![0.0, 4.0];
        let area_sq16 = triangle_area_sq16(&a, &b, &c);
        assert!((area_sq16 - 576.0).abs() < 1e-9);

        // Collinear points in 4D -> area = 0.
        let p0 = vec![0.0, 1.0, 2.0, 3.0];
        let p1 = vec![0.5, 2.0, 4.0, 6.0];
        let p2 = vec![1.0, 3.0, 6.0, 9.0];
        assert!(triangle_area_sq16(&p0, &p1, &p2) < 1e-12);
    }

    #[test]
    fn test_collinear_removed_first_and_endpoints_preserved() {
        // 5 points:
        // t=0:   y=0   (start endpoint -> preserved)
        // t=100: y=5   (collinear on line (0,0)-(200,10) -> area 0, removed FIRST!)
        // t=200: y=10  (sharp peak -> preserved with high priority!)
        // t=300: y=5   (collinear on line (200,10)-(400,0) -> area 0, removed SECOND!)
        // t=400: y=0   (end endpoint -> preserved)
        let ticks = vec![0, 100, 200, 300, 400];
        let points = vec![
            vec![0.0, 0.0],
            vec![0.25, 5.0],
            vec![0.5, 10.0],
            vec![0.75, 5.0],
            vec![1.0, 0.0],
        ];

        let order = VisvalingamWhyatt::reduce(&ticks, &points);
        assert_eq!(order.len(), 5);
        // Endpoints are always first two entries.
        assert_eq!(order[0], 0);
        assert_eq!(order[1], 400);
        // Peak at tick 200 is the most important interior keyframe.
        assert_eq!(order[2], 200);
        // Collinear ticks 100 and 300 are the least important (last two).
        let least: BTreeSet<Tick> = order[3..].iter().copied().collect();
        assert_eq!(least, BTreeSet::from([100, 300]));
    }

    #[test]
    fn test_compute_reduction_order_and_build_reduced_animation() {
        let mut anim = AnimationData::new_rig_animation("ReductionTest");
        let root_track = anim.add_track(
            "Root",
            "HumanoidRootPart",
            TrackType::CFrame,
            TrackType::EulerAngles,
            RotationOrder::XYZ,
        );

        // Linear ramp up from 0..200, then linear ramp down from 200..400.
        // Interior ticks 100 and 300 are collinear; tick 200 is the sharp apex.
        let samples = [
            (0, Vec3::new(0.0, 0.0, 0.0)),
            (100, Vec3::new(0.0, 5.0, 0.0)),
            (200, Vec3::new(0.0, 10.0, 0.0)),
            (300, Vec3::new(0.0, 5.0, 0.0)),
            (400, Vec3::new(0.0, 0.0, 0.0)),
        ];
        for (tick, pos) in samples {
            root_track.add_keyframe(
                tick,
                Keyframe::new_pose(KeyframeValue::CFrame(Xform {
                    pos,
                    rot: Mat3::IDENTITY,
                })),
            );
        }
        anim.set_end_tick();

        let ordered = compute_reduction_order(&anim);
        assert_eq!(ordered.len(), 5);
        assert_eq!(ordered[0], 0);
        assert_eq!(ordered[1], 400);
        assert_eq!(ordered[2], 200);

        // Reduce to exactly 3 keyframes -> must keep {0, 200, 400}.
        let reduced_3 = build_reduced_animation(&anim, &ordered, 3);
        assert_eq!(reduced_3.get_summary_ticks(), vec![0, 200, 400]);
        assert_eq!(reduced_3.metadata.end_tick, 400);

        // Reduce to exactly 2 keyframes -> must keep endpoints {0, 400}.
        let reduced_2 = build_reduced_animation(&anim, &ordered, 2);
        assert_eq!(reduced_2.get_summary_ticks(), vec![0, 400]);

        // Reduce with keep_count >= total -> keeps all 5 keyframes.
        let reduced_all = build_reduced_animation(&anim, &ordered, 10);
        assert_eq!(reduced_all.get_summary_ticks(), vec![0, 100, 200, 300, 400]);
    }
}
