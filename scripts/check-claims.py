#!/usr/bin/env python3
"""Verify every claim in claims.toml is backed by a test that really exists, then run them.

See claims.toml's own header for why this exists and what belongs in it. In short: this project
asserts a great deal about itself in prose, and prose does not fail when it stops being true.

Usage:
    scripts/check-claims.py            # verify the named tests exist, then run them
    scripts/check-claims.py --list     # verify only; don't run anything

Exit status is 0 only when every claim names a test that exists and that test passes.
"""

from __future__ import annotations

import subprocess
import sys
import tomllib
from collections import defaultdict
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / "claims.toml"

# Tests behind an optional feature don't exist in a default build, so `--list` would report them
# missing and this check would fail for a claim that is perfectly well covered. These are the
# same features .github/workflows/ci.yml's `optional features` job builds.
FEATURES = [
    "hyperion-console/real-http",
    "hyperion-console/openai-compat",
    "hyperion-console/anthropic",
    "hyperion-console/gemini",
    "hyperion-console/mdns",
]


def cargo(args: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["cargo", *args], cwd=REPO_ROOT, capture_output=True, text=True, check=False
    )


def feature_args(crate: str) -> list[str]:
    """`--features` only accepts features of crates in the same invocation."""
    relevant = [f for f in FEATURES if f.split("/")[0] == crate]
    return ["--features", ",".join(relevant)] if relevant else []


def tests_in(crate: str) -> set[str]:
    """Every test `cargo test -p <crate> -- --list` reports, by its full path."""
    result = cargo(["test", "--locked", "-p", crate, *feature_args(crate), "--", "--list"])
    if result.returncode != 0:
        sys.exit(
            f"could not list tests for {crate} -- the crate must build before its claims can be "
            f"checked:\n{result.stderr}"
        )
    return {
        line.removesuffix(": test")
        for line in result.stdout.splitlines()
        if line.endswith(": test")
    }


def main() -> int:
    list_only = "--list" in sys.argv[1:]

    manifest = tomllib.loads(MANIFEST.read_text())
    claims = manifest.get("claim", [])
    if not claims:
        sys.exit(f"{MANIFEST} declares no claims -- that is a finding, not a pass.")

    by_crate: dict[str, list[dict]] = defaultdict(list)
    for claim in claims:
        for field in ("id", "claim", "crate", "test"):
            if not claim.get(field):
                sys.exit(f"a claim is missing its `{field}`: {claim}")
        by_crate[claim["crate"]].append(claim)

    duplicates = {c["id"] for c in claims if [x["id"] for x in claims].count(c["id"]) > 1}
    if duplicates:
        sys.exit(f"duplicate claim ids: {', '.join(sorted(duplicates))}")

    print(f"checking {len(claims)} claims across {len(by_crate)} crates\n")

    missing: list[tuple[str, str, str]] = []
    for crate in sorted(by_crate):
        known = tests_in(crate)
        for claim in by_crate[crate]:
            if claim["test"] not in known:
                missing.append((claim["id"], crate, claim["test"]))

    if missing:
        print("These claims name a test that does not exist:\n", file=sys.stderr)
        for claim_id, crate, test in missing:
            print(f"  {claim_id}\n    {crate}: {test}", file=sys.stderr)
        print(
            "\nA claim without a live test behind it is prose. Either restore the test, point the "
            "claim at whatever replaced it, or -- if the guarantee genuinely no longer holds -- "
            "remove the claim and say so in the changelog.",
            file=sys.stderr,
        )
        return 1

    print(f"all {len(claims)} claims name a test that exists")
    if list_only:
        return 0

    failed = []
    for crate in sorted(by_crate):
        for claim in by_crate[crate]:
            result = cargo(
                [
                    "test",
                    "--locked",
                    "-p",
                    crate,
                    *feature_args(crate),
                    "--",
                    "--exact",
                    claim["test"],
                ]
            )
            status = "ok " if result.returncode == 0 else "FAIL"
            print(f"  {status}  {claim['id']}")
            if result.returncode != 0:
                failed.append((claim, result.stdout + result.stderr))

    if failed:
        print(f"\n{len(failed)} claim(s) are no longer true:\n", file=sys.stderr)
        for claim, output in failed:
            print(f"--- {claim['id']}: {claim['claim']}", file=sys.stderr)
            print(output[-2000:], file=sys.stderr)
        return 1

    print(f"\nall {len(claims)} claims hold")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
