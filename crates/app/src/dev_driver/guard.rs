//! Where the driver listens, and when it refuses to: only against a scratch
//! data dir named by `ORRE_DATA_DIR`, never the platform data dir that
//! holds the user's real database.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use orre_core::paths::{self, DATA_DIR_ENV};

/// The socket's file name, in the data dir.
pub const SOCKET_FILE: &str = "dev-driver.sock";

/// The longest socket path the platform takes (`sun_path`, less its NUL).
#[cfg(target_os = "linux")]
const MAX_SOCKET_PATH: usize = 107;
#[cfg(not(target_os = "linux"))]
const MAX_SOCKET_PATH: usize = 103;

/// Why the driver won't listen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `ORRE_DATA_DIR` isn't set: the data dir is the platform one.
    NoScratchDir,
    /// `ORRE_DATA_DIR` names the platform data dir.
    PlatformDir(PathBuf),
    /// The socket's path is too long for a Unix socket.
    TooLong(PathBuf),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NoScratchDir => write!(
                f,
                "the dev driver needs {DATA_DIR_ENV} set to a scratch data dir; it never \
                 drives the real one"
            ),
            Refusal::PlatformDir(dir) => write!(
                f,
                "{DATA_DIR_ENV} is the platform data dir ({}); the dev driver never drives \
                 the real one",
                dir.display()
            ),
            Refusal::TooLong(path) => write!(
                f,
                "the socket path {} is longer than a Unix socket allows ({MAX_SOCKET_PATH} \
                 bytes); choose a shorter {DATA_DIR_ENV}",
                path.display()
            ),
        }
    }
}

/// The socket's path, for the data dir `ORRE_DATA_DIR` names, unless the
/// driver must refuse it.
pub fn socket_path() -> Result<PathBuf, Refusal> {
    check(
        std::env::var_os(DATA_DIR_ENV),
        paths::platform_data_dir().ok(),
    )
}

/// [`socket_path`] for `scratch` (the variable's value) and the platform
/// data dir `platform`.
pub fn check(scratch: Option<OsString>, platform: Option<PathBuf>) -> Result<PathBuf, Refusal> {
    let dir = match scratch {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => return Err(Refusal::NoScratchDir),
    };
    if let Some(platform) = platform
        && same_dir(&dir, &platform)
    {
        return Err(Refusal::PlatformDir(dir));
    }
    let socket = dir.join(SOCKET_FILE);
    if socket.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(Refusal::TooLong(socket));
    }
    Ok(socket)
}

/// Whether `a` and `b` are the same directory: through symlinks and `..`
/// where both exist, else as written.
fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.components().eq(b.components()),
    }
}
