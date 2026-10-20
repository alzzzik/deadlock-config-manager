//! Low-level file and process helpers.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn tmp_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    target.with_file_name(name)
}

/// Clears the read-only attribute. Returns true if it was set.
pub fn clear_readonly(path: &Path) -> io::Result<bool> {
    let Ok(meta) = fs::metadata(path) else {
        return Ok(false);
    };
    let mut perms = meta.permissions();
    if !perms.readonly() {
        return Ok(false);
    }
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(path, perms)?;
    Ok(true)
}

/// Writes `<file>.tmp` next to the target, flushes it to disk, then renames it over the target.
/// Creates the parent folder if needed (a new account's `local\cfg` may not exist yet).
pub fn atomic_write(target: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let tmp = tmp_path(target);
    let result = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn is_access_denied(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(5)
}

#[cfg(windows)]
pub fn is_process_running(exe_name: &str) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    // SAFETY: plain Win32 calls; the snapshot handle is checked and closed, and the entry
    // struct is zero-initialised with dwSize set as the API requires.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(exe_name) {
                found = true;
                break;
            }
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
        found
    }
}

#[cfg(not(windows))]
pub fn is_process_running(_exe_name: &str) -> bool {
    false
}

pub fn deadlock_running() -> bool {
    is_process_running("deadlock.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_readonly_target_and_leaves_no_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("gameinfo.gi");
        fs::write(&target, "old").unwrap();
        let mut perms = fs::metadata(&target).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&target, perms).unwrap();

        assert!(clear_readonly(&target).unwrap());
        atomic_write(&target, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(!tmp_path(&target).exists());
        assert!(!clear_readonly(&target).unwrap());
    }
}
