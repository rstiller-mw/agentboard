use std::fs;

/// Fields of /proc/<pid>/stat after the command name, starting at field 3 (state).
fn stat_fields(pid: u32) -> Option<Vec<String>> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = &stat[stat.rfind(')')? + 1..];
    Some(after_comm.split_whitespace().map(str::to_owned).collect())
}

pub fn parent(pid: u32) -> Option<u32> {
    stat_fields(pid)?.get(1)?.parse().ok()
}

/// Process start time in clock ticks; pairs with a PID to detect PID reuse.
pub fn start_ticks(pid: u32) -> Option<String> {
    stat_fields(pid)?.get(19).cloned()
}

pub fn all_pids() -> Vec<u32> {
    fs::read_dir("/proc").into_iter().flatten().flatten().filter_map(|e| e.file_name().to_str()?.parse().ok()).collect()
}

pub fn comm(pid: u32) -> Option<String> {
    Some(fs::read_to_string(format!("/proc/{pid}/comm")).ok()?.trim_end().to_owned())
}

pub fn args(pid: u32) -> Vec<String> {
    fs::read(format!("/proc/{pid}/cmdline"))
        .map(|raw| raw.split(|&b| b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect())
        .unwrap_or_default()
}

pub fn cwd(pid: u32) -> Option<String> {
    Some(fs::read_link(format!("/proc/{pid}/cwd")).ok()?.to_string_lossy().into_owned())
}

/// Whether the process is attached to a terminal (field 7, `tty_nr`, is non-zero).
pub fn has_tty(pid: u32) -> bool {
    stat_fields(pid).and_then(|f| f.get(4)?.parse::<i64>().ok()).is_some_and(|tty| tty != 0)
}

/// utime + stime in clock ticks (fields 14 and 15).
pub fn cpu_ticks(pid: u32) -> Option<u64> {
    let f = stat_fields(pid)?;
    Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
}

/// Process start as epoch milliseconds: boot time plus start ticks.
pub fn start_ms(pid: u32) -> Option<u64> {
    let ticks: u64 = start_ticks(pid)?.parse().ok()?;
    let boot_secs: u64 = fs::read_to_string("/proc/stat").ok()?.lines().find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok())?;
    // SAFETY: sysconf only reads a constant.
    let hz = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok().filter(|&h| h > 0)?;
    Some(boot_secs * 1000 + ticks * 1000 / hz)
}

/// The terminal device a process writes to, e.g. `/dev/pts/3`.
pub fn tty_path(pid: u32) -> Option<String> {
    (0..=2).find_map(|fd| {
        let target = fs::read_link(format!("/proc/{pid}/fd/{fd}")).ok()?.to_string_lossy().into_owned();
        target.starts_with("/dev/pts/").then_some(target)
    })
}
