#!/usr/bin/env python3
"""Does a GitHub release channel behave the way the publication design assumes?

T-003, and the three claims the publication topology rests on:

    C-14  `/releases/latest` resolves to the newest non-prerelease by date,
          **not** to a tag literally named `latest`
    C-15  a release asset can be replaced, and
          `.../releases/download/<tag>/<name>` stays a stable URL
    C-17  moving a git tag updates the associated release's target

⛔ **This one mutates.** It creates releases and tags in **this repository**
and deletes them again. RULES 13.1 sanctions exactly that and nothing else:
every other repository is read-only, under any framing.

⚠ **One tag deliberately breaks the `test-*` convention.** C-14 is the question
"does a tag literally named `latest` win?", and it cannot be asked without a
tag literally named `latest`. It exists for the length of one run and the
cleanup verifies it is gone.

THE CONTROLS

    tier 0   the repository responds and is the one we think it is, and its
             release and tag counts are recorded **before** anything is
             created -- so cleanup is checked against the state we found
             rather than against zero.
    tier 1   an asset fetched over plain HTTPS with no credentials, which is
             how a consumer reads it. Fetching it through the API would
             measure our token instead of the published URL.

⭐ **Every subject is measured twice where the answer could be a cache.** An
asset is fetched before and after replacement, and the second fetch is the one
that decides C-15.

Exit codes:
    0  measured
    1  measured, and an `--expect` was violated
    2  could not run
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _conditions  # noqa: E402

REPO = "Azathothas/Trackers"

#: Everything this run creates. Named so a human reading the release list knows
#: what it is, and so cleanup can find them without guessing.
TAG_LATEST = "latest"
TAG_NEWER = "test-t003-newer"
TAG_PRERELEASE = "test-t003-prerelease"
TAG_MOVE = "test-t003-move"
ALL_TAGS = (TAG_LATEST, TAG_NEWER, TAG_PRERELEASE, TAG_MOVE)

ASSET = "t003-probe.txt"
#: Deliberately different lengths. Equal-length payloads make `size` useless as
#: a discriminator, and `size` is what distinguishes "the replacement never
#: happened" from "it happened and a cache is still serving the old bytes".
ASSET_V1 = b"version one\n"
ASSET_V2 = b"version two, and longer so the size differs\n"

#: How long to keep asking after a replacement. One fetch cannot tell a cache
#: from a refutation, and RULES 2 requires a control that isolates the cause
#: before one is named.
RECHECK_DELAYS = (0, 10, 30)


class CouldNotRun(RuntimeError):
    """Exit 2. The question was not answered and nothing pretends it was."""


def gh(*args: str, check: bool = True) -> str:
    """One `gh` invocation. Returns stdout; never merges stderr into it.

    stdout and stderr are different streams and anything reading a value reads
    stdout alone (`docs/conventions/shell.md` section 3).
    """
    proc = subprocess.run(("gh",) + args, capture_output=True, text=True,
                          encoding="utf-8", errors="replace", timeout=120)
    if check and proc.returncode != 0:
        raise CouldNotRun(f"gh {' '.join(args)} exited {proc.returncode}: "
                          f"{proc.stderr.strip()[:400]}")
    return proc.stdout.strip()


def api(path: str, *extra: str) -> object:
    # An empty path means the repository itself. Joining it with a separator
    # anyway produces a trailing slash, which GitHub answers with 404 -- caught
    # by the tier 0 control on the first run, which is what tier 0 is for.
    route = f"repos/{REPO}/{path}" if path else f"repos/{REPO}"
    out = gh("api", route, *extra)
    return json.loads(out) if out else None


def fetch(url: str) -> dict:
    """Fetch as a consumer does: plain HTTPS, no credentials.

    ⛔ Not through the API. A token would make this a measurement of our own
    access rather than of the URL the README tells people to use.
    """
    req = urllib.request.Request(url, headers={"User-Agent": "curl/8.5.0"})
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            body = resp.read(64 * 1024)
            return {
                "status": resp.status,
                "final_url": resp.geturl(),
                "body": body.decode("utf-8", errors="replace"),
                "etag": resp.headers.get("ETag", _conditions.UNKNOWN),
                "cache_control": resp.headers.get("Cache-Control",
                                                  _conditions.UNKNOWN),
                "content_type": resp.headers.get("Content-Type",
                                                 _conditions.UNKNOWN),
            }
    except urllib.error.HTTPError as e:
        return {"status": e.code, "final_url": url, "body": "",
                "etag": _conditions.UNKNOWN,
                "cache_control": _conditions.UNKNOWN,
                "content_type": _conditions.UNKNOWN}


def delete_everything(report: list[str]) -> None:
    """Remove every release and tag this script creates. Idempotent.

    RULES 13.1: a throwaway created to answer a question is deleted once the
    answer is recorded. A throwaway that outlives its question is litter in
    somebody's release list.
    """
    for tag in ALL_TAGS:
        proc = subprocess.run(("gh", "release", "delete", tag, "--repo", REPO,
                               "--yes", "--cleanup-tag"),
                              capture_output=True, text=True,
                              encoding="utf-8", errors="replace", timeout=120)
        if proc.returncode == 0:
            report.append(f"deleted release and tag {tag}")
        # A tag can outlive its release if `--cleanup-tag` did not reach it.
        ref = subprocess.run(("gh", "api", "-X", "DELETE",
                              f"repos/{REPO}/git/refs/tags/{tag}"),
                             capture_output=True, text=True,
                             encoding="utf-8", errors="replace", timeout=120)
        if ref.returncode == 0:
            report.append(f"deleted leftover tag ref {tag}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--expect-design", action="store_true",
                    help="exit 1 if the publication design's assumptions are "
                         "violated. Off by default: the job is to find out "
                         "what is true, not to assert what was hoped.")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    results: dict[str, object] = {}
    cleanup: list[str] = []

    try:
        # -- tier 0 -----------------------------------------------------------
        repo = api("")
        if not isinstance(repo, dict) or repo.get("full_name") != REPO:
            raise CouldNotRun(f"tier 0: expected {REPO}, got {repo!r:.120}")
        before_releases = api("releases", "--jq", "length")
        before_tags = api("tags", "--jq", "length")
        results["tier0"] = {
            "repo": repo.get("full_name"),
            "can_push": bool(repo.get("permissions", {}).get("push")),
            "releases_before": before_releases,
            "tags_before": before_tags,
        }
        if not repo.get("permissions", {}).get("push"):
            raise CouldNotRun("tier 0: no push permission; nothing to measure")

        head = gh("api", f"repos/{REPO}/commits/HEAD", "--jq", ".sha")
        parent = gh("api", f"repos/{REPO}/commits/HEAD",
                    "--jq", ".parents[0].sha")
        if not head or not parent:
            raise CouldNotRun("tier 0: could not read two commits to move a tag between")

        # Start from a clean slate even if a previous run died mid-way.
        delete_everything(cleanup)

        # -- C-14 -------------------------------------------------------------
        # Created oldest first, so "newest by date" and "named latest" point at
        # different releases and the endpoint has to choose.
        gh("release", "create", TAG_LATEST, "--repo", REPO, "--target", head,
           "--title", "T-003 throwaway: tag literally named latest",
           "--notes", "Created by experiments/24 to answer C-14. Deleted "
                      "immediately afterwards.")
        time.sleep(2)
        gh("release", "create", TAG_NEWER, "--repo", REPO, "--target", head,
           "--title", "T-003 throwaway: newer non-prerelease",
           "--notes", "Created by experiments/24 to answer C-14.")
        time.sleep(2)

        resolved = api("releases/latest")
        results["c14_two_releases"] = {
            "created_order": [TAG_LATEST, TAG_NEWER],
            "releases_latest_resolves_to": resolved.get("tag_name"),
            "resolves_to_newest_non_prerelease":
                resolved.get("tag_name") == TAG_NEWER,
            "resolves_to_tag_named_latest":
                resolved.get("tag_name") == TAG_LATEST,
        }

        # A newer PRERELEASE must not win, which is the half the channel design
        # actually rests on.
        gh("release", "create", TAG_PRERELEASE, "--repo", REPO, "--target",
           head, "--prerelease",
           "--title", "T-003 throwaway: newest, prerelease",
           "--notes", "Created by experiments/24 to answer C-14.")
        time.sleep(2)
        resolved2 = api("releases/latest")
        results["c14_with_prerelease"] = {
            "newest_release_is": TAG_PRERELEASE,
            "releases_latest_resolves_to": resolved2.get("tag_name"),
            "prerelease_ignored": resolved2.get("tag_name") == TAG_NEWER,
        }

        # -- C-15 -------------------------------------------------------------
        here = os.path.dirname(os.path.abspath(__file__))
        scratch = os.path.join(here, "results", ".t003-scratch")
        os.makedirs(scratch, exist_ok=True)
        asset_path = os.path.join(scratch, ASSET)
        url = f"https://github.com/{REPO}/releases/download/{TAG_NEWER}/{ASSET}"

        def asset_meta() -> dict:
            """What the API says the asset is, which is the server-side truth.

            ⭐ This is the control. If the id and size change and the URL still
            serves the old bytes, the replacement happened and something is
            caching. If they do not change, the replacement never happened and
            no amount of waiting will help. One fetch cannot tell those apart.
            """
            assets = api(f"releases/tags/{TAG_NEWER}", "--jq", ".assets")
            for a in assets or []:
                if a.get("name") == ASSET:
                    return {"id": a.get("id"), "size": a.get("size"),
                            "updated_at": a.get("updated_at"),
                            "download_count": a.get("download_count")}
            return {}

        with open(asset_path, "wb") as fh:
            fh.write(ASSET_V1)
        gh("release", "upload", TAG_NEWER, asset_path, "--repo", REPO)
        time.sleep(3)
        meta_v1 = asset_meta()
        first = fetch(url)

        with open(asset_path, "wb") as fh:
            fh.write(ASSET_V2)
        gh("release", "upload", TAG_NEWER, asset_path, "--repo", REPO,
           "--clobber")
        meta_v2 = asset_meta()

        rechecks = []
        for delay in RECHECK_DELAYS:
            if delay:
                time.sleep(delay)
            got = fetch(url)
            rechecks.append({
                "after_seconds": sum(RECHECK_DELAYS[:RECHECK_DELAYS.index(delay) + 1]),
                "status": got["status"],
                "body": got["body"],
                "etag": got["etag"],
                "serves_v2": got.get("body") == ASSET_V2.decode(),
            })
        second = rechecks[-1]

        served_new = any(r["serves_v2"] for r in rechecks)
        replaced_server_side = (meta_v1.get("id") != meta_v2.get("id")
                                or meta_v1.get("size") != meta_v2.get("size"))
        results["c15_asset_replacement"] = {
            "url": url,
            "asset_before_replacement": meta_v1,
            "asset_after_replacement": meta_v2,
            "replaced_server_side": replaced_server_side,
            "first": first,
            "rechecks": rechecks,
            "url_is_stable": first["status"] == 200 and second["status"] == 200,
            "content_updated": served_new,
            "etag_changed": first.get("etag") != second.get("etag"),
            "propagation_window_seconds": next(
                (r["after_seconds"] for r in rechecks if r["serves_v2"]), None),
            "reading": (
                "url stable and new content served" if served_new else
                "url stable, replacement landed server-side, but the download "
                "URL still served the old bytes for the whole observation "
                "window -- a cache, not a failed replacement"
                if replaced_server_side else
                "the replacement did not land server-side; the URL is not the "
                "thing that failed"),
        }
        try:
            os.remove(asset_path)
            os.rmdir(scratch)
        except OSError:
            pass

        # -- C-17 -------------------------------------------------------------
        gh("release", "create", TAG_MOVE, "--repo", REPO, "--target", parent,
           "--title", "T-003 throwaway: tag move",
           "--notes", "Created by experiments/24 to answer C-17.")
        time.sleep(2)
        before = api(f"releases/tags/{TAG_MOVE}")
        ref_before = gh("api", f"repos/{REPO}/git/refs/tags/{TAG_MOVE}",
                        "--jq", ".object.sha")

        gh("api", "-X", "PATCH", f"repos/{REPO}/git/refs/tags/{TAG_MOVE}",
           "-f", f"sha={head}", "-F", "force=true")
        time.sleep(2)
        after = api(f"releases/tags/{TAG_MOVE}")
        ref_after = gh("api", f"repos/{REPO}/git/refs/tags/{TAG_MOVE}",
                       "--jq", ".object.sha")

        results["c17_tag_move"] = {
            "tag_sha_before": ref_before,
            "tag_sha_after": ref_after,
            "tag_actually_moved": ref_before != ref_after and ref_after == head,
            "release_target_commitish_before": before.get("target_commitish"),
            "release_target_commitish_after": after.get("target_commitish"),
            "release_target_followed_the_tag":
                before.get("target_commitish") != after.get("target_commitish"),
            "tarball_url": after.get("tarball_url"),
        }

    except CouldNotRun as exc:
        delete_everything(cleanup)
        print(f"COULD NOT RUN: {exc}", file=sys.stderr)
        print("cleanup: " + ("; ".join(cleanup) or "nothing to remove"),
              file=sys.stderr)
        return 2
    except Exception as exc:  # noqa: BLE001 - the cleanup matters more
        delete_everything(cleanup)
        print(f"COULD NOT RUN: {type(exc).__name__}: {exc}", file=sys.stderr)
        print("cleanup: " + ("; ".join(cleanup) or "nothing to remove"),
              file=sys.stderr)
        return 2

    # -- cleanup, and proof of it ---------------------------------------------
    delete_everything(cleanup)
    time.sleep(2)
    after_releases = api("releases", "--jq", "length")
    after_tags = api("tags", "--jq", "length")
    results["cleanup"] = {
        "actions": cleanup,
        "releases_after": after_releases,
        "tags_after": after_tags,
        "returned_to_starting_state": (after_releases == before_releases
                                       and after_tags == before_tags),
    }

    conditions = _conditions.collect(sample_counts={
        "releases_created": len(ALL_TAGS), "assets_uploaded": 2})
    conditions["repository"] = REPO
    out = args.out or _conditions.results_path(__file__)
    _conditions.emit(
        "Does a GitHub release channel behave the way the publication design "
        "assumes? (C-14, C-15, C-17)",
        conditions, results, out)

    for key in ("c14_two_releases", "c14_with_prerelease",
                "c15_asset_replacement", "c17_tag_move", "cleanup"):
        print(f"\n{key}:")
        for k, v in results[key].items():
            if isinstance(v, dict):
                v = {kk: (str(vv)[:60]) for kk, vv in v.items()}
            print(f"  {k}: {v}")

    if not results["cleanup"]["returned_to_starting_state"]:
        print("\n⛔ CLEANUP INCOMPLETE. A throwaway outlived its question.",
              file=sys.stderr)
        return 1

    if args.expect_design:
        failed = []
        if not results["c14_two_releases"]["resolves_to_newest_non_prerelease"]:
            failed.append("C-14: /releases/latest did not resolve to the "
                          "newest non-prerelease")
        if not results["c14_with_prerelease"]["prerelease_ignored"]:
            failed.append("C-14: a prerelease won /releases/latest")
        if not results["c15_asset_replacement"]["url_is_stable"]:
            failed.append("C-15: the download URL was not stable")
        if not results["c15_asset_replacement"]["replaced_server_side"]:
            failed.append("C-15: the replacement did not land server-side")
        # ⛔ Deliberately NOT asserted: that new content is *served* within the
        # observation window. That is a propagation time, it was measured
        # between 10 and 40 seconds on 2026-09-05, and it is somebody else's
        # CDN. Asserting it would make this check fail for a reason nobody
        # cares about, which trains people to ignore the ones that matter.
        # It is recorded as `propagation_window_seconds` instead.
        if failed:
            print("\nEXPECTATION VIOLATED:", file=sys.stderr)
            for f in failed:
                print(f"  - {f}", file=sys.stderr)
            return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
