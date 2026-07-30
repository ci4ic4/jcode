//! NetBSD process-memory probes.
//!
//! NetBSD's procfs is only partly Linux-shaped. `/proc/self/statm` is
//! Linux-compatible (whitespace-separated page counts), but `/proc/self/status`
//! is the BSD single-line format with none of the `VmRSS:`-style keys the Linux
//! implementation parses, and there is no `smaps_rollup`, so the PSS/dirty/clean
//! breakdown has no equivalent here and callers see `None` for it.
//!
//! What is available: RSS and virtual size from `statm`, peak RSS from
//! `getrusage`, and the thread count from `/proc/self/task`.

/// Page size for converting `statm`'s page counts to bytes.
pub(super) fn page_size() -> u64 {
    // SAFETY: `sysconf` is thread-safe and takes no pointer arguments.
    let raw = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if raw > 0 { raw as u64 } else { 4096 }
}

/// `(virtual_bytes, rss_bytes)` from `/proc/self/statm`, whose first two
/// fields are total program size and resident set size in pages.
pub(super) fn size_and_rss_bytes() -> (Option<u64>, Option<u64>) {
    let Ok(statm) = std::fs::read_to_string("/proc/self/statm") else {
        return (None, None);
    };
    let page_size = page_size();
    // Parse positionally rather than filtering out unparseable fields: these
    // are fixed positions, so silently skipping field 0 would promote the RSS
    // value into the size slot instead of reporting the field as unknown.
    let mut fields = statm.split_whitespace();
    let mut next_bytes = || -> Option<u64> {
        match fields.next()?.parse::<u64>() {
            Ok(pages) => Some(pages.saturating_mul(page_size)),
            Err(_) => None,
        }
    };
    let size = next_bytes();
    let rss = next_bytes();
    (size, rss)
}

/// Peak RSS via `getrusage(RUSAGE_SELF)`. NetBSD reports `ru_maxrss` in
/// kilobytes.
pub(super) fn peak_rss_bytes() -> Option<u64> {
    // SAFETY: `usage` is a valid, fully-owned `rusage` that the kernel fills in;
    // the return code is checked before any field is read.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    if rc != 0 {
        return None;
    }
    match u64::try_from(usage.ru_maxrss) {
        Ok(kb) => Some(kb.saturating_mul(1024)),
        // A negative ru_maxrss is not meaningful; report "unknown" rather than
        // a wrapped value.
        Err(_) => None,
    }
}

/// Thread count from `/proc/self/task`, which lists one entry per LWP.
pub(super) fn thread_count() -> Option<u64> {
    let Ok(entries) = std::fs::read_dir("/proc/self/task") else {
        return None;
    };
    let count = entries.filter(|entry| entry.is_ok()).count() as u64;
    (count > 0).then_some(count)
}
