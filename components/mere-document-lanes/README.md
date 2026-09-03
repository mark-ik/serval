# mere-document-lanes

Mere's document content lanes above Genet, split out of `genet-documents` on
2026-09-02 under the platform boundary plan (mere
`design_docs/mere_docs/implementation_strategy/2026-09-02_platform_boundary_and_repository_topology_plan.md`,
P1). Homed at `components/mere-document-lanes` in genet until that plan moves
it to Mere.

- `ReaderSessionEngine` / `ReaderDocumentSession` (`genet.reader`): held HTML
  through fleece into the portable document canvas.
- `SmolwebDocument` / `SmolwebSessionEngine` (`smolweb` feature): protocol
  content through Nematic into the same canvas over the errand transport.
- `RemoteFetcher`: the remote half of a host's resource fetcher, http(s) over
  netfetcher (`netfetch` feature) and the smolweb schemes over errand
  (`smolweb` feature). Compose it under `genet_documents::LocalFetcher::with_fallback`.

Every session here implements `document-session-api`, the engine-facing
contracts Genet owns; the lanes themselves are application session and
routing, which is why they are Mere's.
