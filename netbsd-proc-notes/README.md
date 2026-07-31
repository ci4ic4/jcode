# Porting `/proc`-dependent code from Linux to NetBSD

A practical reference for anyone whose program reads `/proc` and now has to run
on NetBSD. It is written from the problem end: *"my Linux code reads X, what do
I do on NetBSD?"*

Everything here was measured on **NetBSD 11.99.6 (amd64)** with procfs mounted
at `/proc`, and cross-checked against `mount_procfs(8)`. Where a claim came
from an experiment rather than the manual, the experiment is described so you
can rerun it.

---

## The one-paragraph version

NetBSD's procfs is *not* a Linux clone, but it is not entirely foreign either.
It ships a handful of deliberately Linux-shaped files (`maps`, `statm`,
`stat`, `limits`) alongside its own native ones (`map`, `limit`, `status`).
The danger is not the files that are missing, because those fail loudly. The
danger is the files that are **present, parse cleanly, and mean something
different**. Two of those cost me real debugging time and are documented under
[Traps](#traps-files-that-lie).

---

## Availability at a glance

| Linux node | NetBSD | Notes |
|---|---|---|
| `stat` | ✅ same shape | 37 fields after `comm` vs Linux's ~50. See [trap 1](#trap-1-tty_nr-never-resets). |
| `statm` | ✅ identical | 7 integers in pages. Fully interchangeable. |
| `maps` | ✅ identical | Linux format, verified 109/109 lines. Native variant is `map`. |
| `limits` | ✅ same shape | Linux `Max <name> <soft> <hard> <units>` table. Native variant is `limit`. |
| `cmdline` | ✅ | NUL-separated, as on Linux. |
| `environ` | ✅ | NUL-separated. |
| `exe`, `cwd`, `root` | ✅ | Symlinks, as on Linux. |
| `auxv` | ✅ | Binary ELF auxiliary vector. |
| `fd/` | ⚠️ present, different | Entries are **not symlinks**. See [trap 2](#trap-2-fdn-is-not-a-symlink). |
| `task/` | ⚠️ present, thinner | One entry per LWP, but **no `children`** file. |
| `status` | ❌ different format | Single space-separated line, not `Key: value`. See below. |
| `wchan` | ❌ absent | Wait-channel *name* is a field inside `status`. |
| `syscall` | ❌ absent | No direct equivalent. |
| `smaps`, `smaps_rollup` | ❌ absent | No PSS / per-mapping accounting. |
| `io` | ❌ absent | Use `getrusage(2)`. |
| `comm` | ❌ absent | Command name is field 1 of `status`. |
| `oom_score`, `cgroup`, `pagemap`, `numa_maps`, `mountinfo`, `net`, `sched*`, `stack`, `personality`, `loginuid`, `children` | ❌ absent | Linux-specific concepts. |

NetBSD-only extras: `map`, `limit`, `emul`, `note`, `notepg`, `regs`, `fpregs`,
`file`.

---

## `status`: the big incompatibility

This is the file most Linux code reaches for, and the one most likely to burn
you, because it exists on both systems with completely different contents.

**Linux** is a `Key: value` table you grep by name:

```
Name:	cat
Pid:	1234
PPid:	1200
Threads:	1
VmRSS:	   2048 kB
```

**NetBSD** is one space-separated line with no keys at all:

```
cat 9290 15669 19993 19993 -1,-1 noflags 1785433833,220200 0,1004 0,1004 pipe_rd 1000 100,100,0
```

Per `mount_procfs(8)`, the 13 fields are:

| # | Field | Example |
|---|---|---|
| 1 | command name | `cat` |
| 2 | process id | `9290` |
| 3 | parent process id | `15669` |
| 4 | process group id | `19993` |
| 5 | session id | `19993` |
| 6 | controlling terminal `major,minor`, or `-1,-1` | `-1,-1` |
| 7 | flags: `ctty`, `sldr`, or `noflags` | `noflags` |
| 8 | start time `sec,usec` | `1785433833,220200` |
| 9 | user time `sec,usec` | `0,1004` |
| 10 | system time `sec,usec` | `0,1004` |
| 11 | **wait channel message** | `pipe_rd` |
| 12 | effective uid | `1000` |
| 13 | credentials: egid + groups | `100,100,0` |

There is **no memory information here at all**. If you are porting code that
reads `VmRSS:` or `Threads:` from `status`, that data is somewhere else
entirely (see [Memory](#memory)).

### Parse fields from the end, not the start

Field 1 is the command name, and a process can set a name containing spaces.
Every field after it is space-free and the count is fixed, so counting from the
right is robust while counting from the left is not:

```rust
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
```

A runnable process reports the literal string `nochan`.

---

## Memory

`status` has no memory fields and there is no `smaps_rollup`. What you do have:

**`statm` is byte-for-byte Linux-compatible**: seven integers in pages, so
multiply by `sysconf(_SC_PAGESIZE)`.

```
5625 2577 0 1 0 24 0
 |    |
 |    +-- field 1: resident set size (pages)
 +------- field 0: total program size (pages)
```

**Peak RSS** comes from `getrusage(RUSAGE_SELF)`, whose `ru_maxrss` is in
**kilobytes** on NetBSD.

**Thread count** comes from counting entries in `/proc/<pid>/task/`.

**PSS and the shared/private breakdown have no equivalent.** Report them as
unknown rather than inventing numbers.

```rust
// Parse statm positionally. Do NOT use filter_map to skip unparseable
// fields: these are fixed positions, so silently dropping field 0 would
// promote RSS into the size slot.
let mut fields = statm.split_whitespace();
let mut next = || -> Option<u64> {
    match fields.next()?.parse::<u64>() {
        Ok(pages) => Some(pages.saturating_mul(page_size)),
        Err(_) => None,
    }
};
let virtual_bytes = next();
let rss_bytes = next();
```

---

## Traps: files that lie

These are the expensive ones. Both files exist, both parse with unmodified
Linux code, and both quietly mean something different. Neither fails loudly.

### Trap 1: `tty_nr` never resets

A widespread Linux idiom detects an abandoned controlling terminal by reading
field 7 of `stat` (`tty_nr`) and checking for `0`, because Linux zeroes it when
the terminal is torn down even while a stale fd 0 still reports `isatty()`.

NetBSD provides `stat`, and the field parses fine. **But it never resets.**

Measured: fork a child under a pty, have it ignore `SIGHUP`, close the master,
and re-read the field.

```
tty_nr with pty open : 1289
tty_nr after teardown: 1289     <- Linux would report 0
```

So the check silently never fires. That is worse than an error: you get a
permanent false negative and a feature that looks implemented. Either find
another signal or explicitly keep the conservative answer on this platform.

### Trap 2: `fd/N` is not a symlink

On Linux, `readlink("/proc/<pid>/fd/0")` yields something like `pipe:[12345]`
or `/dev/pts/3`, and code routinely pattern-matches that string.

On NetBSD the `fd` directory exists and lists the right descriptors, but the
entries are **real nodes, not symlinks**:

```
readlink FAILS: OSError: [Errno 22] Invalid argument
stat mode=0o10600 FIFO=True CHR=False SOCK=False
```

`readlink()` fails with `EINVAL`. The fix is better code anyway: `stat()` the
path and switch on the **file type** instead of parsing a string.

```rust
use std::os::unix::fs::FileTypeExt;

fn stdin_is_pipe_or_tty(pid: u32) -> bool {
    let Ok(meta) = std::fs::metadata(format!("/proc/{pid}/fd/0")) else {
        return false;
    };
    let t = meta.file_type();
    t.is_fifo() || t.is_char_device() || t.is_socket()
}
```

---

## Detecting "is this process blocked reading stdin?"

A worked example, because it needs three separate substitutions.

Linux typically reads `/proc/<pid>/syscall` (exact syscall + fd) and falls back
to `/proc/<pid>/wchan` (a kernel symbol like `pipe_read`), then confirms via
the fd 0 symlink.

NetBSD has **neither file**, but the wait-channel *name* is field 11 of
`status`. Combine that with a `stat()` fd check:

```rust
const READ_WAIT_CHANNELS: &[&str] =
    &["ttyraw", "ttyin", "ttyout", "pipe_rd", "netio"];

fn is_reading_stdin(pid: u32) -> bool {
    let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status"))
    else { return false };
    matches!(wait_channel(&status),
             Some(c) if READ_WAIT_CHANNELS.contains(&c))
        && stdin_is_pipe_or_tty(pid)
}
```

Observed wait-channel values:

| State | NetBSD wchan | Linux equivalent |
|---|---|---|
| blocked reading a pipe | `pipe_rd` | `pipe_read` |
| blocked reading a tty | `ttyraw` / `ttyin` | `n_tty_read` |
| blocked in `sleep(1)` | `nanoslp` | `hrtimer_nanosleep` |
| runnable | `nochan` | (empty) |

These are kernel-internal names and can change between releases. Treat the list
as a heuristic, and make sure a mismatch degrades to "unknown" rather than to a
confident wrong answer.

---

## Walking the process tree

Linux offers `/proc/<pid>/task/<tid>/children`, which is cheap and direct.
**NetBSD has `task/` but no `children`**, so fall back to the portable method:
scan `/proc`, read each `status`, and compare the parent-pid field.

That is O(number of processes) per call, so cache it or bound how often you
poll. Note that Linux code often has this same fallback already, for kernels
built without `CONFIG_PROC_CHILDREN`; if so, you may just need to widen a
`#[cfg]` rather than write anything new.

---

## The `linux/proc` emulation mount is not the escape hatch

NetBSD can mount a second procfs for Linux binary emulation, typically at
`/usr/pkg/emul/linux/proc`. It is tempting to point your code there and assume
Linux semantics.

**That does not work for native binaries.** Measured on the same machine, a
*native* process reading its own `status` through the emulated mount gets the
NetBSD single-line format, not the Linux key-value one:

```
python3.14 9866 2446 2446 2446 -1,-1 noflags ... nochan 1000 100,100,0
has VmRSS: False
```

The emulation layer reshapes output for processes running under Linux ABI
emulation. A native NetBSD binary gets native formats regardless of which mount
it reads through. It also does not conjure up missing nodes: `wchan`,
`syscall`, `smaps_rollup`, `cpuinfo`, and `meminfo` are all still absent.

Related: `mount_procfs(8)` documents a `-o nolinux` option that disables the
Linux-compatibility nodes. Do not assume `maps` and `statm` exist just because
they do on your machine, since an administrator can turn them off. Degrade
gracefully.

---

## Portability checklist

1. **Never assume `status` is `Key: value`.** It is the single most common
   porting bug, and it fails at parse time on every read.
2. **Prefer syscalls to `/proc` where one exists.** `getrusage(2)` and
   `sysconf(3)` are portable across every unix; procfs layout is not.
3. **Parse positional formats from the end**, so a command name containing
   spaces cannot shift your indices.
4. **Use `stat()` file-type checks, not `readlink()` string matching**, for fd
   inspection. This is more portable *and* more correct on Linux too.
5. **Make missing data return "unknown", not a plausible default.** A `null`
   RSS prompts investigation; a `0` RSS silently corrupts dashboards.
6. **Unit-test positional parsers on every platform** using captured fixture
   strings. Field offsets are exactly the kind of detail that rots silently,
   and fixtures let your Linux CI catch it without a NetBSD runner.
7. **Beware `#[cfg]`/`#ifdef` fallbacks that silently no-op.** If a platform
   branch returns `None` or does nothing, make sure that is a deliberate,
   documented decision rather than an accident of `target_os = "linux"` being
   the only case anyone wrote.

Point 7 generalises past procfs. The same pattern shows up in dependencies:
`fontique`, for instance, gates its fontconfig backend on `linux` or `freebsd`
and hands every other unix a stub whose font lookups always return `None`.
Fontconfig works fine on NetBSD; the code simply never asks it.

---

## Reproducing these measurements

Every claim above comes from a script in [`probes/`](probes/), which depend
only on Python 3 (plus `rustc` for the snippet check):

```sh
cd probes
python3 procprobe.py    # what exists, and what Linux node is missing
python3 procverify.py   # do the Linux-shaped files parse as Linux?
python3 proctraps.py    # do both traps reproduce?

# the README's Rust samples are compiled and asserted, not hand-waved
rustc --edition 2021 -o /tmp/doccheck docsnippets.rs && /tmp/doccheck
```

Re-run them on your target release before relying on any of this. Kernel
wait-channel names in particular are not a stable interface, and
`mount_procfs -o nolinux` can remove the compatibility nodes entirely.
