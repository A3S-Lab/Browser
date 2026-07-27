# A3S Browser Driver

`a3s-use-browser-driver` is the process-isolated complete automation runtime
for A3S Browser. It owns the native CLI, standard MCP tools, daemon lifecycle,
Skills, and static Dashboard.

The driver contains modified work from `vercel-labs/agent-browser`. See
[UPSTREAM.md](UPSTREAM.md) and [LICENSE-APACHE-2.0](LICENSE-APACHE-2.0).

Build from the repository root:

```bash
cargo build --release --locked -p a3s-use-browser-driver
```
