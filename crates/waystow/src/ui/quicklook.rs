//! Quick Look: a large preview (Space on a shelf item). Images at full size,
//! videos and PDFs as a large thumbnail, text as text.

use std::io::Read;
use std::path::Path;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use waystow_core::uri;

use super::widgets::{icon_button, label};
use super::{Inner, thumbs};
use crate::files;

pub struct QuickLook {
    window: gtk::Window,
}

const TEXT_LIMIT: u64 = 256 * 1024;

fn is_text(mime: &str) -> bool {
    mime.starts_with("text/")
        || matches!(
            mime,
            "application/json"
                | "application/xml"
                | "application/toml"
                | "application/x-yaml"
                | "application/javascript"
                | "application/x-shellscript"
        )
}

impl QuickLook {
    pub fn open(inner: &Rc<Inner>, path: &Path, mime: &str) -> Self {
        let window = gtk::Window::new();
        window.add_css_class("waystow");
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace(Some("waystow-quicklook"));
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        let (mw, mh) = gdk::Display::default()
            .and_then(|d| d.monitors().item(0).and_downcast::<gdk::Monitor>())
            .map_or((1600, 1000), |m| (m.geometry().width(), m.geometry().height()));
        let (w, h) = ((f64::from(mw) * 0.6) as i32, (f64::from(mh) * 0.7) as i32);
        window.set_default_size(w, h);

        let col = gtk::Box::new(gtk::Orientation::Vertical, 10);
        col.add_css_class("ws-quicklook");
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let size = std::fs::metadata(path).map(|m| files::human_size(m.len())).unwrap_or_default();
        let words = gtk::Box::new(gtk::Orientation::Vertical, 1);
        words.set_hexpand(true);
        let t = label(&name, "ws-heading");
        t.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        words.append(&t);
        words.append(&label(&format!("{} · {size}", files::describe(mime)), "ws-sub"));
        head.append(&words);
        let open = icon_button("ws-open-symbolic", "Open");
        let target = uri::from_path(path);
        open.connect_clicked(move |_| {
            let _ = crate::system::open(&target);
        });
        head.append(&open);
        let close = icon_button("ws-close-symbolic", "Close (Space or Esc)");
        let wi = Rc::downgrade(inner);
        close.connect_clicked(move |_| {
            if let Some(i) = wi.upgrade() {
                i.close_quick_look();
            }
        });
        head.append(&close);
        col.append(&head);

        let body: gtk::Widget = if mime.starts_with("image/") {
            let pic = gtk::Picture::for_filename(path);
            pic.set_content_fit(gtk::ContentFit::Contain);
            pic.set_vexpand(true);
            pic.upcast()
        } else if is_text(mime) {
            let mut buf = Vec::new();
            if let Ok(f) = std::fs::File::open(path) {
                let _ = f.take(TEXT_LIMIT).read_to_end(&mut buf);
            }
            let view = gtk::TextView::new();
            view.add_css_class("ws-text");
            view.set_editable(false);
            view.set_monospace(true);
            view.set_wrap_mode(gtk::WrapMode::WordChar);
            view.buffer().set_text(&String::from_utf8_lossy(&buf));
            gtk::ScrolledWindow::builder().child(&view).vexpand(true).build().upcast()
        } else if thumbs::supported(mime) {
            let pic = gtk::Picture::new();
            pic.set_content_fit(gtk::ContentFit::Contain);
            pic.set_vexpand(true);
            let p = pic.clone();
            thumbs::load(path, mime, 1024, move |tex| {
                if let Some(t) = tex {
                    p.set_paintable(Some(&t));
                }
            });
            pic.upcast()
        } else {
            let icon = super::widgets::type_icon(mime, 128);
            icon.set_vexpand(true);
            icon
        };
        col.append(&body);
        window.set_child(Some(&col));

        let keys = gtk::EventControllerKey::new();
        let wi = Rc::downgrade(inner);
        keys.connect_key_pressed(move |_, key, _, _| {
            if matches!(key, gdk::Key::Escape | gdk::Key::space)
                && let Some(i) = wi.upgrade()
            {
                i.close_quick_look();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(keys);
        window.present();
        Self { window }
    }

    pub fn close(self) {
        self.window.destroy();
    }
}
