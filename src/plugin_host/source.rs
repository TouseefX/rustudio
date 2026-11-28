//! Plugin sources: a file (.lua / .luau / .rbxm / .rbxmx), an installed
//! Creator Store plugin's model, or an in-memory instance tree, read into
//! the model the loader copies under the Plugin instance.
//!
//! Studio names (measured, 0.747): a local plugin file `X.rbxmx` loads as
//! Plugin `user_X.rbxmx`, a bare `X.lua` as `user_X.lua` holding one Script
//! named "Script", a .lua in a subfolder as `user_Sub/Deeper/X.lua`;
//! installed plugins are `cloud_<asset id>` (Studio's log: "Running plugin
//! cloud_701506235"). A Plugins folder: `.lua` files
//! (any case) at any depth, `.rbxmx` / `.rbxm` at the top level only,
//! `.luau` not at all; loaded as all .lua sorted case-insensitively by
//! path, then the .rbxmx, then the .rbxm (Studio reads them in parallel and
//! its order varies; this is its deterministic run). `--plugin X.luau`
//! still loads a .luau file given by name.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rbx_dom_weak::types::Ref;
use rbx_dom_weak::{InstanceBuilder, WeakDom};
use rustudio_core::core_scripts::Bytecode;

/// A decoded plugin model: its instance tree plus any ProtectedString Luau
/// bytecode extracted from `Source` properties (built-in plugins ship compiled
/// Luau bytecode v14).
#[derive(Clone)]
pub struct PluginModel {
    pub dom: Arc<WeakDom>,
    pub bytecode: Arc<HashMap<Ref, Bytecode>>,
}

/// Where a plugin comes from.
#[derive(Clone, Debug)]
pub enum PluginSource {
    /// A local plugin file in a Plugins folder (or given with --plugin).
    File(PathBuf),
    /// A .lua in a subfolder of a Plugins folder: `rel` is its path below
    /// the folder with '/' separators (Plugin.Name `user_<rel>`).
    Nested { path: PathBuf, rel: String },
    /// An installed (Creator Store) plugin: its model file on disk, as the
    /// store downloaded it.
    Cloud { asset_id: u64, path: PathBuf },
    /// A Roblox Studio built-in plugin from
    /// `<Studio>/BuiltInPlugins/Optimized_Embedded_Signature/<Name>.rbxm`.
    BuiltIn(PathBuf),
    /// A RusTudio-shipped Luau/Vide built-in plugin from `plugins/` (a single
    /// `.luau`/`.lua`/`.rbxm`/`.rbxmx` file or a plugin directory with
    /// `init.server.luau` / `init.luau` + `ModuleScript`s and shared `_packages/`).
    Bundled(PathBuf),
    /// A modular native Rust [`super::native::NativePlugin`] hosted inside
    /// `PluginHost`.
    Native(String),
    /// An instance tree already in memory: the root's children are the
    /// plugin's top-level items. `name` is the Plugin's Name as Studio
    /// would give it (`user_Name.rbxmx`, …).
    Tree { name: String, asset_id: Option<u64>, dom: Arc<WeakDom> },
}

impl PluginSource {
    /// Whether this is a built-in plugin (Roblox Studio's or RusTudio's own
    /// bundled suite: higher security identity 6, RobloxPluginGuiService
    /// widgets, not copied into Play DataModels).
    pub fn is_builtin(&self) -> bool {
        matches!(self, PluginSource::BuiltIn(_) | PluginSource::Bundled(_) | PluginSource::Native(_))
    }

    /// Whether this is a RusTudio-bundled Luau plugin.
    pub fn is_bundled(&self) -> bool {
        matches!(self, PluginSource::Bundled(_))
    }

    /// The Plugin instance's Name.
    pub fn plugin_name(&self) -> String {
        match self {
            PluginSource::File(p) => {
                format!("user_{}", p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default())
            }
            PluginSource::Nested { rel, .. } => format!("user_{rel}"),
            PluginSource::Cloud { asset_id, .. } => format!("cloud_{asset_id}"),
            PluginSource::BuiltIn(p) => {
                format!("builtin_{}", p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default())
            }
            PluginSource::Bundled(p) => {
                let stem = p
                    .file_stem()
                    .or_else(|| p.file_name())
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                format!("rustudio_{stem}")
            }
            PluginSource::Native(id) => id.clone(),
            PluginSource::Tree { name, .. } => name.clone(),
        }
    }

    /// printidentity's asset id: the store id, -1 for built-ins, -2 for local plugins.
    pub fn asset_id(&self) -> i64 {
        match self {
            PluginSource::Cloud { asset_id, .. } => *asset_id as i64,
            PluginSource::BuiltIn(_) | PluginSource::Bundled(_) | PluginSource::Native(_) => -1,
            PluginSource::Tree { asset_id: Some(a), .. } => *a as i64,
            _ => -2,
        }
    }

    /// The settings file the plugin's GetSetting / SetSetting use: local and
    /// built-in plugins share InstalledPlugins/0, installed ones have their own.
    pub fn settings_key(&self) -> String {
        match self {
            PluginSource::Cloud { asset_id, .. } => asset_id.to_string(),
            PluginSource::Tree { asset_id: Some(a), .. } => a.to_string(),
            _ => "0".to_string(),
        }
    }

    /// The file it was read from, if any.
    pub fn path(&self) -> Option<&Path> {
        match self {
            PluginSource::File(p)
            | PluginSource::Nested { path: p, .. }
            | PluginSource::Cloud { path: p, .. }
            | PluginSource::BuiltIn(p)
            | PluginSource::Bundled(p) => Some(p),
            PluginSource::Native(_) | PluginSource::Tree { .. } => None,
        }
    }

    /// Reads the plugin's model (again, for a reload).
    pub fn read(&self) -> Result<PluginModel, String> {
        match self {
            PluginSource::BuiltIn(p) => read_builtin_file(p),
            PluginSource::Bundled(p) => {
                read_bundled_plugin(p).map(|dom| PluginModel { dom: Arc::new(dom), bytecode: Arc::new(HashMap::new()) })
            }
            PluginSource::Native(id) => Err(format!("Native plugin {id} is hosted directly in Rust")),
            PluginSource::File(p) | PluginSource::Nested { path: p, .. } | PluginSource::Cloud { path: p, .. } => {
                read_file(p).map(|dom| PluginModel { dom: Arc::new(dom), bytecode: Arc::new(HashMap::new()) })
            }
            PluginSource::Tree { dom, .. } => Ok(PluginModel { dom: dom.clone(), bytecode: Arc::new(HashMap::new()) }),
        }
    }
}

/// Decodes a Studio built-in plugin `.rbxm` (ProtectedString Luau bytecode
/// sources, embedded HASH / SIGN signature chunks stripped).
pub fn read_builtin_file(path: &Path) -> Result<PluginModel, String> {
    let patch = rustudio_core::core_scripts::read(path)?;
    Ok(PluginModel {
        dom: Arc::new(patch.dom),
        bytecode: Arc::new(patch.bytecode),
    })
}

/// Built-in `.rbxm` files in `dir`, sorted case-insensitively by filename.
/// When `filter` is `Some`, only entries whose stem or filename matches
/// (case-insensitively) are included.
pub fn builtin_dir_order(dir: &Path, filter: Option<&[String]>) -> Result<Vec<PluginSource>, String> {
    if !dir.is_dir() {
        return Err(format!("{}: not a folder", dir.display()));
    }
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let is_rbxm = p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| x.eq_ignore_ascii_case("rbxm"));
        if !is_rbxm {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(wanted) = filter {
            let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let hit = wanted.iter().any(|w| {
                w.eq_ignore_ascii_case(&name)
                    || w.eq_ignore_ascii_case(&stem)
                    || w.strip_prefix("builtin_").is_some_and(|s| s.eq_ignore_ascii_case(&name) || s.eq_ignore_ascii_case(&stem))
            });
            if !hit {
                continue;
            }
        }
        files.push((name, p));
    }
    files.sort_by_key(|(n, _)| n.to_lowercase());
    Ok(files.into_iter().map(|(_, p)| PluginSource::BuiltIn(p)).collect())
}

/// A plugin file as a model: a script file becomes one Script named
/// "Script" (measured), a model file its top-level items.
pub fn read_file(path: &Path) -> Result<WeakDom, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    match ext.as_str() {
        "lua" | "luau" => {
            let src = String::from_utf8_lossy(&bytes).into_owned();
            let mut dom = WeakDom::new(InstanceBuilder::new("DataModel"));
            let root = dom.root_ref();
            dom.insert(root, InstanceBuilder::new("Script").with_name("Script").with_property("Source", src));
            Ok(dom)
        }
        "rbxm" | "rbxmx" => rustudio_core::doc::Document::decode(&bytes).map_err(|e| format!("{}: {e}", path.display())),
        _ => Err(format!("{}: not a plugin file (.lua, .luau, .rbxm or .rbxmx)", path.display())),
    }
}

/// Whether Studio would load this file as a local plugin.
pub fn is_plugin_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("lua" | "luau" | "rbxm" | "rbxmx")
    )
}

/// The plugins of a Plugins folder in Studio's load order (see the header).
pub fn dir_order(dir: &Path) -> Result<Vec<PluginSource>, String> {
    let ext = |p: &Path| p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    let mut lua: Vec<(String, PathBuf)> = Vec::new();
    let mut models: Vec<(u8, String, PathBuf)> = Vec::new();
    fn walk(dir: &Path, rel: &str, depth: usize, out: &mut Vec<(String, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if p.is_dir() {
                if depth < 32 {
                    walk(&p, &r, depth + 1, out);
                }
            } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("lua")) {
                out.push((r, p));
            }
        }
    }
    if !dir.is_dir() {
        return Err(format!("{}: not a folder", dir.display()));
    }
    walk(dir, "", 0, &mut lua);
    for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        match ext(&p).as_str() {
            "rbxmx" => models.push((0, name, p)),
            "rbxm" => models.push((1, name, p)),
            _ => {}
        }
    }
    lua.sort_by_key(|(r, _)| r.to_lowercase());
    models.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    let mut out: Vec<PluginSource> = lua
        .into_iter()
        .map(|(rel, path)| if rel.contains('/') { PluginSource::Nested { path, rel } } else { PluginSource::File(path) })
        .collect();
    out.extend(models.into_iter().map(|(_, _, p)| PluginSource::File(p)));
    Ok(out)
}

/// Discovers RusTudio's bundled Luau/Vide built-in plugin suite directory, if
/// present on disk:
/// 1. `RUSTUDIO_SUITE_PLUGINS_DIR` env override
/// 2. `<exe_dir>/plugins`, `<exe_dir>/../Resources/plugins` (macOS `.app`),
///    or `<exe_dir>/../share/rustudio/plugins`
/// 3. `<data_dir>/rustudio/plugins`
/// 4. `<CARGO_MANIFEST_DIR>/plugins` (dev checkout fallback)
pub fn rustudio_suite_plugins_dir() -> Option<PathBuf> {
    if let Some(env) = std::env::var_os("RUSTUDIO_SUITE_PLUGINS_DIR") {
        let p = PathBuf::from(env);
        if p.is_dir() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let exe = exe.canonicalize().unwrap_or(exe);
        if let Some(dir) = exe.parent() {
            for p in [
                dir.join("plugins"),
                dir.join("../Resources/plugins"),
                dir.join("../share/rustudio/plugins"),
            ] {
                if p.is_dir() {
                    return Some(p);
                }
            }
        }
    }
    let data = rustudio_core::paths::data().join("rustudio").join("plugins");
    if data.is_dir() {
        return Some(data);
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins");
    if dev.is_dir() {
        return Some(dev);
    }
    None
}

/// Lists RusTudio bundled Luau plugins in `dir`, sorted case-insensitively.
/// Each top-level `.luau` / `.lua` / `.rbxm` / `.rbxmx` file or subdirectory
/// (ignoring hidden entries and `_packages`) becomes a `PluginSource::Bundled`.
pub fn bundled_dir_order(dir: &Path) -> Result<Vec<PluginSource>, String> {
    if !dir.is_dir() {
        return Err(format!("{}: not a folder", dir.display()));
    }
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.starts_with('_') {
            continue;
        }
        if p.is_dir() {
            if has_directory_entry_script(&p) {
                entries.push((name, p));
            }
        } else if p.is_file() && is_plugin_file(&p) {
            entries.push((name, p));
        }
    }
    entries.sort_by_key(|(n, _)| n.to_lowercase());
    Ok(entries.into_iter().map(|(_, p)| PluginSource::Bundled(p)).collect())
}

fn has_directory_entry_script(dir: &Path) -> bool {
    ["init.server.luau", "init.server.lua", "init.luau", "init.lua", "Main.server.luau", "Main.server.lua"]
        .iter()
        .any(|n| dir.join(n).is_file())
}

/// Decodes a RusTudio bundled Luau plugin from a single file or a directory.
///
/// Directory layout:
/// - `init.server.luau` / `init.luau` / `Main.server.luau` becomes the root
///   `Script` (inside a root `Folder` named after the plugin directory).
/// - Sibling `.luau`/`.lua` files become `ModuleScript` children (or `Script`
///   for `*.server.luau` / `*.server.lua`), and subdirectories become `Folder`
///   or `ModuleScript` (when they contain `init.luau` / `init.lua`).
/// - If a sibling `_packages/` directory exists beside the plugin directory,
///   its modules are mounted under a `Packages` `Folder` sibling so every
///   bundled plugin can `require(script.Parent.Packages.Vide)` directly.
pub fn read_bundled_plugin(path: &Path) -> Result<WeakDom, String> {
    if path.is_file() {
        let mut dom = read_file(path)?;
        if let Some(packages_dir) = path.parent().map(|p| p.join("_packages")).filter(|p| p.is_dir()) {
            let root = dom.root_ref();
            let pkg_folder = dom.insert(root, InstanceBuilder::new("Folder").with_name("Packages"));
            populate_luau_dir(&mut dom, pkg_folder, &packages_dir, false)?;
        }
        return Ok(dom);
    }
    if !path.is_dir() {
        return Err(format!("{}: not a file or plugin directory", path.display()));
    }
    let plugin_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Plugin".to_string());
    let mut dom = WeakDom::new(InstanceBuilder::new("DataModel"));
    let root = dom.root_ref();
    let container = dom.insert(root, InstanceBuilder::new("Folder").with_name(plugin_name));
    populate_luau_dir(&mut dom, container, path, true)?;
    if let Some(packages_dir) = path.parent().map(|p| p.join("_packages")).filter(|p| p.is_dir()) {
        let pkg_folder = dom.insert(container, InstanceBuilder::new("Folder").with_name("Packages"));
        populate_luau_dir(&mut dom, pkg_folder, &packages_dir, false)?;
    }
    Ok(dom)
}

fn populate_luau_dir(dom: &mut WeakDom, parent: Ref, dir: &Path, is_plugin_root: bool) -> Result<(), String> {
    let mut items: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(n, _)| !n.starts_with('.'))
        .collect();
    items.sort_by_key(|(n, _)| n.to_lowercase());

    for (name, p) in items {
        if p.is_dir() {
            let init_mod = ["init.luau", "init.lua"].iter().map(|f| p.join(f)).find(|f| f.is_file());
            let node = if let Some(init_path) = init_mod {
                let src = std::fs::read_to_string(&init_path)
                    .map_err(|e| format!("{}: {e}", init_path.display()))?;
                dom.insert(
                    parent,
                    InstanceBuilder::new("ModuleScript").with_name(name).with_property("Source", src),
                )
            } else {
                dom.insert(parent, InstanceBuilder::new("Folder").with_name(name))
            };
            populate_luau_dir(dom, node, &p, false)?;
            continue;
        }
        if !p.is_file() {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        let is_lua = lower.ends_with(".luau") || lower.ends_with(".lua");
        if !is_lua {
            continue;
        }
        // Directory `init.luau`/`init.lua` for non-root subdirectories was already
        // loaded as the directory's ModuleScript body.
        if !is_plugin_root && (lower == "init.luau" || lower == "init.lua") {
            continue;
        }
        let src = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let is_entry_script = is_plugin_root
            && matches!(
                lower.as_str(),
                "init.server.luau" | "init.server.lua" | "init.luau" | "init.lua" | "main.server.luau" | "main.server.lua"
            );
        let is_server_script = is_entry_script || lower.ends_with(".server.luau") || lower.ends_with(".server.lua");
        let class = if is_server_script { "Script" } else { "ModuleScript" };
        let inst_name = strip_luau_suffix(&name);
        dom.insert(parent, InstanceBuilder::new(class).with_name(inst_name).with_property("Source", src));
    }
    Ok(())
}

fn strip_luau_suffix(name: &str) -> String {
    for suffix in [".server.luau", ".server.lua", ".client.luau", ".client.lua", ".luau", ".lua"] {
        if name.len() > suffix.len() && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix) {
            return name[..name.len() - suffix.len()].to_string();
        }
    }
    name.to_string()
}
