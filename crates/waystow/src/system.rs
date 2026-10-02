//! Desktop integration over GIO and D-Bus: open, reveal, Trash (and restore),
//! notifications, and the portal's colour scheme and accent colour.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use gtk::gio;
use gtk::glib::Variant;
use gtk::prelude::*;
use waystow_core::uri;

fn session() -> Option<gio::DBusConnection> {
    gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()
}

/// Open with the default app.
pub fn open(target: &str) -> anyhow::Result<()> {
    gio::AppInfo::launch_default_for_uri(target, gio::AppLaunchContext::NONE)
        .with_context(|| format!("no app opens {target}"))
}

/// Show files selected in the file manager (`FileManager1`), else open their folder.
pub fn reveal(paths: &[PathBuf]) {
    let uris: Vec<String> = paths.iter().map(|p| uri::from_path(p)).collect();
    let fallback = paths.first().and_then(|p| p.parent()).map(Path::to_path_buf);
    let Some(bus) = session() else {
        if let Some(dir) = fallback {
            let _ = open(&uri::from_path(&dir));
        }
        return;
    };
    bus.call(
        Some("org.freedesktop.FileManager1"),
        "/org/freedesktop/FileManager1",
        "org.freedesktop.FileManager1",
        "ShowItems",
        Some(&(uris, String::new()).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        3000,
        gio::Cancellable::NONE,
        move |r| {
            if r.is_err()
                && let Some(dir) = fallback
            {
                let _ = open(&uri::from_path(&dir));
            }
        },
    );
}

/// Move to the Trash.
pub fn trash(path: &Path) -> anyhow::Result<()> {
    gio::File::for_path(path)
        .trash(gio::Cancellable::NONE)
        .with_context(|| format!("moving {} to the Trash", path.display()))
}

/// Move the most recently trashed file that came from `original` back.
pub fn restore(original: &Path) -> anyhow::Result<()> {
    if original.exists() {
        bail!("{} exists again; not overwriting it", original.display());
    }
    let trash = gio::File::for_uri("trash:///");
    let children = trash
        .enumerate_children(
            "standard::name,trash::orig-path,trash::deletion-date",
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        )
        .context("reading the Trash")?;
    let mut best: Option<(gio::FileInfo, String)> = None;
    while let Some(info) = children.next_file(gio::Cancellable::NONE)? {
        let orig = info.attribute_byte_string("trash::orig-path").map(|s| PathBuf::from(s.as_str()));
        if orig.as_deref() != Some(original) {
            continue;
        }
        let date = info.attribute_string("trash::deletion-date").map(|s| s.to_string()).unwrap_or_default();
        if best.as_ref().is_none_or(|(_, d)| date > *d) {
            best = Some((info, date));
        }
    }
    let (info, _) = best.with_context(|| format!("{} isn't in the Trash", original.display()))?;
    let item = trash.child(info.name());
    item.move_(&gio::File::for_path(original), gio::FileCopyFlags::NOFOLLOW_SYMLINKS, gio::Cancellable::NONE, None)
        .with_context(|| format!("restoring {}", original.display()))
}

/// A desktop notification, optionally with a progress value (0–100).
pub fn notify(summary: &str, body: &str, progress: Option<u32>) {
    let Some(bus) = session() else { return };
    let mut hints: HashMap<String, Variant> = HashMap::new();
    if let Some(p) = progress {
        hints.insert("value".into(), (p.min(100) as i32).to_variant());
    }
    hints.insert("desktop-entry".into(), "waystow".to_variant());
    let params =
        ("WayStow", 0u32, "ws-tray-symbolic", summary, body, Vec::<String>::new(), hints, 5000i32).to_variant();
    bus.call(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "Notify",
        Some(&params),
        None,
        gio::DBusCallFlags::NONE,
        3000,
        gio::Cancellable::NONE,
        |_| {},
    );
}

/// Light or dark, from the portal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// No preference or unknown.
    Default,
    /// Prefers dark.
    Dark,
    /// Prefers light.
    Light,
}

/// What the desktop says about appearance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Appearance {
    /// Colour scheme.
    pub scheme: Scheme,
    /// Accent colour, 0–1 RGB.
    pub accent: Option<(f64, f64, f64)>,
}

fn read_setting(bus: &gio::DBusConnection, key: &str) -> Option<Variant> {
    let call = |method: &str| {
        bus.call_sync(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            method,
            Some(&("org.freedesktop.appearance", key).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            1000,
            gio::Cancellable::NONE,
        )
        .ok()
    };
    let reply = call("ReadOne").or_else(|| call("Read"))?;
    // `Read` wraps the value twice.
    Some(unbox(reply.child_value(0)))
}

/// Strip `v` (variant) wrappers.
fn unbox(mut v: Variant) -> Variant {
    while v.is_type(gtk::glib::VariantTy::VARIANT) {
        match v.as_variant() {
            Some(inner) => v = inner,
            None => break,
        }
    }
    v
}

fn parse_scheme(v: &Variant) -> Scheme {
    match v.get::<u32>() {
        Some(1) => Scheme::Dark,
        Some(2) => Scheme::Light,
        _ => Scheme::Default,
    }
}

fn parse_accent(v: &Variant) -> Option<(f64, f64, f64)> {
    let (r, g, b) = v.get::<(f64, f64, f64)>()?;
    [r, g, b].iter().all(|c| (0.0..=1.0).contains(c)).then_some((r, g, b))
}

/// Read the appearance now and call `changed` whenever it changes (while
/// the returned subscription lives).
pub fn watch_appearance(changed: impl Fn(Appearance) + 'static) -> (Appearance, Option<gio::SignalSubscription>) {
    let Some(bus) = session() else { return (Appearance { scheme: Scheme::Default, accent: None }, None) };
    let current = Appearance {
        scheme: read_setting(&bus, "color-scheme").map_or(Scheme::Default, |v| parse_scheme(&v)),
        accent: read_setting(&bus, "accent-color").and_then(|v| parse_accent(&v)),
    };
    let state = std::cell::Cell::new(current);
    let sub = bus.subscribe_to_signal(
        Some("org.freedesktop.portal.Desktop"),
        Some("org.freedesktop.portal.Settings"),
        Some("SettingChanged"),
        Some("/org/freedesktop/portal/desktop"),
        Some("org.freedesktop.appearance"),
        gio::DBusSignalFlags::NONE,
        move |sig| {
            let params = sig.parameters;
            let key = params.child_value(1).get::<String>().unwrap_or_default();
            let value = unbox(params.child_value(2));
            let mut a = state.get();
            match key.as_str() {
                "color-scheme" => a.scheme = parse_scheme(&value),
                "accent-color" => a.accent = parse_accent(&value),
                _ => return,
            }
            state.set(a);
            changed(a);
        },
    );
    (current, Some(sub))
}
