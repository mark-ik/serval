# Cambium

Cambium is a Genet-native reactive GUI toolkit. It combines Meristem's
reactive view core with a Genet DOM backend and Sprigging custom leaves.

This repository was extracted from Genet's former Serval tree. Its public
backend vocabulary is now the `Genet*` family. Deprecated `Serval*` aliases
remain for source compatibility during consumer migration.

## Crates

- `meristem`: renderer-independent reactive diff and message core
- `cambium`: Genet backend, application runner, controls, and composition
- `cambium-winit`: winit keyboard translation for Cambium applications
- `cambium-nematic`: reactive views and themes over Errand's smolweb ASTs
- `sprigging`: engine-neutral custom leaves and arrangement geometry

Every crate is MPL-2.0 (see the repository `LICENSE`); Meristem, a Xilem
derivative, keeps the Xilem Authors' Apache-2.0 notice in each derived file.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the ownership rule and
[docs/upstream-xilem.md](docs/upstream-xilem.md) for provenance. Licenses are
recorded in the repository [LICENSES.md](../../LICENSES.md), and the claimed
package names in [docs/namespace-claims.md](docs/namespace-claims.md).
Standalone and sibling-checkout development are described in
[docs/local-genet-development.md](docs/local-genet-development.md).

## Component acceptance surface

The executable component catalog covers Cambium's controls, hover routing,
editors, action list, overlay menu, virtualized grid, and Sprigging glyph
leaves. Run it with:

```sh
cargo run -p cambium --example component_catalog --all-features
```

The same assertions run in CI as an example test. See
[docs/component-catalog.md](docs/component-catalog.md) for the coverage rule.

## License

MPL-2.0 (see the repository `LICENSE`). `meristem` retains the Xilem Authors'
Apache-2.0 notice as a derivative; see the repository
[`LICENSES.md`](../../LICENSES.md).
