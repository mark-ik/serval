#!/usr/bin/env python3

# Copyright 2026 Mark Alan Boykin
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
# SPDX-License-Identifier: MPL-2.0

"""Dependency-cone witnesses for Genet profile boundaries."""

from __future__ import annotations

import json
import pathlib
import re
import subprocess
import sys
import tomllib


ROOT = pathlib.Path(__file__).resolve().parents[2]


def fail(message: str) -> None:
    print(f"dependency-cone witness failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def load_toml(path: pathlib.Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def dependency_names(table: dict) -> set[str]:
    return set(table.keys())


def is_beneath(path: pathlib.Path, parent: pathlib.Path) -> bool:
    try:
        path.resolve().relative_to(parent.resolve())
    except ValueError:
        return False
    return True


def assert_fleece_cone() -> None:
    manifest = load_toml(ROOT / "components" / "fleece" / "Cargo.toml")
    deps = dependency_names(manifest.get("dependencies", {}))
    expected = {"layout_dom_api", "unicode-segmentation"}
    if deps != expected:
        fail(f"fleece dependencies are {sorted(deps)}, expected {sorted(expected)}")
    build_deps = dependency_names(manifest.get("build-dependencies", {}))
    if build_deps:
        fail(f"fleece build-dependencies must stay empty, found {sorted(build_deps)}")
    dev_deps = dependency_names(manifest.get("dev-dependencies", {}))
    allowed_dev_deps = {"genet-static-dom"}
    if dev_deps - allowed_dev_deps:
        fail(
            "fleece dev-dependencies contain non-fixture deps: "
            f"{sorted(dev_deps - allowed_dev_deps)}"
        )

    forbidden = {
        "genet-layout",
        "genet-render",
        "paint",
        "paint_list_render",
        "netrender",
        "wgpu",
    }
    if deps & forbidden:
        fail(f"fleece pulled render deps directly: {sorted(deps & forbidden)}")


def cargo_metadata() -> dict:
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        fail(f"cargo metadata failed:\n{result.stderr}")
    return json.loads(result.stdout)


def assert_cargo_metadata_sees_fleece(metadata: dict) -> None:
    names = {package["name"] for package in metadata["packages"]}
    missing = {"fleece"} - names
    if missing:
        fail(f"cargo metadata did not report fleece package {sorted(missing)}")


def assert_ports_depend_inward(metadata: dict) -> None:
    components = (ROOT / "components").resolve()
    ports = (ROOT / "ports").resolve()
    host_api = []
    ortet_packages = {}

    for package in metadata["packages"]:
        manifest = pathlib.Path(package["manifest_path"]).resolve()
        if package["name"] == "genet-host-api":
            host_api.append(manifest)
        if package["name"] == "ortet":
            ortet_packages[package["name"]] = manifest
        if not is_beneath(manifest, components):
            continue
        for dependency in package["dependencies"]:
            path = dependency.get("path")
            if path is not None and is_beneath(pathlib.Path(path), ports):
                fail(
                    f"{manifest.relative_to(ROOT)} depends on port package "
                    f"{dependency['name']} at {pathlib.Path(path).relative_to(ROOT)}"
                )

    expected_api = (components / "genet-host-api" / "Cargo.toml").resolve()
    if host_api != [expected_api]:
        rendered = [str(path.relative_to(ROOT)) for path in host_api]
        fail(
            f"genet-host-api manifests are {rendered}, "
            "expected components/genet-host-api/Cargo.toml"
        )

    # Pelt left for mere on 2026-09-03 (platform boundary plan, consumers-first
    # order); ortet is genet's headed host and takes the same assertion
    # (design_docs/2026-09-03_ortet_founding_plan.md, O4).
    expected_ortet = {"ortet": (ports / "ortet" / "Cargo.toml").resolve()}
    if ortet_packages != expected_ortet:
        rendered = {
            name: str(path.relative_to(ROOT)) for name, path in ortet_packages.items()
        }
        fail(f"Ortet package manifests are {rendered}, expected {expected_ortet}")


# Genet resolves without Mere (platform boundary plan, mere
# design_docs/mere_docs/implementation_strategy/
# 2026-09-02_platform_boundary_and_repository_topology_plan.md, invariant 1).
# Three ways a Mere source can enter the graph, each checked: a git source at
# Mere's repository, a path dependency that leaves this repository for a `mere`
# checkout, and a registry crate that Mere publishes. The name list is the
# unprefixed publishable members of Mere's workspace on 2026-09-02; prefixed
# families are matched by prefix. Extend it when Mere publishes a new family.
MERE_GIT_SOURCE = re.compile(r"merely-made/mere(?:\.git)?(?:[?#/]|$)")
MERE_CRATE_PREFIXES = ("mere-", "graphshell", "sceno", "register-")
MERE_CRATES = {
    "mere", "scenograph", "armillary", "chartulary", "codicil", "muniment",
    "scholia", "tulpa", "personae", "dramatis", "gaz", "gazette", "servitor",
    "vates", "sibylla", "conatus", "numen", "quint", "seiche", "murm",
    "moothold", "mooting", "gemot", "castellan", "chatelaine", "chirograph",
    "distillery", "djinn", "esp", "graphlets", "incipit", "insigne", "luggage",
    "mien", "nisus", "notochord", "pandect", "pictograph", "platen",
    "stickleback", "titulus", "ux-events", "uxtree", "script-rhai",
}


def is_mere_crate(name: str) -> bool:
    return name in MERE_CRATES or name.startswith(MERE_CRATE_PREFIXES)


def cargo_metadata_resolved() -> dict:
    """The full resolved graph: transitive sources are what the invariant is about."""
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1"],
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        fail(f"cargo metadata (resolved) failed:\n{result.stderr}")
    return json.loads(result.stdout)


def mere_sources(metadata: dict) -> list[str]:
    """Every package in the resolved graph that comes from Mere, as messages."""
    found = []
    for package in metadata["packages"]:
        name = package["name"]
        source = package.get("source") or ""
        manifest = pathlib.Path(package["manifest_path"])
        if MERE_GIT_SOURCE.search(source):
            found.append(f"{name} resolves from Mere's repository: {source}")
        elif not source and not is_beneath(manifest, ROOT):
            if "mere" in {part.lower() for part in manifest.parts}:
                found.append(f"{name} is a path dependency into a mere checkout: {manifest}")
        elif source.startswith("registry+") and is_mere_crate(name):
            found.append(f"{name} is a Mere-published crate pulled from the registry ({package['version']})")
    return found


def assert_no_mere_source(metadata: dict) -> None:
    found = mere_sources(metadata)
    if found:
        fail("Genet's graph reaches Mere:\n  " + "\n  ".join(found))


def self_test_mere_witness() -> None:
    """Positive control: the witness must catch each of the three entry routes."""
    fake = {"packages": [
        {"name": "sceno", "version": "0.0.3", "source": "registry+https://github.com/rust-lang/crates.io-index",
         "manifest_path": str(ROOT / "x" / "Cargo.toml")},
        {"name": "anything", "version": "0.1.0",
         "source": "git+https://github.com/merely-made/mere.git?rev=abc#abc",
         "manifest_path": str(ROOT / "y" / "Cargo.toml")},
        {"name": "moothold", "version": "0.1.0", "source": None,
         "manifest_path": str(ROOT.parent / "mere" / "crates" / "moothold" / "Cargo.toml")},
        {"name": "netrender", "version": "0.1.0",
         "source": "git+https://github.com/merely-made/netrender.git?rev=abc#abc",
         "manifest_path": str(ROOT / "z" / "Cargo.toml")},
        {"name": "mer3ly-ish", "version": "0.1.0",
         "source": "git+https://github.com/merely-made/mer3ly.git?rev=abc#abc",
         "manifest_path": str(ROOT / "w" / "Cargo.toml")},
    ]}
    found = mere_sources(fake)
    if len(found) != 3 or not any("registry" in f for f in found) or not any("repository" in f for f in found) \
            or not any("checkout" in f for f in found):
        fail(f"mere witness self-test expected exactly the three Mere routes, got {found}")


# netfetcher's authority split (platform boundary plan P1): the Fetch semantics
# Genet owns must link no transport. With default features off the resolved
# graph may not contain any of these; the transport lanes bring them in only
# behind `hyper-transport`, `h3` and `websocket`.
NETFETCHER_TRANSPORT_CRATES = {
    "hyper", "hyper-util", "hyper-rustls", "rustls", "quinn", "h3", "h3-quinn",
    "tokio-tungstenite", "webpki-roots", "http", "http-body-util",
}


def assert_netfetcher_semantics_cone() -> None:
    result = subprocess.run(
        ["cargo", "tree", "-p", "netfetcher", "--no-default-features", "-e", "normal",
         "--prefix", "none"],
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        fail(f"cargo tree for netfetcher without default features failed:\n{result.stderr}")
    names = {line.split()[0] for line in result.stdout.splitlines() if line.strip()}
    if "netfetcher" not in names:
        fail("cargo tree for netfetcher did not list netfetcher itself")
    reached = sorted(names & NETFETCHER_TRANSPORT_CRATES)
    if reached:
        fail(f"netfetcher's semantics-only build reaches transport crates: {reached}")


# genet-host-api's authority split (platform boundary plan P1): the raw engine
# host half keeps the name and depends on nothing in the workspace; the
# application half, mere-surface-api, may depend on workbench and nothing
# else here, since both move to Mere together.
def assert_host_api_cone(metadata: dict) -> None:
    members = {package["name"] for package in metadata["packages"]}
    by_name = {package["name"]: package for package in metadata["packages"]}
    for name, allowed in (("genet-host-api", set()), ("mere-surface-api", {"workbench"}),
                          # inker's engine-facing contract half (P1): a leaf.
                          ("document-session-api", set())):
        package = by_name.get(name)
        if package is None:
            fail(f"{name} is not a workspace member")
        deps = {d["name"] for d in package["dependencies"] if d["name"] in members}
        if deps - allowed:
            fail(f"{name} depends on workspace crates {sorted(deps - allowed)}; allowed {sorted(allowed)}")
    # Engine crates the plan keeps never depend on crates it moves. genet-render
    # reaches inker's contracts through the contract crate; genet-documents,
    # split by authority, keeps only the Livery and Scripted lanes and links no
    # controller, content lane or transport.
    forbidden = {
        "genet-render": {"inker"},
        "genet-documents": {"inker", "nematic", "errand", "document-canvas", "netfetcher"},
    }
    for name, banned in forbidden.items():
        deps = {d["name"] for d in by_name[name]["dependencies"]}
        if deps & banned:
            fail(f"{name} depends on {sorted(deps & banned)}, which the boundary plan moves to Mere")


# Ortet's cone (2026-09-03 ortet founding plan, O0). Ortet is the one headed
# host that proves the engine runs without Mere, so its resolved cone must
# contain no crate the platform boundary plan moves to Mere, and no other port.
# `tabard`, `knot-editor-host` and the `pelt` prefix name crates that left genet
# on 2026-09-03. They stay in the list: it is a set of names the cone may not
# reach, not a set of workspace members, so keeping them fails loudly if any of
# them ever arrives back from mere as a git or registry source.
ORTET_FORBIDDEN = {
    "inker", "workbench", "nematic", "errand", "document-canvas",
    "tabard", "knot-editor-host",
}
ORTET_FORBIDDEN_PREFIXES = ("cambium", "mere-", "pelt")
# `fleece` is named by the founding plan's forbidden list, but section 9.1 of
# the boundary plan reclasses it *independent* -- "it may stay in genet as a
# lower library or leave for its own repository, but it does not go to Mere" --
# and
# `genet-documents` reaches it unconditionally at `src/engines/clip.rs:114`
# (`fleece::extract_main_text`). Ortet cannot have the Livery session engine
# without it. It is carved out here rather than dropped silently, so the
# exception is visible on every CI run and fails loudly if it ever widens.
ORTET_RECLASSED = {"fleece"}


def is_ortet_forbidden(name: str) -> bool:
    return name in ORTET_FORBIDDEN or name.startswith(ORTET_FORBIDDEN_PREFIXES)


def resolved_cone(metadata: dict, package: str) -> set[str]:
    """Package names reachable from `package` over normal (non-dev, non-build)
    dependency edges in cargo's resolve graph."""
    resolve = metadata.get("resolve")
    if resolve is None:
        fail("cargo metadata carried no resolve graph")
    ids = {node["id"]: node for node in resolve["nodes"]}
    name_of = {p["id"]: p["name"] for p in metadata["packages"]}
    roots = [pid for pid, name in name_of.items() if name == package]
    if len(roots) != 1:
        fail(f"expected exactly one {package} package, found {len(roots)}")

    seen: set[str] = set()
    queue = [roots[0]]
    while queue:
        current = queue.pop()
        node = ids.get(current)
        if node is None:
            continue
        for dependency in node["deps"]:
            kinds = dependency.get("dep_kinds") or [{"kind": None}]
            # A `kind` of None is a normal dependency; "dev" and "build" are not
            # part of what the binary links.
            if not any(entry.get("kind") is None for entry in kinds):
                continue
            name = name_of.get(dependency["pkg"])
            if name is None or name in seen:
                continue
            seen.add(name)
            queue.append(dependency["pkg"])
    return seen


def assert_ortet_cone(metadata: dict) -> None:
    cone = resolved_cone(metadata, "ortet")
    if "genet-documents" not in cone or "netrender" not in cone:
        fail(
            "ortet's resolved cone is missing the engine crates it is built on "
            f"(saw {len(cone)} packages); the cone walk is not reading ortet"
        )
    breached = sorted(name for name in cone if is_ortet_forbidden(name))
    if breached:
        fail(
            "ortet's cone reaches crates the platform boundary plan moves to "
            f"Mere: {breached}"
        )

    # Positive controls, in the same function and over the same walk: the check
    # must be able to SEE what it forbids. The control was pelt-desktop until
    # Pelt left for mere on 2026-09-03. A walk that reports nothing on a cone
    # that is known to contain forbidden crates is a broken instrument, not a
    # clean cone.
    # Two controls, because pelt-desktop's cone used to exercise both halves of
    # `is_ortet_forbidden` at once: an exact name (`inker`) and a prefix
    # (`cambium`, `mere-`). No single remaining member reaches both, so the
    # controls are split rather than weakened.
    controls = (("document-canvas", "inker"), ("cambium-genet-winit-host", "cambium"))
    reported = {}
    for control_package, must_report in controls:
        control = resolved_cone(metadata, control_package)
        control_hits = sorted(name for name in control if is_ortet_forbidden(name))
        if must_report not in control_hits:
            fail(
                "cone witness positive control failed: the same check over "
                f"{control_package} did not report {must_report} "
                f"(reported {control_hits})"
            )
        reported[control_package] = control_hits
    rendered = "; ".join(f"{name} reports {hits}" for name, hits in reported.items())
    print(
        f"ortet cone: {len(cone)} packages, none forbidden; "
        f"positive controls: {rendered}"
    )

    reclassed = sorted(name for name in cone if name in ORTET_RECLASSED)
    if reclassed:
        print(
            f"ortet cone note: {reclassed} present - reclassed independent by "
            "the boundary plan 9.1, reached through genet-documents' clip lane"
        )


def main() -> None:
    assert_fleece_cone()
    metadata = cargo_metadata()
    assert_cargo_metadata_sees_fleece(metadata)
    assert_ports_depend_inward(metadata)
    assert_host_api_cone(metadata)
    self_test_mere_witness()
    resolved = cargo_metadata_resolved()
    assert_no_mere_source(resolved)
    assert_ortet_cone(resolved)
    assert_netfetcher_semantics_cone()
    print("dependency-cone witnesses passed")


if __name__ == "__main__":
    main()
