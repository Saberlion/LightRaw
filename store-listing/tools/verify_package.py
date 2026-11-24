"""Verify an AGC submission .app: inner HAP permissions, no sourcemap, stripped .so.

Usage: python verify_package.py <path-to-entry-default.app>
Checks (docs/HARMONYOS_PORTING.md §6.17 / .csr/SIGNING.md §8.2):
  1. .app entries == {entry-default.hap, pack.info}
  2. HAP module.json requestPermissions == expected set (7 perms: 2 system_grant
     + 5 user_grant, matching the AGC hosted privacy declaration — 2026-11-23 方案 C)
  3. no ets/sourceMaps.map entry
  4. every packed .so: no .debug_* sections, no SHT_SYMTAB
  5. prints sizes + SHA256 of .app and inner HAP
"""
import hashlib
import io
import json
import struct
import sys
import zipfile

EXPECTED = {"ohos.permission.INTERNET", "ohos.permission.FILE_ACCESS_PERSIST",
            "ohos.permission.WRITE_IMAGEVIDEO",
            "ohos.permission.READ_WRITE_DESKTOP_DIRECTORY",
            "ohos.permission.READ_WRITE_DOCUMENTS_DIRECTORY",
            "ohos.permission.READ_WRITE_DOWNLOAD_DIRECTORY",
            "ohos.permission.MEDIA_LOCATION"}
# v3 removal, must never come back (docs/HARMONYOS_PORTING.md §6.17)
FORBIDDEN = {"ohos.permission.READ_IMAGEVIDEO"}

def sha(b):
    return hashlib.sha256(b).hexdigest().upper()

def elf_issues(data):
    if data[:4] != b"\x7fELF" or data[4] != 2:
        return ["not ELF64"]
    e_shoff, = struct.unpack_from("<Q", data, 0x28)
    e_shentsize, e_shnum, e_shstrndx = struct.unpack_from("<HHH", data, 0x3A)
    if e_shoff == 0 or e_shnum == 0:
        return []
    hdrs = [struct.unpack_from("<IIQQQQIIQQ", data, e_shoff + i * e_shentsize) for i in range(e_shnum)]
    strtab = hdrs[e_shstrndx]
    names = data[strtab[4]:strtab[4] + strtab[5]]
    issues = []
    for h in hdrs:
        nm = names[h[0]:].split(b"\0")[0].decode(errors="replace")
        if nm.startswith(".debug_"):
            issues.append(f"debug section {nm}")
        if h[1] == 2:  # SHT_SYMTAB
            issues.append("symtab present")
    return issues

def main(path):
    app = open(path, "rb").read()
    print("app:", path, len(app), "B  SHA256", sha(app))
    z = zipfile.ZipFile(io.BytesIO(app))
    names = z.namelist()
    print("entries:", names)
    assert set(names) == {"entry-default.hap", "pack.info"}, "unexpected .app entries"
    hap = z.read("entry-default.hap")
    print("inner hap:", len(hap), "B  SHA256", sha(hap))
    hz = zipfile.ZipFile(io.BytesIO(hap))
    hn = hz.namelist()
    assert "ets/sourceMaps.map" not in hn, "sourceMaps.map packed!"
    mj = json.loads(hz.read("module.json"))
    mod = mj.get("module", mj)
    perms = [p["name"] for p in mod.get("requestPermissions", [])]
    print("permissions:", perms)
    print("deviceTypes:", mod.get("deviceTypes"))
    bad = set(perms) & FORBIDDEN
    assert not bad, f"forbidden permissions present: {bad}"
    assert set(perms) == EXPECTED, f"permission set differs from expected: {set(perms) ^ EXPECTED}"
    sos = [n for n in hn if n.endswith(".so")]
    print("so count:", len(sos))
    worst = []
    for n in sos:
        d = hz.read(n)
        iss = elf_issues(d)
        if iss:
            worst.append((n, iss))
        print(f"  {n}  {len(d)/1e6:.1f} MB  {'OK' if not iss else iss}")
    assert not worst, f"unstripped libs: {worst}"
    print("VERIFY OK: 7 permissions (5 user_grant, matches AGC declaration), no sourcemap, all .so stripped")

if __name__ == "__main__":
    main(sys.argv[1])