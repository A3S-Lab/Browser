# a3s-use-browser

`a3s-use-browser` is the provider-oriented Browser library for A3S. Its stable
boundary is the object-safe `PageRenderer` trait and typed
`RenderRequest`/`RenderedPage` contract.

The default `chrome` feature supplies local Chrome discovery, bounded managed
installation, pooled rendering, and persistent sessions. Callers may inject
another `PageRenderer` without depending on the complete Browser driver.

See the [repository README](../../README.md) for architecture, build, and
release ownership.
