//! Light and Dark Studio color palettes for `AnimationClipEditor`
//! (derived from `Util/Theme` and `Util/Constants`).

use egui::Color32;

use crate::anim_editor::constants::{PROP_X, PROP_Y, PROP_Z};
use crate::anim_editor::model::{EasingStyle, KeyInterpolationMode, TrackType};

// ============================================================================
// Viewport Microbone & IK Constants (from `Util/Constants` & `Theme`)
// ============================================================================

pub const BONE_NODE_COLOR: Color32 = Color32::from_rgb(232, 112, 240);
pub const BONE_LINK_COLOR: Color32 = Color32::from_rgb(107, 50, 124);
pub const BONE_CONE_COLOR: Color32 = Color32::from_rgb(207, 88, 224);

pub const BONE_COLOR_DEFAULT: Color32 = Color32::from_rgb(207, 88, 224);
/// Literal `Color3.fromRGB(300, 88, 224)` in Studio, clamped to 255 in 8-bit RGB.
pub const BONE_COLOR_HOVER: Color32 = Color32::from_rgb(255, 88, 224);
pub const BONE_COLOR_SELECTED: Color32 = Color32::from_rgb(13, 105, 172);

pub const IK_LINE_COLOR: Color32 = Color32::from_rgb(182, 80, 203);

// ============================================================================
// Keyframe Diamond Color Tuple
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyframeColors {
    pub background: Color32,
    pub border: Color32,
    pub selected_background: Color32,
    pub selected_border: Color32,
}

impl KeyframeColors {
    pub const fn fill(self, selected: bool) -> Color32 {
        if selected {
            self.selected_background
        } else {
            self.background
        }
    }

    pub const fn stroke(self, selected: bool) -> Color32 {
        if selected {
            self.selected_border
        } else {
            self.border
        }
    }
}

// ============================================================================
// AceTheme
// ============================================================================

/// Complete visual theme for the Animation Clip Editor, matching Roblox Studio's
/// `Util/Theme` dark and light palettes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AceTheme {
    pub is_dark: bool,

    // Base Studio chrome
    pub background: Color32,
    pub border: Color32,
    pub text: Color32,
    pub dimmed_text: Color32,
    pub bright_text: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub selection_box_fill: Color32,
    pub selection_box_border: Color32,

    // Track rows
    pub track_shaded_bg: Color32,
    pub track_title_bg: Color32,
    pub track_primary_bg: Color32,
    pub track_selected_bg: Color32,
    pub track_text: Color32,
    pub track_selected_text: Color32,

    // Timeline & grid lines
    pub timeline_bg: Color32,
    pub timeline_bar: Color32,
    pub timeline_line: Color32,
    pub timeline_text: Color32,
    pub timeline_dimmed: Color32,
    /// Vertical gridline for non-second ticks (`lowerTransparency = 0.85`).
    pub timeline_grid_line: Color32,
    /// Vertical gridline for whole-second ticks (`lowerBrightTransparency = 0.70`).
    pub timeline_grid_line_bright: Color32,

    // Scrubber & scale controls
    pub scrubber: Color32,
    pub scale_controls_main: Color32,
    pub scale_controls_text: Color32,

    // Scrollbars & ZoomBar
    pub scrollbar_bg: Color32,
    pub scrollbar_control: Color32,
    pub scrollbar_hover: Color32,
    pub scrollbar_pressed: Color32,
    pub scrollbar_arrow: Color32,

    // Keyframe diamond palettes
    pub keyframe_default: KeyframeColors,
    pub keyframe_primary: KeyframeColors,
    pub keyframe_bounce: KeyframeColors,
    pub keyframe_constant: KeyframeColors,
    pub keyframe_cubic: KeyframeColors,
    pub keyframe_elastic: KeyframeColors,
    pub keyframe_error: KeyframeColors,
    pub keyframe_primary_error: KeyframeColors,
    pub keyframe_cluster: Color32,
    pub keyframe_primary_cluster: Color32,

    // Curve editor colors
    pub curve_default: Color32,
    pub curve_pos_x: Color32,
    pub curve_pos_y: Color32,
    pub curve_pos_z: Color32,
    pub curve_rot_x: Color32,
    pub curve_rot_y: Color32,
    pub curve_rot_z: Color32,
    pub curve_quat: Color32,
    pub curve_facs: Color32,
    pub curve_selected: Color32,
    pub curve_x_axis: Color32,
    pub tangent_control: Color32,

    // Event markers
    pub event_marker_bg: Color32,
    pub event_marker_border: Color32,
    pub event_marker_selected_border: Color32,

    // Viewport bones & IK
    pub bone_color_default: Color32,
    pub bone_color_hover: Color32,
    pub bone_color_selected: Color32,
    pub bone_node_color: Color32,
    pub bone_link_color: Color32,
    pub bone_cone_color: Color32,
    pub ik_line_color: Color32,

    // Dialogs, toasts & overlays
    pub dialog_text: Color32,
    pub dialog_sub_text: Color32,
    pub dialog_error_text: Color32,
    pub toast_text: Color32,
    pub toast_shadow: Color32,
    pub start_screen_text: Color32,
    pub start_screen_dark_text: Color32,
    pub overlay_dim: Color32,

    // Typography sizes
    pub font_size_track: f32,
    pub font_size_timeline: f32,
    pub font_size_textbox: f32,
    pub font_size_dialog: f32,
    pub font_size_dialog_sub: f32,
    pub font_size_dialog_header: f32,
}

impl AceTheme {
    /// Dark theme palette aligned with RusTudio's main Studio chrome
    /// (`rustudio_engine::theme`: `#191a1f` panel, `#202227` headers, `#353741` dividers).
    pub fn dark() -> Self {
        use rustudio_engine::theme as ui;

        let accent = ui::SELECT; // Studio Select blue (25, 153, 255)
        let border = ui::DIVIDER; // #353741
        let dimmed = ui::TEXT_DIM; // #7d7f84
        let main_text = ui::TEXT; // #dedfe2
        let bright_text = Color32::WHITE;
        let gray_light = Color32::from_rgb(184, 184, 184);
        let kf_easing_border = Color32::from_rgb(56, 56, 56);
        let dark_red = Color32::from_rgb(255, 68, 68);

        Self {
            is_dark: true,

            background: ui::PANEL, // #191a1f (25, 26, 31)
            border,
            text: main_text,
            dimmed_text: dimmed,
            bright_text,
            accent,
            accent_text: Color32::WHITE,
            selection_box_fill: Color32::from_rgba_unmultiplied(25, 153, 255, 51),
            selection_box_border: accent,

            track_shaded_bg: Color32::from_rgb(30, 31, 37), // matches egui faint_bg_color
            track_title_bg: ui::PANEL_DARK,                 // #202227 (32, 34, 39)
            track_primary_bg: Color32::from_rgb(20, 21, 26),
            track_selected_bg: ui::SELECT_BG,               // (38, 62, 120)
            track_text: main_text,
            track_selected_text: Color32::WHITE,

            timeline_bg: ui::PANEL_DARK, // #202227 (32, 34, 39)
            timeline_bar: ui::PANEL,     // #191a1f (25, 26, 31)
            timeline_line: dimmed,
            timeline_text: dimmed,
            timeline_dimmed: Color32::from_rgb(92, 94, 102),
            timeline_grid_line: Color32::from_rgba_unmultiplied(125, 127, 132, 38),
            timeline_grid_line_bright: Color32::from_rgba_unmultiplied(125, 127, 132, 77),

            scrubber: accent,
            scale_controls_main: accent,
            scale_controls_text: Color32::WHITE,

            scrollbar_bg: ui::PANEL,
            scrollbar_control: ui::PANEL_ALT, // #2c2d33 (44, 45, 51)
            scrollbar_hover: ui::ACTIVE_TAB,  // #33343a (51, 52, 58)
            scrollbar_pressed: Color32::from_rgb(62, 64, 74),
            scrollbar_arrow: main_text,

            keyframe_default: KeyframeColors {
                background: Color32::from_rgb(92, 92, 92),
                border: dimmed,
                selected_background: Color32::from_rgb(92, 92, 92),
                selected_border: accent,
            },
            keyframe_primary: KeyframeColors {
                background: gray_light,
                border,
                selected_background: gray_light,
                selected_border: accent,
            },
            keyframe_bounce: KeyframeColors {
                background: Color32::from_rgb(155, 198, 204),
                border: kf_easing_border,
                selected_background: Color32::from_rgb(155, 198, 204),
                selected_border: accent,
            },
            keyframe_constant: KeyframeColors {
                background: Color32::from_rgb(156, 147, 226),
                border: kf_easing_border,
                selected_background: Color32::from_rgb(156, 147, 226),
                selected_border: accent,
            },
            keyframe_cubic: KeyframeColors {
                background: Color32::from_rgb(254, 189, 81),
                border: kf_easing_border,
                selected_background: Color32::from_rgb(254, 189, 81),
                selected_border: accent,
            },
            keyframe_elastic: KeyframeColors {
                background: Color32::from_rgb(137, 187, 77),
                border: kf_easing_border,
                selected_background: Color32::from_rgb(137, 187, 77),
                selected_border: accent,
            },
            keyframe_error: KeyframeColors {
                background: Color32::from_rgb(102, 102, 102),
                border: dark_red,
                selected_background: Color32::from_rgb(170, 170, 170),
                selected_border: dark_red,
            },
            keyframe_primary_error: KeyframeColors {
                background: Color32::from_rgb(102, 102, 102),
                border: dark_red,
                selected_background: Color32::from_rgb(170, 170, 170),
                selected_border: dark_red,
            },
            keyframe_cluster: dimmed,
            keyframe_primary_cluster: Color32::from_rgb(170, 170, 170),

            curve_default: main_text,
            curve_pos_x: Color32::from_rgb(255, 0, 0),
            curve_pos_y: Color32::from_rgb(3, 201, 3),
            curve_pos_z: Color32::from_rgb(0, 0, 255),
            curve_rot_x: Color32::from_rgb(47, 128, 123),
            curve_rot_y: Color32::from_rgb(255, 0, 255),
            curve_rot_z: Color32::from_rgb(255, 165, 0),
            curve_quat: main_text,
            curve_facs: main_text,
            curve_selected: accent,
            curve_x_axis: bright_text,
            tangent_control: bright_text,

            event_marker_bg: Color32::from_rgb(204, 204, 204),
            event_marker_border: Color32::from_rgb(56, 56, 56),
            event_marker_selected_border: accent,

            bone_color_default: BONE_COLOR_DEFAULT,
            bone_color_hover: BONE_COLOR_HOVER,
            bone_color_selected: BONE_COLOR_SELECTED,
            bone_node_color: BONE_NODE_COLOR,
            bone_link_color: BONE_LINK_COLOR,
            bone_cone_color: BONE_CONE_COLOR,
            ik_line_color: IK_LINE_COLOR,

            dialog_text: main_text,
            dialog_sub_text: dimmed,
            dialog_error_text: dark_red,
            toast_text: main_text,
            toast_shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 64),
            start_screen_text: main_text,
            start_screen_dark_text: Color32::WHITE,
            overlay_dim: Color32::from_rgba_unmultiplied(0, 0, 0, 166), // transparency 0.35

            font_size_track: 15.0,
            font_size_timeline: 15.0,
            font_size_textbox: 16.0,
            font_size_dialog: 16.0,
            font_size_dialog_sub: 15.0,
            font_size_dialog_header: 18.0,
        }
    }

    /// Roblox Studio Light theme palette (`Util/Theme`).
    pub fn light() -> Self {
        let accent = Color32::from_rgb(0, 162, 255);
        let border = Color32::from_rgb(200, 200, 200);
        let dimmed = Color32::from_rgb(117, 117, 117);
        let main_text = Color32::from_rgb(51, 51, 51);
        let bright_text = Color32::from_rgb(0, 0, 0);
        let kf_easing_border = Color32::from_rgb(56, 56, 56);
        let light_red = Color32::from_rgb(255, 68, 68);

        Self {
            is_dark: false,

            background: Color32::WHITE,
            border,
            text: main_text,
            dimmed_text: dimmed,
            bright_text,
            accent,
            accent_text: Color32::WHITE,
            selection_box_fill: Color32::from_rgba_unmultiplied(0, 162, 255, 51),
            selection_box_border: accent,

            track_shaded_bg: Color32::from_rgb(243, 243, 243),
            track_title_bg: Color32::from_rgb(227, 227, 227),
            track_primary_bg: Color32::from_rgb(243, 243, 243),
            track_selected_bg: Color32::from_rgb(104, 148, 217),
            track_text: main_text,
            track_selected_text: Color32::WHITE,

            timeline_bg: Color32::from_rgb(243, 243, 243),
            timeline_bar: Color32::WHITE,
            timeline_line: dimmed,
            timeline_text: dimmed,
            timeline_dimmed: Color32::from_rgb(184, 184, 184),
            timeline_grid_line: Color32::from_rgba_unmultiplied(117, 117, 117, 38),
            timeline_grid_line_bright: Color32::from_rgba_unmultiplied(117, 117, 117, 77),

            scrubber: accent,
            scale_controls_main: accent,
            scale_controls_text: Color32::WHITE,

            scrollbar_bg: Color32::from_rgb(240, 240, 240),
            scrollbar_control: Color32::WHITE,
            scrollbar_hover: Color32::from_rgb(231, 240, 250),
            scrollbar_pressed: Color32::from_rgb(224, 224, 224),
            scrollbar_arrow: main_text,

            keyframe_default: KeyframeColors {
                background: Color32::from_rgb(184, 184, 184),
                border: dimmed,
                selected_background: Color32::from_rgb(184, 184, 184),
                selected_border: accent,
            },
            keyframe_primary: KeyframeColors {
                background: Color32::WHITE,
                border,
                selected_background: Color32::WHITE,
                selected_border: accent,
            },
            keyframe_bounce: KeyframeColors {
                background: Color32::from_rgb(155, 198, 204),
                border: kf_easing_border,
                selected_background: Color32::from_rgb(155, 198, 204),
                selected_border: accent,
            },
            keyframe_constant: KeyframeColors {
                background: Color32::from_rgb(156, 147, 226),
                border: kf_easing_border,
                selected_background: Color32::from_rgb(156, 147, 226),
                selected_border: accent,
            },
            keyframe_cubic: KeyframeColors {
                background: Color32::from_rgb(254, 189, 81),
                border: kf_easing_border,
                selected_background: Color32::from_rgb(254, 189, 81),
                selected_border: accent,
            },
            keyframe_elastic: KeyframeColors {
                background: Color32::from_rgb(137, 187, 77),
                border: kf_easing_border,
                selected_background: Color32::from_rgb(137, 187, 77),
                selected_border: accent,
            },
            keyframe_error: KeyframeColors {
                background: Color32::from_rgb(255, 161, 161),
                border: Color32::from_rgb(168, 132, 132),
                selected_background: Color32::from_rgb(255, 161, 161),
                selected_border: light_red,
            },
            keyframe_primary_error: KeyframeColors {
                background: Color32::from_rgb(255, 161, 161),
                border: Color32::from_rgb(168, 132, 132),
                selected_background: Color32::from_rgb(255, 161, 161),
                selected_border: light_red,
            },
            keyframe_cluster: dimmed,
            keyframe_primary_cluster: Color32::from_rgb(136, 136, 136),

            curve_default: main_text,
            curve_pos_x: Color32::from_rgb(255, 0, 0),
            curve_pos_y: Color32::from_rgb(3, 201, 3),
            curve_pos_z: Color32::from_rgb(0, 0, 255),
            curve_rot_x: Color32::from_rgb(47, 128, 123),
            curve_rot_y: Color32::from_rgb(255, 0, 255),
            curve_rot_z: Color32::from_rgb(255, 165, 0),
            curve_quat: main_text,
            curve_facs: main_text,
            curve_selected: accent,
            curve_x_axis: bright_text,
            tangent_control: bright_text,

            event_marker_bg: Color32::from_rgb(102, 102, 102),
            event_marker_border: Color32::from_rgb(56, 56, 56),
            event_marker_selected_border: accent,

            bone_color_default: BONE_COLOR_DEFAULT,
            bone_color_hover: BONE_COLOR_HOVER,
            bone_color_selected: BONE_COLOR_SELECTED,
            bone_node_color: BONE_NODE_COLOR,
            bone_link_color: BONE_LINK_COLOR,
            bone_cone_color: BONE_CONE_COLOR,
            ik_line_color: IK_LINE_COLOR,

            dialog_text: main_text,
            dialog_sub_text: dimmed,
            dialog_error_text: light_red,
            toast_text: main_text,
            toast_shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 64),
            start_screen_text: main_text,
            start_screen_dark_text: Color32::WHITE,
            overlay_dim: Color32::from_rgba_unmultiplied(0, 0, 0, 166),

            font_size_track: 15.0,
            font_size_timeline: 15.0,
            font_size_textbox: 16.0,
            font_size_dialog: 16.0,
            font_size_dialog_sub: 15.0,
            font_size_dialog_header: 18.0,
        }
    }

    /// Default active theme (Dark). Use `for_dark_mode` or `from_egui` to follow egui visuals.
    pub fn current() -> Self {
        Self::dark()
    }

    pub fn for_dark_mode(dark: bool) -> Self {
        if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }

    pub fn from_egui(visuals: &egui::Visuals) -> Self {
        Self::for_dark_mode(visuals.dark_mode)
    }

    /// Resolves keyframe diamond colors matching `Constants.KEYFRAME_STYLE`.
    pub fn keyframe_colors(
        &self,
        easing_style: Option<EasingStyle>,
        interpolation_mode: Option<KeyInterpolationMode>,
        is_primary: bool,
        is_error: bool,
    ) -> KeyframeColors {
        if is_error {
            return if is_primary {
                self.keyframe_primary_error
            } else {
                self.keyframe_error
            };
        }
        if is_primary {
            return self.keyframe_primary;
        }
        if let Some(style) = easing_style {
            return match style {
                EasingStyle::Linear => self.keyframe_default,
                EasingStyle::Constant => self.keyframe_constant,
                EasingStyle::Cubic | EasingStyle::CubicV2 => self.keyframe_cubic,
                EasingStyle::Elastic => self.keyframe_elastic,
                EasingStyle::Bounce => self.keyframe_bounce,
            };
        }
        if let Some(mode) = interpolation_mode {
            return match mode {
                KeyInterpolationMode::Linear => self.keyframe_default,
                KeyInterpolationMode::Constant => self.keyframe_constant,
                KeyInterpolationMode::Cubic => self.keyframe_cubic,
            };
        }
        self.keyframe_default
    }

    /// Resolves curve stroke color matching `Constants.TRACK_THEME_MAPPING`.
    pub fn curve_color_for_track(&self, track_type: TrackType, leaf_name: &str) -> Color32 {
        match (track_type, leaf_name) {
            (TrackType::Number, PROP_X) => self.curve_pos_x,
            (TrackType::Number, PROP_Y) => self.curve_pos_y,
            (TrackType::Number, PROP_Z) => self.curve_pos_z,
            (TrackType::Angle, PROP_X) => self.curve_rot_x,
            (TrackType::Angle, PROP_Y) => self.curve_rot_y,
            (TrackType::Angle, PROP_Z) => self.curve_rot_z,
            (TrackType::Quaternion, _) => self.curve_quat,
            (TrackType::Facs, _) => self.curve_facs,
            _ => self.curve_default,
        }
    }
}
