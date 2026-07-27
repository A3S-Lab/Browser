# A3S Browser

A3S Browser is the independently maintained Browser capability used by A3S
Use and A3S Search. One repository owns two deliberate layers:

```text
typed SDK caller -> PageRenderer -> a3s-use-browser
                                      |
A3S Use CLI/MCP -> process boundary -> a3s-use-browser-driver
                                      +-> Skills and static Dashboard
```

`a3s-use-browser` exposes the object-safe `PageRenderer` contract, typed render
requests and results, browser lifecycle, and persistent session primitives.
Embedded callers inject `Arc<dyn PageRenderer>` and do not require the driver,
MCP, CLI state, or a resident process. A3S Search consumes this layer directly.

`a3s-use-browser-driver` owns the complete interactive automation surface,
standard MCP tools, native CLI compatibility, daemon lifecycle, Skills, and
Dashboard. A3S Use launches it as a sibling process so advanced automation
state does not leak into the reusable library.

The repository preserves the existing `a3s-use-browser` and
`a3s-use-browser-driver` identities. A3S Use pins an immutable repository
revision when assembling its built-in `browser` route and release assets.

## Typed rendering

```rust
use std::sync::Arc;

use a3s_use_browser::{BrowserRuntime, PageRenderer, RenderRequest};
use url::Url;

# async fn example(renderer: Arc<dyn PageRenderer>) -> a3s_use_browser::UseResult<()> {
let browser = BrowserRuntime::new(renderer);
let page = browser
    .render(RenderRequest::new(Url::parse("https://example.com").unwrap()))
    .await?;
assert_eq!(page.status, Some(200));
# Ok(())
# }
```

Provider selection remains typed. The default embedded provider discovers a
local Chrome-compatible browser and can use the bounded managed cache. The
optional `lightpanda` feature enables the separately managed Lightpanda
provider.

## Build

```bash
cargo fmt --all -- --check
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo package -p a3s-use-browser --locked
```

Build the checked-in Dashboard bundle separately:

```bash
pnpm --dir crates/browser-driver/dashboard install --frozen-lockfile
pnpm --dir crates/browser-driver/dashboard build
```

## Release ownership

This repository owns Browser source, tests, crate publication, platform driver
archives, Skills, Dashboard assets, and upstream provenance. A3S Use owns the
built-in route, final product assembly, and capability projection. A3S Search
owns only its adapter from `PageRenderer` to search fetching.

## Licensing

The typed Browser library and original A3S integration code are licensed under
the [MIT License](LICENSE). The compatibility driver and Dashboard contain
work derived from `vercel-labs/agent-browser` under Apache-2.0; see the
[driver license](crates/browser-driver/LICENSE-APACHE-2.0),
[upstream provenance](crates/browser-driver/UPSTREAM.md), and
[third-party notices](THIRD_PARTY_NOTICES.md).
