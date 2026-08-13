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

Restrict a session to exact Web origins with `--allowed-origins`. Origins are
normalized by scheme, host, and effective port; matching WebSocket schemes use
the same authority. Add `--allowed-domains` only for deliberate hostname-wide
network access. Browser 0.4.0 applies the union before requests are sent and
rechecks every redirect. Existing CDP sessions, auto-connect, persistent
profiles, restore or state replay, unsafe startup arguments, direct-page
providers, iOS, and Safari are rejected because they cannot provide the same
pre-script boundary.
