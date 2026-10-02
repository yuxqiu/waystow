//! Clipboard I/O over ext-/wlr-data-control (no keyboard focus needed), a
//! watcher that reports what each copy offers, and the compositor's globals
//! for `waystow doctor`.

mod watch;

use std::io::Read;

use anyhow::{Context, bail};
use wl_clipboard_rs::copy::{self, MimeSource, Options};
use wl_clipboard_rs::paste::{self, ClipboardType, Error, MimeType, Seat};

pub use watch::watch;

/// MIME types on the clipboard right now (empty when it's empty).
///
/// # Errors
///
/// The compositor can't be reached or lacks data-control.
pub fn mime_types() -> anyhow::Result<Vec<String>> {
    match paste::get_mime_types(ClipboardType::Regular, Seat::Unspecified) {
        Ok(set) => Ok(set.into_iter().collect()),
        Err(Error::NoSeats | Error::ClipboardEmpty) => Ok(Vec::new()),
        Err(e) => Err(e).context("reading the Wayland clipboard (does the compositor support data-control?)"),
    }
}

/// Read the clipboard as `mime`, refusing anything over `cap` bytes.
/// `Ok(None)` when the clipboard is empty or lacks that type.
///
/// # Errors
///
/// The compositor can't be reached, or the content is over the cap.
pub fn read(mime: &str, cap: u64) -> anyhow::Result<Option<Vec<u8>>> {
    match paste::get_contents(ClipboardType::Regular, Seat::Unspecified, MimeType::Specific(mime)) {
        Ok((pipe, _)) => read_capped(pipe, cap).map(Some),
        Err(Error::NoSeats | Error::ClipboardEmpty | Error::NoMimeType) => Ok(None),
        Err(e) => Err(e).context("reading the Wayland clipboard"),
    }
}

fn read_capped(pipe: impl Read, cap: u64) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    pipe.take(cap + 1).read_to_end(&mut buf).context("reading the clipboard")?;
    if buf.len() as u64 > cap {
        bail!("the clipboard holds more than the {} MiB limit", cap >> 20);
    }
    Ok(buf)
}

/// Put several representations on the clipboard at once. A background
/// thread serves them until something else is copied, so the daemon must stay
/// alive.
///
/// # Errors
///
/// The compositor can't be reached or lacks data-control.
pub fn copy(sources: Vec<(String, Vec<u8>)>) -> anyhow::Result<()> {
    let sources = sources
        .into_iter()
        .map(|(mime, bytes)| MimeSource {
            source: copy::Source::Bytes(bytes.into_boxed_slice()),
            mime_type: copy::MimeType::Specific(mime),
        })
        .collect();
    Options::new().copy_multi(sources).context("setting the Wayland clipboard")
}

/// Put text on the clipboard.
///
/// # Errors
///
/// As [`copy`].
pub fn copy_text(text: &str) -> anyhow::Result<()> {
    Options::new()
        .copy(copy::Source::Bytes(text.as_bytes().into()), copy::MimeType::Text)
        .context("setting the Wayland clipboard")
}

/// Interface names (with versions) the compositor advertises.
///
/// # Errors
///
/// No Wayland connection.
pub fn globals() -> anyhow::Result<Vec<(String, u32)>> {
    use wayland_client::Connection;
    use wayland_client::globals::registry_queue_init;
    let conn = Connection::connect_to_env().context("connecting to Wayland")?;
    let (globals, _queue) = registry_queue_init::<watch::Ignore>(&conn)?;
    Ok(globals.contents().clone_list().into_iter().map(|g| (g.interface, g.version)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_refuses_instead_of_truncating() {
        let exact = vec![1u8; 1 << 20];
        assert_eq!(read_capped(exact.as_slice(), 1 << 20).unwrap().len(), 1 << 20);
        let e = read_capped(vec![0u8; (1 << 20) + 1].as_slice(), 1 << 20).unwrap_err();
        assert!(e.to_string().contains("1 MiB limit"), "{e}");
    }
}
