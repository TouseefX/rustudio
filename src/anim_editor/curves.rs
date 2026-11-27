//! 1:1 curve math, tangent computation, Euler unwrapping, curve generation,
//! and analytical extent evaluation for RusTudio's native `AnimationClipEditor`
//! (derived from `Util/KeyframeUtils`, `Util/CurveUtils`,
//! `Util/TrackUtils`, `Thunks/SplitTrack`, and
//! `Thunks/Selection/GenerateCurve`).

use std::collections::{BTreeMap, BTreeSet};

use glam::{Mat3, Quat, Vec3};
use rustudio_core::math::Xform;

use crate::anim_editor::constants::{
    CLAMPED_AUTO_TANGENT_THRESHOLD, PROP_POSITION, PROP_ROTATION, PROP_X, PROP_Y, PROP_Z,
};
use crate::anim_editor::model::{
    nearest_tick, unwrap_euler, AnimationData, EasingDirection, EasingStyle, KeyInterpolationMode,
    Keyframe, KeyframeValue, RotationOrder, Tick, Track, TrackType,
};
use crate::anim_editor::state::TangentSide;

// ============================================================================
// Easing Math (`TweenService:GetValue` / `Enum.PoseEasingStyle`)
// ============================================================================

/// Exact Roblox `PoseEasingStyle` / `Animator` easing function.
///
/// Supports `Linear`, `Constant`, `Cubic` (deprecated, with `In`/`Out` swapped
/// matching real Roblox Studio), `CubicV2`, `Bounce` (including Studio's
/// `InOut` behavior), and `Elastic` across `In`, `Out`, and `InOut` directions.
pub fn easing_alpha(alpha: f32, style: EasingStyle, direction: EasingDirection) -> f32 {
    let t = alpha.clamp(0.0, 1.0);
    match style {
        EasingStyle::Linear => t,
        EasingStyle::Constant => match direction {
            EasingDirection::In => {
                if t > 0.0 {
                    1.0
                } else {
                    0.0
                }
            }
            EasingDirection::Out => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            EasingDirection::InOut => {
                if t >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
        },
        // Deprecated PoseEasingStyle.Cubic has In and Out swapped in Roblox Studio.
        EasingStyle::Cubic => match direction {
            EasingDirection::In => {
                let u = 1.0 - t;
                1.0 - u * u * u
            }
            EasingDirection::Out => t * t * t,
            EasingDirection::InOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let u = -2.0 * t + 2.0;
                    1.0 - (u * u * u) * 0.5
                }
            }
        },
        EasingStyle::CubicV2 => match direction {
            EasingDirection::In => t * t * t,
            EasingDirection::Out => {
                let u = 1.0 - t;
                1.0 - u * u * u
            }
            EasingDirection::InOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let u = -2.0 * t + 2.0;
                    1.0 - (u * u * u) * 0.5
                }
            }
        },
        EasingStyle::Bounce => match direction {
            EasingDirection::In => 1.0 - bounce_out(1.0 - t),
            EasingDirection::Out => bounce_out(t),
            // Real Roblox Studio's PoseEasingStyle.Bounce + InOut uses bounceIn on both halves.
            EasingDirection::InOut => {
                if t < 0.5 {
                    0.5 * (1.0 - bounce_out(1.0 - 2.0 * t))
                } else {
                    0.5 + 0.5 * (1.0 - bounce_out(2.0 - 2.0 * t))
                }
            }
        },
        EasingStyle::Elastic => {
            if t <= 0.0 {
                return 0.0;
            }
            if t >= 1.0 {
                return 1.0;
            }
            let tau = std::f32::consts::TAU;
            match direction {
                EasingDirection::In => {
                    let p = 0.3_f32;
                    let s = p * 0.25;
                    let u = t - 1.0;
                    -(2.0_f32.powf(10.0 * u)) * ((u - s) * tau / p).sin()
                }
                EasingDirection::Out => {
                    let p = 0.3_f32;
                    let s = p * 0.25;
                    2.0_f32.powf(-10.0 * t) * ((t - s) * tau / p).sin() + 1.0
                }
                EasingDirection::InOut => {
                    let p = 0.45_f32;
                    let s = p * 0.25;
                    let u = 2.0 * t - 1.0;
                    if t < 0.5 {
                        -0.5 * 2.0_f32.powf(10.0 * u) * ((u - s) * tau / p).sin()
                    } else {
                        0.5 * 2.0_f32.powf(-10.0 * u) * ((u - s) * tau / p).sin() + 1.0
                    }
                }
            }
        }
    }
}

#[inline]
fn bounce_out(t: f32) -> f32 {
    const N1: f32 = 7.5625;
    const D1: f32 = 2.75;
    if t < 1.0 / D1 {
        N1 * t * t
    } else if t < 2.0 / D1 {
        let u = t - 1.5 / D1;
        N1 * u * u + 0.75
    } else if t < 2.5 / D1 {
        let u = t - 2.25 / D1;
        N1 * u * u + 0.9375
    } else {
        let u = t - 2.625 / D1;
        N1 * u * u + 0.984375
    }
}

// ============================================================================
// Keyframe Binary Search (`KeyframeUtils.findNearestKeyframesProperly`)
// ============================================================================

/// Exact binary search matching `KeyframeUtils.findNearestKeyframesProperly`.
///
/// Returns `(exact_idx, prev_idx, next_idx)` as 0-based indices into `keyframes`:
/// - Empty slice -> `(None, None, None)`
/// - `tick < keyframes[0]` -> `(None, None, Some(0))`
/// - `tick > keyframes[last]` -> `(None, Some(len - 1), None)`
/// - Exact match at `i` -> `(Some(i), None, None)`
/// - Strictly between `i` and `i + 1` -> `(None, Some(i), Some(i + 1))`
pub fn find_nearest_keyframes(
    keyframes: &[Tick],
    tick: Tick,
) -> (Option<usize>, Option<usize>, Option<usize>) {
    if keyframes.is_empty() {
        return (None, None, None);
    }
    if tick < keyframes[0] {
        return (None, None, Some(0));
    }
    let last = keyframes.len() - 1;
    if tick > keyframes[last] {
        return (None, Some(last), None);
    }
    match keyframes.binary_search(&tick) {
        Ok(idx) => (Some(idx), None, None),
        Err(insert_idx) => (None, Some(insert_idx - 1), Some(insert_idx)),
    }
}

// ============================================================================
// Tangent / Slope Computation (`KeyframeUtils.getSlope` & `getSlopes`)
// ============================================================================

#[inline]
fn effective_interpolation_mode(kf: &Keyframe) -> KeyInterpolationMode {
    kf.interpolation_mode
        .or_else(|| kf.easing_style.map(EasingStyle::to_key_interpolation))
        .unwrap_or(KeyInterpolationMode::Cubic)
}

/// Clamped Catmull-Rom auto-tangent helper matching `KeyframeUtils` `PROTO_11`/`PROTO_12`.
fn compute_auto_tangent(track: &Track, idx: usize) -> f32 {
    if idx == 0 || idx + 1 >= track.keyframes.len() {
        return 0.0;
    }
    let t0 = track.keyframes[idx - 1];
    let t1 = track.keyframes[idx];
    let t2 = track.keyframes[idx + 1];

    let dt_prev = (t1 - t0) as f32;
    let dt_next = (t2 - t1) as f32;
    if dt_prev.abs() <= f32::EPSILON || dt_next.abs() <= f32::EPSILON {
        return 0.0;
    }

    if track.track_type == TrackType::Quaternion {
        return 0.5 * (1.0 / dt_next + 1.0 / dt_prev);
    }

    let (Some(k0), Some(k1), Some(k2)) = (
        track.data.get(&t0),
        track.data.get(&t1),
        track.data.get(&t2),
    ) else {
        return 0.0;
    };
    let (Some(v0), Some(v1), Some(v2)) = (k0.value.as_f32(), k1.value.as_f32(), k2.value.as_f32())
    else {
        return 0.0;
    };

    // Unclamped Catmull-Rom finite-difference average:
    // 0.5 * ((v2 - v1) / (t2 - t1) + (v0 - v1) / (t0 - t1))
    let s_raw = 0.5 * ((v2 - v1) / dt_next + (v1 - v0) / dt_prev);

    let span = v2 - v0;
    if span.abs() <= f32::EPSILON {
        // v0 == v2: v1 is either flat or a strict local extremum -> flatten to 0.
        return 0.0;
    }

    let u = (v1 - v0) / span;
    let thresh = CLAMPED_AUTO_TANGENT_THRESHOLD;
    let factor = (u / thresh).min((1.0 - u) / thresh).min(1.0).max(0.0);
    s_raw * factor
}

fn get_slope_inner(track: &Track, idx: usize, side: TangentSide, is_recursive: bool) -> f32 {
    let tick = track.keyframes[idx];
    let Some(kf) = track.data.get(&tick) else {
        return 0.0;
    };
    let is_quat = track.track_type == TrackType::Quaternion;

    match side {
        TangentSide::Right => match effective_interpolation_mode(kf) {
            KeyInterpolationMode::Constant => 0.0,
            KeyInterpolationMode::Linear => {
                if idx + 1 >= track.keyframes.len() {
                    return 0.0;
                }
                let next_tick = track.keyframes[idx + 1];
                let dt = (next_tick - tick) as f32;
                if dt.abs() <= f32::EPSILON {
                    return 0.0;
                }
                if is_quat {
                    1.0 / dt
                } else {
                    let Some(next_kf) = track.data.get(&next_tick) else {
                        return 0.0;
                    };
                    match (kf.value.as_f32(), next_kf.value.as_f32()) {
                        (Some(v0), Some(v1)) => (v1 - v0) / dt,
                        _ => 0.0,
                    }
                }
            }
            KeyInterpolationMode::Cubic => {
                if let Some(rs) = kf.right_slope {
                    rs
                } else if !is_recursive {
                    get_slope_inner(track, idx, TangentSide::Left, true)
                } else {
                    compute_auto_tangent(track, idx)
                }
            }
        },
        TangentSide::Left => {
            if idx == 0 {
                return 0.0;
            }
            let prev_tick = track.keyframes[idx - 1];
            let Some(prev_kf) = track.data.get(&prev_tick) else {
                return 0.0;
            };
            match effective_interpolation_mode(prev_kf) {
                KeyInterpolationMode::Constant => 0.0,
                KeyInterpolationMode::Linear => {
                    let dt = (tick - prev_tick) as f32;
                    if dt.abs() <= f32::EPSILON {
                        return 0.0;
                    }
                    if is_quat {
                        1.0 / dt
                    } else {
                        match (prev_kf.value.as_f32(), kf.value.as_f32()) {
                            (Some(v0), Some(v1)) => (v1 - v0) / dt,
                            _ => 0.0,
                        }
                    }
                }
                KeyInterpolationMode::Cubic => {
                    if let Some(ls) = kf.left_slope {
                        ls
                    } else if !is_recursive {
                        get_slope_inner(track, idx, TangentSide::Right, true)
                    } else {
                        compute_auto_tangent(track, idx)
                    }
                }
            }
        }
    }
}

/// Computes the left or right tangent slope at an exact keyframe tick
/// (`KeyframeUtils.getSlope`).
///
/// Returns `None` if `tick` is not an existing keyframe in `track`.
pub fn get_slope(track: &Track, tick: Tick, side: TangentSide) -> Option<f32> {
    let (Some(idx), _, _) = find_nearest_keyframes(&track.keyframes, tick) else {
        return None;
    };
    Some(get_slope_inner(track, idx, side, false))
}

/// Computes `(left_slope, right_slope)` at any tick (`KeyframeUtils.getSlopes`).
///
/// - Empty track -> `(None, None)`
/// - Existing keyframe -> `(get_slope(Left), get_slope(Right))`
/// - Outside keyframe range -> `(Some(0.0), Some(0.0))`
/// - Strictly between keyframes `t0 < tick < t1`:
///   - `Constant` -> `(Some(0.0), Some(0.0))`
///   - `Linear` -> linear slope on both sides
///   - `Cubic` -> analytical Hermite derivative; for `Quaternion` tracks,
///     renormalizes `(d_alpha / alpha, d_alpha / (1 - alpha))` for sub-interval splitting.
pub fn get_slopes(track: &Track, tick: Tick) -> (Option<f32>, Option<f32>) {
    if track.keyframes.is_empty() {
        return (None, None);
    }

    let (exact, prev, next) = find_nearest_keyframes(&track.keyframes, tick);
    if let Some(idx) = exact {
        return (
            Some(get_slope_inner(track, idx, TangentSide::Left, false)),
            Some(get_slope_inner(track, idx, TangentSide::Right, false)),
        );
    }

    let (Some(p_idx), Some(n_idx)) = (prev, next) else {
        return (Some(0.0), Some(0.0));
    };

    let t0 = track.keyframes[p_idx];
    let t1 = track.keyframes[n_idx];
    let (Some(k0), Some(k1)) = (track.data.get(&t0), track.data.get(&t1)) else {
        return (Some(0.0), Some(0.0));
    };

    let dt = (t1 - t0) as f32;
    if dt.abs() <= f32::EPSILON {
        return (Some(0.0), Some(0.0));
    }

    match effective_interpolation_mode(k0) {
        KeyInterpolationMode::Constant => (Some(0.0), Some(0.0)),
        KeyInterpolationMode::Linear => {
            if track.track_type == TrackType::Quaternion {
                let s = 1.0 / dt;
                (Some(s), Some(s))
            } else {
                let (Some(v0), Some(v1)) = (k0.value.as_f32(), k1.value.as_f32()) else {
                    return (Some(0.0), Some(0.0));
                };
                let s = (v1 - v0) / dt;
                (Some(s), Some(s))
            }
        }
        KeyInterpolationMode::Cubic => {
            let u = (tick - t0) as f32 / dt;
            let s0 = get_slope_inner(track, p_idx, TangentSide::Right, false);
            let s1 = get_slope_inner(track, n_idx, TangentSide::Left, false);

            let dh00 = 6.0 * u * (u - 1.0);
            let dh10 = (u - 1.0) * (3.0 * u - 1.0);
            let dh01 = -dh00;
            let dh11 = u * (3.0 * u - 2.0);

            if track.track_type == TrackType::Quaternion {
                let u2 = u * u;
                let h10 = ((u - 2.0) * u + 1.0) * u;
                let h01 = u2 * (3.0 - 2.0 * u);
                let h11 = u2 * (u - 1.0);

                let alpha = h10 * dt * s0 + h01 + h11 * dt * s1;
                let d_alpha = dh01 / dt + dh10 * s0 + dh11 * s1;

                let left = if alpha.abs() > 1e-8 {
                    d_alpha / alpha
                } else {
                    0.0
                };
                let right = if (1.0 - alpha).abs() > 1e-8 {
                    d_alpha / (1.0 - alpha)
                } else {
                    0.0
                };
                (Some(left), Some(right))
            } else {
                let (Some(v0), Some(v1)) = (k0.value.as_f32(), k1.value.as_f32()) else {
                    return (Some(0.0), Some(0.0));
                };
                let s = (dh00 * v0 + dh01 * v1) / dt + dh10 * s0 + dh11 * s1;
                (Some(s), Some(s))
            }
        }
    }
}

// ============================================================================
// Track Evaluation (`KeyframeUtils.blendCurveKeyframes`, `blendKeyframes`, `getValue`)
// ============================================================================

#[inline]
fn hermite_eval(v0: f32, s0: f32, v1: f32, s1: f32, dt: f32, u: f32) -> f32 {
    let u2 = u * u;
    let h00 = u2 * (2.0 * u - 3.0) + 1.0;
    let h10 = ((u - 2.0) * u + 1.0) * u;
    let h01 = u2 * (3.0 - 2.0 * u);
    let h11 = u2 * (u - 1.0);
    h00 * v0 + h10 * dt * s0 + h01 * v1 + h11 * dt * s1
}

#[inline]
fn hermite_quat_alpha(s0: f32, s1: f32, dt: f32, u: f32) -> f32 {
    let u2 = u * u;
    let h10 = ((u - 2.0) * u + 1.0) * u;
    let h01 = u2 * (3.0 - 2.0 * u);
    let h11 = u2 * (u - 1.0);
    h10 * dt * s0 + h01 + h11 * dt * s1
}

/// Evaluates a scalar track (`Number`, `Angle`, or `Facs`) at `tick`.
///
/// Supports cubic Hermite interpolation, Linear, Constant, legacy easing styles,
/// and clamps `Facs` tracks to `[0.0, 1.0]`.
pub fn evaluate_number_track(track: &Track, tick: Tick) -> Option<f32> {
    if track.keyframes.is_empty() || track.data.is_empty() {
        return None;
    }

    let (exact, prev, next) = find_nearest_keyframes(&track.keyframes, tick);
    let raw = match (exact, prev, next) {
        (Some(idx), _, _) | (None, None, Some(idx)) | (None, Some(idx), None) => {
            track.data.get(&track.keyframes[idx])?.value.as_f32()?
        }
        (None, Some(p_idx), Some(n_idx)) => {
            let t0 = track.keyframes[p_idx];
            let t1 = track.keyframes[n_idx];
            let k0 = track.data.get(&t0)?;
            let k1 = track.data.get(&t1)?;
            let v0 = k0.value.as_f32()?;
            let v1 = k1.value.as_f32()?;
            let dt = (t1 - t0) as f32;
            let u = (tick - t0) as f32 / dt;

            if track.is_curve_track {
                match effective_interpolation_mode(k0) {
                    KeyInterpolationMode::Constant => v0,
                    KeyInterpolationMode::Linear => v0 + (v1 - v0) * u,
                    KeyInterpolationMode::Cubic => {
                        let s0 = get_slope_inner(track, p_idx, TangentSide::Right, false);
                        let s1 = get_slope_inner(track, n_idx, TangentSide::Left, false);
                        hermite_eval(v0, s0, v1, s1, dt, u)
                    }
                }
            } else {
                let style = k0.easing_style.unwrap_or(EasingStyle::Linear);
                let dir = k0.easing_direction.unwrap_or(EasingDirection::In);
                let alpha = easing_alpha(u, style, dir);
                v0 + (v1 - v0) * alpha
            }
        }
        (None, None, None) => return None,
    };

    let result = if track.track_type == TrackType::Facs {
        raw.clamp(0.0, 1.0)
    } else {
        raw
    };
    Some(result)
}

/// Evaluates a `Quaternion` track at `tick`.
///
/// Uses Hermite `0 -> 1` slerp progress per segment for `Cubic`, linear slerp
/// for `Linear`, and step hold for `Constant`.
pub fn evaluate_quaternion_track(track: &Track, tick: Tick) -> Option<Quat> {
    if track.keyframes.is_empty() || track.data.is_empty() {
        return None;
    }

    let (exact, prev, next) = find_nearest_keyframes(&track.keyframes, tick);
    match (exact, prev, next) {
        (Some(idx), _, _) | (None, None, Some(idx)) | (None, Some(idx), None) => Some(
            track
                .data
                .get(&track.keyframes[idx])?
                .value
                .as_quat()?
                .normalize(),
        ),
        (None, Some(p_idx), Some(n_idx)) => {
            let t0 = track.keyframes[p_idx];
            let t1 = track.keyframes[n_idx];
            let k0 = track.data.get(&t0)?;
            let k1 = track.data.get(&t1)?;
            let q0 = k0.value.as_quat()?.normalize();
            let q1 = k1.value.as_quat()?.normalize();
            let dt = (t1 - t0) as f32;
            let u = (tick - t0) as f32 / dt;

            if track.is_curve_track {
                match effective_interpolation_mode(k0) {
                    KeyInterpolationMode::Constant => Some(q0),
                    KeyInterpolationMode::Linear => Some(q0.slerp(q1, u).normalize()),
                    KeyInterpolationMode::Cubic => {
                        let s0 = get_slope_inner(track, p_idx, TangentSide::Right, false);
                        let s1 = get_slope_inner(track, n_idx, TangentSide::Left, false);
                        let alpha = hermite_quat_alpha(s0, s1, dt, u);
                        Some(q0.slerp(q1, alpha).normalize())
                    }
                }
            } else {
                let style = k0.easing_style.unwrap_or(EasingStyle::Linear);
                let dir = k0.easing_direction.unwrap_or(EasingDirection::In);
                let alpha = easing_alpha(u, style, dir);
                Some(q0.slerp(q1, alpha).normalize())
            }
        }
        (None, None, None) => None,
    }
}

/// Evaluates any track (leaf or composite) at `tick`, matching `KeyframeUtils.getValue`.
pub fn evaluate_track_value(track: &Track, tick: Tick) -> Option<KeyframeValue> {
    // Leaf track evaluation
    if !track.keyframes.is_empty() && !track.data.is_empty() {
        return match track.track_type {
            TrackType::Number => evaluate_number_track(track, tick).map(KeyframeValue::Number),
            TrackType::Angle => evaluate_number_track(track, tick).map(KeyframeValue::Angle),
            TrackType::Facs => evaluate_number_track(track, tick).map(KeyframeValue::Facs),
            TrackType::Quaternion => {
                evaluate_quaternion_track(track, tick).map(KeyframeValue::Quaternion)
            }
            _ => evaluate_leaf_generic(track, tick),
        };
    }

    // Composite track evaluation
    match track.track_type {
        TrackType::CFrame => {
            let pos_val = track
                .components
                .get(PROP_POSITION)
                .and_then(|t| evaluate_track_value(t, tick))
                .and_then(KeyframeValue::as_position);

            let rot_mat = track.components.get(PROP_ROTATION).and_then(|rot_track| {
                if rot_track.track_type == TrackType::EulerAngles {
                    let angles = evaluate_track_value(rot_track, tick)?.as_position()?;
                    let order = rot_track.euler_angles_order.unwrap_or_default();
                    Some(order.from_euler_angles(angles))
                } else {
                    let q = evaluate_quaternion_track(rot_track, tick)?;
                    Some(Mat3::from_quat(q))
                }
            });

            if pos_val.is_none() && rot_mat.is_none() {
                return None;
            }
            Some(KeyframeValue::CFrame(Xform {
                pos: pos_val.unwrap_or(Vec3::ZERO),
                rot: rot_mat.unwrap_or(Mat3::IDENTITY),
            }))
        }
        TrackType::Position | TrackType::EulerAngles => {
            let x = track
                .components
                .get(PROP_X)
                .and_then(|t| evaluate_number_track(t, tick));
            let y = track
                .components
                .get(PROP_Y)
                .and_then(|t| evaluate_number_track(t, tick));
            let z = track
                .components
                .get(PROP_Z)
                .and_then(|t| evaluate_number_track(t, tick));
            if x.is_none() && y.is_none() && z.is_none() {
                return None;
            }
            Some(KeyframeValue::Position(Vec3::new(
                x.unwrap_or(0.0),
                y.unwrap_or(0.0),
                z.unwrap_or(0.0),
            )))
        }
        _ => None,
    }
}

fn quat_to_rotvec(q: Quat) -> Vec3 {
    let mut q = q.normalize();
    if q.w < 0.0 {
        q = -q;
    }
    let v = Vec3::new(q.x, q.y, q.z);
    let len = v.length();
    if len <= 1e-7 {
        Vec3::ZERO
    } else {
        let angle = 2.0 * len.atan2(q.w);
        v * (angle / len)
    }
}

fn rotvec_to_quat(rv: Vec3) -> Quat {
    let angle = rv.length();
    if angle <= 1e-7 {
        Quat::IDENTITY
    } else {
        let half = 0.5 * angle;
        let s = half.sin() / angle;
        Quat::from_xyzw(rv.x * s, rv.y * s, rv.z * s, half.cos()).normalize()
    }
}

/// Interpolates a `KeyframeSequence` `Pose` `CFrame` using Roblox Studio's
/// `Animator` rotation-vector (exponential map) lerp `exp((1-a)*log(q0) + a*log(q1))`.
fn lerp_pose_cframe(a: Xform, b: Xform, alpha: f32) -> Xform {
    if alpha == 0.0 {
        return a;
    }
    if alpha == 1.0 {
        return b;
    }
    let pos = a.pos.lerp(b.pos, alpha);
    let rv0 = quat_to_rotvec(Quat::from_mat3(&a.rot));
    let rv1 = quat_to_rotvec(Quat::from_mat3(&b.rot));
    let q = rotvec_to_quat(rv0.lerp(rv1, alpha));
    Xform {
        pos,
        rot: Mat3::from_quat(q),
    }
}

fn evaluate_leaf_generic(track: &Track, tick: Tick) -> Option<KeyframeValue> {
    let (exact, prev, next) = find_nearest_keyframes(&track.keyframes, tick);
    match (exact, prev, next) {
        (Some(idx), _, _) | (None, None, Some(idx)) | (None, Some(idx), None) => {
            Some(track.data.get(&track.keyframes[idx])?.value)
        }
        (None, Some(p_idx), Some(n_idx)) => {
            let t0 = track.keyframes[p_idx];
            let t1 = track.keyframes[n_idx];
            let k0 = track.data.get(&t0)?;
            let k1 = track.data.get(&t1)?;
            let u = (tick - t0) as f32 / (t1 - t0) as f32;

            if track.is_curve_track {
                match effective_interpolation_mode(k0) {
                    KeyInterpolationMode::Constant => Some(k0.value),
                    KeyInterpolationMode::Linear | KeyInterpolationMode::Cubic => {
                        Some(k0.value.lerp(k1.value, u))
                    }
                }
            } else {
                let style = k0.easing_style.unwrap_or(EasingStyle::Linear);
                let dir = k0.easing_direction.unwrap_or(EasingDirection::In);
                let alpha = easing_alpha(u, style, dir);
                match (k0.value, k1.value) {
                    (KeyframeValue::CFrame(a), KeyframeValue::CFrame(b)) => {
                        Some(KeyframeValue::CFrame(lerp_pose_cframe(a, b, alpha)))
                    }
                    _ => Some(k0.value.lerp(k1.value, alpha)),
                }
            }
        }
        (None, None, None) => None,
    }
}

/// Evaluates a `CFrame`, `Quaternion`, or `EulerAngles` track into an `Xform` at `tick`.
pub fn evaluate_cframe_track(track: &Track, tick: Tick) -> Option<Xform> {
    if track.track_type == TrackType::EulerAngles {
        let angles = evaluate_track_value(track, tick)?.as_position()?;
        let order = track.euler_angles_order.unwrap_or_default();
        return Some(Xform {
            pos: Vec3::ZERO,
            rot: order.from_euler_angles(angles),
        });
    }
    evaluate_track_value(track, tick)?.as_cframe()
}

// ============================================================================
// Analytical Cubic Hermite Y-Extents (`CurveUtils.getYExtents`)
// ============================================================================

/// Computes the exact analytical `(min_y, max_y)` of a cubic Hermite segment
/// over `[t0, t1]` with endpoint values `(v0, v1)` and slopes `(s0, s1)`.
pub fn get_y_extents(t0: f32, v0: f32, s0: f32, t1: f32, v1: f32, s1: f32) -> (f32, f32) {
    let mut min_y = v0.min(v1) as f64;
    let mut max_y = v0.max(v1) as f64;

    let dt = (t1 as f64) - (t0 as f64);
    if dt.abs() <= 1e-12 {
        return (min_y as f32, max_y as f32);
    }

    let y0 = v0 as f64;
    let y1 = v1 as f64;
    let dv = y1 - y0;
    let dts0 = dt * (s0 as f64);
    let dts1 = dt * (s1 as f64);

    // v(u) = y0 + c1*u + c2*u^2 + c3*u^3
    // v'(u) = c1 + 2*c2*u + 3*c3*u^2 = A*u^2 + B*u + C
    let c1 = dts0;
    let c2 = 3.0 * dv - 2.0 * dts0 - dts1;
    let c3 = -2.0 * dv + dts0 + dts1;

    let eval = |u: f64| -> f64 { y0 + u * (c1 + u * (c2 + u * c3)) };

    let a = 3.0 * c3;
    let b = 2.0 * c2;
    let c = c1;

    if a.abs() <= 1e-12 {
        if b.abs() > 1e-12 {
            let u = -c / b;
            if u > 0.0 && u < 1.0 {
                let y = eval(u);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc >= 0.0 {
            let sqrt_disc = disc.sqrt();
            let denom = 2.0 * a;
            for u in [(-b + sqrt_disc) / denom, (-b - sqrt_disc) / denom] {
                if u > 0.0 && u < 1.0 {
                    let y = eval(u);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
    }

    (min_y as f32, max_y as f32)
}

// ============================================================================
// Rotation Track Conversions (`TrackUtils.convertTrackToEulerAngles` & inverse)
// ============================================================================

/// Converts a `Quaternion` (or `CFrame` / `EulerAngles`) track into an
/// `EulerAngles` track with `X`, `Y`, `Z` `Angle` curve sub-tracks using
/// continuous Euler angle unwrapping across all 6 `RotationOrder`s.
pub fn convert_track_to_euler_angles(track: &Track, order: RotationOrder) -> Track {
    if track.track_type == TrackType::EulerAngles && track.euler_angles_order == Some(order) {
        return track.clone();
    }

    let mut out = Track::new(TrackType::EulerAngles);
    out.euler_angles_order = Some(order);
    let mut tx = Track::new_curve(TrackType::Angle);
    let mut ty = Track::new_curve(TrackType::Angle);
    let mut tz = Track::new_curve(TrackType::Angle);

    let ticks: Vec<Tick> = if !track.keyframes.is_empty() {
        track.keyframes.clone()
    } else {
        let mut set = BTreeSet::new();
        track.collect_ticks(&mut set);
        set.into_iter().collect()
    };

    let mut unwrapped_values: Vec<Vec3> = Vec::with_capacity(ticks.len());
    let mut prev_euler: Option<Vec3> = None;

    for &tick in &ticks {
        let rot = if let Some(kf) = track.data.get(&tick) {
            if let Some(q) = kf.value.as_quat() {
                Mat3::from_quat(q.normalize())
            } else if let Some(xf) = kf.value.as_cframe() {
                xf.rot
            } else {
                Mat3::IDENTITY
            }
        } else {
            evaluate_cframe_track(track, tick)
                .map(|xf| xf.rot)
                .unwrap_or(Mat3::IDENTITY)
        };

        let raw = order.to_euler_angles(rot);
        let unwrapped = unwrap_euler(raw, prev_euler, order);
        prev_euler = Some(unwrapped);
        unwrapped_values.push(unwrapped);

        let interp = track
            .data
            .get(&tick)
            .map(effective_interpolation_mode)
            .or_else(|| {
                track
                    .components
                    .get(PROP_X)
                    .and_then(|c| c.data.get(&tick))
                    .map(effective_interpolation_mode)
            })
            .unwrap_or(KeyInterpolationMode::Cubic);

        let make_kf = |angle: f32| Keyframe {
            value: KeyframeValue::Angle(angle),
            easing_style: None,
            easing_direction: None,
            interpolation_mode: Some(interp),
            left_slope: None,
            right_slope: None,
        };

        tx.add_keyframe(tick, make_kf(unwrapped.x));
        ty.add_keyframe(tick, make_kf(unwrapped.y));
        tz.add_keyframe(tick, make_kf(unwrapped.z));
    }

    // Convert explicit quaternion slopes by multiplying by angle deltas.
    if track.track_type == TrackType::Quaternion {
        for (i, &tick) in ticks.iter().enumerate() {
            let Some(quat_kf) = track.data.get(&tick) else {
                continue;
            };
            let cur = unwrapped_values[i];

            if let Some(ls) = quat_kf.left_slope {
                if i > 0 {
                    let prev = unwrapped_values[i - 1];
                    if let Some(k) = tx.data.get_mut(&tick) {
                        k.left_slope = Some(ls * (cur.x - prev.x));
                    }
                    if let Some(k) = ty.data.get_mut(&tick) {
                        k.left_slope = Some(ls * (cur.y - prev.y));
                    }
                    if let Some(k) = tz.data.get_mut(&tick) {
                        k.left_slope = Some(ls * (cur.z - prev.z));
                    }
                }
            }

            if let Some(rs) = quat_kf.right_slope {
                if i + 1 < ticks.len() {
                    let next = unwrapped_values[i + 1];
                    if let Some(k) = tx.data.get_mut(&tick) {
                        k.right_slope = Some(rs * (next.x - cur.x));
                    }
                    if let Some(k) = ty.data.get_mut(&tick) {
                        k.right_slope = Some(rs * (next.y - cur.y));
                    }
                    if let Some(k) = tz.data.get_mut(&tick) {
                        k.right_slope = Some(rs * (next.z - cur.z));
                    }
                }
            }
        }
    }

    out.components.insert(PROP_X.to_string(), tx);
    out.components.insert(PROP_Y.to_string(), ty);
    out.components.insert(PROP_Z.to_string(), tz);
    out
}

/// Converts an `EulerAngles` (or `CFrame` / `Quaternion`) track into a
/// `Quaternion` curve track.
pub fn convert_track_to_quaternion(track: &Track) -> Track {
    if track.track_type == TrackType::Quaternion {
        return track.clone();
    }

    let mut out = Track::new_curve(TrackType::Quaternion);
    let mut tick_set = BTreeSet::new();
    track.collect_ticks(&mut tick_set);

    for tick in tick_set {
        let q = evaluate_cframe_track(track, tick)
            .map(|xf| Quat::from_mat3(&xf.rot).normalize())
            .unwrap_or(Quat::IDENTITY);

        let interp = track
            .data
            .get(&tick)
            .map(effective_interpolation_mode)
            .or_else(|| {
                track
                    .components
                    .get(PROP_X)
                    .and_then(|c| c.data.get(&tick))
                    .map(effective_interpolation_mode)
            })
            .unwrap_or(KeyInterpolationMode::Cubic);

        out.add_keyframe(
            tick,
            Keyframe {
                value: KeyframeValue::Quaternion(q),
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(interp),
                left_slope: None,
                right_slope: None,
            },
        );
    }

    out
}

// ============================================================================
// Track Splitting (`Thunks/SplitTrack`)
// ============================================================================

/// Inserts an evaluated keyframe at `tick` into `track`, preserving curve shape
/// across the split and renormalizing Hermite/quaternion slopes on the left and
/// right subsegments.
pub fn split_track(track: &mut Track, tick: Tick) {
    if !track.components.is_empty() {
        for comp in track.components.values_mut() {
            split_track(comp, tick);
        }
        return;
    }

    let (exact, prev, next) = find_nearest_keyframes(&track.keyframes, tick);
    if exact.is_some() {
        return;
    }

    let Some(p_idx) = prev else {
        // Empty track or tick before first keyframe: insert default keyframe.
        let kf = Keyframe::default_for(track.track_type, track.is_curve_track);
        track.add_keyframe(tick, kf);
        return;
    };

    let prev_tick = track.keyframes[p_idx];
    let Some(prev_kf) = track.data.get(&prev_tick).copied() else {
        return;
    };
    let value = evaluate_track_value(track, tick)
        .unwrap_or_else(|| KeyframeValue::default_for(track.track_type));

    if !track.is_curve_track {
        let kf = Keyframe {
            value,
            easing_style: prev_kf.easing_style.or(Some(EasingStyle::Linear)),
            easing_direction: prev_kf.easing_direction.or(Some(EasingDirection::In)),
            interpolation_mode: None,
            left_slope: None,
            right_slope: None,
        };
        track.add_keyframe(tick, kf);
        return;
    }

    let prev_mode = effective_interpolation_mode(&prev_kf);
    let mut left_slope = None;
    let mut right_slope = None;

    if prev_mode == KeyInterpolationMode::Cubic {
        if let Some(n_idx) = next {
            let next_tick = track.keyframes[n_idx];
            let (ls, rs) = get_slopes(track, tick);
            left_slope = ls;
            right_slope = rs;

            if track.track_type == TrackType::Quaternion {
                let dt = (next_tick - prev_tick) as f32;
                let u = (tick - prev_tick) as f32 / dt;
                let s0 = get_slope_inner(track, p_idx, TangentSide::Right, false);
                let s1 = get_slope_inner(track, n_idx, TangentSide::Left, false);
                let alpha = hermite_quat_alpha(s0, s1, dt, u);

                if alpha.abs() > 1e-8 {
                    if let Some(k0) = track.data.get_mut(&prev_tick) {
                        k0.right_slope = Some(s0 / alpha);
                    }
                }
                if (1.0 - alpha).abs() > 1e-8 {
                    if let Some(k1) = track.data.get_mut(&next_tick) {
                        k1.left_slope = Some(s1 / (1.0 - alpha));
                    }
                }
            }
        }
    }

    let new_kf = Keyframe {
        value,
        easing_style: None,
        easing_direction: None,
        interpolation_mode: Some(prev_mode),
        left_slope,
        right_slope,
    };
    track.add_keyframe(tick, new_kf);
}

// ============================================================================
// Curve Synthesis from Easing (`Util/CurveUtils`)
// ============================================================================

/// Exact `CurveUtils.makeCubic`.
pub fn make_cubic(
    direction: EasingDirection,
    low_tick: Tick,
    low_kf: &mut Keyframe,
    high_tick: Tick,
    high_kf: &mut Keyframe,
    is_quaternion: bool,
) -> BTreeMap<Tick, Keyframe> {
    let dt = (high_tick - low_tick) as f32;
    if dt.abs() <= f32::EPSILON {
        return BTreeMap::new();
    }
    let dv = if is_quaternion {
        1.0
    } else {
        high_kf.value.as_f32().unwrap_or(0.0) - low_kf.value.as_f32().unwrap_or(0.0)
    };
    if direction == EasingDirection::Out {
        low_kf.right_slope = Some(3.0 * dv / dt);
        high_kf.left_slope = Some(0.0);
    } else {
        low_kf.right_slope = Some(0.0);
        high_kf.left_slope = Some(3.0 * dv / dt);
    }
    BTreeMap::new()
}

/// Exact `CurveUtils.makeBounce` (default `bounce_factor = 0.25`, `num_bounces = 3`).
pub fn make_bounce(
    direction: EasingDirection,
    low_tick: Tick,
    low_kf: &mut Keyframe,
    high_tick: Tick,
    high_kf: &mut Keyframe,
    is_quaternion: bool,
    bounce_factor: f64,
    num_bounces: usize,
) -> BTreeMap<Tick, Keyframe> {
    let mut keys = BTreeMap::new();
    let dt = (high_tick - low_tick) as f64;
    if dt.abs() <= 1e-12 || num_bounces == 0 {
        return keys;
    }
    let dv = if is_quaternion {
        1.0
    } else {
        (high_kf.value.as_f32().unwrap_or(0.0) - low_kf.value.as_f32().unwrap_or(0.0)) as f64
    };

    let r = bounce_factor.sqrt();
    let mut total_t = 1.0;
    for i in 1..=num_bounces {
        total_t += 2.0 * r.powi(i as i32);
    }

    let mut h = r;
    let mut v0 = 2.0 * total_t;
    let mut t_acc = 1.0;
    let mut last_slope = 0.0_f32;

    for _ in 1..=num_bounces {
        let mut t_hit = t_acc / total_t;
        let (mut s_l, mut s_r) = if is_quaternion {
            (2.0 * total_t * r / h, 2.0 * total_t / h)
        } else {
            (v0, -v0 * r)
        };
        let mut t_peak = t_hit + h / total_t;
        let mut y_peak = 1.0 - h * h;

        if direction == EasingDirection::In {
            t_hit = 1.0 - t_hit;
            std::mem::swap(&mut s_l, &mut s_r);
            t_peak = 1.0 - t_peak;
            y_peak = 1.0 - y_peak;
        }

        let impact_tick = nearest_tick(low_tick as f64 + t_hit * dt);
        let impact_kf = Keyframe {
            value: if direction == EasingDirection::In {
                low_kf.value
            } else {
                high_kf.value
            },
            easing_style: None,
            easing_direction: None,
            interpolation_mode: Some(KeyInterpolationMode::Cubic),
            left_slope: Some((s_l * dv / dt) as f32),
            right_slope: Some((s_r * dv / dt) as f32),
        };
        last_slope = if direction == EasingDirection::Out {
            impact_kf.right_slope.unwrap_or(0.0)
        } else {
            impact_kf.left_slope.unwrap_or(0.0)
        };
        keys.insert(impact_tick, impact_kf);

        let peak_tick = nearest_tick(low_tick as f64 + t_peak * dt);
        let peak_kf = Keyframe {
            value: low_kf.value.lerp(high_kf.value, y_peak as f32),
            easing_style: None,
            easing_direction: None,
            interpolation_mode: Some(KeyInterpolationMode::Cubic),
            left_slope: Some(0.0),
            right_slope: Some(0.0),
        };
        keys.insert(peak_tick, peak_kf);

        t_acc += 2.0 * h;
        h *= r;
        v0 *= r;
    }

    if direction == EasingDirection::In {
        low_kf.right_slope = Some(if is_quaternion {
            last_slope
        } else {
            -last_slope
        });
        high_kf.left_slope = Some(0.0);
    } else {
        low_kf.right_slope = Some(0.0);
        high_kf.left_slope = Some(if is_quaternion {
            last_slope
        } else {
            -last_slope
        });
    }

    keys
}

/// Exact `CurveUtils.makeElastic` (default `num_oscillations = 10.0 / 3.0`, `decay = 10.0`).
pub fn make_elastic(
    direction: EasingDirection,
    low_tick: Tick,
    low_kf: &mut Keyframe,
    high_tick: Tick,
    high_kf: &mut Keyframe,
    is_quaternion: bool,
    num_oscillations: f64,
    decay: f64,
) -> BTreeMap<Tick, Keyframe> {
    let mut keys = BTreeMap::new();
    let dt = (high_tick - low_tick) as f64;
    if dt.abs() <= 1e-12 || num_oscillations <= 0.0 {
        return keys;
    }
    let dv = if is_quaternion {
        1.0
    } else {
        (high_kf.value.as_f32().unwrap_or(0.0) - low_kf.value.as_f32().unwrap_or(0.0)) as f64
    };

    let omega = std::f64::consts::TAU * num_oscillations;
    let t0 = (-decay * std::f64::consts::LN_2 / omega).atan() / omega;
    let count = (2.0 * num_oscillations).floor() as usize;

    for i in 1..=count {
        let mut t = t0 + (i as f64) / (2.0 * num_oscillations);
        let mut y = 1.0 - 2.0_f64.powf(-decay * t) * (omega * t).cos();
        if direction == EasingDirection::In {
            t = 1.0 - t;
            y = 1.0 - y;
        }
        let tick = nearest_tick(low_tick as f64 + t * dt);
        keys.insert(
            tick,
            Keyframe {
                value: low_kf.value.lerp(high_kf.value, y as f32),
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(KeyInterpolationMode::Cubic),
                left_slope: Some(0.0),
                right_slope: Some(0.0),
            },
        );
    }

    let endpoint_slope = (decay * std::f64::consts::LN_2 * dv / dt) as f32;
    if direction == EasingDirection::In {
        low_kf.right_slope = Some(0.0);
        high_kf.left_slope = Some(endpoint_slope);
    } else {
        low_kf.right_slope = Some(endpoint_slope);
        high_kf.left_slope = Some(0.0);
    }

    keys
}

/// Exact `CurveUtils.generateCurve`.
pub fn generate_curve(
    track_type: TrackType,
    style: EasingStyle,
    direction: EasingDirection,
    low_tick: Tick,
    low_kf: &mut Keyframe,
    high_tick: Tick,
    high_kf: &mut Keyframe,
) -> BTreeMap<Tick, Keyframe> {
    low_kf.interpolation_mode = Some(style.to_key_interpolation());
    low_kf.easing_style = None;
    low_kf.easing_direction = None;

    if direction == EasingDirection::InOut
        && !matches!(style, EasingStyle::Constant | EasingStyle::Linear)
    {
        let mid_tick = nearest_tick(0.5 * (low_tick as f64 + high_tick as f64));
        let mut mid_kf = Keyframe::new_curve(low_kf.value.lerp(high_kf.value, 0.5));
        let mut result = generate_curve(
            track_type,
            style,
            EasingDirection::In,
            low_tick,
            low_kf,
            mid_tick,
            &mut mid_kf,
        );
        let right = generate_curve(
            track_type,
            style,
            EasingDirection::Out,
            mid_tick,
            &mut mid_kf,
            high_tick,
            high_kf,
        );
        result.extend(right);
        if style != EasingStyle::Elastic {
            result.insert(mid_tick, mid_kf);
        }
        return result;
    }

    let is_quaternion = track_type == TrackType::Quaternion;
    match style {
        EasingStyle::Linear | EasingStyle::Constant => BTreeMap::new(),
        EasingStyle::Cubic | EasingStyle::CubicV2 => make_cubic(
            direction,
            low_tick,
            low_kf,
            high_tick,
            high_kf,
            is_quaternion,
        ),
        EasingStyle::Bounce => make_bounce(
            direction,
            low_tick,
            low_kf,
            high_tick,
            high_kf,
            is_quaternion,
            0.25,
            3,
        ),
        EasingStyle::Elastic => make_elastic(
            direction,
            low_tick,
            low_kf,
            high_tick,
            high_kf,
            is_quaternion,
            10.0 / 3.0,
            10.0,
        ),
    }
}

/// Replaces the segment `[start_tick, end_tick]` on `track` with synthesized
/// curve keyframes matching `style` and `direction`
/// (`Thunks/Selection/GenerateCurve`).
pub fn generate_curve_from_easing(
    track: &mut Track,
    start_tick: Tick,
    end_tick: Tick,
    style: EasingStyle,
    direction: EasingDirection,
) {
    if start_tick >= end_tick {
        return;
    }

    if !track.components.is_empty() {
        for comp in track.components.values_mut() {
            generate_curve_from_easing(comp, start_tick, end_tick, style, direction);
        }
        return;
    }

    // Ensure endpoint keyframes exist.
    if !track.data.contains_key(&start_tick) {
        let value = evaluate_track_value(track, start_tick)
            .unwrap_or_else(|| KeyframeValue::default_for(track.track_type));
        let (left_slope, _) = get_slopes(track, start_tick);
        track.add_keyframe(
            start_tick,
            Keyframe {
                value,
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(KeyInterpolationMode::Cubic),
                left_slope,
                right_slope: None,
            },
        );
    }

    if !track.data.contains_key(&end_tick) {
        let value = evaluate_track_value(track, end_tick)
            .unwrap_or_else(|| KeyframeValue::default_for(track.track_type));
        let (_, right_slope) = get_slopes(track, end_tick);
        let (_, prev, _) = find_nearest_keyframes(&track.keyframes, end_tick);
        let interp = prev
            .and_then(|i| track.data.get(&track.keyframes[i]))
            .map(effective_interpolation_mode)
            .unwrap_or(KeyInterpolationMode::Cubic);
        track.add_keyframe(
            end_tick,
            Keyframe {
                value,
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(interp),
                left_slope: None,
                right_slope,
            },
        );
    }

    // Remove any existing keyframes strictly between start_tick and end_tick.
    let interior: Vec<Tick> = track
        .keyframes
        .iter()
        .copied()
        .filter(|&t| t > start_tick && t < end_tick)
        .collect();
    for t in interior {
        track.delete_keyframe(t);
    }

    let Some(mut low_kf) = track.data.get(&start_tick).copied() else {
        return;
    };
    let Some(mut high_kf) = track.data.get(&end_tick).copied() else {
        return;
    };

    let generated = generate_curve(
        track.track_type,
        style,
        direction,
        start_tick,
        &mut low_kf,
        end_tick,
        &mut high_kf,
    );

    track.add_keyframe(start_tick, low_kf);
    track.add_keyframe(end_tick, high_kf);
    for (t, kf) in generated {
        if t > start_tick && t < end_tick {
            track.add_keyframe(t, kf);
        }
    }
    track.is_curve_track = true;
}

/// Returns `true` if `data` can be edited directly in the Curve Editor
/// (i.e. it has already been promoted to channel/curve animation).
pub fn can_use_curve_editor(data: &AnimationData) -> bool {
    data.is_channel_animation()
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim_editor::constants::EULER_ANGLES_ORDER;

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn test_find_nearest_keyframes() {
        let empty: [Tick; 0] = [];
        assert_eq!(find_nearest_keyframes(&empty, 10), (None, None, None));

        let keys = [10, 20, 30, 50];
        assert_eq!(find_nearest_keyframes(&keys, 0), (None, None, Some(0)));
        assert_eq!(find_nearest_keyframes(&keys, 10), (Some(0), None, None));
        assert_eq!(find_nearest_keyframes(&keys, 15), (None, Some(0), Some(1)));
        assert_eq!(find_nearest_keyframes(&keys, 30), (Some(2), None, None));
        assert_eq!(find_nearest_keyframes(&keys, 45), (None, Some(2), Some(3)));
        assert_eq!(find_nearest_keyframes(&keys, 50), (Some(3), None, None));
        assert_eq!(find_nearest_keyframes(&keys, 99), (None, Some(3), None));
    }

    #[test]
    fn test_easing_alpha_all_styles_and_directions() {
        for style in [
            EasingStyle::Linear,
            EasingStyle::Constant,
            EasingStyle::Cubic,
            EasingStyle::CubicV2,
            EasingStyle::Bounce,
            EasingStyle::Elastic,
        ] {
            for dir in [
                EasingDirection::In,
                EasingDirection::Out,
                EasingDirection::InOut,
            ] {
                assert!(approx(easing_alpha(0.0, style, dir), 0.0, 1e-5));
                assert!(approx(easing_alpha(1.0, style, dir), 1.0, 1e-5));
            }
        }

        // Linear
        assert!(approx(
            easing_alpha(0.35, EasingStyle::Linear, EasingDirection::In),
            0.35,
            1e-6
        ));
        // Constant (In jumps immediately for t>0, Out holds until t>=1, InOut jumps at 0.5)
        assert_eq!(
            easing_alpha(0.01, EasingStyle::Constant, EasingDirection::In),
            1.0
        );
        assert_eq!(
            easing_alpha(0.99, EasingStyle::Constant, EasingDirection::Out),
            0.0
        );
        assert_eq!(
            easing_alpha(0.49, EasingStyle::Constant, EasingDirection::InOut),
            0.0
        );
        assert_eq!(
            easing_alpha(0.50, EasingStyle::Constant, EasingDirection::InOut),
            1.0
        );
        // Cubic (deprecated: In/Out swapped) vs CubicV2
        assert!(approx(
            easing_alpha(0.5, EasingStyle::Cubic, EasingDirection::In),
            0.875,
            1e-6
        ));
        assert!(approx(
            easing_alpha(0.5, EasingStyle::Cubic, EasingDirection::Out),
            0.125,
            1e-6
        ));
        assert!(approx(
            easing_alpha(0.5, EasingStyle::CubicV2, EasingDirection::In),
            0.125,
            1e-6
        ));
        assert!(approx(
            easing_alpha(0.5, EasingStyle::CubicV2, EasingDirection::Out),
            0.875,
            1e-6
        ));
        assert!(approx(
            easing_alpha(0.5, EasingStyle::CubicV2, EasingDirection::InOut),
            0.5,
            1e-6
        ));
        // Bounce InOut midpoint & second half (Studio uses bounceIn on both halves)
        assert!(approx(
            easing_alpha(0.5, EasingStyle::Bounce, EasingDirection::InOut),
            0.5,
            1e-5
        ));
        assert!(approx(
            easing_alpha(0.6, EasingStyle::Bounce, EasingDirection::InOut),
            0.53,
            1e-5
        ));
        // Elastic InOut midpoint
        assert!(approx(
            easing_alpha(0.5, EasingStyle::Elastic, EasingDirection::InOut),
            0.5,
            1e-5
        ));
    }

    #[test]
    fn test_hermite_evaluation_and_facs_clamping() {
        let mut track = Track::new_curve(TrackType::Number);
        track.add_keyframe(
            0,
            Keyframe {
                value: KeyframeValue::Number(0.0),
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(KeyInterpolationMode::Cubic),
                left_slope: None,
                right_slope: Some(0.0),
            },
        );
        track.add_keyframe(
            100,
            Keyframe {
                value: KeyframeValue::Number(10.0),
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(KeyInterpolationMode::Cubic),
                left_slope: Some(0.0),
                right_slope: None,
            },
        );

        // Smoothstep with zero tangents: h01(0.5) = 0.5, h01(0.25) = 0.15625
        assert!(approx(
            evaluate_number_track(&track, 50).unwrap(),
            5.0,
            1e-5
        ));
        assert!(approx(
            evaluate_number_track(&track, 25).unwrap(),
            1.5625,
            1e-5
        ));

        // FACS overshoot clamping to [0, 1]
        let mut facs = Track::new_curve(TrackType::Facs);
        facs.add_keyframe(
            0,
            Keyframe {
                value: KeyframeValue::Facs(0.0),
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(KeyInterpolationMode::Cubic),
                left_slope: None,
                right_slope: Some(0.1), // steep positive slope -> overshoots 1.0
            },
        );
        facs.add_keyframe(
            100,
            Keyframe {
                value: KeyframeValue::Facs(1.0),
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(KeyInterpolationMode::Cubic),
                left_slope: Some(0.0),
                right_slope: None,
            },
        );
        assert_eq!(evaluate_number_track(&facs, 50).unwrap(), 1.0);
    }

    #[test]
    fn test_clamped_auto_tangents_and_local_extrema_flattening() {
        // Monotonic interior point at u = 0.5 -> full Catmull-Rom slope (factor = 1.0)
        let mut mono = Track::new_curve(TrackType::Number);
        mono.add_keyframe(0, Keyframe::new_curve(KeyframeValue::Number(0.0)));
        mono.add_keyframe(10, Keyframe::new_curve(KeyframeValue::Number(5.0)));
        mono.add_keyframe(20, Keyframe::new_curve(KeyframeValue::Number(10.0)));
        let s = get_slope(&mono, 10, TangentSide::Right).unwrap();
        assert!(approx(s, 0.5, 1e-6));

        // Near extremum u = 0.125 (< 0.25) -> damped by factor = 0.125 / 0.25 = 0.5
        let mut damped = Track::new_curve(TrackType::Number);
        damped.add_keyframe(0, Keyframe::new_curve(KeyframeValue::Number(0.0)));
        damped.add_keyframe(10, Keyframe::new_curve(KeyframeValue::Number(1.0)));
        damped.add_keyframe(20, Keyframe::new_curve(KeyframeValue::Number(8.0)));
        // s_raw = 0.5 * ((8-1)/10 + (1-0)/10) = 0.4, u = 1/8 = 0.125, factor = 0.5 -> 0.2
        assert!(approx(
            get_slope(&damped, 10, TangentSide::Right).unwrap(),
            0.2,
            1e-6
        ));

        // Strict local maximum (v1 > v0 and v1 > v2) -> flattened to 0.0
        let mut peak = Track::new_curve(TrackType::Number);
        peak.add_keyframe(0, Keyframe::new_curve(KeyframeValue::Number(0.0)));
        peak.add_keyframe(10, Keyframe::new_curve(KeyframeValue::Number(10.0)));
        peak.add_keyframe(20, Keyframe::new_curve(KeyframeValue::Number(4.0)));
        assert!(approx(
            get_slope(&peak, 10, TangentSide::Left).unwrap(),
            0.0,
            1e-6
        ));
        assert!(approx(
            get_slope(&peak, 10, TangentSide::Right).unwrap(),
            0.0,
            1e-6
        ));

        // Strict local minimum (v1 < v0 and v1 < v2) -> flattened to 0.0
        let mut valley = Track::new_curve(TrackType::Number);
        valley.add_keyframe(0, Keyframe::new_curve(KeyframeValue::Number(5.0)));
        valley.add_keyframe(10, Keyframe::new_curve(KeyframeValue::Number(-2.0)));
        valley.add_keyframe(20, Keyframe::new_curve(KeyframeValue::Number(5.0)));
        assert!(approx(
            get_slope(&valley, 10, TangentSide::Right).unwrap(),
            0.0,
            1e-6
        ));
    }

    #[test]
    fn test_euler_unwrapping_all_six_orders() {
        for &order in EULER_ANGLES_ORDER {
            let mut q_track = Track::new_curve(TrackType::Quaternion);
            // Smooth rotation sweeping past 180 degrees around an oblique axis.
            let axis = Vec3::new(0.3, 0.8, 0.5).normalize();
            let steps = 12;
            for i in 0..=steps {
                let angle = (i as f32) * std::f32::consts::TAU / (steps as f32);
                let q = Quat::from_axis_angle(axis, angle);
                q_track.add_keyframe(
                    (i * 100) as Tick,
                    Keyframe::new_curve(KeyframeValue::Quaternion(q)),
                );
            }

            let euler_track = convert_track_to_euler_angles(&q_track, order);
            assert_eq!(euler_track.track_type, TrackType::EulerAngles);
            assert_eq!(euler_track.euler_angles_order, Some(order));

            // Every keyframe must reconstruct the exact rotation matrix and avoid large jumps.
            let mut prev_angles: Option<Vec3> = None;
            for i in 0..=steps {
                let tick = (i * 100) as Tick;
                let expected_q = evaluate_quaternion_track(&q_track, tick).unwrap();
                let expected_rot = Mat3::from_quat(expected_q);
                let actual_rot = evaluate_cframe_track(&euler_track, tick).unwrap().rot;

                let diff = (expected_rot.x_axis - actual_rot.x_axis).length()
                    + (expected_rot.y_axis - actual_rot.y_axis).length()
                    + (expected_rot.z_axis - actual_rot.z_axis).length();
                assert!(
                    diff < 1e-3,
                    "Order {:?} tick {} rotation mismatch: {}",
                    order,
                    tick,
                    diff
                );

                let angles = evaluate_track_value(&euler_track, tick)
                    .unwrap()
                    .as_position()
                    .unwrap();
                if let Some(p) = prev_angles {
                    let jump = (angles - p).length();
                    assert!(
                        jump < std::f32::consts::PI,
                        "Order {:?} tick {} had Euler jump {}",
                        order,
                        tick,
                        jump
                    );
                }
                prev_angles = Some(angles);
            }
        }
    }

    #[test]
    fn test_quaternion_split_slope_renormalization() {
        let mut q_track = Track::new_curve(TrackType::Quaternion);
        let q0 = Quat::IDENTITY;
        let q1 = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        q_track.add_keyframe(
            0,
            Keyframe {
                value: KeyframeValue::Quaternion(q0),
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(KeyInterpolationMode::Cubic),
                left_slope: None,
                right_slope: Some(0.015),
            },
        );
        q_track.add_keyframe(
            100,
            Keyframe {
                value: KeyframeValue::Quaternion(q1),
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(KeyInterpolationMode::Cubic),
                left_slope: Some(0.005),
                right_slope: None,
            },
        );

        // Sample the curve before splitting.
        let samples_before: Vec<(Tick, Quat)> = (0..=100)
            .step_by(5)
            .map(|t| (t, evaluate_quaternion_track(&q_track, t).unwrap()))
            .collect();

        // Split at tick 40 (asymmetric split!).
        split_track(&mut q_track, 40);
        assert_eq!(q_track.keyframes, vec![0, 40, 100]);

        // Verify the split curve matches the original at EVERY sample tick.
        for (t, expected_q) in samples_before {
            let actual_q = evaluate_quaternion_track(&q_track, t).unwrap();
            let dot = expected_q.dot(actual_q).abs();
            assert!(
                dot > 0.99999,
                "Quaternion split mismatch at tick {}: dot = {}",
                t,
                dot
            );
        }
    }

    #[test]
    fn test_bounce_and_elastic_curve_synthesis() {
        // Bounce Out generates 2 * 3 = 6 interior keyframes (3 impacts + 3 peaks).
        let mut bounce_track = Track::new_curve(TrackType::Number);
        bounce_track.add_keyframe(0, Keyframe::new_curve(KeyframeValue::Number(0.0)));
        bounce_track.add_keyframe(2400, Keyframe::new_curve(KeyframeValue::Number(10.0)));
        generate_curve_from_easing(
            &mut bounce_track,
            0,
            2400,
            EasingStyle::Bounce,
            EasingDirection::Out,
        );
        assert_eq!(bounce_track.keyframes.len(), 2 + 6);
        assert!(approx(
            evaluate_number_track(&bounce_track, 0).unwrap(),
            0.0,
            1e-4
        ));
        assert!(approx(
            evaluate_number_track(&bounce_track, 2400).unwrap(),
            10.0,
            1e-4
        ));

        // Elastic Out generates 6 oscillation extrema keyframes.
        let mut elastic_track = Track::new_curve(TrackType::Number);
        elastic_track.add_keyframe(0, Keyframe::new_curve(KeyframeValue::Number(0.0)));
        elastic_track.add_keyframe(2400, Keyframe::new_curve(KeyframeValue::Number(1.0)));
        generate_curve_from_easing(
            &mut elastic_track,
            0,
            2400,
            EasingStyle::Elastic,
            EasingDirection::Out,
        );
        assert_eq!(elastic_track.keyframes.len(), 2 + 6);
        // Elastic Out overshoots 1.0 on its first peak.
        let max_val = elastic_track
            .data
            .values()
            .filter_map(|k| k.value.as_f32())
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(max_val > 1.0);

        // Bounce InOut inserts midpoint keyframe + both halves.
        let mut inout_track = Track::new_curve(TrackType::Number);
        inout_track.add_keyframe(0, Keyframe::new_curve(KeyframeValue::Number(0.0)));
        inout_track.add_keyframe(2400, Keyframe::new_curve(KeyframeValue::Number(10.0)));
        generate_curve_from_easing(
            &mut inout_track,
            0,
            2400,
            EasingStyle::Bounce,
            EasingDirection::InOut,
        );
        assert!(inout_track.data.contains_key(&1200));
        assert!(approx(
            evaluate_number_track(&inout_track, 1200).unwrap(),
            5.0,
            1e-4
        ));
    }

    #[test]
    fn test_analytical_y_extents() {
        // Symmetric S-curve with overshoot: v0=0, v1=0, s0=1, s1=-1 over [0, 1]
        // v(u) = u - 2u^2 + u^3 + (-u^3 + u^2) ... wait:
        // h10(u)*1 + h11(u)*(-1) = (u^3 - 2u^2 + u) - (u^3 - u^2) = u - u^2
        // Maximum at u = 0.5 is 0.25!
        let (min_y, max_y) = get_y_extents(0.0, 0.0, 1.0, 1.0, 0.0, -1.0);
        assert!(approx(min_y, 0.0, 1e-6));
        assert!(approx(max_y, 0.25, 1e-6));

        // Antisymmetric overshoot: v0=0, v1=0, s0=3, s1=3 over [0, 1]
        // v(u) = 3*(u^3 - 2u^2 + u) + 3*(u^3 - u^2) = 6u^3 - 9u^2 + 3u
        // v'(u) = 18u^2 - 18u + 3 = 0 -> u = (3 +/- sqrt(3)) / 6
        // extrema = +/- sqrt(3)/6 ≈ +/- 0.28867513
        let (min_y, max_y) = get_y_extents(0.0, 0.0, 3.0, 1.0, 0.0, 3.0);
        let expected = 3.0_f32.sqrt() / 6.0;
        assert!(approx(min_y, -expected, 1e-5));
        assert!(approx(max_y, expected, 1e-5));

        // Monotonic segment with zero slopes: extents equal endpoints
        let (min_y, max_y) = get_y_extents(0.0, 2.0, 0.0, 100.0, 8.0, 0.0);
        assert!(approx(min_y, 2.0, 1e-6));
        assert!(approx(max_y, 8.0, 1e-6));
    }

    #[test]
    fn test_can_use_curve_editor() {
        let mut anim = AnimationData::new("Test");
        assert!(!can_use_curve_editor(&anim));
        anim.promote_to_curves(TrackType::EulerAngles, RotationOrder::XYZ);
        assert!(can_use_curve_editor(&anim));
    }
}
