"""Diff two exact genet-wpt result maps without hiding reason-only movement."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def load(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as stream:
        value = json.load(stream)
    tests = value.get("tests")
    if not isinstance(tests, dict):
        raise ValueError(f"{path} does not carry a `tests` object")
    return tests


def record(value: Any) -> tuple[str, str | None]:
    if isinstance(value, str):
        return value.lower(), None
    if isinstance(value, dict) and isinstance(value.get("status"), str):
        reason = value.get("reason")
        if reason is not None and not isinstance(reason, str):
            raise ValueError("result reason must be a string or null")
        return value["status"].lower(), reason
    raise ValueError("result must be a status string or an object with `status`")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--write-json", type=Path)
    parser.add_argument("--fail-on-loss", action="store_true")
    args = parser.parse_args()

    baseline = load(args.baseline)
    candidate = load(args.candidate)
    baseline_names = set(baseline)
    candidate_names = set(candidate)
    missing = sorted(baseline_names - candidate_names)
    added = sorted(candidate_names - baseline_names)
    status_changes: list[dict[str, Any]] = []
    reason_changes: list[dict[str, Any]] = []

    for name in sorted(baseline_names & candidate_names):
        old_status, old_reason = record(baseline[name])
        new_status, new_reason = record(candidate[name])
        if old_status != new_status:
            status_changes.append(
                {"test": name, "before": old_status, "after": new_status}
            )
        if old_reason != new_reason:
            reason_changes.append(
                {"test": name, "before": old_reason, "after": new_reason}
            )

    gains = [
        change
        for change in status_changes
        if change["before"] != "pass" and change["after"] == "pass"
    ]
    losses = [
        change
        for change in status_changes
        if change["before"] == "pass" and change["after"] != "pass"
    ]
    summary = {
        "baseline_tests": len(baseline),
        "candidate_tests": len(candidate),
        "added": added,
        "missing": missing,
        "status_changes": status_changes,
        "reason_changes": reason_changes,
        "gains": gains,
        "losses": losses,
    }

    print(
        "wpt result diff: "
        f"baseline={len(baseline)} candidate={len(candidate)} "
        f"gains={len(gains)} losses={len(losses)} "
        f"other-status={len(status_changes) - len(gains) - len(losses)} "
        f"reason={len(reason_changes)} added={len(added)} missing={len(missing)}"
    )
    for heading, changes in (
        ("LOSSES", losses),
        ("GAINS", gains),
        ("OTHER STATUS CHANGES", [
            change
            for change in status_changes
            if change not in gains and change not in losses
        ]),
        ("REASON CHANGES", reason_changes),
    ):
        if not changes:
            continue
        print(f"\n{heading} ({len(changes)})")
        for change in changes:
            print(
                f"  {change['before'] or '-'} -> {change['after'] or '-'}  "
                f"{change['test']}"
            )

    if added:
        print(f"\nADDED ({len(added)})")
        for name in added:
            print(f"  {name}")
    if missing:
        print(f"\nMISSING ({len(missing)})")
        for name in missing:
            print(f"  {name}")

    if args.write_json:
        args.write_json.parent.mkdir(parents=True, exist_ok=True)
        args.write_json.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")

    return 1 if args.fail_on_loss and (losses or missing) else 0


if __name__ == "__main__":
    raise SystemExit(main())
