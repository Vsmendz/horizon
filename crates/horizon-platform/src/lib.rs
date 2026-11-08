//! Safe platform services, independent of windows, graphics and the game loop.
use std::{io, path::Path};

#[cfg(any(target_os = "horizon", all(unix, test)))]
mod newlib;

/// Usable bytes on the filesystem containing `path`. Failures stay visible to
/// the caller, which decides whether a failed estimate should prevent saving.
pub fn available_space(path: &Path) -> io::Result<u64> {
    #[cfg(target_os = "horizon")]
    {
        newlib::available_space(path)
    }
    #[cfg(not(target_os = "horizon"))]
    {
        fs4::available_space(path)
    }
}
