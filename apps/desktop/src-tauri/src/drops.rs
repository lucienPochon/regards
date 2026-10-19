//! Files dropped on the window, to attach (plan §5.9). The paths stay here,
//! as the system gave them: the UI learns only the files' names and sizes,
//! and asks to attach a drop to a draft by its id. As with the file picker,
//! the WebView never handles a path (ADR-0006 §4).

use crate::{Bridge, describe};
use regards_ipc::protocol::{DraftAttachFileParams, DraftView, methods};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{DragDropEvent, Emitter, Manager, State, Window};

/// Drops kept, the newest last: an older one is of no use any more.
const KEPT: usize = 8;

/// A drop: its id, and the paths the system gave.
type Kept = (u64, Vec<PathBuf>);

/// The last id given, and the drops kept.
#[derive(Default)]
pub(crate) struct Drops(Mutex<(u64, Vec<Kept>)>);

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DroppedFile {
    pub name: String,
    pub size: Option<u64>,
    /// An image, which a composer may put in the text instead.
    pub image: bool,
}

/// The `files_dropped` event.
#[derive(Debug, Clone, Serialize)]
struct Dropped {
    id: u64,
    files: Vec<DroppedFile>,
    /// Where in the window, in CSS pixels: on the text, images go in it.
    x: f64,
    y: f64,
}

/// What attaching a drop gave: the draft as it now is, and what could not be attached.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Attached {
    pub draft: Option<DraftView>,
    pub refused: Vec<Refused>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Refused {
    pub name: String,
    pub error: String,
}

fn is_image(path: &std::path::Path) -> bool {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp")
}

fn name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Tell the UI files are being dragged over the window (`files_drag`), and
/// what was dropped (`files_dropped`).
pub(crate) fn on_window(window: &Window, event: &DragDropEvent) {
    match event {
        DragDropEvent::Enter { .. } => {
            let _ = window.emit("files_drag", serde_json::json!({ "over": true }));
        }
        DragDropEvent::Leave => {
            let _ = window.emit("files_drag", serde_json::json!({ "over": false }));
        }
        DragDropEvent::Drop { paths, position } => {
            let at = position.to_logical::<f64>(window.scale_factor().unwrap_or(1.0));
            let _ = window.emit("files_drag", serde_json::json!({ "over": false }));
            let files: Vec<DroppedFile> = paths
                .iter()
                .map(|path| DroppedFile {
                    name: name(path),
                    size: std::fs::metadata(path).ok().filter(|m| m.is_file()).map(|m| m.len()),
                    image: is_image(path),
                })
                .collect();
            let drops = window.state::<Drops>();
            let mut kept = drops.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            kept.0 += 1;
            let id = kept.0;
            kept.1.push((id, paths.clone()));
            let excess = kept.1.len().saturating_sub(KEPT);
            kept.1.drain(..excess);
            drop(kept);
            let _ = window.emit(
                "files_dropped",
                Dropped {
                    id,
                    files,
                    x: at.x,
                    y: at.y,
                },
            );
        }
        _ => {}
    }
}

/// Attach the files of a drop to a draft (once: the drop is then forgotten),
/// with `inline` as images in its text.
#[tauri::command]
pub(crate) async fn draft_attach_dropped(
    bridge: State<'_, Bridge>,
    drops: State<'_, Drops>,
    draft_id: i64,
    drop_id: u64,
    inline: Option<bool>,
) -> Result<Attached, String> {
    let paths = {
        let mut kept = drops.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let at = kept
            .1
            .iter()
            .position(|(id, _)| *id == drop_id)
            .ok_or("those files are no longer at hand; drop them again")?;
        kept.1.remove(at).1
    };
    let mut client = bridge.connect().await?.client;
    let mut attached = Attached {
        draft: None,
        refused: Vec::new(),
    };
    for path in paths {
        let params = DraftAttachFileParams {
            draft_id,
            path: path.to_string_lossy().into_owned(),
            inline: inline.unwrap_or(false),
        };
        match client.call::<_, DraftView>(methods::DRAFT_ATTACH_FILE, params).await {
            Ok(draft) => attached.draft = Some(draft),
            Err(error) => attached.refused.push(Refused {
                name: name(&path),
                error: describe(error),
            }),
        }
    }
    Ok(attached)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_are_told_apart_by_their_kind() {
        assert!(is_image(std::path::Path::new("C:/photos/Lake.JPG")));
        assert!(is_image(std::path::Path::new("/tmp/chart.png")));
        assert!(!is_image(std::path::Path::new("/tmp/report.pdf")));
        assert!(!is_image(std::path::Path::new("/tmp/folder")));
        assert_eq!(name(std::path::Path::new("/tmp/report.pdf")), "report.pdf");
    }
}
