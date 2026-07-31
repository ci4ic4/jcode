//! Compile-and-run check for the Rust snippets in NETBSD_PROC_PORTING.md.
//! Run with: rustc --edition 2021 -o /tmp/doccheck docsnippets.rs && /tmp/doccheck

use std::os::unix::fs::FileTypeExt;

/// Wait channel (field 11): 3rd from the end.
fn wait_channel(status: &str) -> Option<&str> {
    let f: Vec<&str> = status.split_whitespace().collect();
    let w = *f.get(f.len().checked_sub(3)?)?;
    (w != "nochan").then_some(w)
}

/// Parent pid (field 3): 11th from the end.
fn parent_pid(status: &str) -> Option<u32> {
    let f: Vec<&str> = status.split_whitespace().collect();
    f.get(f.len().checked_sub(11)?)?.parse().ok()
}

fn stdin_is_pipe_or_tty(pid: u32) -> bool {
    let Ok(meta) = std::fs::metadata(format!("/proc/{pid}/fd/0")) else {
        return false;
    };
    let t = meta.file_type();
    t.is_fifo() || t.is_char_device() || t.is_socket()
}

const READ_WAIT_CHANNELS: &[&str] = &["ttyraw", "ttyin", "ttyout", "pipe_rd", "netio"];

fn is_reading_stdin(pid: u32) -> bool {
    let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
        return false;
    };
    matches!(wait_channel(&status), Some(c) if READ_WAIT_CHANNELS.contains(&c))
        && stdin_is_pipe_or_tty(pid)
}

fn statm_bytes(statm: &str, page_size: u64) -> (Option<u64>, Option<u64>) {
    let mut fields = statm.split_whitespace();
    let mut next = || -> Option<u64> {
        match fields.next()?.parse::<u64>() {
            Ok(pages) => Some(pages.saturating_mul(page_size)),
            Err(_) => None,
        }
    };
    let virtual_bytes = next();
    let rss_bytes = next();
    (virtual_bytes, rss_bytes)
}

fn main() {
    // Fixtures captured verbatim from NetBSD 11.99.6.
    let pipe_reader =
        "cat 9290 15669 19993 19993 -1,-1 noflags 1785433833,220200 0,1004 0,1004 pipe_rd 1000 100,100,0";
    let runnable =
        "python3.14 22072 19993 19993 19993 -1,-1 noflags 1785433788,399468 0,48818 0,27896 nochan 1000 100,100,0";
    let tty_reader =
        "cat 27999 974 27999 27999 5,14 ctty,sldr 1785433994,550096 0,2151 0,3226 ttyraw 1000 100,100,0";
    let spaced_name =
        "my weird proc 9290 15669 19993 19993 -1,-1 noflags 1785433833,220200 0,1004 0,1004 pipe_rd 1000 100,100,0";

    assert_eq!(wait_channel(pipe_reader), Some("pipe_rd"));
    assert_eq!(wait_channel(tty_reader), Some("ttyraw"));
    assert_eq!(wait_channel(runnable), None, "nochan means runnable");
    assert_eq!(parent_pid(pipe_reader), Some(15669));
    assert_eq!(parent_pid(runnable), Some(19993));
    assert_eq!(parent_pid(tty_reader), Some(974));

    // The whole reason for counting from the end.
    assert_eq!(wait_channel(spaced_name), Some("pipe_rd"));
    assert_eq!(parent_pid(spaced_name), Some(15669));

    // Malformed input must yield "unknown", never a bogus default.
    assert_eq!(wait_channel(""), None);
    assert_eq!(parent_pid(""), None);
    assert_eq!(parent_pid("cat 1 2"), None);

    // statm: real capture "5625 2577 0 1 0 24 0" at 4096-byte pages.
    let (v, r) = statm_bytes("5625 2577 0 1 0 24 0", 4096);
    assert_eq!(v, Some(5625 * 4096));
    assert_eq!(r, Some(2577 * 4096));
    // A corrupt leading field must not promote RSS into the size slot.
    let (v2, _) = statm_bytes("xxx 2577", 4096);
    assert_eq!(v2, None, "positional parse must not skip a bad field");

    // Live check against this very process.
    let me = std::process::id();
    let live = std::fs::read_to_string(format!("/proc/{me}/status")).unwrap();
    assert!(parent_pid(&live).is_some(), "live status must parse");
    let _ = is_reading_stdin(me);
    let _ = stdin_is_pipe_or_tty(me);

    println!("all doc snippets compile and pass");
    println!("live status parsed: ppid={:?}", parent_pid(&live));
}
