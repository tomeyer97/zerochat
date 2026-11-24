//! Reading files from a run's workspace without leaving it.
//!
//! The workspace is written by agents, so anything in it may be hostile: a symlink to
//! `~/.claude/.credentials.json` or `/proc/self/environ`, a FIFO that blocks `open()` forever,
//! or a multi-gigabyte artefact. Every read here:
//! - resolves the path and refuses anything whose real location is outside the workspace,
//! - opens the final component with `O_NOFOLLOW | O_NONBLOCK` (a swapped-in symlink fails, a FIFO
//!   does not block; Windows: `FILE_FLAG_OPEN_REPARSE_POINT`) and checks the opened file
//!   descriptor again (Linux: `/proc/self/fd`),
//! - accepts regular files only (no FIFO, device or socket),
//! - checks the size before reading and never reads more than the cap,
//! - runs on the blocking pool with a deadline, so a slow disk cannot park a runtime worker.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Why a workspace file was not read.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    InvalidPath,
    NotFound,
    /// The path (or a symlink in it) leads outside the workspace.
    Outside,
    /// A FIFO, socket, device or directory.
    NotRegular(&'static str),
    TooLarge {
        size: u64,
        limit: u64,
    },
    TimedOut,
    Io(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::InvalidPath => write!(f, "invalid path"),
            ReadError::NotFound => write!(f, "no such file in the workspace"),
            ReadError::Outside => write!(f, "path is outside the workspace"),
            ReadError::NotRegular(kind) => write!(f, "not a regular file ({kind})"),
            ReadError::TooLarge { size, limit } => write!(
                f,
                "file is too large to preview ({}; the limit is {})",
                bytes(*size),
                bytes(*limit)
            ),
            ReadError::TimedOut => write!(f, "reading the file took too long"),
            ReadError::Io(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// A size as `TooLarge` words it: `12 B`, `3 KiB`, `2.0 MiB`, `1.5 GiB`.
fn bytes(n: u64) -> String {
    const KIB: u64 = 1 << 10;
    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;
    match n {
        0..KIB => format!("{n} B"),
        KIB..MIB => format!("{:.0} KiB", n as f64 / KIB as f64),
        MIB..GIB => format!("{:.1} MiB", n as f64 / MIB as f64),
        _ => format!("{:.1} GiB", n as f64 / GIB as f64),
    }
}

/// What a relative workspace path is, without following it out of the workspace.
#[derive(Debug, PartialEq, Eq)]
pub enum Entry {
    File(Vec<u8>),
    /// A symlink, shown the way git shows one: its target text, never the target's contents.
    Symlink(String),
}

/// A relative path with no `..`, no leading `/` and nothing that looks like an option
/// (`validate::is_relative_path`).
pub fn safe_relative(path: &str) -> Result<&str, ReadError> {
    if !crate::validate::is_relative_path(path) {
        return Err(ReadError::InvalidPath);
    }
    Ok(path)
}

/// An I/O failure as a refusal: a symlink met by `O_NOFOLLOW` leads outside, an `ENXIO` is a
/// socket or device; anything else keeps the system's message.
impl From<std::io::Error> for ReadError {
    fn from(error: std::io::Error) -> ReadError {
        match error.kind() {
            std::io::ErrorKind::NotFound => ReadError::NotFound,
            #[cfg(unix)]
            _ => match error.raw_os_error() {
                Some(code) if code == libc::ELOOP => ReadError::Outside,
                Some(code) if code == libc::ENXIO => ReadError::NotRegular("socket or device"),
                _ => ReadError::Io(error.to_string()),
            },
            #[cfg(not(unix))]
            _ => ReadError::Io(error.to_string()),
        }
    }
}

#[cfg(unix)]
fn kind_name(file_type: std::fs::FileType) -> &'static str {
    use std::os::unix::fs::FileTypeExt;
    if file_type.is_dir() {
        "directory"
    } else if file_type.is_fifo() {
        "FIFO"
    } else if file_type.is_socket() {
        "socket"
    } else if file_type.is_block_device() || file_type.is_char_device() {
        "device"
    } else {
        "special file"
    }
}

/// Windows has no FIFOs or device files in a workspace; anything not a file is a directory or a
/// reparse point the canonical-path check already resolved.
#[cfg(not(unix))]
fn kind_name(file_type: std::fs::FileType) -> &'static str {
    if file_type.is_dir() {
        "directory"
    } else {
        "special file"
    }
}

/// The real location of `relative`, which must be inside `root` (already canonical).
fn resolve(root: &Path, relative: &str) -> Result<PathBuf, ReadError> {
    let real = root.join(relative).canonicalize()?;
    if !real.starts_with(root) {
        return Err(ReadError::Outside);
    }
    Ok(real)
}

/// Reads `relative` in the workspace (blocking). A symlink is returned as its target text when
/// `symlinks_as_text` (diffs, like git); otherwise it is followed only when it stays inside.
pub fn read_blocking(
    workspace: &Path,
    relative: &str,
    limit: u64,
    symlinks_as_text: bool,
) -> Result<Entry, ReadError> {
    let relative = safe_relative(relative)?;
    let root = workspace.canonicalize()?;
    let lexical = root.join(relative);
    let meta = std::fs::symlink_metadata(&lexical)?;
    if meta.file_type().is_symlink() && symlinks_as_text {
        // Its parent directories must still be inside (a symlinked directory could lead out).
        let parent = lexical
            .parent()
            .ok_or(ReadError::InvalidPath)?
            .canonicalize()?;
        if !parent.starts_with(&root) {
            return Err(ReadError::Outside);
        }
        let target = std::fs::read_link(&lexical)?;
        return Ok(Entry::Symlink(target.to_string_lossy().into_owned()));
    }
    let real = resolve(&root, relative)?;
    let file = open_no_follow(&real)?;
    verify_open_file(&file, &root)?;
    let meta = file.metadata()?;
    if !meta.file_type().is_file() {
        return Err(ReadError::NotRegular(kind_name(meta.file_type())));
    }
    if meta.len() > limit {
        return Err(ReadError::TooLarge {
            size: meta.len(),
            limit,
        });
    }
    let mut buffer = Vec::with_capacity(meta.len() as usize);
    file.take(limit + 1).read_to_end(&mut buffer)?;
    if buffer.len() as u64 > limit {
        // It grew while being read.
        return Err(ReadError::TooLarge {
            size: buffer.len() as u64,
            limit,
        });
    }
    Ok(Entry::File(buffer))
}

#[cfg(unix)]
fn open_no_follow(path: &Path) -> Result<File, ReadError> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)
        .map_err(ReadError::from)
}

/// Opens the reparse point (symlink, junction) itself instead of following it, so a link swapped
/// in after [`resolve`] is not followed out of the workspace (its metadata is then not a file).
#[cfg(windows)]
fn open_no_follow(path: &Path) -> Result<File, ReadError> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(ReadError::from)
}

#[cfg(not(any(unix, windows)))]
fn open_no_follow(path: &Path) -> Result<File, ReadError> {
    File::open(path).map_err(ReadError::from)
}

/// The descriptor that was opened is really inside the workspace (guards against a directory
/// being swapped for a symlink between resolving and opening).
#[cfg(target_os = "linux")]
fn verify_open_file(file: &File, root: &Path) -> Result<(), ReadError> {
    use std::os::fd::AsRawFd;
    match std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())) {
        Ok(actual) if actual.starts_with(root) => Ok(()),
        Ok(_) => Err(ReadError::Outside),
        // No /proc (unusual sandbox): the canonical-path check already ran.
        Err(_) => Ok(()),
    }
}

#[cfg(not(target_os = "linux"))]
fn verify_open_file(_file: &File, _root: &Path) -> Result<(), ReadError> {
    Ok(())
}

/// How long one read, or one batch of line counts, may take.
const DEADLINE: Duration = Duration::from_secs(5);
/// How much longer [`line_counts`] waits for its task than the task's own budget: the task
/// stops starting new files after [`DEADLINE`] but finishes the one it is reading (itself cut
/// at the size limit), so the outer timeout only fires when that read is stuck.
const DEADLINE_GRACE: Duration = Duration::from_secs(1);

/// [`read_blocking`] on the blocking pool, with a deadline.
pub async fn read(
    workspace: &Path,
    relative: &str,
    limit: u64,
    symlinks_as_text: bool,
) -> Result<Entry, ReadError> {
    let (workspace, relative) = (workspace.to_owned(), relative.to_owned());
    let task = tokio::task::spawn_blocking(move || {
        read_blocking(&workspace, &relative, limit, symlinks_as_text)
    });
    match tokio::time::timeout(DEADLINE, task).await {
        Ok(Ok(result)) => result,
        Ok(Err(join)) => Err(ReadError::Io(join.to_string())),
        Err(_) => Err(ReadError::TimedOut),
    }
}

/// Line counts of untracked files (None when the file was not read: outside, special, too large,
/// binary). Symlinks count as one line, like git. Runs on the blocking pool with one deadline.
pub async fn line_counts(
    workspace: &Path,
    paths: Vec<String>,
    limit: u64,
) -> Vec<(String, Option<u64>)> {
    let workspace = workspace.to_owned();
    let task = tokio::task::spawn_blocking(move || {
        let started = std::time::Instant::now();
        paths
            .into_iter()
            .map(|path| {
                if started.elapsed() > DEADLINE {
                    return (path, None);
                }
                let lines = match read_blocking(&workspace, &path, limit, true) {
                    Ok(Entry::Symlink(_)) => Some(1),
                    Ok(Entry::File(bytes)) => std::str::from_utf8(&bytes)
                        .ok()
                        .map(|t| t.lines().count() as u64),
                    Err(_) => None,
                };
                (path, lines)
            })
            .collect::<Vec<_>>()
    });
    match tokio::time::timeout(DEADLINE + DEADLINE_GRACE, task).await {
        Ok(Ok(counts)) => counts,
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("ws");
        let outside = dir.path().join("secret.txt");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::write(workspace.join("src/a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(&outside, "SECRET-OUTSIDE-WORKSPACE").unwrap();
        (dir, workspace, outside)
    }

    #[test]
    fn reads_a_regular_file_inside() {
        let (_dir, ws, _) = setup();
        assert_eq!(
            read_blocking(&ws, "src/a.txt", 1024, false).unwrap(),
            Entry::File(b"one\ntwo\n".to_vec())
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_out_of_the_workspace_are_refused() {
        let (_dir, ws, outside) = setup();
        std::os::unix::fs::symlink(&outside, ws.join("link.txt")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", ws.join("passwd")).unwrap();
        assert_eq!(
            read_blocking(&ws, "link.txt", 1024, false),
            Err(ReadError::Outside)
        );
        assert_eq!(
            read_blocking(&ws, "passwd", 1024, false),
            Err(ReadError::Outside)
        );
        // A diff shows the link itself, never what it points to.
        assert_eq!(
            read_blocking(&ws, "link.txt", 1024, true).unwrap(),
            Entry::Symlink(outside.display().to_string())
        );
        // Through a symlinked directory.
        std::os::unix::fs::symlink(outside.parent().unwrap(), ws.join("up")).unwrap();
        assert_eq!(
            read_blocking(&ws, "up/secret.txt", 1024, false),
            Err(ReadError::Outside)
        );
        assert_eq!(
            read_blocking(&ws, "up/secret.txt", 1024, true),
            Err(ReadError::Outside)
        );
        assert_eq!(
            read_blocking(&ws, "../secret.txt", 1024, false),
            Err(ReadError::InvalidPath)
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_inside_the_workspace_are_followed() {
        let (_dir, ws, _) = setup();
        std::os::unix::fs::symlink("src/a.txt", ws.join("inside")).unwrap();
        assert_eq!(
            read_blocking(&ws, "inside", 1024, false).unwrap(),
            Entry::File(b"one\ntwo\n".to_vec())
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_without_blocking() {
        let (_dir, ws, _) = setup();
        let fifo = ws.join("pipe.txt");
        let c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let started = std::time::Instant::now();
        assert_eq!(
            read_blocking(&ws, "pipe.txt", 1024, false),
            Err(ReadError::NotRegular("FIFO"))
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(
            read_blocking(&ws, "src", 1024, false),
            Err(ReadError::NotRegular("directory"))
        );
    }

    #[test]
    fn large_files_are_refused_before_reading() {
        let (_dir, ws, _) = setup();
        let file = File::create(ws.join("big.bin")).unwrap();
        file.set_len(400 << 20).unwrap();
        match read_blocking(&ws, "big.bin", 2 << 20, false) {
            Err(ReadError::TooLarge { size, limit }) => {
                assert_eq!((size, limit), (400 << 20, 2 << 20))
            }
            other => panic!("{other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn line_counts_skip_what_cannot_be_read() {
        let (_dir, ws, outside) = setup();
        std::os::unix::fs::symlink(&outside, ws.join("link.txt")).unwrap();
        let counts = line_counts(
            &ws,
            vec!["src/a.txt".into(), "link.txt".into(), "missing".into()],
            1024,
        )
        .await;
        assert_eq!(
            counts,
            [
                ("src/a.txt".to_owned(), Some(2)),
                ("link.txt".to_owned(), Some(1)),
                ("missing".to_owned(), None)
            ]
        );
    }
}
