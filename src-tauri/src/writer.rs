//! Writing target files, elevating only the write step when Windows denies access.
//!
//! The elevated path re-runs this exe as `--write-elevated <job.json>` via ShellExecute "runas",
//! so the user sees one UAC prompt and the UI itself never runs as admin.

use crate::fsops;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub type FileWrite = (PathBuf, Vec<u8>);

/// Only these file names may be written by the elevated helper.
const ALLOWED_NAMES: [&str; 3] = ["gameinfo.gi", "video.txt", "autoexec.cfg"];

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteOutcome {
    pub readonly_cleared: Vec<String>,
    pub elevated: bool,
}

/// Writes files in order. On the first access-denied error, that file and every remaining one
/// go to `elevate` together, so a declined UAC prompt leaves those untouched.
pub fn write_files(files: &[FileWrite], elevate: &dyn Fn(&[FileWrite]) -> Result<(), String>) -> Result<WriteOutcome, String> {
    let mut outcome = WriteOutcome::default();
    for (i, (target, bytes)) in files.iter().enumerate() {
        let attempt = fsops::clear_readonly(target).and_then(|was_ro| {
            if was_ro {
                outcome.readonly_cleared.push(target.to_string_lossy().into_owned());
            }
            fsops::atomic_write(target, bytes)
        });
        match attempt {
            Ok(()) => {}
            Err(e) if fsops::is_access_denied(&e) => {
                elevate(&files[i..])?;
                outcome.elevated = true;
                return Ok(outcome);
            }
            Err(e) => return Err(format!("Could not write {}: {e}", target.display())),
        }
    }
    Ok(outcome)
}

pub fn write_without_elevation(files: &[FileWrite]) -> Result<WriteOutcome, String> {
    write_files(files, &|_| Err("Access denied and elevation is not available here.".into()))
}

#[derive(Serialize, Deserialize)]
struct Job {
    files: Vec<JobFile>,
}

#[derive(Serialize, Deserialize)]
struct JobFile {
    target: String,
    staged: String,
}

#[derive(Serialize, Deserialize)]
struct JobResult {
    ok: bool,
    error: Option<String>,
}

/// Stages the content, runs the elevated helper and waits for its result.
pub fn elevate_via_runas(job_root: &Path, files: &[FileWrite]) -> Result<(), String> {
    let dir = job_root.join(format!("job-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| format!("Could not prepare elevated write: {e}"))?;
    let result = (|| {
        let staged: Vec<JobFile> = files
            .iter()
            .enumerate()
            .map(|(i, (target, bytes))| {
                let path = dir.join(format!("{i}.bin"));
                fs::write(&path, bytes).map_err(|e| e.to_string())?;
                Ok(JobFile { target: target.to_string_lossy().into_owned(), staged: path.to_string_lossy().into_owned() })
            })
            .collect::<Result<_, String>>()?;
        let job_path = dir.join("job.json");
        fs::write(&job_path, serde_json::to_vec(&Job { files: staged }).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        run_elevated(&job_path)?;
        let text = fs::read_to_string(dir.join("result.json"))
            .map_err(|_| "The elevated write did not report back; nothing was confirmed written.".to_string())?;
        let res: JobResult = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if res.ok {
            Ok(())
        } else {
            Err(res.error.unwrap_or_else(|| "Elevated write failed.".into()))
        }
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

#[cfg(windows)]
fn run_elevated(job_path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED};
    use windows_sys::Win32::System::Threading::{WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_w = wide(exe.as_os_str());
    let verb_w = wide("runas".as_ref());
    let params = format!("--write-elevated \"{}\"", job_path.display());
    let params_w = wide(params.as_ref());

    // SAFETY: all string pointers stay alive for the duration of the call; the struct is
    // zero-initialised with cbSize set, and the returned process handle is closed below.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb_w.as_ptr();
        info.lpFile = exe_w.as_ptr();
        info.lpParameters = params_w.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 {
            return Err(if GetLastError() == ERROR_CANCELLED {
                "Administrator permission was declined, so nothing was written.".into()
            } else {
                "Could not start the elevated writer.".into()
            });
        }
        if !info.hProcess.is_null() {
            WaitForSingleObject(info.hProcess, INFINITE);
            CloseHandle(info.hProcess);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn run_elevated(_job_path: &Path) -> Result<(), String> {
    Err("Elevation is only supported on Windows.".into())
}

/// Entry point of the elevated helper process. Returns the process exit code.
pub fn run_job_file(job_path: &Path) -> i32 {
    let result = execute_job(job_path);
    let report = JobResult { ok: result.is_ok(), error: result.err() };
    let out = job_path.with_file_name("result.json");
    let written = serde_json::to_vec(&report).ok().and_then(|b| fs::write(out, b).ok());
    match (report.ok, written) {
        (true, Some(())) => 0,
        _ => 1,
    }
}

fn execute_job(job_path: &Path) -> Result<(), String> {
    let text = fs::read_to_string(job_path).map_err(|e| format!("Cannot read job: {e}"))?;
    let job: Job = serde_json::from_str(&text).map_err(|e| format!("Bad job file: {e}"))?;
    let job_dir = job_path.parent().ok_or("Bad job path.")?;
    let files: Vec<FileWrite> = job
        .files
        .iter()
        .map(|f| {
            let target = PathBuf::from(&f.target);
            let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !target.is_absolute() || !ALLOWED_NAMES.iter().any(|a| a.eq_ignore_ascii_case(name)) {
                return Err(format!("Refusing to write {}", target.display()));
            }
            // Content only comes from files staged next to the job, so an edited job can't make the
            // admin process copy some other file.
            let staged = PathBuf::from(&f.staged);
            if staged.parent() != Some(job_dir) {
                return Err(format!("Refusing to read {}", staged.display()));
            }
            fs::read(&staged).map(|b| (target, b)).map_err(|e| e.to_string())
        })
        .collect::<Result<_, _>>()?;
    write_without_elevation(&files).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn writes_all_files_without_elevation() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("gameinfo.gi");
        let b = dir.path().join("video.txt");
        let out = write_without_elevation(&[(a.clone(), b"A".to_vec()), (b.clone(), b"B".to_vec())]).unwrap();
        assert!(!out.elevated);
        assert_eq!(fs::read(a).unwrap(), b"A");
        assert_eq!(fs::read(b).unwrap(), b"B");
    }

    #[test]
    fn job_file_refuses_unexpected_targets() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("0.bin");
        fs::write(&staged, "x").unwrap();
        let job = Job {
            files: vec![JobFile {
                target: dir.path().join("evil.dll").to_string_lossy().into_owned(),
                staged: staged.to_string_lossy().into_owned(),
            }],
        };
        let job_path = dir.path().join("job.json");
        fs::write(&job_path, serde_json::to_vec(&job).unwrap()).unwrap();
        assert_eq!(run_job_file(&job_path), 1);
        assert!(!dir.path().join("evil.dll").exists());
        let res: JobResult = serde_json::from_str(&fs::read_to_string(dir.path().join("result.json")).unwrap()).unwrap();
        assert!(res.error.unwrap().contains("Refusing"));
    }

    #[test]
    fn job_file_refuses_content_staged_elsewhere() {
        let dir = tempfile::tempdir().unwrap();
        let job_dir = dir.path().join("job-1");
        fs::create_dir(&job_dir).unwrap();
        let secret = dir.path().join("secret.txt");
        fs::write(&secret, "x").unwrap();
        let target = dir.path().join("video.txt");
        let job = Job { files: vec![JobFile { target: target.to_string_lossy().into_owned(), staged: secret.to_string_lossy().into_owned() }] };
        let job_path = job_dir.join("job.json");
        fs::write(&job_path, serde_json::to_vec(&job).unwrap()).unwrap();
        assert_eq!(run_job_file(&job_path), 1);
        assert!(!target.exists());
    }

    #[test]
    fn denied_write_hands_it_and_remaining_files_to_elevation() {
        let dir = tempfile::tempdir().unwrap();
        // Renaming onto a directory fails with ERROR_ACCESS_DENIED on Windows.
        let gi = dir.path().join("gameinfo.gi");
        fs::create_dir(&gi).unwrap();
        let video = dir.path().join("video.txt");
        let handed = RefCell::new(Vec::new());
        let elevate = |files: &[FileWrite]| {
            handed.borrow_mut().extend(files.iter().map(|(p, _)| p.clone()));
            Ok(())
        };
        let out = write_files(&[(gi.clone(), b"g".to_vec()), (video.clone(), b"v".to_vec())], &elevate).unwrap();
        assert!(out.elevated);
        assert_eq!(*handed.borrow(), vec![gi, video.clone()]);
        assert!(!video.exists(), "files after the denied one must wait for elevation");

        let declined = write_files(&[(dir.path().join("gameinfo.gi"), b"g".to_vec())], &|_| Err("declined".into()));
        assert_eq!(declined.unwrap_err(), "declined");
    }
}
