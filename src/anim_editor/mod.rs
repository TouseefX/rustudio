//! Native Animation Editor integrated into RusTudio's dockable panel system
//! and 3D viewport.

pub mod constants;
pub mod curves;
pub mod events;
pub mod facs;
pub mod history;
pub mod host;
pub mod ik;
pub mod model;
pub mod ops;
pub mod playback;
pub mod reduction;
pub mod rig;
pub mod rig_viz;
pub mod selection;
pub mod serialization;
pub mod state;
pub mod theme;
pub mod ui_control_panel;
pub mod ui_curve_editor;
pub mod ui_dialogs;
pub mod ui_dopesheet;
pub mod ui_facs;
pub mod ui_ik;
pub mod ui_main;
pub mod ui_menus;
pub mod ui_timeline;
pub mod ui_tracklist;
pub mod viewport_dragger;

#[cfg(test)]
mod tests;

pub use state::AnimationEditor;
