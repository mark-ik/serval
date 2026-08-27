# Row 18 `flex-basis: content` release receipt

The eight focused Flexbox reftests passed under the Livery renderer in two
independent release-mode runs. Every corresponding `release-1-*` and
`release-2-*` JSON map is byte-identical.

- Runner SHA-256:
  `eb236a1c866e105810f1d46240f30cc9fb4030aa1a9c5a2b62dfa1378b136b60`
- WPT manifest SHA-256:
  `d5ec5be9bf1a75ed00d7e7ab28afe8a694a55e11682ba74305874d70b18dd422`
- Renderer and policy: `livery`, `exact`
- Cases: `css/css-flexbox/flexbox-flex-basis-content-001a.html` through
  `004b.html`

Build the runner with:

```powershell
$env:CARGO_TARGET_DIR = 'C:\t\genet-row18-release'
$env:CARGO_PROFILE_RELEASE_DEBUG = '0'
cargo build -p genet-wpt --release -j 1 --offline
```

Then run each case separately with:

```powershell
& 'C:\t\genet-row18-release\release\genet-wpt.exe' reftest 'css/css-flexbox/flexbox-flex-basis-content-001a.html' --renderer livery --write-expectations '<output>.json' --expectation-policy exact
```
