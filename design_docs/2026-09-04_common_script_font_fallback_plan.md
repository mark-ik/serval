# Common-script font fallback

**Status:** scope (2026-09-04). Diagnosis verified in source and stated in §2;
T0 through T3 open. Founded when Isometry's side panel drew its disclosure
markers as tofu and the workaround was to retreat to ASCII.

**Related:** `components/genet-livery/src/text.rs` (the stack's whole font
story: it builds the parley `FontContext` and hands parley the resolved
family); `isometry/design_docs/2026-09-03_side_panel_diet_plan.md` (the
consumer that hit it, and the ASCII retreat recorded there).

## 1. Why

A cross-platform stack cannot draw only the characters its default UI font
happens to carry. Isometry put `▾` and `▸` on a disclosure trigger and got two
tofu boxes; it now ships `[-]` and `[+]`. That is a real product cost paid to
work around a stack defect, and the next non-ASCII glyph anyone reaches for
pays it again.

The framing that came out of that pass — "our font fallback covers Latin-1 but
not Geometric Shapes" — is not what is happening, and the true statement is
much broader. Latin-1 renders because the *primary* family carries it and no
fallback is needed. What is actually true is below.

## 2. The diagnosis, verified

**On Windows and macOS, font fallback is never successfully consulted for any
codepoint whose Unicode script is Common.** Not "fails for symbols": returns
nothing, always, by construction. The chain, each link read in source:

1. `▾` U+25BE is Geometric Shapes, whose Unicode script property is
   **Common (`Zyyy`)**. So are the arrows, box drawing, dingbats, most
   punctuation above Latin-1, and the general symbol blocks.
2. parley derives the run's fallback key from that script and passes it
   through unchanged — `parley-0.10.0/src/convert.rs:8-18` maps the script to
   its short name, or `Zzzz` when unknown — then
   `shape/mod.rs:559` sets `fontique::FallbackKey::new(fb_script, locale)`.
   So fontique is asked for "a family for script `Zyyy`".
3. fontique's fallback is **script-keyed, not codepoint-keyed**. The
   DirectWrite backend does not ask DirectWrite which font has the character.
   It asks for a *sample string* for the script and requests the default
   family for that sample: `fontique-0.10.0/src/backend/dwrite.rs:115-123`,
   `let text = key.script().sample()?;`.
4. **There is no sample for Common.** `fontique-0.10.0/src/script.rs` carries
   161 script samples and has no `Zyyy` entry, and none for `Zzzz` either.
   `Script::sample()` returns `None`.
5. The `?` on that line therefore returns `None` from `fallback()`. No
   candidate family is offered, the notdef from the primary family stands, and
   the glyph paints as tofu.

**macOS is the same defect, same shape**: `backend/coretext.rs:68-78` also
does `script.sample()?` before `create_fallback_font_for_text`.

**Linux is a different path and is not yet assessed.**
`backend/fontconfig.rs:735` builds a fontconfig `Pattern` from lang and script
rather than a sample, so it may or may not resolve Common. T3 settles it
rather than assuming; the workspace has both a Fedora Wayland and a Mint X11
machine to answer it on.

The irony worth recording: the DirectWrite backend already holds an
`IDWriteFontFallback` (`dwrite.rs:137`) — the very interface whose
`MapCharacters` answers "which font covers this text" — and the script path
never uses it for characters. CoreText has the same shape available in
`CTFontCreateForString`.

## 3. What it costs, concretely

Every consumer of this stack, on two of three desktop platforms, silently
cannot render: geometric shapes and arrows (so no disclosure markers, sort
indicators, or breadcrumb separators), box drawing, dingbats and check marks,
most currency and mathematical symbols, and general punctuation beyond
Latin-1 — unless the resolved primary family happens to carry them. It is
invisible until someone types one, and the failure is a silent tofu rather
than an error.

## 4. Gates

**T0 — The instrument.** A test in `genet-livery` that shapes a Common-script
codepoint through the real `TextSystem` and asserts the resulting glyph is not
notdef, plus a Latin control in the same run so a pass cannot be a false
negative. It must **fail** on Windows before anything is fixed; a green T0 at
the start means the test is wrong.
**Done when:** the failing test exists, is committed, and its failure is the
recorded shape of the defect.

**T1 — The stack-side repair.** genet-livery stops depending on a fallback
that cannot answer. Three candidates, to be chosen against T0 rather than
argued: append a symbol-bearing family to the family list the stack resolves,
so the primary stack itself covers Common; register a bundled symbol face at
`TextSystem::new` the way `register_font_bytes` already registers host fonts;
or have the stack consult a codepoint-aware fallback itself before handing
parley a family. The first two are small and platform-independent; the third
duplicates what upstream should do.
**Done when:** T0 passes on Windows with no upstream change, no consumer has
to avoid a character, and a headed capture shows a Common-script glyph
painting in a real app.

**T2 — The upstream fix.** fontique's fallback should be codepoint-aware on
the backends whose platform API already is: `IDWriteFontFallback::MapCharacters`
on Windows, `CTFontCreateForString` on macOS. A narrower stopgap is to add
`Zyyy` and `Zzzz` samples to the script table, which is a one-line data change
but keeps the wrong shape — a sample is a guess about a script, and Common is
precisely the script for which no sample can be representative.
**Done when:** the change exists upstream or on a fork carried the way the
workspace already carries others, T0 passes with the stack-side repair
removed, and the disposition of T1 is recorded — kept as defence in depth, or
retired.

**T3 — Linux.** Determine whether fontconfig resolves Common, on both Fedora
Wayland and Mint X11.
**Done when:** T0 has run on both and the answer is in this plan's Findings.

## 5. Stop rules

- No consumer is asked to avoid a character as the fix. Isometry's ASCII
  retreat is a workaround this plan exists to retire, not a precedent.
- The stack-side repair does not become a private font stack that diverges
  from what CSS asked for; a family the author named still wins.
- T2 does not wait on T1 and T1 does not wait on T2. A consumer-visible fix
  should not be gated on an upstream release.

## Findings

### 2026-09-04 — the chain, and what it corrects

The five links in §2 were each read in source rather than inferred, which
matters because the obvious framing was wrong twice over. It is not that
fallback covers Latin-1 and stops; the primary font covers Latin-1 and
fallback covers *nothing* for Common. And it is not a Geometric Shapes
problem; it is every Common-script codepoint, on two platforms.

One hypothesis was raised and killed on the way, worth recording so nobody
spends the same hour: `genet-livery`'s `font_family` (`text.rs:3722`) maps
only `system-ui` and the user-agent default to a `GenericFamily`, and passes
everything else — including the CSS generic keyword `sans-serif` — through as
`FontFamily::Source`, a literal family name. That looks like the bug and is
not: parley's `resolve/mod.rs:217-230` runs `FontFamilyName::parse_css_list`
over the source string, which recognises the generic keywords and expands them
through `generic_families`. The generic resolves correctly. The defect is
downstream of family resolution, in what happens when the resolved family has
no glyph.

## Progress

- **2026-09-04.** Scoped after Isometry's disclosure markers rendered as tofu.
  Diagnosis verified end to end; T0 through T3 open.
