#!/usr/bin/env python3
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
    pelt_packages = {}

    for package in metadata["packages"]:
        manifest = pathlib.Path(package["manifest_path"]).resolve()
        if package["name"] == "genet-host-api":
            host_api.append(manifest)
        if package["name"] in {"pelt", "pelt-desktop"}:
            pelt_packages[package["name"]] = manifest
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

    expected_pelt = {
        "pelt": (ports / "pelt" / "Cargo.toml").resolve(),
        "pelt-desktop": (ports / "pelt" / "desktop" / "Cargo.toml").resolve(),
    }
    if pelt_packages != expected_pelt:
        rendered = {
            name: str(path.relative_to(ROOT)) for name, path in pelt_packages.items()
        }
        fail(f"Pelt package manifests are {rendered}, expected {expected_pelt}")


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


def main() -> None:
    assert_fleece_cone()
    metadata = cargo_metadata()
    assert_cargo_metadata_sees_fleece(metadata)
    assert_ports_depend_inward(metadata)
    self_test_mere_witness()
    assert_no_mere_source(cargo_metadata_resolved())
    print("dependency-cone witnesses passed")


if __name__ == "__main__":
    main()
