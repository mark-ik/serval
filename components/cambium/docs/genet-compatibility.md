# Genet compatibility

Cambium consumes Genet through published seam packages. `cambium-nematic`
adds reactive projections over Errand's portable smolweb ASTs; Genet's retained
document runtime remains in `genet-documents`.

## Current verified set

Verified on 2026-07-22:

- `genet-scripted-dom = 0.1.0`
- `layout-dom-api = 0.1.0`
- `errand = 0.1.3`
- core provider release commit:
  `2e462fe8975`

| Package | Version | Current source |
| --- | --- | --- |
| `layout-dom-api` | 0.1.0 | crates.io and sibling path |
| `errand` | 0.1.3 | crates.io and sibling path |
| `genet-paint-types` | 0.1.0 | crates.io |
| `engine-observables-api` | 0.1.1 | crates.io |
| `genet-static-dom` | 0.1.0 | crates.io |
| `genet-scripted-dom` | 0.1.0 | crates.io and sibling path |

## The Cambium stack: source vs registry (verified 2026-08-09)

| Package | Workspace | crates.io | State |
| --- | --- | --- | --- |
| `meristem` | 0.1.1 | 0.1.1 | current |
| `sprigging` | 0.2.1 | 0.2.1 | current |
| `cambium` | 0.3.2 | 0.3.2 | current |
| `cambium-nematic` | 0.3.1 | 0.3.1 | current |
| `cambium-winit` | 0.3.0 | 0.1.0 installable; 0.2.0 **yanked** | publishable, unpublished |
| `cambium-winit-a11y` | 0.3.0 | never published | `publish = false` by design |

`cambium-winit` 0.3.0 became publishable on 2026-07-26, when the
accessibility host moved out to `cambium-winit-a11y` exactly because
`genet-layout` and `genet-winit-host` inherit Genet's `publish = false`.
This document's earlier guidance ("keep its published line at 0.2.0 until
that dependency closes") is superseded: the dependency closed with that
split, and 0.2.0 is yanked on crates.io, so the installable registry line is
0.1.0 until 0.3.0 ships. `cambium-winit-a11y` can never publish, and that is
the reason it exists: holding the genet-coupled half keeps `cambium-winit`
down to `cambium` + `winit`.

Until `cambium-winit` 0.3.0 is published, the registry graph does not
resolve into a usable input-mapped host stack. Consumers ride the git-first
rule regardless (every sibling takes the family from genet.git by branch,
per the 2026-07-26 ruling); the registry serves external consumers only.
Cambium Nematic's release boundary is Cambium plus the protocol AST package,
without Genet's layout or rendering engine.

## Custom-leaf protocol

Cambium emits Genet's neutral `<custom-leaf>` element and related attribute
vocabulary. Genet temporarily accepts `<chisel-leaf>` as a read-side
compatibility alias for older documents.

## Direction rule

Cambium may depend on Genet seam crates. Genet engine crates must remain free
of Cambium, Meristem, and Sprigging dependencies. Reference applications such as
Pelt may depend on all three.
