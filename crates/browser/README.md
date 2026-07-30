# a3s-use-browser

`a3s-use-browser` is the provider-oriented Browser library for A3S. Its stable
boundary is the object-safe `PageRenderer` trait and typed
`RenderRequest`/`RenderedPage` contract.

The default `chrome` feature supplies local Chrome discovery, bounded managed
installation, pooled rendering, and persistent sessions. Callers may inject
another `PageRenderer` without depending on the complete Browser driver.

The optional `lightpanda` feature supplies bounded command-line HTML rendering
and CDP-backed interactive sessions. Command rendering enforces the caller's
single deadline and reaps timed-out provider processes. Chrome remains the
provider for exact user-agent overrides, selector waits, and screenshots.

See the [repository README](../../README.md) for architecture, build, and
release ownership.
