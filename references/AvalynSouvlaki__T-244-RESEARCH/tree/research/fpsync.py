#!/usr/bin/env python3
"""
fpsync — keep browser-impersonation profiles honest as browsers ship new versions.

Standalone: Python 3.8+, standard library only, no pip install.

Browsers ship every few weeks. A profile table pinned to Chrome 151 is a
*correct* fingerprint of a browser nobody runs any more, which is its own tell.
This tool answers three questions on a schedule:

  1. What is actually stable right now?          fpsync.py versions
  2. Has our profile table drifted behind it?    fpsync.py drift   (exit 1 on drift)
  3. What does a REAL browser put on the wire?   fpsync.py capture (ground truth)

`capture` is the authoritative one: it drives a locally installed Chrome/Edge at
`tlsprobe` and records the JA4 and Akamai fingerprint the real browser emits, so
you can diff your impersonation against the thing itself rather than against a
blog post. Everything else is a cheap early warning.

Typical CI use:

    fpsync.py drift --impit-src vendor/impit || echo "profiles are behind stable"
    fpsync.py report --json > fingerprints/upstream-$(date +%F).json
"""

from __future__ import annotations

import argparse
import io
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import time
import urllib.error
import urllib.request

UA = "fpsync/1.0 (+profile drift check)"
TIMEOUT = 25


# ---------------------------------------------------------------- http

def get(url: str, accept: str = "application/json"):
    """GET with a real UA. Returns parsed JSON, or text when not JSON."""
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": accept})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        raw = r.read().decode("utf-8", "replace")
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return raw


def safe(fn, *a, **kw):
    """Never let one unreachable API kill the whole run."""
    try:
        return fn(*a, **kw)
    except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError, OSError, ValueError) as e:
        return {"error": f"{type(e).__name__}: {e}"}


# ------------------------------------------------- upstream stable versions

def chrome_stable(pf: str = "linux") -> dict:
    """Google's own version history API. Platforms: linux, win, mac, android."""
    d = get(f"https://versionhistory.googleapis.com/v1/chrome/platforms/{pf}"
            f"/channels/stable/versions?pageSize=1")
    v = d["versions"][0]["version"]
    return {"platform": pf, "version": v, "major": int(v.split(".")[0])}


def firefox_stable() -> dict:
    d = get("https://product-details.mozilla.org/1.0/firefox_versions.json")
    v = d["LATEST_FIREFOX_VERSION"]
    return {"version": v, "major": int(v.split(".")[0]),
            "esr": d.get("FIREFOX_ESR", "")}


def edge_stable() -> dict:
    """Microsoft's enterprise feed. Large payload — pull only what we need."""
    d = get("https://edgeupdates.microsoft.com/api/products?view=enterprise")
    for p in d:
        if p.get("Product") == "Stable":
            vs = sorted({r["ProductVersion"] for r in p.get("Releases", [])},
                        key=lambda s: [int(x) for x in s.split(".")], reverse=True)
            if vs:
                return {"version": vs[0], "major": int(vs[0].split(".")[0])}
    return {"error": "no Stable product in feed"}


def safari_stable() -> dict:
    # Apple publishes no machine-readable version feed. Track by hand.
    return {"note": "no public API; Safari/WebKit versions must be tracked manually"}


def all_versions() -> dict:
    return {
        "fetched_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "chrome": {pf: safe(chrome_stable, pf) for pf in ("linux", "win", "mac")},
        "firefox": safe(firefox_stable),
        "edge": safe(edge_stable),
        "safari": safari_stable(),
    }


# ------------------------------------------------- what the crates ship

def impit_profiles(src: str) -> dict:
    """Parse impit's fingerprint database re-export list."""
    path = os.path.join(src, "impit", "src", "fingerprint", "database.rs")
    if not os.path.isfile(path):
        alt = os.path.join(src, "src", "fingerprint", "database.rs")
        path = alt if os.path.isfile(alt) else path
    if not os.path.isfile(path):
        return {"error": f"database.rs not found under {src}"}
    text = open(path, encoding="utf-8").read()
    out: dict[str, list[int]] = {}
    for fam in ("chrome", "firefox", "safari", "okhttp"):
        vs = sorted({int(m) for m in re.findall(rf"\b{fam}_(\d+)\b", text)})
        if vs:
            out[fam] = vs
    ios = sorted({int(m) for m in re.findall(r"\bios_(\d+)\b", text)})
    if ios:
        out["ios"] = ios
    return out


def wreq_util_profiles(version: str | None = None) -> dict:
    """Emulation variants in a published wreq-util, read from the crate source.

    Reads the .crate tarball rather than docs.rs: rendered docs move around and
    feature-gated items may not be built, but the published source is the source.
    """
    if not version:
        version = get("https://crates.io/api/v1/crates/wreq-util")["crate"]["max_stable_version"]
    url = f"https://static.crates.io/crates/wreq-util/wreq-util-{version}.crate"
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        blob = r.read()

    text = []
    with tarfile.open(fileobj=io.BytesIO(blob)) as t:
        for name in t.getnames():
            if name.endswith(".rs"):
                f = t.extractfile(name)
                if f:
                    text.append(f.read().decode("utf-8", "replace"))
    src = "".join(text)

    fams: dict[str, list[int]] = {}
    for fam in ("Chrome", "Firefox", "Edge", "Safari", "Opera", "OkHttp"):
        vs = sorted({int(m) for m in re.findall(rf"\b{fam}(\d+)", src)})
        if vs:
            fams[fam.lower()] = vs
    return {"version": version, "profiles": fams,
            "total": sum(len(v) for v in fams.values())}


# ------------------------------------------------- drift

def drift(impit_src: str | None) -> dict:
    up = all_versions()
    rep: dict = {"upstream": up, "drift": []}

    def note(what, ours, theirs, sev):
        rep["drift"].append({"what": what, "ours": ours, "stable": theirs, "severity": sev})

    if impit_src:
        prof = impit_profiles(impit_src)
        rep["impit_profiles"] = prof
        if "error" not in prof:
            ch = up["chrome"]["linux"]
            if "major" in ch and prof.get("chrome"):
                ours = max(prof["chrome"])
                if ours < ch["major"]:
                    note("impit chrome profile", ours, ch["major"],
                         "high" if ch["major"] - ours >= 2 else "low")
            ff = up["firefox"]
            if "major" in ff and prof.get("firefox"):
                ours = max(prof["firefox"])
                if ours < ff["major"]:
                    note("impit firefox profile", ours, ff["major"],
                         "high" if ff["major"] - ours >= 2 else "low")

    wu = safe(wreq_util_profiles)
    rep["wreq_util"] = wu
    if isinstance(wu, dict) and wu.get("profiles", {}).get("chrome"):
        ch = up["chrome"]["linux"]
        if "major" in ch:
            theirs = max(wu["profiles"]["chrome"])
            if theirs < ch["major"]:
                note("wreq-util chrome profile", theirs, ch["major"], "info")
    return rep


# ------------------------------------------------- ground truth capture

CANDIDATES = {
    "Linux": ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser",
              "microsoft-edge", "microsoft-edge-stable", "brave-browser"],
    "Darwin": ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
               "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
               "/Applications/Chromium.app/Contents/MacOS/Chromium"],
    "Windows": [r"C:\Program Files\Google\Chrome\Application\chrome.exe",
                r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
                r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"],
}


def find_browser(explicit: str | None) -> str | None:
    """The §10 resolver: explicit override, then PATH, then platform defaults."""
    if explicit:
        return explicit if os.path.isfile(explicit) or shutil.which(explicit) else None
    for c in CANDIDATES.get(platform.system(), CANDIDATES["Linux"]):
        p = shutil.which(c) if not os.path.isabs(c) else (c if os.path.isfile(c) else None)
        if p:
            return p
    return None


def capture(browser: str | None, tlsprobe: str, port: int) -> dict:
    """Drive a real installed browser at tlsprobe and record what it emits."""
    exe = find_browser(browser)
    if not exe:
        return {"error": "no installed browser found",
                "searched": CANDIDATES.get(platform.system(), CANDIDATES["Linux"])}
    if not (os.path.isfile(tlsprobe) and os.access(tlsprobe, os.X_OK)):
        return {"error": f"tlsprobe not executable at {tlsprobe}",
                "hint": "cd research/tlsprobe && cargo build --release"}

    ver = ""
    try:
        ver = subprocess.run([exe, "--version"], capture_output=True, text=True,
                             timeout=20).stdout.strip()
    except (subprocess.SubprocessError, OSError):
        pass

    probe = subprocess.Popen([tlsprobe, "--port", str(port), "--json", "--once"],
                             stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    time.sleep(1.5)
    proc = None
    try:
        proc = subprocess.Popen(
            [exe, "--headless=new", "--disable-gpu", "--no-sandbox",
             "--ignore-certificate-errors", "--no-first-run", "--no-default-browser-check",
             f"--user-data-dir={os.path.join(os.path.sep, 'tmp', 'fpsync-profile')}",
             f"https://127.0.0.1:{port}/"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            env={**os.environ, "NO_PROXY": "127.0.0.1,localhost", "no_proxy": "127.0.0.1,localhost"})
        out, _ = probe.communicate(timeout=60)
    except subprocess.TimeoutExpired:
        probe.kill()
        out = ""
    finally:
        if proc:
            proc.terminate()

    line = next((l for l in (out or "").splitlines() if l.startswith("{")), None)
    if not line:
        return {"browser": exe, "browser_version": ver,
                "error": "browser did not complete a handshake with tlsprobe"}
    fp = json.loads(line)
    return {"browser": exe, "browser_version": ver, "captured_at":
            time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "ja4": fp.get("ja4"), "ja4_r": fp.get("ja4_r"),
            "akamai": fp.get("akamai"), "header_order": fp.get("header_order"),
            "note": "THIS is ground truth — diff your impersonation against it"}


# ------------------------------------------------- cli

def main() -> int:
    ap = argparse.ArgumentParser(
        description="Track browser releases and keep impersonation profiles current.",
        formatter_class=argparse.RawDescriptionHelpFormatter, epilog=__doc__)
    # `--json` is accepted both before and after the subcommand, because both
    # readings are natural and neither should be an error.
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--json", action="store_true", help="machine-readable output")
    ap.add_argument("--json", action="store_true", help="machine-readable output")
    sub = ap.add_subparsers(dest="cmd", required=True)

    sub.add_parser("versions", parents=[common],
                   help="current stable versions from official APIs")
    sub.add_parser("upstream", parents=[common],
                   help="profiles shipped by the newest wreq-util")

    d = sub.add_parser("drift", parents=[common],
                       help="compare local profiles against stable; exit 1 on drift")
    d.add_argument("--impit-src", help="path to an impit checkout")

    c = sub.add_parser("capture", parents=[common],
                       help="capture a real installed browser's fingerprint")
    c.add_argument("--browser", help="explicit browser binary (overrides discovery)")
    c.add_argument("--tlsprobe", default="./tlsprobe/target/release/tlsprobe")
    c.add_argument("--port", type=int, default=8443)

    r = sub.add_parser("report", parents=[common], help="everything at once")
    r.add_argument("--impit-src")

    a = ap.parse_args()

    if a.cmd == "versions":
        out = all_versions()
    elif a.cmd == "upstream":
        out = safe(wreq_util_profiles)
    elif a.cmd == "drift":
        out = drift(a.impit_src)
    elif a.cmd == "capture":
        out = capture(a.browser, a.tlsprobe, a.port)
    else:
        out = {"versions": all_versions(), "wreq_util": safe(wreq_util_profiles)}
        if a.impit_src:
            out["impit_profiles"] = impit_profiles(a.impit_src)

    if a.json:
        print(json.dumps(out, indent=2, sort_keys=True))
    else:
        render(a.cmd, out)

    if a.cmd == "drift" and out.get("drift"):
        for x in out["drift"]:
            print(f"DRIFT [{x['severity']}] {x['what']}: ours={x['ours']} stable={x['stable']}",
                  file=sys.stderr)
        return 1
    if isinstance(out, dict) and out.get("error"):
        return 2
    return 0


def render(cmd: str, out: dict) -> None:
    if cmd in ("versions", "report", "drift"):
        v = out.get("versions") or out.get("upstream") or out
        if "chrome" in v:
            print("current stable")
            for pf, d in v["chrome"].items():
                print(f"  chrome/{pf:<5} {d.get('version', d.get('error'))}")
            ff, ed = v.get("firefox", {}), v.get("edge", {})
            print(f"  firefox      {ff.get('version', ff.get('error'))}  (esr {ff.get('esr','?')})")
            print(f"  edge         {ed.get('version', ed.get('error'))}")
    if out.get("impit_profiles") and "error" not in out["impit_profiles"]:
        print("\nimpit profiles")
        for fam, vs in out["impit_profiles"].items():
            print(f"  {fam:<8} newest={max(vs):<5} ({len(vs)} profiles)")
    wu = out.get("wreq_util") or (out if cmd == "upstream" else None)
    if isinstance(wu, dict) and wu.get("profiles"):
        print(f"\nwreq-util {wu['version']} — {wu.get('total', '?')} variants")
        for fam, vs in wu["profiles"].items():
            print(f"  {fam:<8} newest={max(vs):<5} ({len(vs)} variants)")
    if cmd == "capture":
        if out.get("error"):
            print(f"capture failed: {out['error']}")
            if out.get("searched"):
                print("  searched: " + ", ".join(out["searched"]))
        else:
            print(f"ground truth from {out['browser_version'] or out['browser']}")
            print(f"  JA4     {out['ja4']}")
            print(f"  Akamai  {out['akamai']}")
            if out.get("header_order"):
                print("  headers " + ", ".join(out["header_order"]))
    if cmd == "drift" and not out.get("drift"):
        print("\nno drift — profiles match current stable")


if __name__ == "__main__":
    sys.exit(main())
