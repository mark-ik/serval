# genet-documents

Genet's retained document sessions: Livery HTML, scripted HTML, and smolweb
content lanes as inker **session engines** (the third engine kind — spawn a
session, take paint frames, scroll, click, settle).

> **Home:** [`merely-made/genet`](https://github.com/merely-made/genet), at
> `components/genet-documents`. Born 2026-07-10 in the session-engines
> plan: these types began as pelt's convenience lanes and were promoted to
> an engine-grade component; pelt is now one consumer among hosts
> (turnstone's mere, meerkat).

- `LiveryDocument` / `LiverySessionEngine` (`livery` feature,
  `genet.livery`): the owned script-free HTML implementation. It retains
  style/layout/text paint state and lowers the neutral PaintList into the same
  scene contract. It also routes bounded viewport scrolling, retained link
  rectangles, pointer hit testing, fragment navigation, focus state, text
  editing, IME composition, and structured form submission. Script-free form
  mutations reuse the retained DOM and travel through Livery restyle, Buckram
  layout, and paint. The session also exposes the retained animation clock for
  host-driven opacity frames, bounded CSS opacity/background-color/color
  transitions, and opacity-only `@keyframes` with named timing functions.
  Nested scroll chaining is routed through the retained session and chains at
  its boundary. The session asks the host `ResourceFetcher`
  for CSS/DOM image URLs and feeds returned bytes into the neutral image side
  table. The bounded lane includes text color and border-top-color/border-bottom-color interpolation;
  broader transition-property lists/interpolation remain open. Livery's image gate
  covers two-stop gradients, raster `data:` URLs, host-resolved local and
  remote-looking bytes, replaced-element intrinsic sizing, and bounded
  position/repeat modes; URL policy and caching remain a host fetch/cache
  concern.
- `ScriptedDocument` sessions / `ScriptedSessionEngine<E, _>` (`scripted`
  feature): the same Livery/Buckram document path with a live DOM whose JS
  runs on Boa (or Nova on the nova rung), plus the tick + quiescence seam
  (`pump` / `settled`).
- `SmolwebDocument` / `SmolwebSessionEngine` (`smolweb` feature): capsules
  rendered through the engine-native document path: Nematic lowers protocol
  content to `EngineDocument`, then document-canvas lays it out and lowers its
  PaintList to a scene.

Construction seams (fetchers, cookie jars, themes) live on the engine at
registration; the spawn request stays plain data. The session wrappers are
public for hosts with richer seams. Unpublished: this crate rides genet's
in-tree components; consume it as a git dependency.
