use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

pub fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing state directory"))?;
    let (temp, mut file) = loop {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".rustcatch-write-{}-{id}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    let result = result
        .and_then(|_| replace_file(&temp, path))
        .and_then(|_| sync_dir(parent));
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

/// Publishes on the destination volume without replacing an existing name.
pub fn publish_new(source: &Path, target: &Path) -> io::Result<()> {
    #[cfg(windows)]
    move_file(source, target, false)?;
    #[cfg(unix)]
    fs::hard_link(source, target)?;
    sync_dir(target.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "missing destination directory")
    })?)
}

#[cfg(unix)]
pub fn sync_dir(path: &Path) -> io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(windows)]
pub fn sync_dir(_path: &Path) -> io::Result<()> {
    // MoveFileExW requests write-through for both publishing and replacing records.
    Ok(())
}

#[cfg(unix)]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    fs::rename(source, target)
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    move_file(source, target, true)
}

#[cfg(windows)]
fn move_file(source: &Path, target: &Path, replace: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let wide = |path: &Path| -> Vec<u16> {
        let p = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        p.as_os_str().encode_wide().chain([0]).collect()
    };
    let source = wide(source);
    // Keep the destination name itself. An existing symlink must count as a collision.
    let target: Vec<u16> = target.as_os_str().encode_wide().chain([0]).collect();
    let flags = MOVEFILE_WRITE_THROUGH
        | if replace {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), flags) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
