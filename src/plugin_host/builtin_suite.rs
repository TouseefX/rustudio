//! Modular Built-in Suite registrations for RusTudio's native Rust Studio tools.
//!
//! Each native Rust tool registers as a first-class [`NativePlugin`] entry in
//! [`super::native::NativePluginHost`] so it appears in **Plugins → Manage
//! Plugins → Built-in Suite**, persists enable/disable state in
//! `plugin_suite.json`, unloads cleanly when toggled off or switched to
//! Roblox's `.rbxm` built-in in Studio Settings, and never boots the Luau VM.

use std::any::Any;

use rustudio_core::rustudio_settings::BuiltinTool;

use super::native::{NativePlugin, NativePluginEntry, NativePluginHost, NativePluginMeta};

pub const TOOLBOX_ID: &str = "rustudio_Toolbox";
pub const TERRAIN_EDITOR_ID: &str = "rustudio_TerrainEditor";
pub const ASSET_MANAGER_ID: &str = "rustudio_AssetManager";
pub const RIG_BUILDER_ID: &str = "rustudio_RigBuilder";
pub const VIEW_SELECTOR_ID: &str = "rustudio_ViewSelector";
pub const MATERIAL_PICKER_ID: &str = "rustudio_MaterialPicker";
pub const DRAGGERS_ID: &str = "rustudio_Draggers";

/// All 7 remaining native Rust built-in tools registered by this module.
pub const ALL_IDS: &[&str] = &[
    TOOLBOX_ID,
    TERRAIN_EDITOR_ID,
    ASSET_MANAGER_ID,
    RIG_BUILDER_ID,
    VIEW_SELECTOR_ID,
    MATERIAL_PICKER_ID,
    DRAGGERS_ID,
];

/// Lightweight [`NativePlugin`] module representing a native Rust Studio tool
/// managed through the Built-in Suite.
pub struct BuiltinSuitePlugin {
    meta: NativePluginMeta,
}

impl NativePlugin for BuiltinSuitePlugin {
    fn meta(&self) -> NativePluginMeta {
        self.meta
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn entry(meta: NativePluginMeta) -> NativePluginEntry {
    NativePluginEntry {
        meta,
        factory: Box::new(move || Box::new(BuiltinSuitePlugin { meta })),
    }
}

/// Registers all remaining native Rust Studio tools into `host`.
pub fn register(host: &mut NativePluginHost) {
    host.register_factory(entry(NativePluginMeta {
        id: TOOLBOX_ID,
        display_name: "Toolbox",
        description: "Creator Store, Inventory, and Recent models, decals, meshes, and audio (native Rust)",
        builtin_tool: Some(BuiltinTool::Toolbox),
        capability: Some("toolbox"),
        widget: None,
        has_bottom_bar: false,
    }));

    host.register_factory(entry(NativePluginMeta {
        id: TERRAIN_EDITOR_ID,
        display_name: "Terrain Editor",
        description: "Voxel terrain generator, sculpt brushes, paint, and region tools (native Rust)",
        builtin_tool: Some(BuiltinTool::TerrainEditor),
        capability: Some("terrain_editor"),
        widget: None,
        has_bottom_bar: false,
    }));

    host.register_factory(entry(NativePluginMeta {
        id: ASSET_MANAGER_ID,
        display_name: "Asset Manager",
        description: "Place and experience images, meshes, audio, models, and bulk import (native Rust)",
        builtin_tool: None,
        capability: Some("asset_manager"),
        widget: None,
        has_bottom_bar: false,
    }));

    host.register_factory(entry(NativePluginMeta {
        id: RIG_BUILDER_ID,
        display_name: "Character / Rig Builder",
        description: "Insert R15, R6, Mesh, and user avatar rigs into Workspace (native Rust)",
        builtin_tool: Some(BuiltinTool::RigBuilder),
        capability: Some("character"),
        widget: None,
        has_bottom_bar: false,
    }));

    host.register_factory(entry(NativePluginMeta {
        id: VIEW_SELECTOR_ID,
        display_name: "View Selector",
        description: "3D viewport orientation cube for snapping to axis views (native Rust)",
        builtin_tool: Some(BuiltinTool::ViewSelector),
        capability: Some("view_selector"),
        widget: None,
        has_bottom_bar: false,
    }));

    host.register_factory(entry(NativePluginMeta {
        id: MATERIAL_PICKER_ID,
        display_name: "Material Picker",
        description: "Home & Model ribbon material swatch grid for selected parts (native Rust)",
        builtin_tool: Some(BuiltinTool::MaterialPicker),
        capability: Some("material"),
        widget: None,
        has_bottom_bar: false,
    }));

    host.register_factory(entry(NativePluginMeta {
        id: DRAGGERS_ID,
        display_name: "Viewport Draggers",
        description: "Select, Move, Scale, and Rotate 3D part gizmos and handles (native Rust)",
        builtin_tool: Some(BuiltinTool::Draggers),
        capability: Some("draggers"),
        widget: None,
        has_bottom_bar: false,
    }));
}
