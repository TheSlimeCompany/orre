//! The socket: a thread accepts connections and reads one line from each;
//! the UI thread does the command ([`super::act`]) and the thread writes the
//! reply and closes the connection.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use gpui_kit::{AnyWindowHandle, App, Entity};

use crate::shell::ShellView;

use super::{Command, act, guard};

/// How often the UI thread looks for a command. Dev builds only.
const POLL: Duration = Duration::from_millis(50);

/// How long a connection has to send its line.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// A command line and where its reply goes.
type Request = (String, mpsc::Sender<String>);

/// Starts the driver for `shell` in `window`, unless [`guard`] refuses or
/// the socket can't be opened; either way the window carries on, and the
/// log says why.
pub fn start(window: AnyWindowHandle, shell: Entity<ShellView>, cx: &mut App) {
    let path = match guard::socket_path() {
        Ok(path) => path,
        Err(refusal) => {
            tracing::warn!(%refusal, "the dev driver isn't listening");
            return;
        }
    };
    let listener = match listen(&path) {
        Ok(listener) => listener,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "the dev driver isn't listening");
            return;
        }
    };
    tracing::info!(path = %path.display(), "the dev driver is listening");
    let (requests, received) = mpsc::channel::<Request>();
    std::thread::spawn(move || accept(&listener, &requests));
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(POLL).await;
            while let Ok((line, reply)) = received.try_recv() {
                let answer = match line.parse::<Command>() {
                    Err(unparsed) => Err(unparsed.to_string()),
                    Ok(command) => cx
                        .update(|cx| {
                            window.update(cx, |_, window, cx| act(&command, &shell, window, cx))
                        })
                        .map_err(|error| error.to_string())
                        .and_then(|done| done),
                };
                let _ = reply.send(match answer {
                    Ok(answer) => answer,
                    Err(error) => format!("error: {error}"),
                });
            }
            if cx.update(|cx| window.update(cx, |_, _, _| ())).is_err() {
                // The window closed.
                break;
            }
        }
    })
    .detach();
}

/// Binds `path`, readable and writable by the user only. A stale socket
/// left by a crashed run is replaced; a live one (another Orre driving this
/// data dir) is refused.
fn listen(path: &Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_socket()) {
        if UnixStream::connect(path).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "another Orre is driving this data dir",
            ));
        }
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Reads one line from each connection, hands it to the UI thread, and
/// writes back the reply.
fn accept(listener: &UnixListener, requests: &mpsc::Sender<Request>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        // A client that never sends its line can't hold up the next one.
        if stream.set_read_timeout(Some(READ_TIMEOUT)).is_err() {
            continue;
        }
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() {
            continue;
        }
        let (reply, answer) = mpsc::channel();
        if requests.send((line, reply)).is_err() {
            // The UI thread stopped: the window closed.
            return;
        }
        let answer = answer
            .recv()
            .unwrap_or_else(|_| "error: the window closed".to_owned());
        let mut stream = &stream;
        let _ = writeln!(stream, "{answer}");
    }
}
