//! File dialogs that don't block the UI thread.
//!
//! Desktops: rfd's blocking `FileDialog` stalls the event loop for as long
//! as the dialog is open, so the window stops answering the compositor's
//! pings and the desktop offers to kill rustudio ("rustudio is not
//! responding: Terminate / Wait"). Here the dialog is rfd's
//! `AsyncFileDialog`, created on the UI thread (macOS needs that) and
//! awaited on a worker thread; the app keeps drawing and picks the answer
//! up with [`Dialogs::poll`].
//!
//! Android: rfd has no backend there, and `NativeActivity` can't receive the
//! system picker's `onActivityResult`. The dialog is rustudio's own
//! ([`crate::android::storage::Picker`]), drawn by [`Dialogs::poll`] over the
//! app's files folder (`Android/data/<package>/files`, which file managers
//! can see); files from elsewhere arrive through "Open with" (the VIEW
//! intent, `crate::android::storage::launch_file`).
//!
//! Callers describe the dialog with a [`Request`], the same on every system.

use std::path::PathBuf;
#[cfg(not(target_os = "android"))]
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use rbx_dom_weak::types::Ref;

/// What the chosen path is for.
#[derive(Clone, Debug, PartialEq)]
pub enum Purpose {
    OpenPlace,
    OpenFolder,
    SaveAs,
    InsertModel,
    /// The selection when the dialog opened.
    SaveSelection(Vec<Ref>),
    /// Save as Project Folder: the "Create in" folder.
    ProjectParent,
    /// Terrain Editor > Import: a heightmap (true) or colormap image.
    TerrainImage { height: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    PickFile,
    PickFolder,
    SaveFile,
}

/// What to ask for: the dialog's filters, suggested name and start folder.
#[derive(Clone, Debug, Default)]
pub struct Request {
    pub title: Option<String>,
    /// (label, extensions without the dot).
    pub filters: Vec<(String, Vec<String>)>,
    pub file_name: Option<String>,
    pub directory: Option<PathBuf>,
}

impl Request {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_filter(mut self, name: &str, extensions: &[&str]) -> Self {
        self.filters.push((name.to_string(), extensions.iter().map(|e| e.to_string()).collect()));
        self
    }

    pub fn set_file_name(mut self, name: &str) -> Self {
        self.file_name = Some(name.to_string());
        self
    }

    pub fn set_directory(mut self, dir: impl Into<PathBuf>) -> Self {
        self.directory = Some(dir.into());
        self
    }

    /// (Part of the rfd-shaped builder; no caller sets one yet.)
    #[allow(dead_code)]
    pub fn set_title(mut self, title: &str) -> Self {
        self.title = Some(title.to_string());
        self
    }

    /// Whether `path`'s extension passes the filters (no filters: anything).
    /// The Android picker filters its listing with this; rfd does its own.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn accepts(&self, path: &std::path::Path) -> bool {
        if self.filters.is_empty() {
            return true;
        }
        let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
        self.filters.iter().any(|(_, exts)| exts.iter().any(|e| e.eq_ignore_ascii_case(&ext)))
    }

    #[cfg(not(target_os = "android"))]
    fn into_rfd(self) -> rfd::AsyncFileDialog {
        let mut d = rfd::AsyncFileDialog::new();
        if let Some(t) = &self.title {
            d = d.set_title(t);
        }
        for (name, exts) in &self.filters {
            let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
            d = d.add_filter(name, &exts);
        }
        if let Some(n) = &self.file_name {
            d = d.set_file_name(n);
        }
        if let Some(dir) = &self.directory {
            d = d.set_directory(dir);
        }
        d
    }
}

#[derive(Default)]
pub struct Dialogs {
    #[cfg(not(target_os = "android"))]
    pending: Option<(Purpose, Receiver<Option<PathBuf>>)>,
    #[cfg(target_os = "android")]
    pending: Option<(Purpose, crate::android::storage::Picker)>,
    /// Repainted when a dialog closes (the UI may be idle meanwhile).
    pub ctx: Option<egui::Context>,
}

impl Dialogs {
    pub fn new(ctx: egui::Context) -> Self {
        Self { pending: None, ctx: Some(ctx) }
    }

    /// A dialog is open.
    #[allow(dead_code)]
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Opens the dialog; false (and nothing happens) while another is open.
    #[cfg(not(target_os = "android"))]
    pub fn start(&mut self, purpose: Purpose, request: Request, kind: Kind) -> bool {
        if self.pending.is_some() {
            return false;
        }
        let dialog = request.into_rfd();
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        type Fut = std::pin::Pin<Box<dyn std::future::Future<Output = Option<rfd::FileHandle>> + Send>>;
        let fut: Fut = match kind {
            Kind::PickFile => Box::pin(dialog.pick_file()),
            Kind::PickFolder => Box::pin(dialog.pick_folder()),
            Kind::SaveFile => Box::pin(dialog.save_file()),
        };
        let spawned = std::thread::Builder::new().name("file-dialog".into()).spawn(move || {
            let path = pollster::block_on(fut).map(|h| h.path().to_path_buf());
            let _ = tx.send(path);
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        if spawned.is_err() {
            return false;
        }
        self.pending = Some((purpose, rx));
        true
    }

    /// The finished dialog's purpose and path (None: cancelled).
    #[cfg(not(target_os = "android"))]
    pub fn poll(&mut self) -> Option<(Purpose, Option<PathBuf>)> {
        let (_, rx) = self.pending.as_ref()?;
        let path = match rx.try_recv() {
            Ok(p) => p,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => None,
        };
        self.pending.take().map(|(purpose, _)| (purpose, path))
    }

    /// Opens rustudio's own picker; false while another is open.
    #[cfg(target_os = "android")]
    pub fn start(&mut self, purpose: Purpose, request: Request, kind: Kind) -> bool {
        if self.pending.is_some() {
            return false;
        }
        self.pending = Some((purpose, crate::android::storage::Picker::new(request, kind)));
        if let Some(c) = &self.ctx {
            c.request_repaint();
        }
        true
    }

    /// Draws the open picker (this is called once per frame from the app's
    /// update) and hands back its answer when it closes.
    #[cfg(target_os = "android")]
    pub fn poll(&mut self) -> Option<(Purpose, Option<PathBuf>)> {
        let ctx = self.ctx.clone()?;
        let (_, picker) = self.pending.as_mut()?;
        let done = picker.show(&ctx)?;
        self.pending.take().map(|(purpose, _)| (purpose, done))
    }
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::*;

    #[test]
    fn poll_hands_back_purpose_and_path_once() {
        let mut d = Dialogs::default();
        let (tx, rx) = channel();
        d.pending = Some((Purpose::SaveAs, rx));
        assert!(d.busy() && d.poll().is_none(), "still open");
        tx.send(Some(PathBuf::from("/tmp/x.rbxl"))).unwrap();
        assert_eq!(d.poll(), Some((Purpose::SaveAs, Some(PathBuf::from("/tmp/x.rbxl")))));
        assert!(!d.busy() && d.poll().is_none());
        // A worker that died counts as cancelled.
        let (tx, rx) = channel::<Option<PathBuf>>();
        d.pending = Some((Purpose::OpenPlace, rx));
        drop(tx);
        assert_eq!(d.poll(), Some((Purpose::OpenPlace, None)));
    }

    #[test]
    fn request_filters() {
        let r = Request::new().add_filter("Roblox place", &["rbxl", "rbxlx"]);
        assert!(r.accepts(std::path::Path::new("a/b.RBXL")));
        assert!(!r.accepts(std::path::Path::new("a/b.rbxm")));
        assert!(Request::new().accepts(std::path::Path::new("anything")));
    }
}
