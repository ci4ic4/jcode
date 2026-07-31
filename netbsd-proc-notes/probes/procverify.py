#!/usr/bin/env python3
"""Verify which NetBSD procfs nodes are genuinely Linux-format-compatible.

The interesting cases are the files NetBSD provides in *both* a native and a
Linux-shaped variant. Parsing each with the exact logic Linux code would use
is the only way to know whether the shim is real or superficial.
"""
import os
import re

def parse_linux_maps(text):
    """Linux /proc/pid/maps: addr-addr perms offset dev inode path."""
    pat = re.compile(r"^([0-9a-f]+)-([0-9a-f]+) (\S{4}) ([0-9a-f]+) (\S+) (\d+)\s*(.*)$")
    rows = [pat.match(l) for l in text.splitlines() if l.strip()]
    ok = [m for m in rows if m]
    return len(ok), len(rows)

def parse_linux_statm(text):
    """Linux /proc/pid/statm: 7 integers in pages."""
    f = text.split()
    return len(f), all(x.lstrip("-").isdigit() for x in f)

def parse_linux_stat(text):
    """Linux /proc/pid/stat: fields counted after the last ')'."""
    after = text[text.rfind(")") + 1:].split()
    return len(after)

def parse_linux_limits(text):
    """Linux /proc/pid/limits: header then 'Max <name> <soft> <hard> <units>'."""
    lines = [l for l in text.splitlines() if l.strip()]
    header = lines[0].startswith("Limit")
    maxes = [l for l in lines[1:] if l.startswith("Max ")]
    return header, len(maxes)

print("=== maps (Linux-format) vs map (native BSD) ===")
maps = open("/proc/self/maps").read()
ok, total = parse_linux_maps(maps)
print(f"  maps: {ok}/{total} lines match the Linux regex exactly")
print(f"  sample: {maps.splitlines()[0]!r}")
print(f"  map   : {open('/proc/self/map').read().splitlines()[0]!r}  <- native, different")

print()
print("=== statm ===")
n, alldigits = parse_linux_statm(open("/proc/self/statm").read())
print(f"  fields={n} (Linux has 7), all integers={alldigits}")

print()
print("=== stat ===")
n = parse_linux_stat(open("/proc/self/stat").read())
print(f"  fields after comm={n} (Linux has ~50; field 5 after comm = tty_nr)")

print()
print("=== limits (Linux-format) vs limit (native) ===")
header, n = parse_linux_limits(open("/proc/self/limits").read())
print(f"  limits: Linux-style header={header}, 'Max ...' rows={n}")
print(f"  limit : {open('/proc/self/limit').read().splitlines()[0].strip()!r}  <- native, different")

print()
print("=== status: NetBSD is NOT Linux-shaped ===")
st = open("/proc/self/status").read()
print(f"  NetBSD: single line, {len(st.split())} whitespace fields")
print(f"  Linux would have 'Name:', 'VmRSS:', 'Threads:' keys")
print(f"  has 'VmRSS:'? {'VmRSS:' in st}")
