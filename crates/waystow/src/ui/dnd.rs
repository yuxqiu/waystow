//! Reading drops and starting drags.
//!
//! Drags out offer a `GdkFileList`, so GTK provides `text/uri-list` and the
//! document portal's `application/vnd.portal.filetransfer` (for Flatpak apps)
//! by itself.

use std::path::PathBuf;
use std::rc::{Rc, Weak};

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use waystow_core::model::Content;
use waystow_core::uri;

use super::Inner;
use super::catcher::Catcher;
use crate::daemon::{Cmd, Payload};
use crate::files::{self, SNIPPET, URL};

/// What a drag carries, as far as we know before the drop.
#[derive(Debug, Clone, Default)]
pub struct Offer {
    /// MIME types of the dragged things (files sniffed; synthetic for text).
    pub mimes: Vec<String>,
    /// File names, for the drop view's caption.
    pub names: Vec<String>,
}

const IMAGE_TYPES: [&str; 5] = ["image/png", "image/jpeg", "image/webp", "image/gif", "image/avif"];

impl Offer {
    /// A first guess from the offered formats alone.
    pub fn from_formats(f: &gdk::ContentFormats) -> Self {
        let mimes = if f.contains_type(gdk::FileList::static_type()) {
            Vec::new()
        } else if let Some(m) = IMAGE_TYPES.iter().find(|m| f.contain_mime_type(m)) {
            vec![(*m).to_owned()]
        } else if f.contains_type(String::static_type()) {
            vec![SNIPPET.to_owned()]
        } else {
            Vec::new()
        };
        Self { mimes, names: Vec::new() }
    }
}

fn local_paths(list: &gdk::FileList) -> (Vec<PathBuf>, Vec<String>) {
    let mut paths = Vec::new();
    let mut remote = Vec::new();
    for f in list.files() {
        match f.path() {
            Some(p) => paths.push(p),
            None => remote.push(f.uri().to_string()),
        }
    }
    (paths, remote)
}

impl Offer {
    /// Describe what a read payload holds.
    pub fn of(payload: &Payload) -> Self {
        match payload {
            Payload::Files(paths) => {
                let mut offer = Self::default();
                for p in paths.iter().take(64) {
                    offer.mimes.push(files::probe(p).map_or_else(|_| "application/octet-stream".into(), |(m, _)| m));
                    offer.names.push(p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                }
                offer
            }
            Payload::Image { mime, .. } => Self { mimes: vec![mime.clone()], names: Vec::new() },
            Payload::Text(text) => {
                let lines = uri::parse_list(text);
                let urls = !lines.is_empty() && lines.iter().all(|l| uri::is_web(l));
                Self { mimes: vec![if urls { URL } else { SNIPPET }.to_owned()], names: Vec::new() }
            }
        }
    }
}

async fn read_stream(stream: gio::InputStream, cap: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let chunk = stream.read_bytes_future(64 * 1024, glib::Priority::DEFAULT).await.map_err(|e| e.to_string())?;
        if chunk.is_empty() {
            break;
        }
        out.extend_from_slice(&chunk);
        if out.len() as u64 > cap {
            return Err(format!("the dropped data is larger than the {} MiB limit", cap >> 20));
        }
    }
    Ok(out)
}

/// Read a drop: local files, else image bytes, else text (snippet or URLs).
pub async fn read(drop: &gdk::Drop, cap: u64) -> Result<Payload, String> {
    let formats = drop.formats();
    let remote = if formats.contains_type(gdk::FileList::static_type())
        && let Ok(value) = drop.read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT).await
        && let Ok(list) = value.get::<gdk::FileList>()
    {
        let (paths, r) = local_paths(&list);
        if !paths.is_empty() {
            return Ok(Payload::Files(paths));
        }
        r
    } else {
        Vec::new()
    };
    let images: Vec<&str> = IMAGE_TYPES.iter().copied().filter(|m| formats.contain_mime_type(m)).collect();
    if !images.is_empty() {
        let (stream, mime) = drop
            .read_future(&images, glib::Priority::DEFAULT)
            .await
            .map_err(|e| format!("reading the dropped image: {e}"))?;
        let bytes = read_stream(stream, cap).await?;
        if !bytes.is_empty() {
            return Ok(Payload::Image { mime: mime.to_string(), bytes });
        }
    }
    if !remote.is_empty() {
        return Ok(Payload::Text(remote.join("\n")));
    }
    if formats.contains_type(String::static_type()) {
        let v = drop
            .read_value_future(String::static_type(), glib::Priority::DEFAULT)
            .await
            .map_err(|e| format!("reading the dropped text: {e}"))?;
        let text = v.get::<String>().map_err(|e| e.to_string())?;
        if text.len() as u64 > cap {
            return Err(format!("the dropped text is larger than the {} MiB limit", cap >> 20));
        }
        return Ok(Payload::Text(text));
    }
    Err("WayStow can't keep this kind of drop".into())
}

/// A drag source for shelf items. `ids` is read when the drag starts.
pub fn source(inner: &Rc<Inner>, catcher: &Rc<Catcher>, ids: impl Fn() -> Vec<i64> + 'static) -> gtk::DragSource {
    let src = gtk::DragSource::new();
    src.set_actions(gdk::DragAction::COPY);
    let wi = Rc::downgrade(inner);
    let ids = Rc::new(ids);
    let ids2 = ids.clone();
    src.connect_prepare(move |_, _, _| {
        let inner = wi.upgrade()?;
        let ids = ids();
        let m = inner.model.borrow();
        let items: Vec<_> = m.snap.items.iter().filter(|i| ids.contains(&i.id)).collect();
        let files: Vec<gio::File> =
            items.iter().filter_map(|i| i.content.path().filter(|p| p.exists()).map(gio::File::for_path)).collect();
        if !files.is_empty() {
            let list = gdk::FileList::from_array(&files);
            return Some(gdk::ContentProvider::for_value(&list.to_value()));
        }
        let text: Vec<String> = items
            .iter()
            .filter_map(|i| match &i.content {
                Content::Text(t) | Content::Url(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        (!text.is_empty()).then(|| gdk::ContentProvider::for_value(&text.join("\n").to_value()))
    });
    let (wi, wc) = (Rc::downgrade(inner), Rc::downgrade(catcher));
    src.connect_drag_begin(move |_, drag| {
        let Some(inner) = wi.upgrade() else { return };
        let ids = ids2();
        let icon = {
            let m = inner.model.borrow();
            let items: Vec<_> = m.snap.items.iter().filter(|i| ids.contains(&i.id)).cloned().collect();
            super::widgets::thumb_stack(&items, 64, true)
        };
        if let Ok(di) = gtk::DragIcon::for_drag(drag).downcast::<gtk::DragIcon>() {
            di.set_child(Some(&icon));
        }
        *inner.dragging_ids.borrow_mut() = ids;
        inner.send(Cmd::DraggingOut(true));
        if let Some(c) = wc.upgrade() {
            c.drag_out(true);
        }
    });
    let (wi, wc) = (Rc::downgrade(inner), Rc::downgrade(catcher));
    src.connect_drag_end(move |_, _, _| end_drag(&wi, &wc));
    let (wi, wc) = (Rc::downgrade(inner), Rc::downgrade(catcher));
    src.connect_drag_cancel(move |_, _, _| {
        end_drag(&wi, &wc);
        false
    });
    src
}

fn end_drag(wi: &Weak<Inner>, wc: &Weak<Catcher>) {
    let Some(inner) = wi.upgrade() else { return };
    if inner.dragging_ids.borrow().is_empty() {
        return;
    }
    inner.dragging_ids.borrow_mut().clear();
    inner.send(Cmd::DraggingOut(false));
    if let Some(c) = wc.upgrade() {
        c.drag_out(false);
    }
}

/// A drag source for plain files (Recent rows). Keeps the panel open while dragging.
pub fn path_source(catcher: &Rc<Catcher>, paths: impl Fn() -> Vec<PathBuf> + 'static) -> gtk::DragSource {
    let src = gtk::DragSource::new();
    src.set_actions(gdk::DragAction::COPY);
    src.connect_prepare(move |_, _, _| {
        let files: Vec<gio::File> = paths().iter().filter(|p| p.exists()).map(gio::File::for_path).collect();
        (!files.is_empty()).then(|| gdk::ContentProvider::for_value(&gdk::FileList::from_array(&files).to_value()))
    });
    let wc = Rc::downgrade(catcher);
    src.connect_drag_begin(move |_, _| {
        if let Some(c) = wc.upgrade() {
            c.drag_out(true);
        }
    });
    let wc = Rc::downgrade(catcher);
    src.connect_drag_end(move |_, _, _| {
        if let Some(c) = wc.upgrade() {
            c.drag_out(false);
        }
    });
    src
}
