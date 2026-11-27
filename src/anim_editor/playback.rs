//! Playback engine, transport controls, and time formatting/parsing for
//! RusTudio's native `AnimationClipEditor` (derived from
//! `Components/Playback`, `Thunks/Playback/*`, and
//! `Util/StringUtils`).

use crate::anim_editor::constants::{
    MAX_PLAYBACK_SPEED, MIN_PLAYBACK_SPEED, TICK_FREQUENCY,
};
use crate::anim_editor::model::{clamp_tick, nearest_tick, tick_to_seconds, Tick};
use crate::anim_editor::state::{EditorState, PlayState, TimelineUnit};

/// Advances playback by `dt` seconds.
///
/// Returns `true` if the playhead moved (or wrapped/clamped at a boundary) and
/// the rig needs stepping.
pub fn step_playback(state: &mut EditorState, dt: f32) -> bool {
    if !matches!(
        state.playback.play_state,
        PlayState::Play | PlayState::Reverse
    ) {
        return false;
    }
    if !dt.is_finite() || dt <= 0.0 {
        return false;
    }

    let Some(anim) = state.animation_data.as_ref() else {
        return false;
    };
    let end_tick = anim.metadata.end_tick;
    let looping = anim.metadata.looping;
    if end_tick <= 0 {
        return false;
    }

    let prev_playhead = state.playback.playhead;
    let is_reverse = state.playback.play_state == PlayState::Reverse;

    if state.playback.start_time_secs.is_none() {
        let start_playhead = if !is_reverse && state.playback.playhead >= end_tick {
            0
        } else if is_reverse && state.playback.playhead <= 0 {
            end_tick
        } else {
            state.playback.playhead.clamp(0, end_tick)
        };
        state.playback.start_playhead = start_playhead;
        state.playback.start_time_secs = Some(0.0);
    }

    let speed = state
        .playback
        .playback_speed
        .clamp(MIN_PLAYBACK_SPEED, MAX_PLAYBACK_SPEED) as f64;
    let dir = if is_reverse { -1.0 } else { 1.0 };
    let elapsed_scaled = state.playback.start_time_secs.unwrap_or(0.0) + (dt as f64) * speed * dir;
    state.playback.start_time_secs = Some(elapsed_scaled);

    let raw_tick =
        (state.playback.start_playhead as f64) + elapsed_scaled * (TICK_FREQUENCY as f64);

    if looping {
        let wrapped = raw_tick.rem_euclid(end_tick as f64);
        let mut new_tick = nearest_tick(wrapped);
        if new_tick >= end_tick {
            new_tick = 0;
        }
        state.playback.playhead = new_tick.clamp(0, end_tick);
        state.playback.playhead != prev_playhead || (raw_tick < 0.0 || raw_tick >= end_tick as f64)
    } else if !is_reverse && raw_tick >= end_tick as f64 {
        state.playback.playhead = end_tick;
        state.playback.set_play_state(PlayState::Pause);
        true
    } else if is_reverse && raw_tick <= 0.0 {
        state.playback.playhead = 0;
        state.playback.set_play_state(PlayState::Pause);
        true
    } else {
        let new_tick = nearest_tick(raw_tick).clamp(0, end_tick);
        state.playback.playhead = new_tick;
        new_tick != prev_playhead
    }
}

/// Resolves the active floor-grid scroll speed in studs per second.
pub fn grid_speed_studs_per_sec(state: &EditorState) -> f32 {
    state
        .settings
        .grid_speed
        .resolve_studs_per_sec(state.rig_cache.humanoid_walkspeed)
}

/// Computes the deterministic floor-grid scroll offset (in studs) for the
/// current playhead and `GridSpeedSetting`.
pub fn compute_grid_offset(state: &EditorState) -> f32 {
    tick_to_seconds(state.playback.playhead) * grid_speed_studs_per_sec(state)
}

/// Exact `Thunks/Playback/TogglePlay`.
pub fn toggle_play(state: &mut EditorState) {
    let end_tick = state
        .animation_data
        .as_ref()
        .map(|d| d.metadata.end_tick)
        .unwrap_or(0);
    if state.playback.play_state == PlayState::Pause {
        if end_tick > 0 {
            state.playback.set_play_state(PlayState::Play);
        }
    } else {
        state.playback.set_play_state(PlayState::Pause);
    }
}

/// Toggles reverse playback (`PlayState::Reverse` <-> `PlayState::Pause`).
pub fn toggle_reverse(state: &mut EditorState) {
    let end_tick = state
        .animation_data
        .as_ref()
        .map(|d| d.metadata.end_tick)
        .unwrap_or(0);
    if state.playback.play_state == PlayState::Reverse {
        state.playback.set_play_state(PlayState::Pause);
    } else if end_tick > 0 {
        state.playback.set_play_state(PlayState::Reverse);
    }
}

/// Pauses playback immediately.
pub fn pause(state: &mut EditorState) {
    state.playback.set_play_state(PlayState::Pause);
}

/// Exact `Thunks/Playback/ToggleLooping`.
pub fn toggle_looping(state: &mut EditorState) {
    if let Some(anim) = state.animation_data.as_mut() {
        let next = !anim.metadata.looping;
        anim.set_looping(next);
        state.is_dirty = true;
    }
}

/// Exact `Thunks/Playback/SetPlaybackSpeed`: updates speed and
/// re-anchors playback start info so the playhead does not jump.
pub fn set_playback_speed(state: &mut EditorState, speed: f32) {
    let clamped = speed.clamp(MIN_PLAYBACK_SPEED, MAX_PLAYBACK_SPEED);
    state.playback.playback_speed = clamped;
    if state.playback.play_state != PlayState::Pause {
        state.playback.start_playhead = state.playback.playhead;
        state.playback.start_time_secs = Some(0.0);
    }
}

/// Jumps playhead to the start of the clip (`StartTick`, usually `0`).
pub fn skip_to_start(state: &mut EditorState) {
    let start_tick = state
        .animation_data
        .as_ref()
        .map(|d| d.metadata.start_tick)
        .unwrap_or(0);
    set_playhead(state, start_tick);
}

/// Jumps playhead to the end of the clip (`EndTick`).
pub fn skip_to_end(state: &mut EditorState) {
    let end_tick = state
        .animation_data
        .as_ref()
        .map(|d| d.metadata.end_tick)
        .unwrap_or(0);
    set_playhead(state, end_tick);
}

/// Exact `Thunks/Playback/SkipAnimation` (backward): moves playhead to
/// the greatest keyframe tick strictly less than the current playhead.
pub fn skip_to_prev_keyframe(state: &mut EditorState) {
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    if anim.metadata.end_tick - anim.metadata.start_tick == 0 {
        return;
    }
    let current = state.playback.playhead;
    let summary = anim.get_summary_ticks();
    if let Some(&prev) = summary.iter().rev().find(|&&t| t < current) {
        set_playhead(state, prev);
    }
}

/// Exact `Thunks/Playback/SkipAnimation` (forward): moves playhead to
/// the smallest keyframe tick strictly greater than the current playhead.
pub fn skip_to_next_keyframe(state: &mut EditorState) {
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    if anim.metadata.end_tick - anim.metadata.start_tick == 0 {
        return;
    }
    let current = state.playback.playhead;
    let summary = anim.get_summary_ticks();
    if let Some(&next) = summary.iter().find(|&&t| t > current) {
        set_playhead(state, next);
    }
}

/// Steps the playhead by `delta_frames` frames at the active frame rate.
pub fn step_frame(state: &mut EditorState, delta_frames: i32) {
    let fps = state.frame_rate.max(1) as f64;
    let freq = TICK_FREQUENCY as f64;
    let current_frame = ((state.playback.playhead as f64) * fps / freq + 0.5).floor() as i32;
    let target_frame = (current_frame + delta_frames).max(0);
    let target_tick = nearest_tick((target_frame as f64) * freq / fps);
    let max_end = state
        .animation_data
        .as_ref()
        .map(|d| d.metadata.end_tick.max(state.playback.editing_length))
        .unwrap_or(state.playback.editing_length)
        .max(0);
    let clamped = if max_end > 0 {
        target_tick.clamp(0, max_end)
    } else {
        clamp_tick(target_tick)
    };
    set_playhead(state, clamped);
}

/// Sets the playhead to `tick` (clamped to `[0, MAX_ANIMATION_LENGTH]`) and
/// re-anchors active playback if currently playing.
pub fn set_playhead(state: &mut EditorState, tick: Tick) {
    let clamped = clamp_tick(tick);
    state.playback.playhead = clamped;
    if state.playback.play_state != PlayState::Pause {
        state.playback.start_playhead = clamped;
        state.playback.start_time_secs = Some(0.0);
    }
}

/// Exact `Util/StringUtils` `formatTime(tck, frameRate, unit)`.
///
/// - `TimelineUnit::Frames`: integer frame (`"38"`)
/// - `TimelineUnit::SecondsFrames`: `S:FF` (`"1:08"`)
/// - `TimelineUnit::Seconds`: decimal seconds rounded to 2 decimals (`"1.25"`)
pub fn format_time(tick: Tick, frame_rate: u32, unit: TimelineUnit) -> String {
    let freq = TICK_FREQUENCY as f64;
    match unit {
        TimelineUnit::Frames => {
            if frame_rate == 0 {
                return "0".to_string();
            }
            let frame = ((tick as f64) * (frame_rate as f64) / freq + 1e-6).floor() as i32;
            frame.to_string()
        }
        TimelineUnit::SecondsFrames => {
            if frame_rate == 0 {
                return "0:00".to_string();
            }
            let total_frames = ((tick as f64) * (frame_rate as f64) / freq + 0.5).floor() as i32;
            let fps_i = frame_rate as i32;
            let sec = total_frames.div_euclid(fps_i);
            let rem = total_frames.rem_euclid(fps_i);
            format!("{sec}:{rem:02}")
        }
        TimelineUnit::Seconds => {
            let secs_hundredths = ((tick as f64) / freq * 100.0 + 0.5).floor() as i64;
            let whole = secs_hundredths / 100;
            let frac = (secs_hundredths % 100).abs();
            if frac == 0 {
                format!("{whole}")
            } else if frac % 10 == 0 {
                format!("{whole}.{}", frac / 10)
            } else {
                format!("{whole}.{frac:02}")
            }
        }
    }
}

/// Exact `Util/StringUtils` `parseTime(str, frameRate, unit)`.
pub fn parse_time_input(text: &str, frame_rate: u32, unit: TimelineUnit) -> Option<Tick> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let fps = frame_rate.max(1) as f64;
    let freq = TICK_FREQUENCY as f64;

    if let Some((sec_str, frame_str)) = trimmed.split_once(':') {
        let sec: f64 = sec_str.trim().parse().ok()?;
        let frames: f64 = frame_str.trim().parse().ok()?;
        if !sec.is_finite() || !frames.is_finite() {
            return None;
        }
        let ticks = (sec + frames / fps) * freq;
        return Some(clamp_tick(nearest_tick(ticks)));
    }

    let val: f64 = trimmed.parse().ok()?;
    if !val.is_finite() {
        return None;
    }

    let ticks = match unit {
        TimelineUnit::Frames | TimelineUnit::SecondsFrames => val * freq / fps,
        TimelineUnit::Seconds => val * freq,
    };
    Some(clamp_tick(nearest_tick(ticks)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim_editor::model::{AnimationData, Keyframe, KeyframeValue, TrackType};
    use crate::anim_editor::state::GridSpeedSetting;

    fn make_state_with_end(end_tick: Tick, looping: bool) -> EditorState {
        let mut state = EditorState::new();
        let mut anim = AnimationData::new_rig_animation("TestClip");
        anim.metadata.end_tick = end_tick;
        anim.metadata.looping = looping;
        state.animation_data = Some(anim);
        state
    }

    #[test]
    fn playback_stops_at_end_when_not_looping() {
        let mut state = make_state_with_end(2400, false);
        toggle_play(&mut state);
        assert_eq!(state.playback.play_state, PlayState::Play);

        assert!(step_playback(&mut state, 0.5));
        assert_eq!(state.playback.playhead, 1200);

        assert!(step_playback(&mut state, 0.75));
        assert_eq!(state.playback.playhead, 2400);
        assert_eq!(state.playback.play_state, PlayState::Pause);
    }

    #[test]
    fn playback_wraps_when_looping() {
        let mut state = make_state_with_end(2400, true);
        toggle_play(&mut state);
        assert!(step_playback(&mut state, 1.25));
        assert_eq!(state.playback.playhead, 600);
        assert_eq!(state.playback.play_state, PlayState::Play);
    }

    #[test]
    fn reverse_playback_and_looping() {
        let mut state = make_state_with_end(2400, true);
        state.playback.playhead = 600;
        toggle_reverse(&mut state);
        assert_eq!(state.playback.play_state, PlayState::Reverse);

        assert!(step_playback(&mut state, 0.5));
        assert_eq!(state.playback.playhead, 1800);

        state.animation_data.as_mut().unwrap().metadata.looping = false;
        assert!(step_playback(&mut state, 1.0));
        assert_eq!(state.playback.playhead, 0);
        assert_eq!(state.playback.play_state, PlayState::Pause);
    }

    #[test]
    fn playback_speed_scales_advance_without_jumping() {
        let mut state = make_state_with_end(4800, false);
        set_playback_speed(&mut state, 2.0);
        toggle_play(&mut state);
        assert!(step_playback(&mut state, 0.5));
        assert_eq!(state.playback.playhead, 2400);

        set_playback_speed(&mut state, 0.5);
        assert!(step_playback(&mut state, 0.5));
        assert_eq!(state.playback.playhead, 3000);
    }

    #[test]
    fn skip_prev_and_next_keyframe() {
        let mut state = make_state_with_end(2400, false);
        {
            let anim = state.animation_data.as_mut().unwrap();
            let track = anim.add_track(
                "Root",
                "Head",
                TrackType::CFrame,
                TrackType::EulerAngles,
                Default::default(),
            );
            for &t in &[0, 480, 1200, 2400] {
                track.add_keyframe(
                    t,
                    Keyframe::new_pose(KeyframeValue::default_for(TrackType::CFrame)),
                );
            }
            anim.set_end_tick();
        }

        state.playback.playhead = 0;
        skip_to_next_keyframe(&mut state);
        assert_eq!(state.playback.playhead, 480);
        skip_to_next_keyframe(&mut state);
        assert_eq!(state.playback.playhead, 1200);
        skip_to_prev_keyframe(&mut state);
        assert_eq!(state.playback.playhead, 480);
        skip_to_end(&mut state);
        assert_eq!(state.playback.playhead, 2400);
        skip_to_start(&mut state);
        assert_eq!(state.playback.playhead, 0);
    }

    #[test]
    fn time_format_and_parse_roundtrip() {
        // 30 fps: 80 ticks per frame, 2400 ticks = 1 second
        assert_eq!(format_time(0, 30, TimelineUnit::Frames), "0");
        assert_eq!(format_time(80, 30, TimelineUnit::Frames), "1");
        assert_eq!(format_time(2400, 30, TimelineUnit::Frames), "30");

        assert_eq!(format_time(0, 30, TimelineUnit::SecondsFrames), "0:00");
        assert_eq!(format_time(3040, 30, TimelineUnit::SecondsFrames), "1:08");

        assert_eq!(format_time(0, 30, TimelineUnit::Seconds), "0");
        assert_eq!(format_time(3000, 30, TimelineUnit::Seconds), "1.25");
        assert_eq!(format_time(3600, 30, TimelineUnit::Seconds), "1.5");

        assert_eq!(parse_time_input("30", 30, TimelineUnit::Frames), Some(2400));
        assert_eq!(
            parse_time_input("1:08", 30, TimelineUnit::SecondsFrames),
            Some(3040)
        );
        assert_eq!(
            parse_time_input("1.25", 30, TimelineUnit::Seconds),
            Some(3000)
        );
        assert_eq!(parse_time_input("bad", 30, TimelineUnit::Seconds), None);
    }

    #[test]
    fn grid_offset_reflects_speed_setting() {
        let mut state = make_state_with_end(2400, false);
        state.playback.playhead = 1200; // 0.5 sec
        state.settings.grid_speed = GridSpeedSetting::Default; // 16 studs/sec
        assert!((compute_grid_offset(&state) - 8.0).abs() < 1e-4);
    }
}
