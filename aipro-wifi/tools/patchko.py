#!/usr/bin/env python3
"""Patch init relocation 0x150 -> 0x170 in Huawei-kernel mismatched modules."""
import struct, subprocess, sys

def patch(path, want_old=0x150, new=0x170):
    out = subprocess.run(["readelf", "-SW", path], capture_output=True, text=True).stdout
    off = None
    for l in out.splitlines():
        if ".rela.gnu.linkonce.this_module" in l:
            off = int(l.split()[4], 16)
    assert off, "section not found in " + path
    with open(path, "r+b") as f:
        f.seek(off)
        r_offset, r_info, r_addend = struct.unpack("<QQq", f.read(24))
        assert r_offset == want_old, f"{path}: unexpected offset {hex(r_offset)}"
        f.seek(off)
        f.write(struct.pack("<QQq", new, r_info, r_addend))
    print(f"{path}: init reloc {hex(want_old)} -> {hex(new)}")

patch("/lib/modules/5.10.0+/kernel/net/wireless/cfg80211.ko")
patch("/lib/modules/5.10.0+/kernel/net/mac80211/mac80211.ko")
