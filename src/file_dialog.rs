//! Native file dialogs that don't block the UI thread.
//!
//! rfd's blocking `FileDialog` stalls the event loop for as long as the
//! dialog is open, so the window stops answering the compositor's pings and
//! the desktop offers to kill rustudio ("rustudio is not responding:
//! Terminate / Wait"). Here the dialog is rfd's `AsyncFileDialog`, created
//! on the UI thread (macOS needs that) and awaited on a worker thread; the
//! app keeps drawing and picks the answer up with [`Dialogs::poll`].

use std::path::PathBuf;
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

pub enum Kind {
    PickFile,
    PickFolder,
    SaveFile,
}

#[derive(Default)]
pub struct Dialogs {
    pending: Option<(Purpose, Receiver<Option<PathBuf>>)>,
    /// Repainted when a dialog closes (the UI may be idle meanwhile).
    pub ctx: Option<egui::Context>,
}

impl Dialogs {
    pub fn new(ctx: egui::Context) -> Self {
        Self { pending: None, ctx: Some(ctx) }
    }

    /// A dialog is open.
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Opens `dialog`; false (and nothing happens) while another is open.
    pub fn start(&mut self, purpose: Purpose, dialog: rfd::AsyncFileDialog, kind: Kind) -> bool {
        if self.pending.is_some() {
            return false;
        }
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
    pub fn poll(&mut self) -> Option<(Purpose, Option<PathBuf>)> {
        let (_, rx) = self.pending.as_ref()?;
        let path = match rx.try_recv() {
            Ok(p) => p,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => None,
        };
        self.pending.take().map(|(purpose, _)| (purpose, path))
    }
}

#[cfg(test)]
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
}
