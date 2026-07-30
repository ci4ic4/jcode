use super::*;
use std::process::{Command, Stdio};

#[test]
fn test_own_process_not_reading_stdin() {
    let pid = std::process::id();
    let state = is_waiting_for_stdin(pid);
    assert_ne!(state, StdinState::Reading);
}

#[test]
fn test_nonexistent_pid() {
    let state = is_waiting_for_stdin(u32::MAX);
    assert_ne!(state, StdinState::Reading);
}

#[cfg(target_os = "linux")]
#[test]
fn test_blocked_process_detected() {
    let mut child = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to spawn cat");

    let pid = child.id();
    std::thread::sleep(std::time::Duration::from_millis(200));

    let state = linux::check_process_tree(pid);

    child.kill().ok();
    child.wait().ok();

    assert_eq!(
        state,
        StdinState::Reading,
        "cat should be waiting for stdin"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_process_not_reading() {
    let mut child = Command::new("sleep")
        .arg("10")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to spawn sleep");

    let pid = child.id();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let state = linux::check(pid);

    child.kill().ok();
    child.wait().ok();

    assert_eq!(
        state,
        StdinState::NotReading,
        "sleep should not be reading stdin"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn test_child_process_tree_detection() {
    // bash -c "cat" spawns bash which spawns cat - cat is the one reading stdin
    let mut child = Command::new("bash")
        .arg("-c")
        .arg("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to spawn bash");

    let pid = child.id();
    std::thread::sleep(std::time::Duration::from_millis(300));

    // The bash process itself may not be reading, but its child (cat) should be
    let state = linux::check_process_tree(pid);

    child.kill().ok();
    child.wait().ok();

    assert_eq!(
        state,
        StdinState::Reading,
        "child cat should be detected via process tree"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn test_grandchild_process_tree_detection() {
    // Wrapper chain: an outer bash spawns an inner `bash -c cat`, so the actual
    // stdin reader (`cat`) is a GRANDCHILD of the tracked pid. The intermediate
    // bash is not itself reading stdin, so detection requires recursing past
    // direct children (issue #373). A trailing `; true` keeps each bash from
    // exec-optimizing itself away so the nesting (outer bash -> inner bash ->
    // cat) is preserved.
    let mut child = Command::new("bash")
        .arg("-c")
        .arg("bash -c 'cat; true'; true")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to spawn bash");

    let pid = child.id();
    std::thread::sleep(std::time::Duration::from_millis(400));

    let state = linux::check_process_tree(pid);

    child.kill().ok();
    child.wait().ok();

    assert_eq!(
        state,
        StdinState::Reading,
        "grandchild cat should be detected via recursive process-tree walk"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn test_direct_children_lists_immediate_children() {
    // Spawn a parent shell that itself spawns a long-lived child (`sleep`).
    // `direct_children` should report the immediate child PID(s) without
    // scanning all of /proc.
    // Use a compound command so bash does NOT exec-optimize itself away and
    // actually stays alive as the parent of a `sleep` child.
    let mut child = Command::new("bash")
        .arg("-c")
        .arg("sleep 5; true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to spawn bash");

    let pid = child.id();
    std::thread::sleep(std::time::Duration::from_millis(200));

    let kids = linux::direct_children(pid);

    // Verify parentage BEFORE killing the parent, otherwise the child
    // reparents to init (ppid 1) and the check races.
    let mut all_parented_by_pid = !kids.is_empty();
    for kid in &kids {
        let status = std::fs::read_to_string(format!("/proc/{}/status", kid)).unwrap_or_default();
        let ppid = status
            .lines()
            .find_map(|l| l.strip_prefix("PPid:\t"))
            .and_then(|v| v.trim().parse::<u32>().ok());
        if ppid != Some(pid) {
            all_parented_by_pid = false;
        }
    }

    child.kill().ok();
    child.wait().ok();

    assert!(
        !kids.is_empty(),
        "bash should have at least one direct child (the sleep)"
    );
    assert!(
        all_parented_by_pid,
        "every reported child should be parented by {pid}; got {kids:?}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn test_process_that_reads_then_exits() {
    use std::io::Write;

    let mut child = Command::new("head")
        .arg("-n1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to spawn head");

    let pid = child.id();
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Should be reading initially
    let state_before = linux::check(pid);

    // Write a line - head should read it and exit
    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(b"hello\n").ok();
        stdin.flush().ok();
    }

    // Wait for exit
    let status = child.wait().expect("failed to wait");

    // After exit, checking the pid should not report Reading
    let state_after = is_waiting_for_stdin(pid);

    assert_eq!(
        state_before,
        StdinState::Reading,
        "head should be reading before input"
    );
    assert_ne!(
        state_after,
        StdinState::Reading,
        "head should not be reading after exit"
    );
    assert!(status.success(), "head should exit successfully");
}

#[cfg(target_os = "linux")]
#[test]
fn test_process_with_closed_stdin_not_reading() {
    // Spawn a process with stdin completely closed (null)
    let mut child = Command::new("cat")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to spawn cat");

    let pid = child.id();
    // cat with /dev/null as stdin should read EOF immediately and exit
    std::thread::sleep(std::time::Duration::from_millis(200));

    let state = is_waiting_for_stdin(pid);

    child.kill().ok();
    child.wait().ok();

    // cat with /dev/null gets EOF immediately, should not be stuck reading
    assert_ne!(state, StdinState::Reading);
}

#[cfg(target_os = "linux")]
#[test]
fn test_multiple_sequential_reads() {
    use std::io::Write;

    // Use a program that reads multiple lines
    let mut child = Command::new("head")
        .arg("-n2")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("failed to spawn head");

    let pid = child.id();
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Should be reading first line
    let state1 = linux::check(pid);
    assert_eq!(
        state1,
        StdinState::Reading,
        "should be waiting for first line"
    );

    // Send first line
    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(b"line1\n").ok();
        stdin.flush().ok();
    }
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Should be reading second line
    let state2 = linux::check(pid);
    assert_eq!(
        state2,
        StdinState::Reading,
        "should be waiting for second line"
    );

    // Send second line
    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(b"line2\n").ok();
        stdin.flush().ok();
    }

    let status = child.wait().expect("failed to wait");
    assert!(status.success());
}

#[cfg(target_os = "linux")]
#[test]
fn direct_children_of_childless_process_does_not_scan_proc() {
    // Regression test for issue #392 A1 (second occurrence): a childless
    // process must return an empty list via /proc/<pid>/task/<tid>/children
    // without falling back to the whole-/proc scan. We can't observe syscalls
    // here, but we can assert the interface itself reports readable-and-empty
    // for a leaf process we control, which is the branch condition the fix
    // keys on.
    let mut child = std::process::Command::new("sleep")
        .arg("5")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();

    // `sleep` spawns no children. The children interface must be readable so
    // direct_children() returns empty WITHOUT the proc-scan fallback.
    let path = format!("/proc/{}/task/{}/children", pid, pid);
    let readable = std::fs::read_to_string(&path).is_ok();
    let children = super::linux::direct_children(pid);

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        readable,
        "kernel lacks CONFIG_PROC_CHILDREN; fallback scan is expected on this system"
    );
    assert!(
        children.is_empty(),
        "sleep should have no children, got {children:?}"
    );
}

// ---------------------------------------------------------------------------
// NetBSD `/proc/<pid>/status` parsing
//
// Fixtures are verbatim lines captured from NetBSD 11.99 (amd64). They are
// parsed on every platform so the field offsets cannot silently rot: NetBSD's
// status is one whitespace-separated line whose *first* field (the command
// name) is the only one that can contain spaces, hence counting from the end.
// ---------------------------------------------------------------------------

/// `cat` blocked reading a pipe.
const NETBSD_STATUS_PIPE_READ: &str = "cat 9290 15669 19993 19993 -1,-1 noflags 1785433833,220200 0,1004 0,1004 pipe_rd 1000 100,100,0";
/// A runnable process (`nochan` wait channel).
const NETBSD_STATUS_RUNNABLE: &str = "python3.14 22072 19993 19993 19993 -1,-1 noflags 1785433788,399468 0,48818 0,27896 nochan 1000 100,100,0";
/// `cat` blocked reading a tty.
const NETBSD_STATUS_TTY_READ: &str = "cat 27999 974 27999 27999 5,14 ctty,sldr 1785433994,550096 0,2151 0,3226 ttyraw 1000 100,100,0";

#[test]
fn netbsd_wait_channel_is_parsed_from_real_status_lines() {
    use super::netbsd_status::parse_wait_channel;

    assert_eq!(parse_wait_channel(NETBSD_STATUS_PIPE_READ), Some("pipe_rd"));
    assert_eq!(parse_wait_channel(NETBSD_STATUS_TTY_READ), Some("ttyraw"));
}

#[test]
fn netbsd_runnable_process_reports_no_wait_channel() {
    assert_eq!(
        super::netbsd_status::parse_wait_channel(NETBSD_STATUS_RUNNABLE),
        None,
        "`nochan` means runnable, not blocked on a channel"
    );
}

#[test]
fn netbsd_parent_pid_is_parsed_from_real_status_lines() {
    use super::netbsd_status::parse_parent_pid;

    assert_eq!(parse_parent_pid(NETBSD_STATUS_PIPE_READ), Some(15669));
    assert_eq!(parse_parent_pid(NETBSD_STATUS_RUNNABLE), Some(19993));
    assert_eq!(parse_parent_pid(NETBSD_STATUS_TTY_READ), Some(974));
}

/// The reason both parsers count fields from the end: a process can carry a
/// command name containing spaces, which would shift every front-counted index.
#[test]
fn netbsd_parsers_tolerate_spaces_in_the_command_name() {
    use super::netbsd_status::{parse_parent_pid, parse_wait_channel};

    let status = "my weird proc 9290 15669 19993 19993 -1,-1 noflags 1785433833,220200 0,1004 0,1004 pipe_rd 1000 100,100,0";
    assert_eq!(parse_wait_channel(status), Some("pipe_rd"));
    assert_eq!(parse_parent_pid(status), Some(15669));
}

#[test]
fn netbsd_parsers_reject_malformed_status_lines() {
    use super::netbsd_status::{parse_parent_pid, parse_wait_channel};

    assert_eq!(parse_wait_channel(""), None);
    assert_eq!(parse_parent_pid(""), None);
    assert_eq!(parse_parent_pid("cat 1 2"), None);
}
