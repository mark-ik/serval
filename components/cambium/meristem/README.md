# Meristem

Meristem is the renderer-independent reactive diff and message core of
Cambium. Backends implement its view and element contracts; Cambium's primary
backend targets Genet.

The crate supports `no_std` with `alloc` and contains no dependency on Genet,
Chisel, winit, or a renderer.

Meristem is derived from Linebender's Apache-2.0 `xilem_core`. Existing Xilem
copyright and SPDX headers remain in inherited source files. See
[`docs/upstream-xilem.md`](../../docs/upstream-xilem.md) for the recorded bases
and Cambium's semantic patch ledger.

## License

MPL-2.0 (see the repository `LICENSE`), as a derivative of Xilem: the Xilem
Authors' Apache-2.0 notice is retained in every derived file, and their
license text stays in [LICENSE](LICENSE) as the upstream notice.
