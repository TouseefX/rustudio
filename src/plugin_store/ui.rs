//! Plugins > Manage Plugins, laid out like Studio's (PluginManagement):
//! a header with Find Plugins and Update All, then one card per plugin
//! (installed first as Studio lists them, then local): icon, name, creator,
//! version / updated date, the Enabled toggle, an Update button when a
//! newer version exists, a "…" menu (Details, Remove) and the plugin's
//! permission rows (Script Injection, one row per HTTP domain). Toggling
//! takes effect at once. Find Plugins opens a Creator Store search in the
//! same window with Install buttons; permission prompts from running
//! plugins show as Studio's Allow / Deny dialogs. Also Publish as Plugin's
//! confirm dialog.

use std::path::{Path, PathBuf};

use egui::{Align, Color32, Layout, RichText, Sense, Vec2};

use super::manager::PluginManager;
use super::permissions::{self, Ask};
use super::{PluginKind, PluginSource};
use rustudio_core::assets::SharedClient;
use rustudio_engine::theme;

/// Kind of a Built-in Suite entry shown in Manage Plugins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuiteItemKind {
    NativeRust,
    BundledLuau,
}

/// One Built-in Suite plugin entry rendered in Manage Plugins.
#[derive(Clone, Debug)]
pub struct SuiteItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub kind: SuiteItemKind,
    pub enabled: bool,
    /// True when Studio Settings routes this tool to Roblox's `.rbxm` plugin.
    pub overridden_by_roblox: bool,
    pub path: Option<PathBuf>,
    pub errors: Vec<String>,
}

/// User actions triggered in the Built-in Suite section of Manage Plugins.
#[derive(Clone, Debug)]
pub enum SuiteAction {
    ToggleNative(String, bool),
    ToggleBundled(PathBuf, bool),
    ReloadBundled(PathBuf),
    OpenSuiteFolder(PathBuf),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ManageTab {
    #[default]
    Plugins,
    BuiltIn,
}

#[derive(Default)]
pub struct ManageUi {
    pub open: bool,
    pub tab: ManageTab,
    pub suite_filter: String,
    pub find: bool,
    pub query: String,
    pub install_id: String,
    pub details: Option<String>,
    /// The permissions page of this plugin id (Studio's edit icon).
    pub perms_page: Option<String>,
    pub confirm_remove: Option<u64>,
    pub import_open: bool,
    pub import_report: Option<String>,
    pub publish: Option<PublishDialog>,
    icons: std::collections::HashMap<u64, egui::TextureHandle>,
}

/// Publish as Plugin's dialog (name, description, creator, new or overwrite).
pub struct PublishDialog {
    pub name: String,
    pub description: String,
    pub creator: String,
    /// "" = a new plugin; else the asset id to overwrite.
    pub overwrite: String,
    pub rbxm: Vec<u8>,
    pub error: Option<String>,
}

const BLUE: Color32 = Color32::from_rgb(51, 95, 255);
/// Studio's switch is green when on.
const GREEN: Color32 = Color32::from_rgb(0, 176, 111);
const LINK: Color32 = Color32::from_rgb(102, 145, 255);

/// Studio's toggle switch.
fn switch(ui: &mut egui::Ui, on: bool, enabled: bool) -> egui::Response {
    switch_sized(ui, on, enabled, Vec2::new(40.0, 22.0))
}

fn switch_compact(ui: &mut egui::Ui, on: bool, enabled: bool) -> egui::Response {
    switch_sized(ui, on, enabled, Vec2::new(32.0, 17.0))
}

fn switch_sized(ui: &mut egui::Ui, on: bool, enabled: bool, size: Vec2) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let rad = size.y * 0.5;
    let bg = if on { GREEN } else { Color32::from_rgb(110, 110, 110) };
    p.rect_filled(r, rad, if enabled { bg } else { bg.gamma_multiply(0.5) });
    let x = if on { r.right() - rad } else { r.left() + rad };
    p.circle_filled(egui::pos2(x, r.center().y), rad - 2.5, Color32::WHITE);
    resp
}

fn date(iso: &str) -> String {
    // 2026-05-14T17:59:42.128Z → 5/14/2026 (Studio's short date)
    let d = iso.split('T').next().unwrap_or("");
    let p: Vec<&str> = d.split('-').collect();
    match p.as_slice() {
        [y, m, d] => format!("{}/{}/{}", m.trim_start_matches('0'), d.trim_start_matches('0'), y),
        _ => iso.to_string(),
    }
}

impl ManageUi {
    fn icon(&mut self, ctx: &egui::Context, pngs: &std::collections::HashMap<u64, Vec<u8>>, id: u64) -> Option<egui::TextureHandle> {
        if let Some(t) = self.icons.get(&id) {
            return Some(t.clone());
        }
        let png = pngs.get(&id)?;
        let img = image::load_from_memory(png).ok()?.to_rgba8();
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], &img);
        let t = ctx.load_texture(format!("plugin-icon-{id}"), ci, egui::TextureOptions::LINEAR);
        self.icons.insert(id, t.clone());
        Some(t)
    }
}

/// Draws Manage Plugins (when open), the permission prompts and the
/// publish dialog.
pub fn show(ctx: &egui::Context, m: &mut PluginManager, client: Option<SharedClient>) {
    let _ = show_with_suite(ctx, m, client, None, &[]);
}

/// Draws Manage Plugins including the modular Built-in Suite section (native
/// Rust plugins + bundled Luau/Vide plugins) and returns any suite actions.
pub fn show_with_suite(
    ctx: &egui::Context,
    m: &mut PluginManager,
    client: Option<SharedClient>,
    suite_dir: Option<&Path>,
    suite: &[SuiteItem],
) -> Vec<SuiteAction> {
    let mut actions = Vec::new();
    prompts(ctx);
    publish_dialog(ctx, m, client.clone());
    if !m.ui.open {
        return actions;
    }
    let mut open = true;
    let list = m.store.list();
    let ids: Vec<u64> = list.iter().filter_map(|p| p.asset_id).collect();
    m.want_icons(client.clone(), &ids);
    // Studio's window is titled "Plugin Management" (602x554, floating).
    egui::Window::new("Plugin Management")
        .id(egui::Id::new("manage_plugins"))
        .open(&mut open)
        .default_size([602.0, 554.0])
        .min_width(480.0)
        .collapsible(false)
        .show(ctx, |ui| {
            if m.ui.find {
                find_page(ui, m, client.clone());
                return;
            }
            if let Some(id) = m.ui.perms_page.clone() {
                perms_page(ui, m, &id);
                return;
            }
            let has_suite = !suite.is_empty() || suite_dir.is_some();
            if !has_suite {
                m.ui.tab = ManageTab::Plugins;
            }
            // Top bar: tab strip on the left, context actions on the right.
            ui.horizontal(|ui| {
                tab_button(
                    ui,
                    &mut m.ui.tab,
                    ManageTab::Plugins,
                    if list.is_empty() {
                        "Plugins".to_string()
                    } else {
                        format!("Plugins ({})", list.len())
                    },
                );
                if has_suite {
                    ui.add_space(4.0);
                    tab_button(
                        ui,
                        &mut m.ui.tab,
                        ManageTab::BuiltIn,
                        format!("Built-in ({})", suite.len()),
                    );
                }
                if m.ui.tab == ManageTab::Plugins {
                    match &m.synced {
                        None if m.signed_in => {
                            ui.spinner();
                        }
                        Some(Err(e)) => {
                            ui.label(RichText::new("⚠").color(theme::WARN)).on_hover_text(e);
                        }
                        _ => {}
                    }
                    header_actions(ui, m, client.clone(), &list);
                } else {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(dir) = suite_dir {
                            if ui
                                .small_button("Suite Folder…")
                                .on_hover_text(format!("Open {}", dir.display()))
                                .clicked()
                            {
                                actions.push(SuiteAction::OpenSuiteFolder(dir.to_path_buf()));
                            }
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut m.ui.suite_filter)
                                .hint_text("Filter built-ins…")
                                .desired_width(150.0),
                        );
                    });
                }
            });
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match m.ui.tab {
                    ManageTab::BuiltIn => {
                        suite_section(ui, suite_dir, suite, &m.ui.suite_filter, &mut actions);
                    }
                    ManageTab::Plugins => {
                        if !m.signed_in {
                            ui.label(
                                RichText::new("Signed out: installed plugins and updates need you to sign in (File > Account).")
                                    .small()
                                    .color(theme::WARN),
                            );
                        }
                        if !m.loading_on() {
                            ui.label(
                                RichText::new("Plugin loading is turned off for this run.")
                                    .small()
                                    .color(theme::WARN),
                            );
                        }
                        let installed: Vec<&PluginSource> =
                            list.iter().filter(|p| p.kind == PluginKind::Installed).collect();
                        let pending: Vec<&PluginSource> =
                            installed.iter().copied().filter(|p| p.has_update()).collect();
                        let current: Vec<&PluginSource> =
                            installed.iter().copied().filter(|p| !p.has_update()).collect();
                        let local: Vec<&PluginSource> =
                            list.iter().filter(|p| p.kind == PluginKind::Local).collect();
                        if installed.is_empty() && local.is_empty() {
                            ui.add_space(24.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("You don't have any plugins installed.").color(theme::TEXT_DIM),
                                );
                            });
                        }
                        for (title, group) in [
                            ("Pending updates", pending),
                            ("Up to date", current),
                            ("Local plugins", local),
                        ] {
                            if group.is_empty() {
                                continue;
                            }
                            ui.add_space(6.0);
                            ui.label(RichText::new(title).size(15.0));
                            if title == "Local plugins" {
                                ui.label(
                                    RichText::new(m.store.plugins_dir.display().to_string())
                                        .small()
                                        .color(theme::TEXT_DIM),
                                );
                            }
                            ui.add_space(4.0);
                            for p in group {
                                card(ui, m, client.clone(), p);
                            }
                        }
                    }
                }
            });
        });
    m.ui.open = open;
    details_window(ctx, m);
    remove_confirm(ctx, m);
    import_window(ctx, m);
    actions
}

fn tab_button(ui: &mut egui::Ui, active: &mut ManageTab, target: ManageTab, label: String) {
    let selected = *active == target;
    let fill = if selected { Color32::from_rgb(56, 60, 72) } else { Color32::TRANSPARENT };
    let text_color = if selected { Color32::WHITE } else { theme::TEXT_DIM };
    let btn = egui::Button::new(RichText::new(label).size(14.0).strong().color(text_color))
        .fill(fill)
        .corner_radius(4.0)
        .min_size(Vec2::new(0.0, 26.0));
    if ui.add(btn).clicked() {
        *active = target;
    }
}

fn suite_section(
    ui: &mut egui::Ui,
    suite_dir: Option<&Path>,
    suite: &[SuiteItem],
    filter: &str,
    actions: &mut Vec<SuiteAction>,
) {
    let q = filter.trim().to_ascii_lowercase();
    let matches = |item: &SuiteItem| {
        q.is_empty()
            || item.name.to_ascii_lowercase().contains(&q)
            || item.description.to_ascii_lowercase().contains(&q)
    };
    let luau: Vec<&SuiteItem> = suite
        .iter()
        .filter(|i| i.kind == SuiteItemKind::BundledLuau && matches(i))
        .collect();
    let rust: Vec<&SuiteItem> = suite
        .iter()
        .filter(|i| i.kind == SuiteItemKind::NativeRust && matches(i))
        .collect();

    if let Some(dir) = suite_dir {
        ui.label(
            RichText::new(format!("Suite folder: {}", dir.display()))
                .small()
                .color(theme::TEXT_DIM),
        );
        ui.add_space(4.0);
    }

    if luau.is_empty() && rust.is_empty() {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("No matching built-in plugins.").color(theme::TEXT_DIM));
        });
        return;
    }

    for (heading, items) in [
        (format!("Luau / Vide ({})", luau.len()), luau),
        (format!("Native Rust ({})", rust.len()), rust),
    ] {
        if items.is_empty() {
            continue;
        }
        ui.add_space(4.0);
        ui.label(RichText::new(heading).small().strong().color(theme::TEXT_DIM));
        ui.add_space(2.0);
        for (idx, item) in items.iter().enumerate() {
            suite_row(ui, item, idx % 2 == 1, actions);
        }
        ui.add_space(6.0);
    }
}

fn suite_row(ui: &mut egui::Ui, item: &SuiteItem, alt: bool, actions: &mut Vec<SuiteAction>) {
    let frame = egui::Frame::NONE
        .fill(if alt { Color32::from_rgba_unmultiplied(255, 255, 255, 6) } else { Color32::TRANSPARENT })
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(8, 4));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let right_w = if item.kind == SuiteItemKind::BundledLuau { 104.0 } else { 44.0 };
            let text_w = (ui.available_width() - right_w).max(160.0);
            ui.allocate_ui_with_layout(Vec2::new(text_w, 18.0), Layout::left_to_right(Align::Center), |ui| {
                ui.set_max_width(text_w);
                let name_color = if item.enabled && !item.overridden_by_roblox {
                    theme::TEXT
                } else {
                    theme::TEXT_DIM
                };
                let name_resp = ui.label(RichText::new(&item.name).size(13.0).strong().color(name_color));
                if !item.description.is_empty() {
                    name_resp.on_hover_text(&item.description);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&item.description).small().color(theme::TEXT_DIM),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&item.description);
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let interactive = !item.overridden_by_roblox;
                let resp = switch_compact(ui, item.enabled && interactive, interactive);
                if interactive && resp.clicked() {
                    let next = !item.enabled;
                    match item.kind {
                        SuiteItemKind::NativeRust => actions.push(SuiteAction::ToggleNative(item.id.clone(), next)),
                        SuiteItemKind::BundledLuau => {
                            if let Some(p) = &item.path {
                                actions.push(SuiteAction::ToggleBundled(p.clone(), next));
                            }
                        }
                    }
                }
                if item.kind == SuiteItemKind::BundledLuau && item.enabled {
                    if let Some(p) = &item.path {
                        ui.add_space(4.0);
                        if ui.add(egui::Button::new(RichText::new("Reload").small()).min_size(Vec2::new(0.0, 18.0))).clicked() {
                            actions.push(SuiteAction::ReloadBundled(p.clone()));
                        }
                    }
                }
            });
        });
        if item.overridden_by_roblox {
            ui.label(
                RichText::new("Using Roblox's built-in plugin (change in Studio Settings > rustudio)")
                    .small()
                    .color(theme::WARN),
            );
        }
        for e in &item.errors {
            ui.label(RichText::new(e).small().color(theme::ERROR));
        }
    });
}

fn header_actions(ui: &mut egui::Ui, m: &mut PluginManager, client: Option<SharedClient>, list: &[PluginSource]) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        // "+": find plugins (Studio opens the Toolbox's Plugins; rustudio
        // searches the Creator Store here).
        let plus = ui.add(
            egui::Button::new(RichText::new("+").strong().color(Color32::WHITE).size(16.0))
                .fill(BLUE)
                .min_size(Vec2::new(26.0, 26.0)),
        );
        if plus.on_hover_text("Find Plugins").clicked() {
            m.ui.find = true;
        }
        let n = list.iter().filter(|p| p.has_update()).count();
        if ui
            .add_enabled(
                n > 0 && client.is_some(),
                egui::Button::new("Update all").min_size(Vec2::new(88.0, 26.0)),
            )
            .clicked()
        {
            m.update_all(client.clone());
        }
        if ui.small_button("Import from Studio…").clicked() {
            m.ui.import_open = true;
        }
    });
}

fn card(ui: &mut egui::Ui, m: &mut PluginManager, client: Option<SharedClient>, p: &PluginSource) {
    let ctx = ui.ctx().clone();
    let desc = p
        .asset_id
        .and_then(|a| m.store.meta.plugin_metadata.iter().find(|x| x.plugin_asset_id == a))
        .map(|x| x.description.clone())
        .unwrap_or_default();
    ui.horizontal_top(|ui| {
        // Icon (58 px in Studio).
        let (r, _) = ui.allocate_exact_size(Vec2::splat(58.0), Sense::hover());
        match p.asset_id.and_then(|a| m.ui.icon(&ctx, &m.icons, a)) {
            Some(t) => {
                ui.painter().image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                ui.painter().rect_filled(r, 2.0, Color32::from_rgb(60, 62, 70));
                rustudio_engine::ti::paint(ui.painter(), r.shrink(14.0), if p.kind == PluginKind::Local { "file" } else { "plug" }, theme::TEXT_DIM);
            }
        }
        ui.add_space(14.0);
        let right_w = 230.0;
        let text_w = (ui.available_width() - right_w).max(140.0);
        ui.vertical(|ui| {
            ui.set_width(text_w);
            ui.label(RichText::new(&p.name).size(15.0));
            match (&p.creator, p.kind) {
                (Some(c), _) => {
                    ui.label(RichText::new(c).small().color(LINK));
                }
                (None, PluginKind::Local) => {
                    ui.label(RichText::new(p.id.trim_start_matches("user_")).small().color(theme::TEXT_DIM));
                }
                _ => {}
            }
            if !desc.is_empty() {
                let one: String = desc.split_whitespace().collect::<Vec<_>>().join(" ");
                ui.add(egui::Label::new(RichText::new(one).small()).truncate());
            }
            perm_summary(ui, m, p);
            if p.kind == PluginKind::Installed {
                ui.horizontal(|ui| {
                    let mut auto = p.auto_update;
                    if ui.checkbox(&mut auto, RichText::new("Auto Update").small()).changed() {
                        if let Some(a) = p.asset_id {
                            let _ = m.store.set_auto_update(a, auto);
                        }
                    }
                    if let Some(u) = &p.updated {
                        ui.label(RichText::new(format!("|  Last updated on {}", date(u))).small());
                    }
                });
            }
            if p.moderated {
                ui.label(RichText::new("This plugin was moderated.").small().color(theme::ERROR));
            }
            if !p.present && p.kind == PluginKind::Installed {
                let t = if m.busy.contains(&p.id) { "Downloading…" } else { "Not downloaded yet (turn it on to download)" };
                ui.label(RichText::new(t).small().color(theme::TEXT_DIM));
            }
            if let Some(e) = m.errors.get(&p.id) {
                ui.label(RichText::new(e).small().color(theme::ERROR));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.add_space(4.0);
            let dots = ui.add(egui::Button::new("···").min_size(Vec2::new(28.0, 28.0)));
            egui::Popup::menu(&dots).show(|ui| {
                if ui.button("Details").clicked() {
                    match p.asset_id {
                        Some(a) => ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://create.roblox.com/store/asset/{a}"))),
                        None => m.ui.details = Some(p.id.clone()),
                    }
                    ui.close();
                }
                match (p.kind, p.asset_id) {
                    (PluginKind::Installed, Some(a)) => {
                        if ui.button("Uninstall").clicked() {
                            m.ui.confirm_remove = Some(a);
                            ui.close();
                        }
                    }
                    _ => {
                        if ui.button("Show in Plugins Folder").clicked() {
                            open_plugins_folder(m);
                            ui.close();
                        }
                    }
                }
            });
            ui.add_space(10.0);
            let resp = switch(ui, p.enabled, !p.moderated);
            if resp.clicked() {
                m.set_enabled(client.clone(), &p.id, !p.enabled);
            }
            if p.has_update() {
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    let busy = m.busy.contains(&p.id);
                    let b = egui::Button::new(if busy { "Updating…" } else { "Update" }).min_size(Vec2::new(96.0, 28.0));
                    if ui.add_enabled(!busy && client.is_some(), b).clicked() {
                        m.update(client.clone(), p.asset_id.unwrap());
                    }
                    if let Some(u) = &p.updated {
                        ui.label(RichText::new(format!("Updated: {}", date(u))).size(10.0));
                    }
                });
            }
        });
    });
    ui.add_space(6.0);
    ui.separator();
}

/// Studio's permission line: "No Permissions Requested", or
/// "HTTP Requests ✓N | Script Injection Allowed | ✎".
fn perm_summary(ui: &mut egui::Ui, m: &mut PluginManager, p: &PluginSource) {
    let perms = permissions::global().lock().ok().and_then(|g| g.plugins.get(&p.id).cloned()).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if perms.script_injection.is_none() && perms.http.is_empty() {
            ui.label(RichText::new("No Permissions Requested").small());
            return;
        }
        if !perms.http.is_empty() {
            let ok = perms.http.values().filter(|v| **v).count();
            ui.label(RichText::new("HTTP Requests").small());
            ui.label(rustudio_engine::ti::text("check", 12.0));
            ui.label(RichText::new(ok.to_string()).small());
            ui.label(RichText::new("|").small().color(theme::TEXT_DIM));
        }
        if let Some(si) = perms.script_injection {
            ui.label(RichText::new(if si { "Script Injection Allowed" } else { "Script Injection Denied" }).small());
            ui.label(RichText::new("|").small().color(theme::TEXT_DIM));
        }
        if ui.small_button(rustudio_engine::ti::text("pencil", 12.0)).on_hover_text("Edit permissions").clicked() {
            m.ui.perms_page = Some(p.id.clone());
        }
    });
}

/// Studio's permissions page: back arrow, the plugin, its HTTP domains
/// (checkboxes) and the Script Injection switch.
fn perms_page(ui: &mut egui::Ui, m: &mut PluginManager, id: &str) {
    let name = m.store.get(id).map(|p| p.name).unwrap_or_else(|| id.to_string());
    ui.horizontal(|ui| {
        if ui.button("←").clicked() {
            m.ui.perms_page = None;
        }
        ui.separator();
        ui.label(RichText::new(name).strong());
    });
    ui.separator();
    let Ok(mut g) = permissions::global().lock() else { return };
    let perms = g.plugins.get(id).cloned().unwrap_or_default();
    egui::Grid::new("perm-page").num_columns(2).spacing([40.0, 6.0]).show(ui, |ui| {
        ui.label("HTTP Requests");
        ui.vertical(|ui| {
            if perms.http.is_empty() {
                ui.label(RichText::new("None requested").small().color(theme::TEXT_DIM));
            }
            for (d, allowed) in &perms.http {
                let mut on = *allowed;
                if ui.checkbox(&mut on, RichText::new(d).small()).changed() {
                    g.set_http(id, d, on);
                }
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new("By allowing HTTP requests, 3rd party developers are able to gather information on your machine. These might be harmless (such as crash logs) or malicious (such as gathering information on your game's scripts). Disabling HTTP requests might result in a non-working plugin.")
                    .small()
                    .color(theme::TEXT_DIM),
            );
        });
        ui.end_row();
        ui.label("Script Injection");
        let on = perms.script_injection.unwrap_or(false);
        if switch(ui, on, true).clicked() {
            g.set_script_injection(id, !on);
        }
        ui.end_row();
    });
}

fn find_page(ui: &mut egui::Ui, m: &mut PluginManager, client: Option<SharedClient>) {
    ui.horizontal(|ui| {
        if ui.button("‹ Back").clicked() {
            m.ui.find = false;
        }
        ui.label(RichText::new("Find Plugins").strong());
    });
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut m.ui.query).hint_text("Search the Creator Store").desired_width(260.0));
        let go = ui.button("Search").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        if go && !m.ui.query.trim().is_empty() {
            let q = m.ui.query.trim().to_string();
            m.search(client.clone(), q);
        }
        if m.searching {
            ui.spinner();
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Install by asset id").small());
        ui.add(egui::TextEdit::singleline(&mut m.ui.install_id).desired_width(140.0));
        let id = m.ui.install_id.trim().parse::<u64>().ok();
        if ui.add_enabled(id.is_some() && client.is_some(), egui::Button::new("Install")).clicked() {
            m.install(client.clone(), id.unwrap());
        }
    });
    ui.separator();
    let installed: std::collections::HashSet<u64> = m.store.list().iter().filter_map(|p| p.asset_id).collect();
    let hits = match &m.search {
        Some((_, Ok(page))) => page.hits.clone(),
        Some((_, Err(e))) => {
            ui.label(RichText::new(e).color(theme::ERROR));
            Vec::new()
        }
        None => Vec::new(),
    };
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for h in hits {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&h.name).strong());
                    let votes = h.up_percent.map(|p| format!("  ·  {p}% liked")).unwrap_or_default();
                    ui.label(RichText::new(format!("By {}{}{}", h.creator_name, if h.verified { " ✔" } else { "" }, votes)).small().color(theme::TEXT_DIM));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let id = super::cloud_id(h.id);
                    if installed.contains(&h.id) {
                        ui.label(RichText::new("Installed").small().color(theme::TEXT_DIM));
                    } else if m.busy.contains(&id) {
                        ui.spinner();
                    } else if h.is_free {
                        if ui.button("Install").clicked() {
                            m.install(client.clone(), h.id);
                        }
                    } else if ui.button("Buy…").on_hover_text("Paid plugins are bought on the Creator Store website").clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://create.roblox.com/store/asset/{}", h.id)));
                    }
                    if let Some(e) = m.errors.get(&id) {
                        ui.label(RichText::new(e).small().color(theme::ERROR));
                    }
                });
            });
            ui.separator();
        }
    });
}

fn details_window(ctx: &egui::Context, m: &mut PluginManager) {
    let Some(id) = m.ui.details.clone() else { return };
    let Some(p) = m.store.get(&id) else {
        m.ui.details = None;
        return;
    };
    let desc = m.store.meta.plugin_metadata.iter().find(|x| Some(x.plugin_asset_id) == p.asset_id).map(|x| x.description.clone()).unwrap_or_default();
    let mut open = true;
    egui::Window::new(format!("{} - Details", p.name)).open(&mut open).default_width(380.0).collapsible(false).show(ctx, |ui| {
        egui::Grid::new("plugin-details").num_columns(2).show(ui, |ui| {
            let mut row = |k: &str, v: String| {
                ui.label(RichText::new(k).color(theme::TEXT_DIM));
                ui.label(v);
                ui.end_row();
            };
            row("Id", p.id.clone());
            row("Kind", p.kind_label().into());
            if let Some(c) = &p.creator {
                row("Creator", c.clone());
            }
            if let Some(v) = p.version {
                row("Version", v.to_string());
            }
            if let Some(l) = p.latest_version {
                row("Newest version", l.to_string());
            }
            if let Some(u) = &p.updated {
                row("Updated", date(u));
            }
            row("File", p.path.display().to_string());
            row("Settings", p.settings_path.display().to_string());
        });
        if !desc.is_empty() {
            ui.separator();
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| ui.label(desc));
        }
    });
    if !open {
        m.ui.details = None;
    }
}

fn remove_confirm(ctx: &egui::Context, m: &mut PluginManager) {
    let Some(a) = m.ui.confirm_remove else { return };
    let name = m.store.get(&super::cloud_id(a)).map(|p| p.name).unwrap_or_else(|| a.to_string());
    egui::Window::new("Uninstall Plugin").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.label(format!("Uninstall {name}?"));
        ui.label(RichText::new("Your Roblox account keeps it; install it again from Find Plugins.").small().color(theme::TEXT_DIM));
        ui.horizontal(|ui| {
            if ui.button("Uninstall").clicked() {
                m.remove(a);
                m.ui.confirm_remove = None;
            }
            if ui.button("Cancel").clicked() {
                m.ui.confirm_remove = None;
            }
        });
    });
}

fn import_window(ctx: &egui::Context, m: &mut PluginManager) {
    if !m.ui.import_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Import Plugins from Studio").open(&mut open).collapsible(false).default_width(460.0).show(ctx, |ui| {
        ui.label("Copies the local and installed plugins (and their settings) of a Roblox Studio install into rustudio. Studio's files are only read.");
        ui.separator();
        let found = super::import::detect();
        if found.is_empty() {
            ui.label(RichText::new("No Studio install found.").color(theme::TEXT_DIM));
        }
        for s in &found {
            let plan = super::import::plan(s, m.store.user_id);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&s.label).strong());
                ui.label(RichText::new(format!("{} item(s)", plan.len())).small().color(theme::TEXT_DIM));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_enabled(!plan.is_empty(), egui::Button::new("Import")).clicked() {
                        let rep = super::import::run(&mut m.store, &plan);
                        m.ui.import_report = Some(format!(
                            "{}: copied {}, already there {}, failed {}{}",
                            s.label,
                            rep.copied.len(),
                            rep.skipped.len(),
                            rep.failed.len(),
                            rep.failed.first().map(|f| format!(" ({f})")).unwrap_or_default()
                        ));
                        m.queue_startup();
                    }
                });
            });
        }
        if let Some(r) = &m.ui.import_report {
            ui.separator();
            ui.label(r);
        }
    });
    m.ui.import_open = open;
}

/// Studio's permission prompts for running plugins.
fn prompts(ctx: &egui::Context) {
    let first = permissions::global().lock().ok().and_then(|g| g.prompts.first().cloned());
    let Some(p) = first else { return };
    let text = match &p.ask {
        Ask::Http(d) => format!("\"{}\" is requesting permission to make HTTP requests to {d}.", p.plugin_name),
        Ask::ScriptInjection => format!("\"{}\" is requesting permission to inject scripts (create or change scripts).", p.plugin_name),
    };
    egui::Window::new("Plugin Permission").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.label(text);
        ui.label(RichText::new("You can change this later in Plugins > Manage Plugins.").small().color(theme::TEXT_DIM));
        ui.horizontal(|ui| {
            if ui.button("Allow").clicked() {
                if let Ok(mut g) = permissions::global().lock() {
                    g.answer(&p, true);
                }
            }
            if ui.button("Deny").clicked() {
                if let Ok(mut g) = permissions::global().lock() {
                    g.answer(&p, false);
                }
            }
        });
    });
}

fn publish_dialog(ctx: &egui::Context, m: &mut PluginManager, client: Option<SharedClient>) {
    let Some(d) = m.ui.publish.as_mut() else { return };
    let mut close = false;
    let mut send = None;
    egui::Window::new("Publish as Plugin").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        egui::Grid::new("publish-plugin").num_columns(2).show(ui, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut d.name);
            ui.end_row();
            ui.label("Description");
            ui.text_edit_multiline(&mut d.description);
            ui.end_row();
            ui.label("Creator");
            ui.label(&d.creator);
            ui.end_row();
            ui.label("Overwrite");
            ui.add(egui::TextEdit::singleline(&mut d.overwrite).hint_text("asset id (empty: new plugin)"));
            ui.end_row();
        });
        ui.label(RichText::new("The plugin is published private, not for sale.").small().color(theme::TEXT_DIM));
        if let Some(e) = &d.error {
            ui.label(RichText::new(e).color(theme::ERROR));
        }
        ui.horizontal(|ui| {
            let busy = m.publishing;
            if ui.add_enabled(!busy && !d.name.trim().is_empty(), egui::Button::new("Publish")).clicked() {
                let ow = d.overwrite.trim();
                match (ow.is_empty(), ow.parse::<u64>()) {
                    (true, _) => send = Some(None),
                    (false, Ok(id)) => send = Some(Some(id)),
                    (false, Err(_)) => d.error = Some("Overwrite: an asset id (digits)".into()),
                }
            }
            if busy {
                ui.spinner();
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });
    if let Some(overwrite) = send {
        let d = m.ui.publish.take().unwrap();
        let req = rustudio_account::plugins::PublishRequest { overwrite, name: d.name.trim().to_string(), description: d.description, group_id: None };
        m.publish(client, req, d.rbxm);
    } else if close {
        m.ui.publish = None;
    }
}

/// Plugins Folder: opens the local Plugins folder in the file manager
/// (made if missing).
pub fn open_plugins_folder(m: &mut PluginManager) {
    match m.store.ensure_plugins_dir() {
        Ok(d) => {
            // (no file manager to hand it to on Android: the path is reported)
            #[cfg(not(target_os = "android"))]
            {
                let opener = if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
                if std::env::var_os("RUSTUDIO_NO_OPEN").is_none() {
                    let _ = std::process::Command::new(opener).arg(&d).spawn();
                }
            }
            m.lines.push(super::manager::Line { level: super::watcher::Level::Info, text: format!("Plugins folder: {}", d.display()) });
        }
        Err(e) => m.lines.push(super::manager::Line { level: super::watcher::Level::Error, text: format!("Plugins folder: {e}") }),
    }
}
