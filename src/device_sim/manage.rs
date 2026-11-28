//! Manage devices (Studio's dialog): custom devices per form, and which
//! Standard devices the dropdown lists. Studio's is a separate window;
//! Rustudio's is an in-window dialog like Studio Settings.

use egui::{vec2, RichText};
use rustudio_core::devices::{self, CustomDevices, Device, FormButton};
use rustudio_engine::theme;

use super::toolbar::{form_icon, FORMS};

#[derive(Clone, Debug, PartialEq)]
pub struct Manage {
    pub tab: FormButton,
    /// The device shown on the right (custom or standard id).
    pub selected: Option<String>,
    /// The custom device being edited (not applied yet).
    pub draft: Option<Device>,
    pub error: Option<String>,
    /// "Add devices": the Standard devices to list, ticked.
    pub adding: Option<Vec<(String, bool)>>,
    /// Memory "Custom": the typed text.
    pub memory_text: Option<String>,
}

impl Manage {
    pub fn new(tab: FormButton) -> Self {
        Self { tab, selected: None, draft: None, error: None, adding: None, memory_text: None }
    }
}

fn tab_label(b: FormButton) -> &'static str {
    match b {
        FormButton::Mobile => "Mobile",
        FormButton::Desktop => "Desktop",
        FormButton::Console => "Console",
        FormButton::Headset => "Headset",
    }
}

/// What the dialog changed.
#[derive(Default)]
pub struct Outcome {
    pub close: bool,
    /// Custom devices or the Standard list changed: save them.
    pub changed: bool,
}

pub fn window(ctx: &egui::Context, m: &mut Manage, custom: &mut CustomDevices) -> Outcome {
    let mut out = Outcome::default();
    let mut open = true;
    egui::Window::new("Manage devices")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .default_size(vec2(620.0, 440.0))
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_min_size(vec2(620.0, 420.0));
            ui.horizontal(|ui| {
                for b in FORMS {
                    let text = format!("     {}", tab_label(b));
                    let r = ui.selectable_label(m.tab == b, text);
                    rustudio_engine::ti::paint(ui.painter(), egui::Rect::from_center_size(r.rect.left_center() + vec2(9.0, 0.0), vec2(12.0, 12.0)), form_icon(b), theme::TEXT);
                    if r.clicked() && m.tab != b {
                        *m = Manage::new(b);
                    }
                }
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(210.0);
                    ui.set_min_height(360.0);
                    left_lists(ui, m, custom, &mut out);
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(380.0);
                    right_form(ui, m, custom, &mut out);
                });
            });
        });
    if let Some(list) = m.adding.as_mut() {
        let mut keep = true;
        let mut update = false;
        egui::Window::new("Add devices").collapsible(false).resizable(false).open(&mut keep).show(ctx, |ui| {
            ui.label("Select which devices appear in the Standard list.");
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for (id, on) in list.iter_mut() {
                    let name = devices::preset(id).map_or(id.clone(), |d| d.name.clone());
                    ui.checkbox(on, name);
                }
            });
            if ui.button("Update").clicked() {
                update = true;
            }
        });
        if update {
            for (id, on) in list.iter() {
                custom.hidden_standard.retain(|h| h != id);
                if !on {
                    custom.hidden_standard.push(id.clone());
                }
            }
            out.changed = true;
            m.adding = None;
        } else if !keep {
            m.adding = None;
        }
    }
    if !open {
        out.close = true;
    }
    out
}

fn left_lists(ui: &mut egui::Ui, m: &mut Manage, custom: &mut CustomDevices, out: &mut Outcome) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Custom").strong());
        if ui.small_button("+").on_hover_text("New device").clicked() {
            let d = devices::new_device(m.tab, &custom.devices);
            m.selected = Some(d.id.clone());
            m.draft = Some(d);
            m.error = None;
            m.memory_text = None;
        }
    });
    let mine: Vec<Device> = custom.devices.iter().filter(|d| d.button() == m.tab).cloned().collect();
    let mut remove = None;
    for d in &mine {
        ui.horizontal(|ui| {
            if ui.add_sized(vec2(160.0, 18.0), egui::Button::selectable(m.selected.as_deref() == Some(&d.id), &d.name)).clicked() {
                m.selected = Some(d.id.clone());
                m.draft = Some(d.clone());
                m.error = None;
                m.memory_text = None;
            }
            if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                remove = Some(d.id.clone());
            }
        });
    }
    if let Some(id) = remove {
        custom.devices.retain(|d| d.id != id);
        if m.selected.as_deref() == Some(&id) {
            m.selected = None;
            m.draft = None;
        }
        out.changed = true;
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Standard").strong());
        if ui.small_button("+").on_hover_text("Add devices").clicked() {
            let list = devices::presets()
                .iter()
                .filter(|d| d.button() == m.tab)
                .map(|d| (d.id.clone(), !custom.hidden_standard.contains(&d.id)))
                .collect();
            m.adding = Some(list);
        }
    });
    egui::ScrollArea::vertical().id_salt("devsim_standard").max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
        ui.set_width(200.0);
        let mut list: Vec<&Device> = devices::presets().iter().filter(|d| d.button() == m.tab && !custom.hidden_standard.contains(&d.id)).collect();
        list.sort_by_key(|d| d.name.to_lowercase());
        for d in list {
            ui.horizontal(|ui| {
                if ui.add_sized(vec2(160.0, 18.0), egui::Button::selectable(m.selected.as_deref() == Some(&d.id), &d.name)).clicked() {
                    m.selected = Some(d.id.clone());
                    m.draft = None;
                    m.error = None;
                }
                if ui.small_button("×").on_hover_text("Remove from the list").clicked() {
                    custom.hidden_standard.push(d.id.clone());
                    out.changed = true;
                }
            });
        }
    });
}

fn right_form(ui: &mut egui::Ui, m: &mut Manage, custom: &mut CustomDevices, out: &mut Outcome) {
    let standard = m.selected.as_deref().and_then(devices::preset).cloned();
    let Some(mut d) = m.draft.clone().or(standard.clone()) else {
        ui.add_space(150.0);
        ui.vertical_centered(|ui| ui.label(RichText::new("Add a custom device or select a standard device.").color(theme::TEXT_DIM)));
        return;
    };
    let editable = standard.is_none();
    ui.add_enabled_ui(editable, |ui| {
        egui::Grid::new("devsim_manage_form").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut d.name).hint_text("Device name").desired_width(220.0));
            ui.end_row();
            ui.label("Platform");
            egui::ComboBox::from_id_salt("devsim_platform").selected_text(d.platform.label()).width(220.0).show_ui(ui, |ui| {
                for p in devices::platforms(m.tab) {
                    ui.selectable_value(&mut d.platform, *p, p.label());
                }
            });
            ui.end_row();
        });
        egui::CollapsingHeader::new("Display").default_open(true).show(ui, |ui| {
            egui::Grid::new("devsim_manage_display").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                ui.label("Dimensions");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut d.size[0]).prefix("W "));
                    ui.add(egui::DragValue::new(&mut d.size[1]).prefix("H "));
                });
                ui.end_row();
                ui.label("Pixel density");
                ui.add(egui::DragValue::new(&mut d.ppi).suffix(" PPI").speed(1.0));
                ui.end_row();
            });
        });
        egui::CollapsingHeader::new("Performance").default_open(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Memory");
                let gb = d.memory_mb as f32 / 1024.0;
                let opts = devices::memory_options(m.tab);
                let is_custom = m.memory_text.is_some() || !opts.iter().any(|o| (o - gb).abs() < 1e-3);
                let label = if is_custom { "Custom".to_string() } else { format!("{gb} GB") };
                egui::ComboBox::from_id_salt("devsim_memory").selected_text(label).width(110.0).show_ui(ui, |ui| {
                    for o in opts {
                        if ui.selectable_label(!is_custom && (o - gb).abs() < 1e-3, format!("{o} GB")).clicked() {
                            d.memory_mb = (o * 1024.0) as u32;
                            m.memory_text = None;
                        }
                    }
                    if ui.selectable_label(is_custom, "Custom").clicked() {
                        m.memory_text = Some(format!("{gb}"));
                    }
                });
                if is_custom {
                    let text = m.memory_text.get_or_insert_with(|| format!("{gb}"));
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(50.0));
                    ui.label("GB");
                    if r.lost_focus() {
                        // (Studio raises a too-small value to 0.5 GB, with no message)
                        let v = devices::clamp_memory_gb(text.trim().parse().unwrap_or(0.0));
                        *text = format!("{v}");
                        d.memory_mb = (v * 1024.0).round() as u32;
                    }
                }
            });
        });
        egui::CollapsingHeader::new("Keyboard settings").default_open(false).show(ui, |ui| {
            ui.label("Keyboard height");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut d.keyboard_height[0]).prefix("Portrait "));
                ui.add(egui::DragValue::new(&mut d.keyboard_height[1]).prefix("Landscape "));
            });
        });
    });
    if editable {
        m.draft = Some(d.clone());
    }
    if let Some(e) = &m.error {
        ui.colored_label(theme::ERROR, e);
    }
    ui.add_space(10.0);
    // Studio's order, at the right: Apply, Cancel, Duplicate.
    let (mut apply, mut cancel, mut duplicate) = (false, false, false);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        duplicate = ui.button("Duplicate").clicked();
        cancel = ui.button("Cancel").clicked();
        apply = ui.add_enabled(editable, egui::Button::new("Apply").fill(theme::ACCENT)).clicked();
    });
    if duplicate {
        let mut copy = d.clone();
        copy.name = devices::copy_name(&d.name, &custom.devices);
        copy.id = devices::new_device(m.tab, &custom.devices).id;
        copy.custom = true;
        copy.frame = None;
        copy.tier = None;
        m.selected = Some(copy.id.clone());
        m.draft = Some(copy);
        m.error = None;
    }
    if cancel {
        out.close = true;
    }
    if apply {
        let mut nd = d.clone();
        devices::settle(&mut nd);
        match devices::validate(&nd, &custom.devices) {
            Ok(()) => {
                match custom.devices.iter_mut().find(|o| o.id == nd.id) {
                    Some(o) => *o = nd.clone(),
                    None => custom.devices.push(nd.clone()),
                }
                m.draft = Some(nd);
                m.error = None;
                out.changed = true;
            }
            Err(e) => m.error = Some(e),
        }
    }
}
