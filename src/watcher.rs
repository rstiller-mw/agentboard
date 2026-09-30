use std::os::fd::RawFd;
use std::path::{Path, PathBuf};

const EVENTS: u32 = libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_MOVED_FROM | libc::IN_CREATE | libc::IN_DELETE;

/// Wakes the main loop as soon as Claude rewrites a session or job file, instead of waiting for the next poll.
pub struct Watcher {
    fd: RawFd,
    roots: Vec<PathBuf>,
}

impl Watcher {
    /// `None` when inotify is unavailable; the caller then relies on polling alone.
    pub fn new(roots: Vec<PathBuf>) -> Option<Watcher> {
        // SAFETY: plain syscall; a negative result is handled below.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        (fd >= 0).then(|| {
            let watcher = Watcher { fd, roots };
            watcher.refresh();
            watcher
        })
    }

    pub fn fd(&self) -> RawFd {
        self.fd
    }

    /// Watches each root and its direct subdirectories (one per job); adding an existing watch is a no-op.
    pub fn refresh(&self) {
        for root in &self.roots {
            self.add(root);
            for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    self.add(&entry.path());
                }
            }
        }
    }

    fn add(&self, path: &Path) {
        let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else { return };
        // SAFETY: `c_path` is a valid NUL-terminated string that outlives the call.
        unsafe { libc::inotify_add_watch(self.fd, c_path.as_ptr(), EVENTS) };
    }

    /// Discards queued events; returns whether there were any.
    pub fn drain(&self) -> bool {
        let mut buf = [0u8; 4096];
        let mut any = false;
        // SAFETY: reads into a local buffer of the length passed; the fd is non-blocking.
        while unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) } > 0 {
            any = true;
        }
        any
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // SAFETY: closes the descriptor this struct opened.
        unsafe { libc::close(self.fd) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agentboard-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reports_a_file_written_in_a_watched_directory() {
        let dir = scratch("watch-file");
        let watcher = Watcher::new(vec![dir.clone()]).unwrap();
        assert!(!watcher.drain());
        std::fs::write(dir.join("a.json"), "{}").unwrap();
        assert!(watcher.drain());
        assert!(!watcher.drain());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reports_a_file_written_in_a_subdirectory_after_a_refresh() {
        let dir = scratch("watch-sub");
        let watcher = Watcher::new(vec![dir.clone()]).unwrap();
        std::fs::create_dir(dir.join("job")).unwrap();
        assert!(watcher.drain());
        watcher.refresh();
        std::fs::write(dir.join("job/state.json"), "{}").unwrap();
        assert!(watcher.drain());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
