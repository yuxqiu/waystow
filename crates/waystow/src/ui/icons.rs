//! WayStow's own icons, drawn with GSK paths in the widget's CSS colour, so
//! the UI looks the same under any icon theme. (Symbolic SVG icons can't be
//! stroked: GTK forces `fill` on every shape when it recolours them.)

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, graphene, gsk};

/// `(name, SVG elements)` on a 16×16 grid; shapes are stroked 1.5 px with
/// round ends unless they carry `fill`.
#[allow(clippy::needless_raw_string_hashes)] // uniform quoting; some icons contain `"#`
const ICONS: &[(&str, &str)] = &[
    ("keep", r##"<path d="M8 2v7M5 6.5 8 9.5l3-3M2.5 10.5v2a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1v-2"/>"##),
    ("pin", r##"<path d="M9.5 2.5 13.5 6.5M10.5 3.5 7 6 4.5 6.5 9.5 11.5 10 9 12.5 5.5M7 9l-4 4"/>"##),
    ("close", r##"<path d="M4 4l8 8M12 4l-8 8"/>"##),
    (
        "more",
        r##"<circle cx="8" cy="3.5" r=".9" fill="#000"/><circle cx="8" cy="8" r=".9" fill="#000"/><circle cx="8" cy="12.5" r=".9" fill="#000"/>"##,
    ),
    ("folder", r##"<path d="M2 4.5a1 1 0 0 1 1-1h3l1.5 1.5H13a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z"/>"##),
    (
        "copy",
        r##"<rect x="5.5" y="5.5" width="8" height="8" rx="1.5"/><path d="M10.5 3.5V3a1 1 0 0 0-1-1H3a1 1 0 0 0-1 1v6.5a1 1 0 0 0 1 1h.5"/>"##,
    ),
    ("undo", r##"<path d="M5 3 2.5 5.5 5 8M2.5 5.5H10a3.5 3.5 0 0 1 0 7H6"/>"##),
    ("stop", r##"<circle cx="8" cy="8" r="6"/><rect x="6" y="6" width="4" height="4" rx=".5" fill="#000"/>"##),
    ("add", r##"<path d="M8 3v10M3 8h10"/>"##),
    ("check", r##"<path d="M3 8.5 6.5 12 13 4.5"/>"##),
    ("edit", r##"<path d="M10.5 2.5 13.5 5.5 6 13H3v-3zM9 4l3 3"/>"##),
    (
        "text",
        r##"<path d="M2 5V3.5a1 1 0 0 1 1-1h1.5M11.5 2.5H13a1 1 0 0 1 1 1V5M14 11v1.5a1 1 0 0 1-1 1h-1.5M4.5 13.5H3a1 1 0 0 1-1-1V11M5.5 5.5h5M8 5.5v5"/>"##,
    ),
    (
        "image",
        r##"<rect x="2" y="3" width="12" height="10" rx="1.5"/><circle cx="5.5" cy="6.5" r="1"/><path d="M2.5 11.5 6 8.5l3 2.5 2-1.5 2.5 2"/>"##,
    ),
    ("video", r##"<rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M6.5 6v4l3.5-2z" fill="#000"/>"##),
    (
        "audio",
        r##"<path d="M6 12V4l7-1.5v8"/><circle cx="4.5" cy="12" r="1.5"/><circle cx="11.5" cy="10.5" r="1.5"/>"##,
    ),
    (
        "doc",
        r##"<path d="M4 2h5l3.5 3.5V13a1 1 0 0 1-1 1h-7.5a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1zM9 2v3.5h3.5M5.5 8.5h5M5.5 11h3.5"/>"##,
    ),
    (
        "archive",
        r##"<rect x="2" y="2.5" width="12" height="3.5" rx="1"/><path d="M3 6v6.5a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1V6M6.5 8.5h3"/>"##,
    ),
    ("terminal", r##"<rect x="2" y="2.5" width="12" height="11" rx="1.5"/><path d="M4.5 6 6.5 8l-2 2M8 10.5h3.5"/>"##),
    (
        "link",
        r##"<path d="M7 9a2.5 2.5 0 0 0 3.5 0l2-2A2.5 2.5 0 0 0 9 3.5L8 4.5M9 7a2.5 2.5 0 0 0-3.5 0l-2 2A2.5 2.5 0 0 0 7 12.5l1-1"/>"##,
    ),
    (
        "trash",
        r##"<path d="M2.5 4.5h11M6 4.5V3a.5.5 0 0 1 .5-.5h3a.5.5 0 0 1 .5.5v1.5M4 4.5l.7 8.6a1 1 0 0 0 1 .9h4.6a1 1 0 0 0 1-.9l.7-8.6"/>"##,
    ),
    ("resize", r##"<path d="M9.5 2.5h4v4M13.5 2.5 9 7M6.5 13.5h-4v-4M2.5 13.5 7 9"/>"##),
    (
        "sparkle",
        r##"<path d="M8 2l1.3 3.7L13 7l-3.7 1.3L8 12l-1.3-3.7L3 7l3.7-1.3zM12.5 11.5l.5 1.5 1.5.5-1.5.5-.5 1.5-.5-1.5-1.5-.5 1.5-.5z"/>"##,
    ),
    (
        "merge",
        r##"<path d="M3 2.5v2A3.5 3.5 0 0 0 6.5 8H8v5.5M13 2.5v2A3.5 3.5 0 0 1 9.5 8H8M5.5 11 8 13.5 10.5 11"/>"##,
    ),
    ("split", r##"<path d="M8 2.5V8M8 8 3.5 12.5M8 8l4.5 4.5M2.5 10v3h3M13.5 10v3h-3"/>"##),
    (
        "qr",
        r##"<rect x="2" y="2" width="4.5" height="4.5" rx=".5"/><rect x="9.5" y="2" width="4.5" height="4.5" rx=".5"/><rect x="2" y="9.5" width="4.5" height="4.5" rx=".5"/><path d="M9.5 9.5h2v2M14 9.5v4.5h-2M9.5 12.5V14"/>"##,
    ),
    ("list", r##"<path d="M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.5M2.5 8h.5M2.5 12h.5"/>"##),
    ("convert", r##"<path d="M3 6.5A5 5 0 0 1 12.5 5M13 2.5V5h-2.5M13 9.5A5 5 0 0 1 3.5 11M3 13.5V11h2.5"/>"##),
    (
        "film",
        r##"<rect x="2" y="2.5" width="12" height="11" rx="1.5"/><path d="M5 2.5v11M11 2.5v11M2 6h3M2 10h3M11 6h3M11 10h3"/>"##,
    ),
    ("shield", r##"<path d="M8 2 13 4v4c0 3-2.2 5-5 6-2.8-1-5-3-5-6V4z"/><path d="M5.8 8.2 7.3 9.7 10.3 6.5"/>"##),
    ("hash", r##"<path d="M6 2.5 5 13.5M11 2.5l-1 11M3 6h10.5M2.5 10H13"/>"##),
    (
        "open",
        r##"<path d="M9 2.5h4.5V7M13.5 2.5 7.5 8.5M11.5 9.5V13a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V5.5a1 1 0 0 1 1-1h3.5"/>"##,
    ),
    (
        "clipboard",
        r##"<rect x="3" y="3" width="10" height="11" rx="1.5"/><rect x="5.5" y="1.75" width="5" height="2.5" rx=".75"/>"##,
    ),
    ("chevron", r##"<path d="M4 6l4 4 4-4"/>"##),
    (
        "braces",
        r##"<path d="M5.5 2.5c-1.5 0-2 .5-2 2v1.5c0 1-.5 2-1.5 2 1 0 1.5 1 1.5 2v1.5c0 1.5.5 2 2 2M10.5 2.5c1.5 0 2 .5 2 2v1.5c0 1 .5 2 1.5 2-1 0-1.5 1-1.5 2v1.5c0 1.5-.5 2-2 2"/>"##,
    ),
    (
        "compare",
        r##"<rect x="2" y="2.5" width="5" height="11" rx="1"/><rect x="9" y="2.5" width="5" height="11" rx="1"/><path d="M4 6h1M4 9h1M11 6h1M11 9h1"/>"##,
    ),
    (
        "camera",
        r##"<path d="M2 5.5a1 1 0 0 1 1-1h2L6 3h4l1 1.5h2a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z"/><circle cx="8" cy="8.5" r="2.5"/>"##,
    ),
    (
        "eye",
        r##"<path d="M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z"/><circle cx="8" cy="8" r="2"/>"##,
    ),
    ("loudness", r##"<path d="M2.5 6v4M5.5 4v8M8.5 2v12M11.5 5v6M14 7v2"/>"##),
    ("pages", r##"<rect x="4.5" y="2" width="9" height="11" rx="1"/><path d="M2.5 4.5V13a1 1 0 0 0 1 1h7"/>"##),
    (
        "tray",
        r##"<path d="M2 9.5 3.8 3.6a1 1 0 0 1 1-.6h6.4a1 1 0 0 1 1 .6L14 9.5V12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1zM2 9.5h3.5l1 1.5h3l1-1.5H14"/>"##,
    ),
];

struct Shapes {
    stroke: Option<gsk::Path>,
    fill: Option<gsk::Path>,
}

thread_local! {
    static CACHE: RefCell<HashMap<&'static str, Rc<Shapes>>> = RefCell::new(HashMap::new());
}

/// `key="value"` attributes of one element.
fn attrs(tag: &str) -> HashMap<&str, &str> {
    let mut out = HashMap::new();
    let mut rest = tag;
    while let Some(eq) = rest.find("=\"") {
        let key = rest[..eq].rsplit(' ').next().unwrap_or("").trim();
        let after = &rest[eq + 2..];
        let Some(end) = after.find('"') else { break };
        out.insert(key, &after[..end]);
        rest = &after[end + 1..];
    }
    out
}

fn num(a: &HashMap<&str, &str>, k: &str) -> f32 {
    a.get(k).and_then(|v| v.parse().ok()).unwrap_or(0.0)
}

fn build(body: &str) -> Shapes {
    let (stroke, fill) = (gsk::PathBuilder::new(), gsk::PathBuilder::new());
    let (mut any_stroke, mut any_fill) = (false, false);
    for el in body.split('<').filter(|e| !e.trim().is_empty()) {
        let name = el.split(' ').next().unwrap_or("");
        let a = attrs(el);
        let filled = a.contains_key("fill");
        let target = if filled { &fill } else { &stroke };
        match name {
            "path" => {
                if let Some(p) = a.get("d").and_then(|d| gsk::Path::parse(d).ok()) {
                    target.add_path(&p);
                }
            }
            "circle" => target.add_circle(&graphene::Point::new(num(&a, "cx"), num(&a, "cy")), num(&a, "r")),
            "rect" => {
                let r = graphene::Rect::new(num(&a, "x"), num(&a, "y"), num(&a, "width"), num(&a, "height"));
                let rx = num(&a, "rx");
                if rx > 0.0 {
                    target.add_rounded_rect(&gsk::RoundedRect::from_rect(r, rx));
                } else {
                    target.add_rect(&r);
                }
            }
            _ => continue,
        }
        if filled {
            any_fill = true;
        } else {
            any_stroke = true;
        }
    }
    Shapes { stroke: any_stroke.then(|| stroke.to_path()), fill: any_fill.then(|| fill.to_path()) }
}

fn shapes(short: &str) -> Option<Rc<Shapes>> {
    let (key, body) = ICONS.iter().find(|(n, _)| *n == short)?;
    Some(CACHE.with(|c| c.borrow_mut().entry(key).or_insert_with(|| Rc::new(build(body))).clone()))
}

/// The short name of a bundled icon (`ws-copy-symbolic` → `copy`).
fn short(name: &str) -> Option<&str> {
    let s = name.strip_prefix("ws-")?;
    Some(s.strip_suffix("-symbolic").unwrap_or(s))
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Glyph {
        pub name: RefCell<String>,
        pub size: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Glyph {
        const NAME: &'static str = "WaystowGlyph";
        type Type = super::Glyph;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Glyph {}

    impl WidgetImpl for Glyph {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let s = self.size.get();
            (s, s, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let Some(shapes) = shapes(&self.name.borrow()) else { return };
            let w = self.obj();
            let color = w.color();
            let scale = self.size.get() as f32 / 16.0;
            snapshot.save();
            snapshot.scale(scale, scale);
            if let Some(p) = &shapes.stroke {
                let stroke = gsk::Stroke::new(1.5);
                stroke.set_line_cap(gsk::LineCap::Round);
                stroke.set_line_join(gsk::LineJoin::Round);
                snapshot.append_stroke(p, &stroke, &color);
            }
            if let Some(p) = &shapes.fill {
                snapshot.append_fill(p, gsk::FillRule::Winding, &color);
            }
            snapshot.restore();
        }
    }
}

glib::wrapper! {
    /// A bundled icon.
    pub struct Glyph(ObjectSubclass<imp::Glyph>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// A bundled icon by short name (`copy`) or full name (`ws-copy-symbolic`).
pub fn glyph(name: &str, size: i32) -> gtk::Widget {
    let g: Glyph = glib::Object::new();
    short(name).unwrap_or(name).clone_into(&mut g.imp().name.borrow_mut());
    g.imp().size.set(size);
    g.set_valign(gtk::Align::Center);
    g.set_halign(gtk::Align::Center);
    g.add_css_class("ws-glyph");
    g.upcast()
}

/// An icon: bundled (`ws-…`) or from the icon theme.
pub fn widget(name: &str, size: i32) -> gtk::Widget {
    if short(name).is_some_and(|s| ICONS.iter().any(|(n, _)| *n == s)) {
        return glyph(name, size);
    }
    let img = gtk::Image::from_icon_name(name);
    img.set_pixel_size(size);
    img.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_parse() {
        let a = attrs(r#"rect x="2" y="3" width="12" height="10" rx="1.5"/>"#);
        assert_eq!(a["x"], "2");
        assert_eq!(a["rx"], "1.5");
        assert_eq!(short("ws-copy-symbolic"), Some("copy"));
        assert_eq!(short("edit-copy-symbolic"), None);
    }

    #[test]
    fn every_icon_builds() {
        // GSK paths need no display; only one test may initialise GTK (theme.rs).
        for (name, body) in ICONS {
            let s = build(body);
            assert!(s.stroke.is_some() || s.fill.is_some(), "{name} is empty");
        }
    }
}
