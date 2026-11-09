use std::{ffi::CString, io, mem::MaybeUninit, os::unix::ffi::OsStrExt, path::Path};

// libc's unsigned fields differ in width between 32-bit and 64-bit hosts.
#[allow(clippy::unnecessary_cast)]
pub(super) fn available_space(path: &Path) -> io::Result<u64> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let mut stats = MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: path is NUL-terminated and stats points to writable storage of the
    // SDK's statvfs type. The Switch layout is checked against cross-compiled C
    // by ferramentas/switch-tests/runtime-audit/abi_assertions.rs.
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful statvfs initialized the structure.
    let stats = unsafe { stats.assume_init() };
    usable_bytes(stats.f_bavail as u64, stats.f_frsize as u64)
}

fn usable_bytes(blocks: u64, fragment_size: u64) -> io::Result<u64> {
    blocks
        .checked_mul(fragment_size)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "filesystem size overflow"))
}

#[cfg(target_os = "horizon")]
#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), core::num::NonZeroI32> {
    let ret = unsafe { libc::getrandom(dest.cast(), len, 0) };
    if ret >= 0 && (ret as usize) == len {
        Ok(())
    } else {
        Err(core::num::NonZeroI32::new((1 << 16) + 2).unwrap())
    }
}

#[cfg(target_os = "horizon")]
#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn __getrandom_custom(dest: *mut u8, len: usize) -> u32 {
    let ret = unsafe { libc::getrandom(dest.cast(), len, 0) };
    if ret >= 0 && (ret as usize) == len {
        0
    } else {
        (1 << 31) + 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usable_space_keeps_full_width_and_checks_overflow() {
        assert_eq!(usable_bytes(1_048_576, 4096).unwrap(), 4_294_967_296);
        assert_eq!(usable_bytes(0, 4096).unwrap(), 0);
        assert_eq!(
            usable_bytes(u64::MAX, 2).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn filesystem_query_reports_missing_paths_and_invalid_names() {
        let root = std::env::temp_dir();
        assert!(available_space(&root).is_ok());
        let missing = root.join(format!("horizon-space-absent-{}", std::process::id()));
        assert!(!missing.exists());
        assert!(available_space(&missing).is_err());
        assert_eq!(
            available_space(Path::new("bad\0path")).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
