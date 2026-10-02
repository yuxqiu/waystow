//! Small building blocks: thumbnails, the fanned thumbnail stack, rows.

use std::path::Path;

use gtk::prelude::*;
use gtk::{graphene, gsk};
use waystow_core::model::{Content, Item};

use super::thumbs;
use crate::files;

/// The type icon for a MIME type: the theme's, else a bundled one.
pub fn type_icon(mime: &str, px: i32) -> gtk::Widget {
    let gicon = files::icon(mime);
    let themed = gtk::gdk::Display::default().is_some_and(|d| gtk::IconTheme::for_display(&d).has_gicon(&gicon));
    if themed {
        let img = gtk::Image::from_gicon(&gicon);
        img.set_pixel_size(px);
        return img.upcast();
    }
    let short = match mime.split('/').next().unwrap_or("") {
        _ if mime == files::URL => "link",
        _ if mime == waystow_core::mime::DIRECTORY => "folder",
        "image" => "image",
        "video" => "video",
        "audio" => "audio",
        _ if mime.contains("zip") || mime.contains("tar") || mime.contains("compressed") => "archive",
        _ => "doc",
    };
    super::icons::glyph(short, px * 2 / 3)
}

/// A rounded thumbnail (or the type icon), exactly `size` px square.
pub fn thumb(path: Option<&Path>, mime: &str, size: i32) -> gtk::Widget {
    let stage = super::stage::Stage::default();
    stage.set_size(size, size);
    stage.add_css_class("ws-thumb");
    stage.set_overflow(gtk::Overflow::Hidden);
    stage.set_valign(gtk::Align::Center);
    stage.set_halign(gtk::Align::Center);
    let px = (size * 3 / 5).max(16);
    let icon = type_icon(mime, px);
    stage.add(&icon);
    stage.place(&icon, (size - px) / 2, (size - px) / 2, px, px);
    if let Some(p) = path.filter(|_| thumbs::supported(mime)) {
        let st = stage.clone();
        thumbs::load(p, mime, (size * 2).max(128), move |tex| {
            let Some(tex) = tex else { return };
            let pic = gtk::Picture::for_paintable(&tex);
            pic.set_content_fit(gtk::ContentFit::Cover);
            pic.set_can_shrink(true);
            icon.set_visible(false);
            st.add(&pic);
            st.place(&pic, 0, 0, size, size);
        });
    }
    stage.upcast()
}

/// Thumbnail for a shelf item.
pub fn item_thumb(item: &Item, size: i32) -> gtk::Widget {
    thumb(item.content.path(), &item.mime, size)
}

/// Up to three thumbnails fanned out, with a count badge. Items with a real
/// preview (images, videos, PDFs) are preferred; the newest ends up on top.
pub fn thumb_stack(items: &[Item], size: i32, badge: bool) -> gtk::Widget {
    let rank = |i: &&Item| match i.mime.split('/').next() {
        Some("image") => 0,
        Some("video") => 1,
        _ if i.mime == "application/pdf" => 2,
        _ => 3,
    };
    let mut previewable: Vec<&Item> = items.iter().filter(|i| super::thumbs::supported(&i.mime)).collect();
    previewable.sort_by_key(rank);
    let mut picks: Vec<&Item> = previewable.into_iter().take(3).collect();
    for i in items {
        if picks.len() >= 3 {
            break;
        }
        if !picks.iter().any(|p| p.id == i.id) {
            picks.push(i);
        }
    }
    let n = picks.len();
    let angles: &[f32] = match n {
        0 | 1 => &[0.0],
        2 => &[-9.0, 7.0],
        _ => &[-15.0, 12.0, 0.0],
    };
    let pad = size / 3;
    let (w, h) = (size + pad * 2, size + pad);
    let stage = super::stage::Stage::default();
    stage.set_size(w, h);
    // Back to front: the first pick (newest) is added last, so it's on top.
    for (k, item) in picks.iter().enumerate().rev() {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("ws-thumb-frame");
        card.set_overflow(gtk::Overflow::Hidden);
        card.append(&item_thumb(item, size));
        stage.add(&card);
        stage.place(&card, pad, pad / 2, size, size);
        let angle = angles.get(n - 1 - k).copied().unwrap_or(0.0);
        let c = size as f32 / 2.0;
        let t = gsk::Transform::new()
            .translate(&graphene::Point::new(c, c))
            .rotate(angle)
            .translate(&graphene::Point::new(-c, -c));
        stage.set_transform(&card, Some(t));
    }
    if badge && items.len() > 1 {
        let b = gtk::Label::new(Some(&items.len().to_string()));
        b.add_css_class("ws-badge");
        stage.add(&b);
        stage.place(&b, w - 30, h - 20, 26, 18);
    }
    stage.upcast()
}

/// A flat icon button.
pub fn icon_button(icon: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_child(Some(&super::icons::widget(icon, 16)));
    b.add_css_class("ws-icon");
    b.set_tooltip_text(Some(tip));
    b.set_valign(gtk::Align::Center);
    b
}

/// A label with a CSS class.
pub fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(class);
    l.set_xalign(0.0);
    l
}

/// Name and detail stacked; file names are ellipsized in the middle (to keep
/// the extension), other titles at the end.
pub fn two_lines(title: &str, sub: &str) -> gtk::Box {
    two_lines_with(title, sub, gtk::pango::EllipsizeMode::Middle)
}

/// [`two_lines`] with a chosen ellipsis.
pub fn two_lines_with(title: &str, sub: &str, mode: gtk::pango::EllipsizeMode) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 1);
    b.set_hexpand(true);
    b.set_valign(gtk::Align::Center);
    let t = label(title, "ws-title");
    t.set_ellipsize(mode);
    t.set_tooltip_text(Some(title));
    let s = label(sub, "ws-sub");
    s.set_ellipsize(gtk::pango::EllipsizeMode::End);
    b.append(&t);
    b.append(&s);
    b
}

/// The detail line for an item: size · type · time left.
pub fn item_detail(item: &Item, now: i64) -> String {
    let mut parts = Vec::new();
    match &item.content {
        Content::Text(t) => parts.push(format!("{} characters", t.chars().count())),
        Content::Url(u) => parts.push(u.split('/').nth(2).unwrap_or(u).to_owned()),
        _ => {
            if let Some(s) = item.size {
                parts.push(files::human_size(s));
            }
            parts.push(files::describe(&item.mime));
        }
    }
    if item.pinned {
        parts.push("pinned".into());
    } else if let Some(e) = item.expires_at {
        parts.push(files::left(e - now));
    }
    parts.join(" · ")
}

/// Is the item's file gone?
pub fn missing(item: &Item) -> bool {
    item.content.path().is_some_and(|p| !p.exists())
}
