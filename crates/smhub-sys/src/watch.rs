//! Waiting for file system changes with inotify instead of polling (decision D17).

use std::path::PathBuf;

use futures_util::StreamExt;
use inotify::{Inotify, WatchMask};
use tokio::sync::mpsc;

/// Watch paths (files or folders) and send a unit on every change. The task ends when the
/// receiver is dropped. Paths that do not exist are skipped.
pub fn watch(paths: Vec<PathBuf>, mask: WatchMask) -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel(8);
    tokio::spawn(async move {
        let Ok(ino) = Inotify::init() else {
            tracing::warn!("inotify is not available");
            return;
        };
        let mut any = false;
        for p in &paths {
            if ino.watches().add(p, mask).is_ok() {
                any = true;
            }
        }
        if !any {
            return;
        }
        let buf = vec![0u8; 4096];
        let Ok(mut stream) = ino.into_event_stream(buf) else { return };
        while let Some(ev) = stream.next().await {
            if ev.is_err() {
                break;
            }
            match tx.try_send(()) {
                Ok(()) | Err(mpsc::error::TrySendError::Full(())) => {}
                Err(mpsc::error::TrySendError::Closed(())) => break,
            }
        }
    });
    rx
}

/// The usual mask for "a file in this folder or the file itself changed".
pub fn changes() -> WatchMask {
    WatchMask::MODIFY | WatchMask::CREATE | WatchMask::DELETE | WatchMask::MOVED_TO | WatchMask::MOVED_FROM | WatchMask::CLOSE_WRITE
}
