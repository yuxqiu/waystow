//! `waystow doctor`: what works here and what's missing.

use std::fmt::Write as _;

use gtk::gio;
use gtk::prelude::*;
use waystow_core::action;
use waystow_core::config::Config;
use waystow_core::paths;

fn line(ok: bool, what: &str, detail: &str) {
    let mark = if ok { "✓" } else { "✗" };
    if detail.is_empty() {
        println!("  {mark} {what}");
    } else {
        println!("  {mark} {what} — {detail}");
    }
}

pub fn run() -> anyhow::Result<()> {
    let mut problems = 0;
    println!("Session");
    let wayland = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    line(
        !wayland.is_empty(),
        "Wayland session",
        if wayland.is_empty() { "WAYLAND_DISPLAY is not set" } else { &wayland },
    );
    match waystow_wl::globals() {
        Ok(globals) => {
            let has = |name: &str| globals.iter().any(|(g, _)| g == name);
            let checks = [
                ("zwlr_layer_shell_v1", "layer shell (required: the shelf and island)", true),
                ("wl_data_device_manager", "drag and drop (required)", true),
                ("ext_data_control_manager_v1", "clipboard access (ext-data-control)", false),
                ("zwlr_data_control_manager_v1", "clipboard access (wlr-data-control, fallback)", false),
                ("ext_background_effect_manager_v1", "background blur (optional, needs GTK 4.24)", false),
            ];
            for (name, what, required) in checks {
                let ok = has(name);
                if !ok && required {
                    problems += 1;
                }
                line(ok, what, if ok { "" } else { "not offered by this compositor" });
            }
            if !has("ext_data_control_manager_v1") && !has("zwlr_data_control_manager_v1") {
                println!("    Clipboard features (copy actions, \"Shelve clipboard\", Copied in Recent) won't work.");
            }
        }
        Err(e) => {
            problems += 1;
            line(false, "Wayland connection", &format!("{e:#}"));
        }
    }
    let (maj, min, mic) = (gtk::major_version(), gtk::minor_version(), gtk::micro_version());
    let gtk_detail = if min >= 24 {
        format!("{maj}.{min}.{mic}")
    } else {
        format!("{maj}.{min}.{mic} (blur needs 4.24; until then use a compositor rule, see the README)")
    };
    line(true, "GTK", &gtk_detail);
    let portal = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok().and_then(|bus| {
        bus.call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            Some(&("org.freedesktop.portal.Documents",).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            1000,
            gio::Cancellable::NONE,
        )
        .ok()
        .and_then(|v| v.child_value(0).get::<bool>())
    });
    line(
        portal == Some(true),
        "Document portal",
        if portal == Some(true) {
            "files dragged out reach Flatpak apps"
        } else {
            "not running: Flatpak apps may not open dragged files"
        },
    );

    println!("\nConfiguration");
    let config = match Config::load() {
        Ok(c) => {
            line(true, &Config::path().display().to_string(), "valid");
            c
        }
        Err(e) => {
            problems += 1;
            line(false, &Config::path().display().to_string(), &format!("{e:#}"));
            Config::default()
        }
    };
    let actions_dir = paths::config_dir().join("actions");
    match action::load(&actions_dir) {
        Ok(actions) => {
            let mut missing: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
            for a in &actions {
                for r in a.requires.iter().filter(|r| !action::on_path(r)) {
                    missing.entry(r.clone()).or_default().push(a.title.clone());
                }
            }
            let available = actions.iter().filter(|a| a.requires.iter().all(|r| action::on_path(r))).count();
            line(true, "Actions", &format!("{available} of {} available", actions.len()));
            for (tool, titles) in missing {
                let mut s = String::new();
                let _ = write!(s, "install `{tool}` for: {}", titles.join(", "));
                line(false, &tool, &s);
            }
        }
        Err(e) => {
            problems += 1;
            line(false, "Actions", &format!("{e:#}"));
        }
    }

    println!("\nFolders");
    let downloads = paths::user_dir("XDG_DOWNLOAD_DIR", "Downloads");
    line(downloads.is_dir(), "Downloads", &downloads.display().to_string());
    for d in config.recent.screenshot_dirs() {
        line(d.is_dir(), "Screenshots", &d.display().to_string());
    }
    for d in config.recent.watch_dirs() {
        line(d.is_dir(), "Watched", &d.display().to_string());
    }

    println!("\nDaemon");
    let sock = paths::socket();
    let running = std::os::unix::net::UnixStream::connect(&sock).is_ok();
    line(
        running,
        "waystow daemon",
        if running { "running" } else { "not running: `systemctl --user start waystow` or `waystow daemon`" },
    );

    if problems > 0 {
        anyhow::bail!("{problems} problem(s) found");
    }
    Ok(())
}
