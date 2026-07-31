#!/usr/bin/env python3
"""Probe NetBSD procfs and report, per node, what it actually contains.

Written to produce evidence for a Linux-vs-NetBSD /proc comparison, so it
prefers showing real bytes over describing them.
"""
import os

PROC = "/proc/self"

# Files Linux provides that code commonly reaches for. Presence is the point.
LINUX_ONLY = [
    "syscall", "wchan", "smaps", "smaps_rollup", "oom_score", "io",
    "comm", "stack", "sched", "schedstat", "personality", "loginuid",
    "cgroup", "mountinfo", "net", "numa_maps", "pagemap", "children",
]

def show(name, limit=400):
    path = os.path.join(PROC, name)
    try:
        if os.path.isdir(path):
            entries = sorted(os.listdir(path))
            print(f"  {name}/ (dir): {entries[:12]}")
            return
        with open(path, "rb") as fh:
            data = fh.read(limit)
        text = data.decode("utf-8", "replace").replace("\0", "\\0")
        print(f"  {name}: {text.strip()[:limit]!r}")
    except Exception as exc:
        print(f"  {name}: <{type(exc).__name__}: {exc}>")

print("=== present in NetBSD /proc/<pid> ===")
for name in sorted(os.listdir(PROC)):
    show(name)

print()
print("=== Linux nodes and their NetBSD availability ===")
for name in LINUX_ONLY:
    path = os.path.join(PROC, name)
    print(f"  {name}: {'present' if os.path.exists(path) else 'ABSENT'}")

print()
print("=== /proc/<pid>/task contents ===")
task = os.path.join(PROC, "task")
for tid in sorted(os.listdir(task)):
    sub = sorted(os.listdir(os.path.join(task, tid)))
    print(f"  task/{tid}/: {sub}")
    print(f"  task/{tid}/children present: {'children' in sub}")
