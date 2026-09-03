#!/usr/bin/env python3

# Copyright 2026 Mark Alan Boykin
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
# SPDX-License-Identifier: MPL-2.0

"""Report drift in Cargo git dependencies without changing the workspace.

The checker joins four views that are misleading when read separately:

* declared git dependencies in Cargo manifests;
* the active graph reported by ``cargo metadata --locked``;
* local ``paths`` and ``[patch]`` overrides from Cargo configuration; and
* the selected branch or tag on the remote.

Run it for one repository:

    python support/ci/check_git_dependency_state.py

Or for every Cargo repository below a checkout tree:

    python support/ci/check_git_dependency_state.py --tree C:/Users/mark_/Code/repos

The command is read-only. It prints a repair command for stale branch edges but
never runs ``cargo update`` itself.
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import os
import pathlib
import subprocess
import sys
import tomllib
import urllib.parse
from collections import defaultdict
from collections.abc import Iterable, Mapping


DEFAULT_OWNED_URL_PREFIXES = (
    "https://github.com/mark-ik/",
    "https://github.com/merely-made/",
)
PRUNED_DIRECTORIES = {
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    ".rustup",
    ".wpt",
}


def should_prune_directory(name: str) -> bool:
    return (
        name in PRUNED_DIRECTORIES
        or name.startswith("target-")
        or (name.startswith(".") and name not in {".", ".."})
    )


@dataclasses.dataclass(frozen=True, order=True)
class GitSource:
    url: str
    selector_kind: str
    selector_value: str

    @property
    def key(self) -> tuple[str, str, str]:
        return (
            normalize_git_url(self.url),
            self.selector_kind,
            self.selector_value,
        )

    @property
    def label(self) -> str:
        if self.selector_kind == "default":
            return f"{self.url} HEAD"
        return f"{self.url} {self.selector_kind}={self.selector_value}"


@dataclasses.dataclass
class DeclaredEdge:
    source: GitSource
    packages: set[str] = dataclasses.field(default_factory=set)
    manifests: set[pathlib.Path] = dataclasses.field(default_factory=set)
    contexts: set[str] = dataclasses.field(default_factory=set)


@dataclasses.dataclass(frozen=True)
class ExpectedOverride:
    package: str
    source_url: str | None
    path: pathlib.Path
    origin: pathlib.Path
    kind: str = "patch"


@dataclasses.dataclass(frozen=True)
class ActivePackage:
    name: str
    version: str
    source: str | None
    manifest_path: pathlib.Path
    package_id: str


@dataclasses.dataclass(frozen=True)
class RemoteRef:
    sha: str | None
    problem: str | None = None


@dataclasses.dataclass(frozen=True)
class LocalCheckout:
    root: pathlib.Path | None
    head: str | None
    branch: str | None
    origin_url: str | None
    dirty: bool
    problem: str | None = None


@dataclasses.dataclass(frozen=True)
class Finding:
    severity: str
    code: str
    workspace: pathlib.Path
    source: GitSource | None
    message: str
    repair: str | None = None

    def as_json(self) -> dict[str, object]:
        return {
            "severity": self.severity,
            "code": self.code,
            "workspace": str(self.workspace),
            "source": self.source.label if self.source else None,
            "message": self.message,
            "repair": self.repair,
        }


@dataclasses.dataclass(frozen=True)
class Policy:
    owned_url_prefixes: tuple[str, ...]
    lock_policy: str
    ignored_owned_drift: str
    tracked_owned_drift: str
    external_drift: str

    def is_owned(self, url: str) -> bool:
        normalized = normalize_git_url(url)
        for prefix in self.owned_url_prefixes:
            normalized_prefix = normalize_git_url(prefix)
            if normalized == normalized_prefix or normalized.startswith(normalized_prefix + "/"):
                return True
        return False


@dataclasses.dataclass
class WorkspaceAudit:
    workspace: pathlib.Path
    lock_policy: str
    findings: list[Finding]


def load_toml(path: pathlib.Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def normalize_git_url(value: str) -> str:
    """Normalize common Cargo and Git remote spellings for comparison."""

    value = value.strip()
    if value.startswith("git+"):
        value = value[4:]
    value, _fragment = urllib.parse.urldefrag(value)

    if value.startswith("git@") and ":" in value:
        host_part, path_part = value.split(":", 1)
        value = f"ssh://{host_part}/{path_part}"

    parsed = urllib.parse.urlsplit(value)
    if parsed.scheme and parsed.netloc:
        host = parsed.hostname.lower() if parsed.hostname else parsed.netloc.lower()
        path = parsed.path.rstrip("/")
        if path.endswith(".git"):
            path = path[:-4]
        if host == "github.com":
            path = path.lower()
        return f"{host}{path}"

    path = value.rstrip("/")
    if path.endswith(".git"):
        path = path[:-4]
    return path


def git_source_from_spec(spec: Mapping[str, object]) -> GitSource | None:
    url = spec.get("git")
    if not isinstance(url, str):
        return None
    selectors = [
        (kind, spec.get(kind))
        for kind in ("branch", "tag", "rev")
        if isinstance(spec.get(kind), str)
    ]
    if len(selectors) > 1:
        raise ValueError(f"git dependency {url!r} has multiple selectors")
    if selectors:
        kind, value = selectors[0]
        return GitSource(url, kind, str(value))
    return GitSource(url, "default", "HEAD")


def parse_cargo_source(value: str) -> tuple[GitSource, str | None] | None:
    if not value.startswith("git+"):
        return None
    raw = value[4:]
    raw, fragment = urllib.parse.urldefrag(raw)
    parsed = urllib.parse.urlsplit(raw)
    query = urllib.parse.parse_qs(parsed.query)
    url = urllib.parse.urlunsplit(
        (parsed.scheme, parsed.netloc, parsed.path, "", "")
    )
    for kind in ("branch", "tag", "rev"):
        values = query.get(kind)
        if values:
            return GitSource(url, kind, values[0]), fragment or None
    return GitSource(url, "default", "HEAD"), fragment or None


def iter_dependency_tables(document: Mapping[str, object]) -> Iterable[tuple[str, dict]]:
    for name in ("dependencies", "dev-dependencies", "build-dependencies"):
        table = document.get(name)
        if isinstance(table, dict):
            yield name, table

    workspace = document.get("workspace")
    if isinstance(workspace, dict):
        table = workspace.get("dependencies")
        if isinstance(table, dict):
            yield "workspace.dependencies", table

    targets = document.get("target")
    if isinstance(targets, dict):
        for target_name, target in targets.items():
            if not isinstance(target, dict):
                continue
            for name in ("dependencies", "dev-dependencies", "build-dependencies"):
                table = target.get(name)
                if isinstance(table, dict):
                    yield f"target.{target_name}.{name}", table

    patches = document.get("patch")
    if isinstance(patches, dict):
        for patch_source, table in patches.items():
            if isinstance(table, dict):
                yield f"patch.{patch_source}", table

    replacements = document.get("replace")
    if isinstance(replacements, dict):
        yield "replace", replacements


def collect_manifest_edges(manifests: Iterable[pathlib.Path]) -> dict[tuple[str, str, str], DeclaredEdge]:
    edges: dict[tuple[str, str, str], DeclaredEdge] = {}
    for manifest in sorted({path.resolve() for path in manifests}):
        try:
            document = load_toml(manifest)
        except (OSError, tomllib.TOMLDecodeError) as error:
            raise RuntimeError(f"could not read {manifest}: {error}") from error
        for context, table in iter_dependency_tables(document):
            for alias, raw_spec in table.items():
                if not isinstance(raw_spec, dict):
                    continue
                source = git_source_from_spec(raw_spec)
                if source is None:
                    continue
                package = raw_spec.get("package", alias)
                if not isinstance(package, str):
                    continue
                edge = edges.setdefault(source.key, DeclaredEdge(source))
                edge.packages.add(package)
                edge.manifests.add(manifest)
                edge.contexts.add(context)
    return edges


def resolve_config_path(config: pathlib.Path, value: str) -> pathlib.Path:
    path = pathlib.Path(value)
    if path.is_absolute():
        return path.resolve()
    base = config.parent.parent if config.parent.name == ".cargo" else config.parent
    return (base / path).resolve()


def package_manifests_beneath(path: pathlib.Path, max_depth: int = 4) -> dict[str, pathlib.Path]:
    packages: dict[str, pathlib.Path] = {}
    if not path.exists():
        return packages
    candidates = [path / "Cargo.toml"]
    for root, directories, files in os.walk(path):
        root_path = pathlib.Path(root)
        depth = len(root_path.relative_to(path).parts)
        directories[:] = [
            name
            for name in directories
            if not should_prune_directory(name) and depth < max_depth
        ]
        if "Cargo.toml" in files:
            candidates.append(root_path / "Cargo.toml")
    for manifest in dict.fromkeys(candidates):
        if not manifest.is_file():
            continue
        try:
            package = load_toml(manifest).get("package")
        except (OSError, tomllib.TOMLDecodeError):
            continue
        if isinstance(package, dict) and isinstance(package.get("name"), str):
            packages[package["name"]] = manifest.resolve()
    return packages


def cargo_config_files(workspace: pathlib.Path) -> list[pathlib.Path]:
    candidates: list[pathlib.Path] = []
    cargo_home = pathlib.Path(os.environ.get("CARGO_HOME", pathlib.Path.home() / ".cargo"))
    for filename in ("config", "config.toml"):
        candidate = cargo_home / filename
        if candidate.is_file():
            candidates.append(candidate.resolve())

    ancestors = list(workspace.resolve().parents)
    ancestors.reverse()
    ancestors.append(workspace.resolve())
    for ancestor in ancestors:
        cargo_dir = ancestor / ".cargo"
        for filename in ("config", "config.toml"):
            candidate = cargo_dir / filename
            if candidate.is_file():
                candidates.append(candidate.resolve())
    return list(dict.fromkeys(candidates))


def overrides_from_document(
    document: Mapping[str, object], origin: pathlib.Path
) -> list[ExpectedOverride]:
    overrides: list[ExpectedOverride] = []

    paths = document.get("paths")
    if isinstance(paths, list):
        for raw_path in paths:
            if not isinstance(raw_path, str):
                continue
            path = resolve_config_path(origin, raw_path)
            for package, manifest in package_manifests_beneath(path).items():
                overrides.append(
                    ExpectedOverride(package, None, manifest.parent, origin, "paths")
                )

    patches = document.get("patch")
    if isinstance(patches, dict):
        for raw_source, table in patches.items():
            if not isinstance(table, dict):
                continue
            source_url = (
                "registry:crates-io"
                if raw_source == "crates-io"
                else normalize_git_url(raw_source)
            )
            for alias, spec in table.items():
                if not isinstance(spec, dict) or not isinstance(spec.get("path"), str):
                    continue
                package = spec.get("package", alias)
                if not isinstance(package, str):
                    continue
                path = resolve_config_path(origin, spec["path"])
                overrides.append(
                    ExpectedOverride(package, source_url, path, origin, "patch")
                )
    return overrides


def collect_expected_overrides(
    workspace: pathlib.Path, manifests: Iterable[pathlib.Path]
) -> list[ExpectedOverride]:
    overrides: list[ExpectedOverride] = []
    for config in cargo_config_files(workspace):
        try:
            overrides.extend(overrides_from_document(load_toml(config), config))
        except (OSError, tomllib.TOMLDecodeError) as error:
            raise RuntimeError(f"could not read Cargo config {config}: {error}") from error
    for manifest in manifests:
        try:
            document = load_toml(manifest)
        except (OSError, tomllib.TOMLDecodeError):
            continue
        overrides.extend(overrides_from_document(document, manifest))
    return overrides


def run(
    command: list[str],
    *,
    cwd: pathlib.Path,
    timeout: int = 60,
) -> subprocess.CompletedProcess[str]:
    environment = os.environ.copy()
    environment["GIT_TERMINAL_PROMPT"] = "0"
    return subprocess.run(
        command,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=timeout,
        env=environment,
    )


def package_name_from_manifest(manifest: pathlib.Path) -> str | None:
    try:
        package = load_toml(manifest).get("package")
    except (OSError, tomllib.TOMLDecodeError):
        return None
    if isinstance(package, dict) and isinstance(package.get("name"), str):
        return package["name"]
    return None


def active_packages_from_lock(
    lock: pathlib.Path,
    manifests: Iterable[pathlib.Path],
    overrides: Iterable[ExpectedOverride],
) -> list[ActivePackage]:
    """Read active packages while deliberately excluding ``[[patch.unused]]``."""

    packages: list[ActivePackage] = []
    document = load_toml(lock)
    raw_packages = document.get("package")
    if not isinstance(raw_packages, list):
        return packages

    workspace_manifests: dict[str, list[pathlib.Path]] = defaultdict(list)
    for manifest in manifests:
        name = package_name_from_manifest(manifest)
        if name:
            workspace_manifests[name].append(manifest.resolve())
    override_paths: dict[str, list[pathlib.Path]] = defaultdict(list)
    for override in overrides:
        override_paths[override.package].append(override.path.resolve() / "Cargo.toml")

    for package in raw_packages:
        if not isinstance(package, dict):
            continue
        name = package.get("name")
        source = package.get("source")
        version = package.get("version")
        if not isinstance(name, str) or not isinstance(version, str):
            continue
        if source is None:
            candidates = override_paths.get(name, []) + workspace_manifests.get(name, [])
            manifest = next(
                (candidate for candidate in candidates if manifest_version(candidate) == version),
                lock.parent / ".cargo-unresolved-path" / name / "Cargo.toml",
            )
        else:
            manifest = lock.parent / ".cargo-resolved" / name / "Cargo.toml"
        packages.append(
            ActivePackage(
                name=name,
                version=version,
                source=source if isinstance(source, str) else None,
                manifest_path=manifest.resolve(),
                package_id=f"{name}@{version}",
            )
        )
    return packages


def policy_from_file(repo: pathlib.Path, lock_policy: str) -> Policy:
    path = repo / "support" / "git-dependency-state.toml"
    document: dict = {}
    if path.is_file():
        document = load_toml(path)
    configured = document.get("owned-url-prefixes", DEFAULT_OWNED_URL_PREFIXES)
    if not isinstance(configured, (list, tuple)) or not all(isinstance(item, str) for item in configured):
        raise RuntimeError(f"{path}: owned-url-prefixes must be an array of strings")
    return Policy(
        owned_url_prefixes=tuple(configured),
        lock_policy=str(document.get("lock-policy", lock_policy)),
        ignored_owned_drift=str(document.get("ignored-owned-drift", "error")),
        tracked_owned_drift=str(document.get("tracked-owned-drift", "warning")),
        external_drift=str(document.get("external-drift", "warning")),
    )


def git_repo_root(path: pathlib.Path) -> pathlib.Path | None:
    result = run(["git", "rev-parse", "--show-toplevel"], cwd=path)
    if result.returncode != 0:
        return None
    return pathlib.Path(result.stdout.strip()).resolve()


def infer_lock_policy(repo: pathlib.Path, workspace: pathlib.Path) -> str:
    lock = workspace / "Cargo.lock"
    try:
        relative = lock.resolve().relative_to(repo.resolve())
    except ValueError:
        return "unmanaged"
    tracked = run(
        ["git", "ls-files", "--error-unmatch", "--", str(relative)], cwd=repo
    )
    if tracked.returncode == 0:
        return "tracked"
    ignored = run(
        ["git", "check-ignore", "--no-index", "--quiet", "--", str(relative)],
        cwd=repo,
    )
    if ignored.returncode == 0:
        return "ignored"
    return "untracked"


class RemoteResolver:
    def __init__(self, offline: bool) -> None:
        self.offline = offline
        self.cache: dict[tuple[str, str, str], RemoteRef] = {}

    def resolve(self, source: GitSource, cwd: pathlib.Path) -> RemoteRef:
        if source.key in self.cache:
            return self.cache[source.key]
        if source.selector_kind == "rev":
            ref = RemoteRef(source.selector_value)
            self.cache[source.key] = ref
            return ref
        if self.offline:
            ref = RemoteRef(None, "offline mode")
            self.cache[source.key] = ref
            return ref

        if source.selector_kind == "branch":
            refs = [f"refs/heads/{source.selector_value}"]
        elif source.selector_kind == "tag":
            refs = [
                f"refs/tags/{source.selector_value}",
                f"refs/tags/{source.selector_value}^{{}}",
            ]
        elif source.selector_kind == "default":
            refs = ["HEAD"]
        else:
            raise AssertionError(f"unknown git selector kind {source.selector_kind}")

        try:
            result = run(
                ["git", "ls-remote", "--exit-code", source.url, *refs],
                cwd=cwd,
                timeout=30,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            ref = RemoteRef(None, str(error))
            self.cache[source.key] = ref
            return ref
        if result.returncode != 0:
            detail = result.stderr.strip() or f"git ls-remote exited {result.returncode}"
            ref = RemoteRef(None, detail)
            self.cache[source.key] = ref
            return ref

        rows = [line.split() for line in result.stdout.splitlines() if line.split()]
        if not rows:
            ref = RemoteRef(None, "remote ref was not found")
        else:
            peeled = [row for row in rows if len(row) > 1 and row[1].endswith("^{}")]
            ref = RemoteRef((peeled or rows)[-1][0])
        self.cache[source.key] = ref
        return ref


class LocalInspector:
    def __init__(self) -> None:
        self.cache: dict[pathlib.Path, LocalCheckout] = {}

    def inspect(self, manifest: pathlib.Path) -> LocalCheckout:
        directory = manifest.parent
        root = git_repo_root(directory)
        if root is None:
            return LocalCheckout(None, None, None, None, False, "not inside a Git checkout")
        if root in self.cache:
            return self.cache[root]

        head = run(["git", "rev-parse", "HEAD"], cwd=root)
        branch = run(["git", "branch", "--show-current"], cwd=root)
        origin = run(["git", "remote", "get-url", "origin"], cwd=root)
        status = run(["git", "status", "--porcelain"], cwd=root)
        problem = None
        if head.returncode != 0:
            problem = head.stderr.strip() or "could not read local HEAD"
        checkout = LocalCheckout(
            root=root,
            head=head.stdout.strip() if head.returncode == 0 else None,
            branch=branch.stdout.strip() if branch.returncode == 0 else None,
            origin_url=origin.stdout.strip() if origin.returncode == 0 else None,
            dirty=bool(status.stdout.strip()) if status.returncode == 0 else False,
            problem=problem,
        )
        self.cache[root] = checkout
        return checkout


def is_beneath(path: pathlib.Path, parent: pathlib.Path) -> bool:
    try:
        path.resolve().relative_to(parent.resolve())
    except ValueError:
        return False
    return True


def severity_for_drift(policy: Policy, source: GitSource) -> str:
    if not policy.is_owned(source.url):
        return policy.external_drift.upper()
    if policy.lock_policy == "ignored":
        return policy.ignored_owned_drift.upper()
    return policy.tracked_owned_drift.upper()


def repair_command(manifest: pathlib.Path, package: str, sha: str) -> str:
    return f'cargo update --manifest-path "{manifest}" -p {package} --precise {sha}'


def expected_for_edge(
    edge: DeclaredEdge, overrides: Iterable[ExpectedOverride]
) -> dict[str, list[ExpectedOverride]]:
    result: dict[str, list[ExpectedOverride]] = defaultdict(list)
    source_url = normalize_git_url(edge.source.url)
    for override in overrides:
        if override.package not in edge.packages:
            continue
        if override.kind != "paths" and override.source_url != source_url:
            continue
        result[override.package].append(override)
    return result


def manifest_version(manifest: pathlib.Path) -> str | None:
    try:
        document = load_toml(manifest)
    except (OSError, tomllib.TOMLDecodeError):
        return None
    package = document.get("package")
    if isinstance(package, dict) and isinstance(package.get("version"), str):
        return package["version"]
    if not (
        isinstance(package, dict)
        and isinstance(package.get("version"), dict)
        and package["version"].get("workspace") is True
    ):
        return None
    for directory in (manifest.parent, *manifest.parent.parents):
        workspace_manifest = directory / "Cargo.toml"
        if not workspace_manifest.is_file():
            continue
        try:
            workspace = load_toml(workspace_manifest).get("workspace")
        except (OSError, tomllib.TOMLDecodeError):
            continue
        if not isinstance(workspace, dict):
            continue
        workspace_package = workspace.get("package")
        if isinstance(workspace_package, dict) and isinstance(
            workspace_package.get("version"), str
        ):
            return workspace_package["version"]
    return None


def paths_override_replacement(
    package: ActivePackage,
    edge: DeclaredEdge,
    overrides: Iterable[ExpectedOverride],
) -> ActivePackage | None:
    parsed = parse_cargo_source(package.source or "")
    if parsed is None or parsed[0].key != edge.source.key:
        return None
    for override in overrides:
        if override.kind != "paths":
            continue
        local_manifest = override.path / "Cargo.toml"
        if manifest_version(local_manifest) != package.version:
            continue
        return ActivePackage(
            name=package.name,
            version=package.version,
            source=None,
            manifest_path=local_manifest.resolve(),
            package_id=package.package_id,
        )
    return None


def audit_edge(
    edge: DeclaredEdge,
    *,
    workspace: pathlib.Path,
    manifest: pathlib.Path,
    policy: Policy,
    active_by_name: Mapping[str, list[ActivePackage]],
    overrides: Iterable[ExpectedOverride],
    remote: RemoteRef,
    local_inspector: LocalInspector,
) -> list[Finding]:
    findings: list[Finding] = []
    expected = expected_for_edge(edge, overrides)
    relevant: list[ActivePackage] = []
    for package in sorted(edge.packages):
        relevant.extend(active_by_name.get(package, []))

    matching_git: list[tuple[ActivePackage, str | None]] = []
    path_packages: list[ActivePackage] = []
    override_inactive = False
    for package in relevant:
        replacement = paths_override_replacement(
            package, edge, expected.get(package.name, [])
        )
        if replacement is not None:
            path_packages.append(replacement)
            continue
        if package.source is None:
            if package.manifest_path.is_file():
                path_packages.append(package)
            continue
        parsed = parse_cargo_source(package.source)
        if parsed is not None and parsed[0].key == edge.source.key:
            matching_git.append((package, parsed[1]))

    for package, package_overrides in expected.items():
        active_for_package = [item for item in relevant if item.name == package]
        active_paths = [item for item in path_packages if item.name == package]
        if not active_for_package:
            continue
        if not any(
            is_beneath(item.manifest_path, override.path)
            for item in active_paths
            for override in package_overrides
        ):
            override_inactive = True
            configured = ", ".join(str(item.path) for item in package_overrides)
            repair = None
            if remote.sha:
                repair = repair_command(manifest, package, remote.sha)
            findings.append(
                Finding(
                    "ERROR",
                    "override-inactive",
                    workspace,
                    edge.source,
                    f"{package} has a local override at {configured}, but the active graph does not use it",
                    repair,
                )
            )

    seen_git_shas = sorted({sha for _package, sha in matching_git if sha})
    if matching_git:
        if remote.problem:
            findings.append(
                Finding(
                    "UNVERIFIED",
                    "remote-unavailable",
                    workspace,
                    edge.source,
                    f"could not verify the remote ref: {remote.problem}",
                )
            )
        elif edge.source.selector_kind == "rev":
            mismatched = [sha for sha in seen_git_shas if not edge.source.selector_value.startswith(sha) and not sha.startswith(edge.source.selector_value)]
            if mismatched:
                findings.append(
                    Finding(
                        "ERROR",
                        "revision-mismatch",
                        workspace,
                        edge.source,
                        f"active revision(s) {', '.join(mismatched)} do not match the manifest pin",
                    )
                )
            else:
                findings.append(
                    Finding("OK", "revision-pinned", workspace, edge.source, "active revision matches the manifest pin")
                )
        elif remote.sha and seen_git_shas == [remote.sha]:
            findings.append(
                Finding(
                    "OK",
                    "branch-current",
                    workspace,
                    edge.source,
                    f"active git resolution is {remote.sha[:12]}",
                )
            )
        elif remote.sha:
            severity = severity_for_drift(policy, edge.source)
            package = sorted(item.name for item, _sha in matching_git)[0]
            findings.append(
                Finding(
                    severity,
                    "branch-stale",
                    workspace,
                    edge.source,
                    f"active git resolution(s) {', '.join(sha[:12] for sha in seen_git_shas) or 'unknown'} differ from remote {remote.sha[:12]}",
                    repair_command(manifest, package, remote.sha),
                )
            )

    seen_local_roots: set[pathlib.Path] = set()
    for package in path_packages:
        checkout = local_inspector.inspect(package.manifest_path)
        identity = checkout.root or package.manifest_path.parent
        if identity in seen_local_roots:
            continue
        seen_local_roots.add(identity)
        if checkout.problem or checkout.root is None or checkout.head is None:
            findings.append(
                Finding(
                    "ERROR",
                    "override-not-git",
                    workspace,
                    edge.source,
                    f"local resolution at {package.manifest_path.parent} cannot be verified: {checkout.problem}",
                )
            )
            continue
        if checkout.origin_url and normalize_git_url(checkout.origin_url) != normalize_git_url(edge.source.url):
            findings.append(
                Finding(
                    "ERROR",
                    "override-origin-mismatch",
                    workspace,
                    edge.source,
                    f"local override {checkout.root} has origin {checkout.origin_url}",
                )
            )
            continue
        details = [f"local override {checkout.root} at {checkout.head[:12]}"]
        if checkout.branch:
            details.append(f"branch {checkout.branch}")
        if checkout.dirty:
            details.append("dirty")
        severity = "OK"
        code = "override-current"
        if remote.problem:
            severity = "UNVERIFIED"
            code = "remote-unavailable"
            details.append(remote.problem)
        elif remote.sha and checkout.head == remote.sha:
            pass
        elif edge.source.selector_kind == "branch" and checkout.branch and checkout.branch != edge.source.selector_value:
            severity = "WARNING"
            code = "override-branch-mismatch"
            details.append(f"declared branch {edge.source.selector_value}")
            if remote.sha:
                details.append(f"remote {remote.sha[:12]}")
        elif remote.sha and checkout.head != remote.sha:
            severity = "INFO"
            code = "override-diverged"
            details.append(f"remote {remote.sha[:12]}")
        findings.append(
            Finding(severity, code, workspace, edge.source, "; ".join(details))
        )

    if not matching_git and not path_packages and not override_inactive:
        patch_only = all(
            context.startswith("patch.") or context == "replace"
            for context in edge.contexts
        )
        findings.append(
            Finding(
                "INFO" if patch_only else "ERROR",
                "edge-inactive" if patch_only else "locked-edge-missing",
                workspace,
                edge.source,
                f"declared for {', '.join(sorted(edge.packages))}, but inactive in this target/feature graph",
                None
                if patch_only or remote.sha is None
                else repair_command(manifest, sorted(edge.packages)[0], remote.sha),
            )
        )
    return findings


def candidate_workspace_manifests(root_manifest: pathlib.Path) -> set[pathlib.Path]:
    manifests = {root_manifest.resolve()}
    try:
        document = load_toml(root_manifest)
    except (OSError, tomllib.TOMLDecodeError):
        return manifests
    workspace = document.get("workspace")
    if not isinstance(workspace, dict):
        return manifests
    members = workspace.get("members", [])
    excludes = {
        path.resolve()
        for raw in workspace.get("exclude", [])
        if isinstance(raw, str)
        for path in root_manifest.parent.glob(raw)
    }
    if isinstance(members, list):
        for raw in members:
            if not isinstance(raw, str):
                continue
            for member in root_manifest.parent.glob(raw):
                manifest = member / "Cargo.toml" if member.is_dir() else member
                if manifest.is_file() and not any(is_beneath(manifest, item) for item in excludes):
                    manifests.add(manifest.resolve())
    pending = list(manifests)
    while pending:
        manifest = pending.pop()
        try:
            document = load_toml(manifest)
        except (OSError, tomllib.TOMLDecodeError):
            continue
        for _context, table in iter_dependency_tables(document):
            for raw_spec in table.values():
                if not isinstance(raw_spec, dict) or not isinstance(raw_spec.get("path"), str):
                    continue
                dependency = (manifest.parent / raw_spec["path"]).resolve()
                dependency_manifest = dependency if dependency.name == "Cargo.toml" else dependency / "Cargo.toml"
                if not dependency_manifest.is_file() or dependency_manifest in manifests:
                    continue
                manifests.add(dependency_manifest)
                pending.append(dependency_manifest)
    return manifests


def audit_workspace(
    manifest: pathlib.Path,
    repo: pathlib.Path,
    resolver: RemoteResolver,
    local_inspector: LocalInspector,
) -> WorkspaceAudit:
    workspace = manifest.parent.resolve()
    inferred_policy = infer_lock_policy(repo, workspace)
    policy = policy_from_file(repo, inferred_policy)
    findings: list[Finding] = []
    lock = workspace / "Cargo.lock"
    manifests = candidate_workspace_manifests(manifest)
    try:
        edges = collect_manifest_edges(manifests)
        overrides = collect_expected_overrides(workspace, manifests)
    except RuntimeError as error:
        findings.append(Finding("ERROR", "manifest-read-failed", workspace, None, str(error)))
        return WorkspaceAudit(workspace, policy.lock_policy, findings)

    if not edges:
        findings.append(
            Finding("OK", "no-git-dependencies", workspace, None, "no Cargo git dependencies declared")
        )
        return WorkspaceAudit(workspace, policy.lock_policy, findings)

    if not lock.is_file():
        findings.append(
            Finding(
                "INFO",
                "lock-absent",
                workspace,
                None,
                "Cargo.lock is absent; branch dependencies will resolve freshly",
            )
        )
        for edge in sorted(edges.values(), key=lambda item: item.source.key):
            remote = resolver.resolve(edge.source, workspace)
            if remote.problem:
                findings.append(
                    Finding(
                        "UNVERIFIED",
                        "remote-unavailable",
                        workspace,
                        edge.source,
                        f"fresh resolution could not be verified: {remote.problem}",
                    )
                )
            else:
                findings.append(
                    Finding(
                        "OK",
                        "fresh-resolution",
                        workspace,
                        edge.source,
                        f"next resolution selects {remote.sha[:12] if remote.sha else edge.source.selector_value}",
                    )
                )
        return WorkspaceAudit(workspace, policy.lock_policy, findings)

    try:
        active_packages = active_packages_from_lock(lock, manifests, overrides)
    except (OSError, tomllib.TOMLDecodeError) as error:
        findings.append(
            Finding("ERROR", "lock-read-failed", workspace, None, f"could not read {lock}: {error}")
        )
        return WorkspaceAudit(workspace, policy.lock_policy, findings)
    active_by_name: dict[str, list[ActivePackage]] = defaultdict(list)
    for package in active_packages:
        active_by_name[package.name].append(package)

    for edge in sorted(edges.values(), key=lambda item: item.source.key):
        remote = resolver.resolve(edge.source, workspace)
        findings.extend(
            audit_edge(
                edge,
                workspace=workspace,
                manifest=manifest,
                policy=policy,
                active_by_name=active_by_name,
                overrides=overrides,
                remote=remote,
                local_inspector=local_inspector,
            )
        )

    return WorkspaceAudit(workspace, policy.lock_policy, findings)


def nested_lock_workspaces(repo: pathlib.Path) -> list[pathlib.Path]:
    manifests = [repo / "Cargo.toml"]
    for root, directories, files in os.walk(repo):
        root_path = pathlib.Path(root)
        directories[:] = [name for name in directories if not should_prune_directory(name)]
        if root_path == repo or "Cargo.lock" not in files or "Cargo.toml" not in files:
            continue
        manifest = root_path / "Cargo.toml"
        try:
            document = load_toml(manifest)
        except (OSError, tomllib.TOMLDecodeError):
            continue
        if isinstance(document.get("workspace"), dict):
            manifests.append(manifest)
    return list(dict.fromkeys(path.resolve() for path in manifests if path.is_file()))


def discover_repositories(tree: pathlib.Path) -> list[pathlib.Path]:
    repositories: list[pathlib.Path] = []
    for root, directories, files in os.walk(tree):
        root_path = pathlib.Path(root)
        has_git = ".git" in directories or ".git" in files
        if has_git and (root_path / "Cargo.toml").is_file():
            repositories.append(root_path.resolve())
            directories[:] = []
            continue
        directories[:] = [name for name in directories if not should_prune_directory(name)]
    return sorted(set(repositories))


def looks_like_git_dependency_repo(repo: pathlib.Path) -> bool:
    for root, directories, files in os.walk(repo):
        directories[:] = [name for name in directories if not should_prune_directory(name)]
        if "Cargo.toml" not in files:
            continue
        try:
            document = load_toml(pathlib.Path(root) / "Cargo.toml")
        except (OSError, tomllib.TOMLDecodeError):
            continue
        if collect_manifest_edges([pathlib.Path(root) / "Cargo.toml"]):
            return True
    return False


def render_text(audits: Iterable[WorkspaceAudit]) -> None:
    counts: dict[str, int] = defaultdict(int)
    for audit in audits:
        print(f"[{audit.workspace}] lock={audit.lock_policy}")
        for finding in audit.findings:
            counts[finding.severity] += 1
            source = f" {finding.source.label}:" if finding.source else ""
            print(f"  {finding.severity:<10}{source} {finding.message}")
            if finding.repair:
                print(f"             repair: {finding.repair}")
    ordered = ("ERROR", "WARNING", "UNVERIFIED", "INFO", "OK")
    summary = " ".join(f"{name.lower()}={counts[name]}" for name in ordered)
    print(f"git dependency state: {summary}")


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    target = parser.add_mutually_exclusive_group()
    target.add_argument(
        "--workspace",
        type=pathlib.Path,
        default=None,
        help="repository root to inspect (default: current directory)",
    )
    target.add_argument(
        "--tree",
        type=pathlib.Path,
        help="inspect every Cargo git repository below this directory",
    )
    parser.add_argument("--offline", action="store_true", help="skip git ls-remote checks")
    parser.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    if args.tree:
        repositories = [
            repo for repo in discover_repositories(args.tree.resolve()) if looks_like_git_dependency_repo(repo)
        ]
    else:
        requested = (args.workspace or pathlib.Path.cwd()).resolve()
        repo = git_repo_root(requested) or requested
        repositories = [repo]

    resolver = RemoteResolver(args.offline)
    local_inspector = LocalInspector()
    audits: list[WorkspaceAudit] = []
    for repo in repositories:
        for manifest in nested_lock_workspaces(repo):
            audits.append(audit_workspace(manifest, repo, resolver, local_inspector))

    if args.json:
        print(
            json.dumps(
                {
                    "workspaces": [
                        {
                            "workspace": str(audit.workspace),
                            "lock_policy": audit.lock_policy,
                            "findings": [finding.as_json() for finding in audit.findings],
                        }
                        for audit in audits
                    ]
                },
                indent=2,
            )
        )
    else:
        render_text(audits)

    return 1 if any(
        finding.severity == "ERROR"
        for audit in audits
        for finding in audit.findings
    ) else 0


if __name__ == "__main__":
    raise SystemExit(main())
