# A3S Browser

<p align="center">
  <strong>Language / 语言:</strong>
  <a href="README.md">English</a> ·
  <a href="README.zh-CN.md">中文</a>
</p>

A3S Browser 是由 A3S Use 与 A3S Search 使用、独立维护的 Browser 能力。一个仓库刻意分成两层：

```text
typed SDK caller -> PageRenderer -> a3s-use-browser
                                      |
A3S Use CLI/MCP -> process boundary -> a3s-use-browser-driver
                                      +-> Skills and static Dashboard
```

`a3s-use-browser` 暴露对象安全的 `PageRenderer` 契约、类型化的渲染请求与结果、浏览器生命周期，以及持久会话原语。嵌入式调用方注入 `Arc<dyn PageRenderer>`，不需要 driver、MCP、CLI 状态或常驻进程。A3S Search 直接消费这一层。

`a3s-use-browser-driver` 拥有完整的交互式自动化表面、标准 MCP 工具、原生 CLI 兼容、守护进程生命周期、Skills 与 Dashboard。A3S Use 将其作为兄弟进程启动，使高级自动化状态不会泄漏进可复用库。

Browser 0.4.0 为本地 Chromium 会话增加了请求时精确源（exact-origin）遏制。`--allowed-origins` 匹配 scheme、小写 host 与有效端口；对应的 `http`/`ws` 或 `https`/`wss` 权威被视为同一权限。显式的 `--allowed-domains` 条目仍是更宽的仅网络例外。driver 用同一策略检查初始导航、每一次重定向、Fetch 暂停的请求、弹窗、worker、运行时网络 API，以及 `read`。无法在页面代码运行前安装遏制的启动模式会被拒绝。

本仓库保留既有的 `a3s-use-browser` 与 `a3s-use-browser-driver` 身份。A3S Use 在组装内置 `browser` 路由与发布产物时，会钉住不可变的仓库修订。

## 类型化渲染

```rust
use std::sync::Arc;

use a3s_use_browser::{BrowserRuntime, PageRenderer, RenderRequest};
use url::Url;

# async fn example(renderer: Arc<dyn PageRenderer>) -> a3s_use_browser::UseResult<()> {
let browser = BrowserRuntime::new(renderer);
let page = browser
    .render(RenderRequest::new(Url::parse("https://example.com").unwrap()))
    .await?;
assert!(!page.html.is_empty());
# Ok(())
# }
```

Provider 选择保持类型化。默认嵌入式 provider 发现本地 Chrome 兼容浏览器，并可使用有界托管缓存。可选的 `lightpanda` feature 启用单独管理的 Lightpanda provider。Lightpanda 发现也识别 A3S Search 在 Browser 仓库拆分前使用的历史布局 `~/.a3s/lightpanda/<version>/lightpanda`。历史运行时被视为外部只读安装；Browser 仅更新或移除其收据（receipt）支持的托管数据根中的安装。

Lightpanda HTML 渲染使用其有界的 `fetch` 命令，而不是假定完整的 Chromium CDP 生命周期兼容。每一次渲染在 provider 解析、排队、导航、输出收集与可选的拉取后等待之间共享同一个截止时间。超时或调用方取消会调度有界的进程清理，从而杀死并回收 provider 子进程。精确的 user-agent 覆盖、选择器等待与截图仍是仅 Chrome 能力；从 Lightpanda 渲染器请求时会显式失败。

## 构建

```bash
cargo fmt --all -- --check
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo package -p a3s-use-browser --locked
```

单独构建已检入的 Dashboard 产物：

```bash
pnpm --dir crates/browser-driver/dashboard install --frozen-lockfile
pnpm --dir crates/browser-driver/dashboard build
```

## 发布所有权

本仓库拥有 Browser 源码、测试、crate 发布、平台 driver 归档、Skills、Dashboard 资产，以及上游来源证明。A3S Use 拥有内置路由、最终产品组装与能力投影。A3S Search 仅拥有从 `PageRenderer` 到搜索抓取的适配器。

## 许可

类型化 Browser 库与原始 A3S 集成代码采用 [MIT License](LICENSE)。兼容 driver 与 Dashboard 包含源自 `vercel-labs/agent-browser`、采用 Apache-2.0 的工作；参见 [driver 许可证](crates/browser-driver/LICENSE-APACHE-2.0)、[上游来源](crates/browser-driver/UPSTREAM.md) 与 [第三方声明](THIRD_PARTY_NOTICES.md)。
