#!/usr/bin/env python3
"""Focused tests for the Cargo git-dependency state classifier."""

from __future__ import annotations

import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import check_git_dependency_state as check


OWNED = "https://github.com/merely-made/example.git"
OLD = "1" * 40
NEW = "2" * 40


class FakeLocalInspector:
    def __init__(self, checkout: check.LocalCheckout) -> None:
        self.checkout = checkout

    def inspect(self, _manifest: pathlib.Path) -> check.LocalCheckout:
        return self.checkout


def policy(lock_policy: str) -> check.Policy:
    return check.Policy(
        owned_url_prefixes=("https://github.com/merely-made/",),
        lock_policy=lock_policy,
        ignored_owned_drift="error",
        tracked_owned_drift="warning",
        external_drift="warning",
    )


def edge() -> check.DeclaredEdge:
    return check.DeclaredEdge(
        check.GitSource(OWNED, "branch", "main"),
        packages={"example", "example_api"},
    )


def active_git(name: str = "example", sha: str = OLD) -> check.ActivePackage:
    return check.ActivePackage(
        name=name,
        version="0.1.0",
        source=f"git+{OWNED}?branch=main#{sha}",
        manifest_path=pathlib.Path("C:/cargo/git/example/Cargo.toml"),
        package_id=f"{name}@{sha}",
    )


class ManifestTests(unittest.TestCase):
    def test_tree_discovery_descends_through_a_non_cargo_git_parent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            tree = pathlib.Path(directory)
            (tree / ".git").mkdir()
            repository = tree / "repos" / "example"
            repository.mkdir(parents=True)
            (repository / ".git").mkdir()
            (repository / "Cargo.toml").write_text(
                '[package]\nname = "example"\nversion = "0.1.0"\n',
                encoding="utf-8",
            )

            repositories = check.discover_repositories(tree)

        self.assertEqual(repositories, [repository.resolve()])

    def test_only_real_dependency_tables_are_scanned_and_edges_are_grouped(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            manifest = pathlib.Path(directory) / "Cargo.toml"
            manifest.write_text(
                f"""
[package]
name = "fixture"
version = "0.1.0"

[package.metadata.docs.rs]
src_base_git = "https://github.com/not-a-dependency/example"

[dependencies]
example = {{ git = "{OWNED}", branch = "main" }}
example_api = {{ git = "{OWNED}", branch = "main" }}

[target.'cfg(windows)'.dependencies]
pinned = {{ git = "https://github.com/upstream/pinned", rev = "abc123" }}
""",
                encoding="utf-8",
            )
            edges = check.collect_manifest_edges([manifest])

        self.assertEqual(len(edges), 2)
        grouped = edges[check.GitSource(OWNED, "branch", "main").key]
        self.assertEqual(grouped.packages, {"example", "example_api"})

    def test_lock_reader_excludes_patch_unused_entries(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = root / "Cargo.toml"
            manifest.write_text(
                '[package]\nname = "fixture"\nversion = "0.1.0"\n',
                encoding="utf-8",
            )
            lock = root / "Cargo.lock"
            lock.write_text(
                f"""
version = 4

[[package]]
name = "example"
version = "0.1.0"

[[patch.unused]]
name = "example"
version = "0.1.0"
source = "git+{OWNED}?branch=main#{OLD}"
""",
                encoding="utf-8",
            )
            override = check.ExpectedOverride("example", None, root / "example", manifest)
            packages = check.active_packages_from_lock(lock, [manifest], [override])

        self.assertEqual(len(packages), 1)
        self.assertIsNone(packages[0].source)


class ClassificationTests(unittest.TestCase):
    def audit(
        self,
        *,
        lock_policy: str = "ignored",
        packages: list[check.ActivePackage] | None = None,
        overrides: list[check.ExpectedOverride] | None = None,
        checkout: check.LocalCheckout | None = None,
    ) -> list[check.Finding]:
        packages = packages if packages is not None else [active_git()]
        active_by_name: dict[str, list[check.ActivePackage]] = {}
        for package in packages:
            active_by_name.setdefault(package.name, []).append(package)
        checkout = checkout or check.LocalCheckout(
            pathlib.Path("C:/source/example"), NEW, "main", OWNED, False
        )
        return check.audit_edge(
            edge(),
            workspace=pathlib.Path("C:/workspace"),
            manifest=pathlib.Path("C:/workspace/Cargo.toml"),
            policy=policy(lock_policy),
            active_by_name=active_by_name,
            overrides=overrides or [],
            remote=check.RemoteRef(NEW),
            local_inspector=FakeLocalInspector(checkout),
        )

    def test_owned_branch_drift_fails_with_ignored_lock(self) -> None:
        findings = self.audit()
        stale = next(item for item in findings if item.code == "branch-stale")
        self.assertEqual(stale.severity, "ERROR")
        self.assertIn(f"--precise {NEW}", stale.repair or "")

    def test_owned_branch_drift_warns_with_tracked_lock(self) -> None:
        findings = self.audit(lock_policy="tracked")
        stale = next(item for item in findings if item.code == "branch-stale")
        self.assertEqual(stale.severity, "WARNING")

    def test_configured_override_must_be_active(self) -> None:
        override = check.ExpectedOverride(
            "example", check.normalize_git_url(OWNED), pathlib.Path("C:/source/example"), pathlib.Path("C:/workspace/.cargo/config.toml"), "patch"
        )
        findings = self.audit(overrides=[override])
        self.assertIn("override-inactive", {item.code for item in findings})

    def test_paths_override_keeps_git_lock_source_but_uses_matching_local_version(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            local_root = pathlib.Path(directory) / "example"
            local_root.mkdir()
            (local_root / "Cargo.toml").write_text(
                '[package]\nname = "example"\nversion = "0.1.0"\n',
                encoding="utf-8",
            )
            findings = self.audit(
                packages=[active_git(sha=NEW)],
                overrides=[
                    check.ExpectedOverride(
                        "example",
                        None,
                        local_root,
                        pathlib.Path("C:/workspace/.cargo/config.toml"),
                        "paths",
                    )
                ],
            )

        codes = {item.code for item in findings}
        self.assertIn("override-current", codes)
        self.assertNotIn("branch-current", codes)
        self.assertNotIn("override-inactive", codes)

    def test_unused_lock_residue_does_not_override_active_path_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            local_root = pathlib.Path(directory) / "example"
            local_root.mkdir()
            local_manifest = local_root / "Cargo.toml"
            local_manifest.write_text(
                '[package]\nname = "example"\nversion = "0.1.0"\n',
                encoding="utf-8",
            )
            findings = self.audit(
                packages=[
                    check.ActivePackage("example", "0.1.0", None, local_manifest, "path+example"),
                ],
                overrides=[
                    check.ExpectedOverride(
                        "example",
                        check.normalize_git_url(OWNED),
                        local_root,
                        pathlib.Path("C:/workspace/.cargo/config.toml"),
                        "patch",
                    )
                ],
            )
        codes = {item.code for item in findings}
        self.assertIn("override-current", codes)
        self.assertNotIn("branch-stale", codes)
        self.assertNotIn("override-inactive", codes)

    def test_revision_pin_is_integrity_checked(self) -> None:
        pinned = check.DeclaredEdge(
            check.GitSource("https://github.com/upstream/pinned", "rev", OLD),
            packages={"pinned"},
        )
        package = check.ActivePackage(
            "pinned",
            "0.1.0",
            f"git+https://github.com/upstream/pinned?rev={OLD}#{OLD}",
            pathlib.Path("C:/cargo/pinned/Cargo.toml"),
            "pinned",
        )
        findings = check.audit_edge(
            pinned,
            workspace=pathlib.Path("C:/workspace"),
            manifest=pathlib.Path("C:/workspace/Cargo.toml"),
            policy=policy("ignored"),
            active_by_name={"pinned": [package]},
            overrides=[],
            remote=check.RemoteRef(OLD),
            local_inspector=FakeLocalInspector(
                check.LocalCheckout(None, None, None, None, False, "unused")
            ),
        )
        self.assertEqual([item.code for item in findings], ["revision-pinned"])


if __name__ == "__main__":
    unittest.main()
