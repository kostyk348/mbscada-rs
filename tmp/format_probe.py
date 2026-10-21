#!/usr/bin/env python3
"""Probe Modbus Poll .mbp/.mbw byte layouts without any REA/Ghidra dependency.

Outputs per-file: magic, utf16 string runs, candidate tail blocks.
Goal: recover period/slave/fmt16 offsets that the Rust parser still guesses.
"""
import struct
import pathlib

DESK = pathlib.Path(r"C:\Users\Lab\Desktop")

def utf16_runs(b: bytes):
    i = 0
    n = len(b)
    runs = []
    while i < n - 1:
        if 0x20 <= b[i] <= 0x7E and b[i + 1] == 0:
            j = i
            txt = []
            while j < n - 1 and 0x20 <= b[j] <= 0x7E and b[j + 1] == 0:
                txt.append(chr(b[j]))
                j += 2
            if len(txt) >= 2:
                runs.append((i, "".join(txt)))
        i += 1
    return runs

def scan_tail(b: bytes):
    n = len(b)
    for off in range(n - 24, 16, -4):
        if off < 0:
            break
        try:
            pe, sl, a1, a2, a3, q = struct.unpack_from("<6I", b, off)
        except struct.error:
            continue
        if 50 <= pe <= 60000 and 1 <= sl <= 247 and a1 == 1 and a2 == 0 and a3 == 0 and 1 <= q <= 2000:
            return off, pe, sl, q
    return None

for p in sorted(DESK.glob("*.mbp")):
    b = p.read_bytes()
    n = len(b)
    print(f"{'='*70}")
    print(f"{p.name}  size={n}")
    if n < 24:
        continue
    magic = struct.unpack_from("<I", b, 0)[0]
    print(f"  magic={magic} (v4=7000, v5=10010)")
    if magic == 10010 or magic == 7000:
        f144 = struct.unpack_from("<I", b, 4)[0]
        func = struct.unpack_from("<I", b, 8)[0]
        addr = struct.unpack_from("<I", b, 12)[0]
        qty = struct.unpack_from("<I", b, 16)[0]
        print(f"  [4]={f144} [8]func={func} [12]addr={addr:#x} [16]qty={qty}")
    tail = scan_tail(b)
    if tail:
        off, pe, sl, q = tail
        print(f"  tail@{off} period={pe} slave={sl} qty={q} (bars to end: {n - off})")
    runs = utf16_runs(b)
    print(f"  utf16 runs ({len(runs)}):")
    for o, t in runs[:40]:
        print(f"    @{o:6d}  {t!r}")
    if len(runs) > 40:
        print(f"    ... +{len(runs)-40} more")
