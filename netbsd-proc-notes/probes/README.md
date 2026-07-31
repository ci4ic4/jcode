# Probe scripts

Self-contained scripts that produce the evidence in `../README.md`. Nothing
here needs installing: Python 3 and (for the Rust check) `rustc`.

Re-run them on your own NetBSD release before relying on the findings.
Kernel wait-channel names and procfs layout are not stable interfaces.

| Script | What it answers |
|---|---|
| `procprobe.py` | What nodes exist under `/proc/<pid>`, and which Linux-only ones are missing? |
| `procverify.py` | Do the Linux-shaped files actually parse with strict Linux parsers? |
| `proctraps.py` | Do the two "looks compatible but isn't" traps reproduce? |
| `docsnippets.rs` | Do the README's Rust samples compile and behave as documented? |

```sh
python3 procprobe.py
python3 procverify.py
python3 proctraps.py

rustc --edition 2021 -o /tmp/doccheck docsnippets.rs && /tmp/doccheck
```

`proctraps.py` spawns short-lived `sleep`/`cat` children under a pty and kills
them again; it leaves nothing behind. `docsnippets.rs` asserts against fixture
strings captured verbatim from NetBSD 11.99.6 and also parses this machine's
live `/proc`, so it fails loudly if the format ever shifts.
