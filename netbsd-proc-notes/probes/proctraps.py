#!/usr/bin/env python3
"""Verify the two NetBSD procfs traps that look Linux-compatible but are not.

Trap 1: /proc/<pid>/stat exists and parses, but tty_nr does not reset when the
controlling terminal goes away, so the common Linux orphan-detection trick
silently never fires.

Trap 2: /proc/<pid>/fd/N is a real node, not a symlink, so readlink() fails
where it succeeds on Linux. stat() still classifies the fd correctly.
"""
import os
import pty
import signal
import stat as stat_mod
import subprocess
import time


def tty_nr(pid):
    s = open(f"/proc/{pid}/stat").read()
    return int(s[s.rfind(")") + 1:].split()[4])


print("=== TRAP 1: stat tty_nr does not reset on terminal teardown ===")
pid, master = pty.fork()
if pid == 0:
    signal.signal(signal.SIGHUP, signal.SIG_IGN)
    time.sleep(30)
    os._exit(0)
time.sleep(0.5)
before = tty_nr(pid)
os.close(master)          # tear the terminal down
time.sleep(1.5)
after = tty_nr(pid)
os.kill(pid, signal.SIGKILL)
os.waitpid(pid, 0)
print(f"  tty_nr with pty open : {before}")
print(f"  tty_nr after teardown: {after}")
print(f"  Linux would report 0 here; NetBSD keeps {after}")
print(f"  => orphan detection via tty_nr==0 is UNSOUND on NetBSD: "
      f"{'CONFIRMED' if after == before != 0 else 'did not reproduce'}")

print()
print("=== TRAP 2: fd/N is a node, not a symlink ===")
producer = subprocess.Popen(["sleep", "30"], stdout=subprocess.PIPE)
reader = subprocess.Popen(["cat"], stdin=producer.stdout,
                          stdout=subprocess.DEVNULL)
time.sleep(0.5)
path = f"/proc/{reader.pid}/fd/0"
try:
    link = os.readlink(path)
    print(f"  readlink -> {link}")
except OSError as exc:
    print(f"  readlink FAILS: {type(exc).__name__}: {exc}")
st = os.stat(path)
print(f"  stat mode=0o{st.st_mode:o} "
      f"FIFO={stat_mod.S_ISFIFO(st.st_mode)} "
      f"CHR={stat_mod.S_ISCHR(st.st_mode)} "
      f"SOCK={stat_mod.S_ISSOCK(st.st_mode)}")
print("  => use stat() file-type checks, not readlink() path matching")

# Also show the wait-channel signal that replaces Linux's wchan file.
wchan = open(f"/proc/{reader.pid}/status").read().split()[-3]
print(f"  status wait-channel field for a pipe reader: {wchan!r}")
reader.kill(); producer.kill()
reader.wait(); producer.wait()

print()
print("=== wait-channel names for common blocking states ===")
cases = []
p = subprocess.Popen(["sleep", "30"])
time.sleep(0.4)
cases.append(("sleep(1) syscall", open(f"/proc/{p.pid}/status").read().split()[-3]))
p.kill(); p.wait()

pid2, m2 = pty.fork()
if pid2 == 0:
    os.execvp("cat", ["cat"])
time.sleep(0.6)
cases.append(("cat reading a tty", open(f"/proc/{pid2}/status").read().split()[-3]))
os.kill(pid2, signal.SIGKILL); os.waitpid(pid2, 0)
os.close(m2)

for label, ch in cases:
    print(f"  {label:22s} -> {ch!r}")
print("  (a runnable process reports 'nochan')")
