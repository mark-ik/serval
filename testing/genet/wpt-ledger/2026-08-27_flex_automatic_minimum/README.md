# Flex automatic-minimum release receipt

The automatic-minimum residual and eight adjacent `flex-basis: content`
reftests passed under the Livery renderer from HEAD `9d23efc433d15143e8110429c7c5f9d5d7c35fe3`.

- Runner SHA-256:
  `1711e2b1a3a5b943ac92aa83ac4816fb02ed69272b4c17eed525efcfeb5454c4`
- WPT manifest SHA-256:
  `d5ec5be9bf1a75ed00d7e7ab28afe8a694a55e11682ba74305874d70b18dd422`
- Renderer and policy: `livery`, `exact`
- Cases: `flex-minimum-height-flex-items-029.html` and
  `flexbox-flex-basis-content-001a.html` through `004b.html`

Build and freeze commands:

```powershell
$env:CARGO_HOME = 'C:\t\genet-row18-auto-min-cargo-home'
$env:CARGO_TARGET_DIR = 'C:\t\genet-row18-auto-min-release-target'
$env:CARGO_PROFILE_RELEASE_DEBUG = '0'
cargo build -p genet-wpt --release --locked -j 1
Copy-Item 'C:\t\genet-row18-auto-min-release-target\release\genet-wpt.exe' 'C:\t\genet-row18-auto-min-runner-9d23efc433d.exe' -Force
```

Each case was run separately with:

```powershell
& 'C:\t\genet-row18-auto-min-runner-9d23efc433d.exe' reftest '<case>' --renderer livery --write-expectations '<output>.json' --expectation-policy exact
```
