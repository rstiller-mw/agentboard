use std::io::Write;
use std::sync::OnceLock;

static ORIGINAL: OnceLock<libc::termios> = OnceLock::new();

/// Puts the terminal into raw mode on the alternate screen; the guard restores it.
pub struct Term;

impl Term {
    pub fn enter() -> Term {
        // SAFETY: plain libc calls on stdin; `termios` is fully initialised by tcgetattr before use.
        unsafe {
            let mut raw: libc::termios = std::mem::zeroed();
            libc::tcgetattr(0, &mut raw);
            let _ = ORIGINAL.set(raw);
            libc::cfmakeraw(&mut raw);
            libc::tcsetattr(0, libc::TCSANOW, &raw);
            libc::signal(libc::SIGWINCH, on_resize as *const () as usize);
        }
        write_out("\x1b[?1049h\x1b[?25l");
        Term
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        restore();
    }
}

extern "C" fn on_resize(_: libc::c_int) {}

/// Also called from the panic hook, because `panic = "abort"` skips destructors.
pub fn restore() {
    write_out("\x1b[?25h\x1b[?1049l");
    if let Some(original) = ORIGINAL.get() {
        // SAFETY: restores the attributes captured in `enter`.
        unsafe { libc::tcsetattr(0, libc::TCSANOW, original) };
    }
}

pub fn write_out(s: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(s.as_bytes());
    let _ = out.flush();
}

/// (columns, rows)
pub fn size() -> (usize, usize) {
    // SAFETY: TIOCGWINSZ fills a `winsize` and touches nothing else.
    let ws = unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        libc::ioctl(1, libc::TIOCGWINSZ, &mut ws);
        ws
    };
    if ws.ws_col == 0 || ws.ws_row == 0 { (80, 24) } else { (ws.ws_col as usize, ws.ws_row as usize) }
}

#[derive(Debug, PartialEq)]
pub enum Key {
    Up,
    Down,
    First,
    Last,
    Jump,
    ToggleFinished,
    Dismiss,
    Quit,
    /// The extra descriptor became readable (a file changed); not a key press.
    Refresh,
    Other,
}

/// Waits for a key or for `wake_fd` to become readable; `None` on timeout or when a resize interrupts the wait.
pub fn read_key(timeout_ms: i32, wake_fd: Option<i32>) -> Option<Key> {
    let mut fds = [
        libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: wake_fd.unwrap_or(-1), events: libc::POLLIN, revents: 0 },
    ];
    // SAFETY: two valid pollfds (a negative fd is ignored by poll); the read buffer is a local array of the length we pass.
    let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout_ms) };
    if ready <= 0 {
        return None;
    }
    if fds[0].revents == 0 {
        return Some(Key::Refresh);
    }
    let mut buf = [0u8; 16];
    let n = unsafe { libc::read(0, buf.as_mut_ptr().cast(), buf.len()) };
    (n > 0).then(|| parse_key(&buf[..n as usize]))
}

fn parse_key(bytes: &[u8]) -> Key {
    match bytes {
        b"k" | b"\x1b[A" => Key::Up,
        b"j" | b"\x1b[B" => Key::Down,
        b"g" | b"\x1b[H" | b"\x1b[1~" => Key::First,
        b"G" | b"\x1b[F" | b"\x1b[4~" => Key::Last,
        b"\r" | b"\n" | b" " => Key::Jump,
        b"h" => Key::ToggleFinished,
        b"x" | b"d" => Key::Dismiss,
        b"q" | b"\x03" | b"\x1b" => Key::Quit,
        _ => Key::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_vim_and_arrow_keys() {
        assert_eq!(parse_key(b"j"), Key::Down);
        assert_eq!(parse_key(b"\x1b[A"), Key::Up);
        assert_eq!(parse_key(b" "), Key::Jump);
        assert_eq!((parse_key(b"x"), parse_key(b"d")), (Key::Dismiss, Key::Dismiss));
        assert_eq!(parse_key(b"\x1b"), Key::Quit);
        assert_eq!(parse_key(b"\x1b[Z"), Key::Other);
    }
}
