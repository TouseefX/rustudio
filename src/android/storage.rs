//! Files on Android: the app's own folder in shared storage
//! (`Android/data/<package>/files`, visible to file managers, no permission
//! needed), a file picker drawn in egui over it ([`Picker`], behind
//! `crate::file_dialog::Dialogs`), and places handed over by other apps
//! through "Open with" (a `content:` URI in the launch Intent, copied in
//! through ContentResolver: [`launch_file`]).
//!
//! The system document picker (Storage Access Framework) needs
//! `startActivityForResult` and an `onActivityResult`, which `NativeActivity`
//! doesn't pass on to native code; this in-app picker covers the same files
//! without a Java class in the APK. Shared storage outside the app's folder
//! (Download, Documents, …) is listed once the app has storage access:
//! "All files access" on API 30+ (MANAGE_EXTERNAL_STORAGE, granted on a
//! settings page the picker opens), the storage permission below that
//! ([`super::jni::request_storage_access`]).

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::file_dialog::{Kind, Request};

/// A folder the picker can start from.
struct Root {
    label: &'static str,
    path: PathBuf,
}

/// The primary shared storage (`/storage/emulated/0`): the app's external
/// folder is `<shared>/Android/data/<package>/files`.
fn shared_storage() -> PathBuf {
    let mut p = super::data_dir();
    for _ in 0..4 {
        if let Some(parent) = p.parent() {
            p = parent.to_path_buf();
        }
    }
    if p.as_os_str().is_empty() || p == Path::new("/") {
        PathBuf::from("/storage/emulated/0")
    } else {
        p
    }
}

fn roots(storage_access: bool) -> Vec<Root> {
    let mut v = vec![Root { label: "rustudio", path: super::data_dir() }];
    if super::data_dir() != super::files_dir() {
        v.push(Root { label: "App data", path: super::files_dir() });
    }
    // Shared folders: the usual ones when they can be listed (files this
    // app made are always readable; everything with storage access), and
    // the whole shared storage once access was granted.
    let shared = shared_storage();
    for (label, sub) in [("Download", "Download"), ("Documents", "Documents")] {
        let p = shared.join(sub);
        if std::fs::read_dir(&p).is_ok() {
            v.push(Root { label, path: p });
        }
    }
    if storage_access && std::fs::read_dir(&shared).is_ok() {
        v.push(Root { label: "Shared storage", path: shared });
    }
    v
}

/// One folder's listing, sorted folders first.
struct Listing {
    dir: PathBuf,
    folders: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

impl Listing {
    fn read(dir: &Path, request: &Request) -> Self {
        let mut folders = Vec::new();
        let mut files = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                let hidden = p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'));
                if hidden {
                    continue;
                }
                if p.is_dir() {
                    folders.push(p);
                } else if request.accepts(&p) {
                    files.push(p);
                }
            }
        }
        let key = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().to_lowercase());
        folders.sort_by_key(key);
        files.sort_by_key(key);
        Listing { dir: dir.to_path_buf(), folders, files }
    }
}

/// The in-app file dialog.
pub struct Picker {
    request: Request,
    kind: Kind,
    listing: Listing,
    selected: Option<PathBuf>,
    /// Save: the name to write; new folder: its name.
    name: String,
    new_folder: Option<String>,
    error: Option<String>,
    /// Shared storage may be read (asked again now and then while the
    /// dialog is open: the grant happens on a system page).
    storage_access: bool,
    access_checked: Instant,
}

impl Picker {
    pub fn new(request: Request, kind: Kind) -> Self {
        let start = request
            .directory
            .clone()
            .filter(|d| d.is_dir())
            .unwrap_or_else(|| {
                let d = super::data_dir().join("Places");
                let _ = std::fs::create_dir_all(&d);
                d
            });
        let mut name = request.file_name.clone().unwrap_or_default();
        if kind != Kind::SaveFile {
            name.clear();
        }
        Picker {
            listing: Listing::read(&start, &request),
            request,
            kind,
            selected: None,
            name,
            new_folder: None,
            error: None,
            storage_access: super::jni::has_storage_access(),
            access_checked: Instant::now(),
        }
    }

    /// Re-asks the system about storage access about once a second.
    fn poll_access(&mut self, ctx: &egui::Context) {
        if self.access_checked.elapsed().as_secs_f32() > 1.0 {
            self.access_checked = Instant::now();
            let now = super::jni::has_storage_access();
            if now && !self.storage_access {
                // Folders that were unreadable list now.
                self.go(&self.listing.dir.clone());
            }
            self.storage_access = now;
        }
        if !self.storage_access {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }

    fn go(&mut self, dir: &Path) {
        self.listing = Listing::read(dir, &self.request);
        self.selected = None;
        self.error = None;
    }

    /// The answer when the dialog closes: `Some(Some(path))` chosen,
    /// `Some(None)` cancelled, `None` while it stays open.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Option<PathBuf>> {
        let title = self.request.title.clone().unwrap_or_else(|| {
            match self.kind {
                Kind::PickFile => "Open",
                Kind::PickFolder => "Choose folder",
                Kind::SaveFile => "Save as",
            }
            .to_string()
        });
        self.poll_access(ctx);
        let roots = roots(self.storage_access);
        let screen = ctx.content_rect();
        let size = egui::vec2((screen.width() - 32.0).min(720.0), (screen.height() - 32.0).min(560.0));
        let mut result: Option<Option<PathBuf>> = None;
        let mut open = true;
        egui::Window::new(title)
            .id(egui::Id::new("android-file-picker"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .default_size(size)
            .min_size(size)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.spacing_mut().button_padding = egui::vec2(10.0, 8.0);
                // Roots and the path.
                ui.horizontal_wrapped(|ui| {
                    for r in &roots {
                        let here = self.listing.dir.starts_with(&r.path);
                        if ui.selectable_label(here, r.label).clicked() {
                            self.go(&r.path);
                        }
                    }
                });
                if !self.storage_access {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new("Files in Download, Documents or elsewhere in shared storage need file access.")
                                .weak()
                                .size(12.0),
                        );
                        let label = if super::jni::sdk_int() >= 30 { "Allow all files access…" } else { "Allow file access…" };
                        if ui.small_button(label).clicked() {
                            if let Err(e) = super::jni::request_storage_access() {
                                self.error = Some(format!("Could not open the permission page: {e}"));
                            }
                        }
                        ui.label(egui::RichText::new("Or open the file from a Files app with rustudio.").weak().size(12.0));
                    });
                }
                ui.horizontal(|ui| {
                    let up = self.listing.dir.parent().map(Path::to_path_buf);
                    let at_root = roots.iter().any(|r| r.path == self.listing.dir);
                    if ui.add_enabled(up.is_some() && !at_root, egui::Button::new("⬆ Up")).clicked() {
                        if let Some(u) = up {
                            self.go(&u);
                        }
                    }
                    if ui.button("New folder").clicked() {
                        self.new_folder = Some(String::new());
                    }
                    ui.add(egui::Label::new(egui::RichText::new(shorten(&self.listing.dir)).weak()).truncate());
                });
                if let Some(nf) = &mut self.new_folder {
                    let mut make = false;
                    let mut cancel = false;
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        let r = ui.add(egui::TextEdit::singleline(nf).desired_width(220.0));
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            make = true;
                        }
                        make |= ui.button("Create").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                    if make && !nf.trim().is_empty() {
                        let dir = self.listing.dir.join(safe_name(nf.trim()));
                        match std::fs::create_dir_all(&dir) {
                            Ok(()) => {
                                self.new_folder = None;
                                self.go(&dir);
                            }
                            Err(e) => self.error = Some(format!("{}: {e}", dir.display())),
                        }
                    } else if cancel {
                        self.new_folder = None;
                    }
                }
                ui.separator();
                let list_height = ui.available_height() - 56.0;
                egui::ScrollArea::vertical().max_height(list_height.max(80.0)).auto_shrink([false, false]).show(ui, |ui| {
                    let row = egui::vec2(ui.available_width(), 36.0);
                    let mut enter: Option<PathBuf> = None;
                    for f in &self.listing.folders {
                        let name = format!("📁 {}", f.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
                        let sel = self.kind == Kind::PickFolder && self.selected.as_deref() == Some(f.as_path());
                        let r = ui.add_sized(row, egui::Button::selectable(sel, name));
                        if r.double_clicked() || (r.clicked() && self.kind != Kind::PickFolder) {
                            enter = Some(f.clone());
                        } else if r.clicked() {
                            self.selected = Some(f.clone());
                        }
                    }
                    for f in &self.listing.files {
                        let label = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let sel = self.selected.as_deref() == Some(f.as_path());
                        let r = ui.add_sized(row, egui::Button::selectable(sel, format!("📄 {label}")));
                        if r.clicked() {
                            self.selected = Some(f.clone());
                            if self.kind == Kind::SaveFile {
                                self.name = label.clone();
                            }
                        }
                        if r.double_clicked() && self.kind == Kind::PickFile {
                            result = Some(Some(f.clone()));
                        }
                    }
                    if self.listing.folders.is_empty() && self.listing.files.is_empty() {
                        ui.add_space(12.0);
                        ui.weak("Nothing here. Files put in this folder with a file manager, or opened\nfrom one with rustudio, show up in this list.");
                    }
                    if let Some(d) = enter {
                        self.go(&d);
                    }
                });
                ui.separator();
                if let Some(e) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                ui.horizontal(|ui| {
                    if self.kind == Kind::SaveFile {
                        ui.label("Name:");
                        ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(240.0));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (verb, ready) = match self.kind {
                            Kind::PickFile => ("Open", self.selected.is_some()),
                            Kind::PickFolder => ("Choose", true),
                            Kind::SaveFile => ("Save", !self.name.trim().is_empty()),
                        };
                        if ui.add_enabled(ready, egui::Button::new(verb)).clicked() {
                            result = Some(match self.kind {
                                Kind::PickFile => self.selected.clone(),
                                Kind::PickFolder => Some(self.selected.clone().unwrap_or_else(|| self.listing.dir.clone())),
                                Kind::SaveFile => Some(self.listing.dir.join(self.save_name())),
                            });
                        }
                        if ui.button("Cancel").clicked() {
                            result = Some(None);
                        }
                    });
                });
            });
        if !open {
            result = Some(None);
        }
        result
    }

    /// The name to save as, with the first filter's extension when none was
    /// typed (as desktop dialogs add it).
    fn save_name(&self) -> String {
        let name = safe_name(self.name.trim());
        let has_ext = Path::new(&name).extension().is_some();
        match (has_ext, self.request.filters.first().and_then(|(_, e)| e.first())) {
            (false, Some(ext)) => format!("{name}.{ext}"),
            _ => name,
        }
    }
}

/// A file name without path separators or control characters.
fn safe_name(s: &str) -> String {
    let out: String = s.chars().map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c }).collect();
    if out.is_empty() { "untitled".into() } else { out }
}

/// A path with the app's folder prefix replaced by its label.
fn shorten(p: &Path) -> String {
    for r in roots(true) {
        if let Ok(rest) = p.strip_prefix(&r.path) {
            let rest = rest.display().to_string();
            return if rest.is_empty() { r.label.to_string() } else { format!("{} / {}", r.label, rest.replace('/', " / ")) };
        }
    }
    p.display().to_string()
}

/// The place or model this activity was started to open ("Open with
/// rustudio" in a file manager, a share sheet, a download notification):
/// the Intent's data URI, copied into the app's folder when it is a
/// `content:` URI (another app's document, readable only through its
/// ContentResolver).
pub fn launch_file(_app: &super::AndroidApp) -> Option<PathBuf> {
    let uri = match super::jni::launch_intent_uri() {
        Ok(u) => u?,
        Err(e) => {
            log::warn!("launch intent: {e}");
            return None;
        }
    };
    log::info!("launch intent: {uri}");
    import_uri(&uri)
}

/// A local path for `uri`.
pub fn import_uri(uri: &str) -> Option<PathBuf> {
    if let Some(rest) = uri.strip_prefix("file://") {
        let p = PathBuf::from(percent_decode(rest));
        return p.is_file().then_some(p);
    }
    if !uri.starts_with("content://") {
        return None;
    }
    let name = super::jni::content_display_name(uri)
        .ok()
        .flatten()
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "place.rbxl".to_string());
    let bytes = match super::jni::read_content_uri(uri) {
        Ok(b) => b,
        Err(e) => {
            log::error!("{uri}: {e}");
            return None;
        }
    };
    let dir = super::data_dir().join("Imported");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::error!("{}: {e}", dir.display());
        return None;
    }
    let path = unique_path(&dir.join(safe_name(&name)));
    match std::fs::write(&path, &bytes) {
        Ok(()) => {
            log::info!("imported {} bytes to {}", bytes.len(), path.display());
            Some(path)
        }
        Err(e) => {
            log::error!("{}: {e}", path.display());
            None
        }
    }
}

/// `name.ext`, else `name (2).ext`, … so an import never overwrites.
fn unique_path(p: &Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let dir = p.parent().unwrap_or(Path::new("."));
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|c| !c.exists())
        .unwrap_or_else(|| p.to_path_buf())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()).ok_or(()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
