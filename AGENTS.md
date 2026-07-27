# AGENTS.md

## Repository

This repository owns the provider-oriented `a3s-use-browser` library, the
complete `a3s-use-browser-driver` process, Browser Skills, and the static
Dashboard.

## Boundaries

- Keep `PageRenderer` object-safe and `Send + Sync`; Search depends only on
  that typed contract.
- Inject renderers and provider options as typed objects. Do not add a raw
  backend-name switch to the library API.
- Keep the library usable without the driver, MCP, CLI state, or a resident
  process.
- Keep the complete interactive driver process-isolated from library callers.
- Preserve standard MCP and native CLI contracts; do not add a custom JSON-RPC
  envelope.
- Browser provider installation must remain explicit, bounded, and restricted
  to approved sources.
- Keep checked-in Dashboard output synchronized with its source.
- Preserve upstream Apache-2.0 notices and provenance.

## Engineering

- Use Tokio for I/O and avoid blocking inside async contexts.
- Keep public types `Send + Sync` where applicable.
- Return contextual typed errors and avoid production panics.
- Keep all code and documentation in English.
- Run Rust checks from this workspace and Dashboard checks from
  `crates/browser-driver/dashboard`.
