//! `orre dev`: sends one command to the running app's driver and returns
//! its reply.

use std::io::{Read as _, Write as _};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;

use anyhow::{Context as _, bail};

use super::guard;

/// Sends `words` (a command and its arguments) to the driver and returns
/// the reply; a reply the driver marks as an error is an `Err`.
pub fn send(words: &[String]) -> anyhow::Result<String> {
    let path = guard::socket_path().map_err(|refusal| anyhow::anyhow!("{refusal}"))?;
    let mut stream = UnixStream::connect(&path).with_context(|| {
        format!(
            "no dev driver at {}: start a dev build of the app (cargo run --features \
             dev-driver) with the same ORRE_DATA_DIR",
            path.display()
        )
    })?;
    writeln!(stream, "{}", words.join(" "))?;
    stream.shutdown(Shutdown::Write)?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    let reply = reply.trim_end();
    match reply.strip_prefix("error: ") {
        Some(error) => bail!("{error}"),
        None => Ok(reply.to_owned()),
    }
}
