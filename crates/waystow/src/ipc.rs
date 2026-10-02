//! Unix-socket IPC: the CLI sends one JSON line, the daemon answers with one.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, bail};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;
use waystow_core::ipc::{Request, Response};
use waystow_core::paths;

/// Largest request line we accept (text added with `add --text` included).
const MAX_LINE: u64 = 80 << 20;

pub async fn send(req: &Request) -> anyhow::Result<Response> {
    let path = paths::socket();
    let stream = UnixStream::connect(&path).await.with_context(|| {
        format!(
            "the daemon isn't running ({}).\nStart it with `systemctl --user start waystow` or `waystow daemon`",
            path.display()
        )
    })?;
    let (r, mut w) = stream.into_split();
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    w.write_all(line.as_bytes()).await?;
    let mut resp = String::new();
    BufReader::new(r).read_line(&mut resp).await?;
    if resp.is_empty() {
        bail!("the daemon closed the connection without answering");
    }
    Ok(serde_json::from_str(&resp)?)
}

/// A request plus the way to answer it.
pub type Incoming = (Request, oneshot::Sender<Response>);

/// Bind the socket (before the UI starts, so a second daemon fails fast),
/// replacing a stale one but refusing to steal a live daemon's.
pub fn bind() -> anyhow::Result<std::os::unix::net::UnixListener> {
    bind_at(&paths::socket())
}

fn bind_at(path: &Path) -> anyhow::Result<std::os::unix::net::UnixListener> {
    if std::os::unix::net::UnixStream::connect(path).is_ok() {
        bail!("another waystow daemon is already running ({})", path.display());
    }
    let _ = std::fs::remove_file(path);
    let l = std::os::unix::net::UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
    // `XDG_RUNTIME_DIR` is normally private, but the fallback socket lives in /tmp.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    l.set_nonblocking(true)?;
    Ok(l)
}

/// Accept requests on the tokio runtime and hand them to `tx`.
pub fn serve(
    rt: &tokio::runtime::Handle,
    listener: std::os::unix::net::UnixListener,
    tx: async_channel::Sender<Incoming>,
) {
    rt.spawn(async move {
        let listener = match UnixListener::from_std(listener) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("waystow: ipc: {e}");
                return;
            }
        };
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Err(e) = handle(stream, tx).await {
                    eprintln!("waystow: ipc: {e:#}");
                }
            });
        }
    });
}

async fn handle(stream: UnixStream, tx: async_channel::Sender<Incoming>) -> anyhow::Result<()> {
    use tokio::io::AsyncReadExt;
    let (r, mut w) = stream.into_split();
    let mut line = String::new();
    BufReader::new(r.take(MAX_LINE)).read_line(&mut line).await?;
    let resp = match serde_json::from_str::<Request>(&line) {
        Ok(req) => {
            let (otx, orx) = oneshot::channel();
            tx.send((req, otx)).await?;
            orx.await.unwrap_or_else(|_| Response::Error { message: "the daemon dropped the request".into() })
        }
        Err(e) => Response::Error { message: format!("bad request: {e}") },
    };
    let mut out = serde_json::to_string(&resp)?;
    out.push('\n');
    w.write_all(out.as_bytes()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_is_private_and_a_second_daemon_cannot_take_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("waystow.sock");
        let _listener = bind_at(&path).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(bind_at(&path).unwrap_err().to_string().contains("already running"));
    }
}
