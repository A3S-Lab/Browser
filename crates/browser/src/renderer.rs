use std::time::{Duration, Instant};

use a3s_use_core::{Artifact, UseError, UseResult};
use async_trait::async_trait;
use chromiumoxide::cdp::browser_protocol::network::SetUserAgentOverrideParams;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;
use sha2::{Digest, Sha256};
use tracing::warn;
use url::Url;

use crate::pool::{browser_error, BrowserPool};
use crate::{PageRenderer, RenderRequest, RenderedPage, WaitCondition};

#[async_trait]
impl PageRenderer for BrowserPool {
    async fn render(&self, request: RenderRequest) -> UseResult<RenderedPage> {
        #[cfg(feature = "lightpanda")]
        if self.uses_lightpanda() {
            return self.render_with_lightpanda(request).await;
        }

        let timeout = request.timeout();
        match tokio::time::timeout(timeout, self.render_inner(request)).await {
            Ok(result) => result,
            Err(_) => Err(UseError::new(
                "use.browser.timeout",
                format!("Browser rendering exceeded {} ms.", timeout.as_millis()),
            )),
        }
    }
}

impl BrowserPool {
    async fn render_inner(&self, request: RenderRequest) -> UseResult<RenderedPage> {
        let started = Instant::now();
        let _permit = self
            .tab_semaphore()
            .acquire()
            .await
            .map_err(|error| browser_error(format!("Tab limit is closed: {error}")))?;
        let browser = self.acquire_browser().await?;
        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|error| browser_error(format!("Failed to open browser tab: {error}")))?;
        let guard = PageGuard::new(page);
        let page = guard.page()?;

        if let Some(user_agent) = &request.user_agent {
            page.set_user_agent(SetUserAgentOverrideParams::new(user_agent))
                .await
                .map_err(|error| {
                    browser_error(format!("Failed to set browser user agent: {error}"))
                })?;
        }

        page.goto(request.url.as_str())
            .await
            .map_err(|error| browser_error(format!("Browser navigation failed: {error}")))?;
        apply_wait_condition(page, &request.wait).await?;
        let html = page
            .content()
            .await
            .map_err(|error| browser_error(format!("Failed to read rendered HTML: {error}")))?;
        let final_url = page
            .url()
            .await
            .ok()
            .flatten()
            .and_then(|value| Url::parse(&value).ok())
            .unwrap_or_else(|| request.url.clone());
        let artifacts = match &request.screenshot_path {
            Some(path) => vec![capture_screenshot(page, path).await?],
            None => Vec::new(),
        };
        guard.close().await;

        Ok(RenderedPage {
            requested_url: request.url,
            final_url,
            status: None,
            content_type: Some("text/html".to_string()),
            html,
            elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            artifacts,
        })
    }
}

pub(crate) async fn apply_wait_condition(
    page: &chromiumoxide::Page,
    condition: &WaitCondition,
) -> UseResult<()> {
    match condition {
        // `Page::goto` resolves after the requested page is loaded.
        WaitCondition::Load | WaitCondition::DomContentLoaded => {}
        WaitCondition::NetworkIdle { idle_ms } => {
            tokio::time::sleep(Duration::from_millis(*idle_ms)).await;
        }
        WaitCondition::Selector { css, timeout_ms } => {
            match tokio::time::timeout(
                Duration::from_millis(*timeout_ms),
                page.find_element(css.as_str()),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    return Err(browser_error(format!(
                        "Browser selector '{css}' failed: {error}"
                    )))
                }
                Err(_) => {
                    return Err(UseError::new(
                        "use.browser.wait_timeout",
                        format!("Selector '{css}' was not found within {timeout_ms} ms."),
                    ))
                }
            }
        }
        WaitCondition::Delay { ms } => {
            tokio::time::sleep(Duration::from_millis(*ms)).await;
        }
    }
    Ok(())
}

pub(crate) async fn capture_screenshot(
    page: &chromiumoxide::Page,
    path: &std::path::Path,
) -> UseResult<Artifact> {
    let bytes = page
        .save_screenshot(
            ScreenshotParams::builder()
                .format(CaptureScreenshotFormat::Png)
                .full_page(true)
                .build(),
            path,
        )
        .await
        .map_err(|error| browser_error(format!("Failed to save browser screenshot: {error}")))?;
    let sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(Artifact {
        path: path.to_path_buf(),
        media_type: "image/png".to_string(),
        size: bytes.len().try_into().unwrap_or(u64::MAX),
        sha256,
    })
}

struct PageGuard {
    page: Option<chromiumoxide::Page>,
}

impl PageGuard {
    fn new(page: chromiumoxide::Page) -> Self {
        Self { page: Some(page) }
    }

    fn page(&self) -> UseResult<&chromiumoxide::Page> {
        self.page.as_ref().ok_or_else(|| {
            UseError::new(
                "use.browser.page_closed",
                "The browser page was closed before rendering completed.",
            )
        })
    }

    async fn close(mut self) {
        if let Some(page) = self.page.take() {
            if let Err(error) = page.close().await {
                warn!("Failed to close browser tab: {error}");
            }
        }
    }
}

impl Drop for PageGuard {
    fn drop(&mut self) {
        if let Some(page) = self.page.take() {
            match tokio::runtime::Handle::try_current() {
                Ok(runtime) => {
                    runtime.spawn(async move {
                        if let Err(error) = page.close().await {
                            warn!("Failed to close browser tab after cancellation: {error}");
                        }
                    });
                }
                Err(error) => warn!("Cannot schedule browser tab cleanup: {error}"),
            }
        }
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    use crate::{BrowserPoolConfig, BrowserProvider};
    #[cfg(feature = "lightpanda")]
    use std::sync::Arc;

    #[cfg(feature = "lightpanda")]
    fn executable_fixture(contents: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("lightpanda");
        std::fs::write(&executable, contents).unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();
        (directory, executable)
    }

    #[cfg(feature = "lightpanda")]
    #[tokio::test]
    async fn lightpanda_renderer_uses_the_fetch_command_for_html() {
        let (_directory, executable) = executable_fixture(
            "#!/bin/sh\nprintf '<!DOCTYPE html><html><body>cli fixture</body></html>'\n",
        );
        let pool = BrowserPool::new(BrowserPoolConfig {
            provider: BrowserProvider::LightpandaExecutable(executable),
            ..BrowserPoolConfig::default()
        });
        let request = RenderRequest {
            url: Url::parse("https://example.test/search?q=rust").unwrap(),
            timeout_ms: 5_000,
            wait: WaitCondition::Load,
            user_agent: None,
            screenshot_path: None,
        };

        let rendered = pool.render(request).await;
        pool.shutdown().await;

        let rendered = rendered.unwrap();
        assert!(rendered.html.contains("cli fixture"));
        assert_eq!(rendered.content_type.as_deref(), Some("text/html"));
    }

    #[cfg(feature = "lightpanda")]
    #[tokio::test]
    async fn lightpanda_renderer_forwards_url_deadline_and_proxy_as_arguments() {
        let directory = tempfile::tempdir().unwrap();
        let arguments = directory.path().join("arguments.txt");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '<html>arguments fixture</html>'\n",
            arguments.display()
        );
        let (_executable_directory, executable) = executable_fixture(&script);
        let proxy = "http://user:secret@proxy.example:8080";
        let pool = BrowserPool::new(BrowserPoolConfig {
            provider: BrowserProvider::LightpandaExecutable(executable),
            proxy_url: Some(proxy.to_string()),
            ..BrowserPoolConfig::default()
        });
        let request = RenderRequest {
            url: Url::parse("https://example.test/search?q=rust").unwrap(),
            timeout_ms: 5_000,
            wait: WaitCondition::Load,
            user_agent: None,
            screenshot_path: None,
        };

        pool.render(request).await.unwrap();
        pool.shutdown().await;

        let arguments = std::fs::read_to_string(arguments).unwrap();
        assert!(arguments.contains("fetch\n"));
        assert!(arguments.contains("--dump\nhtml\n"));
        assert!(arguments.contains("--http_connect_timeout\n5000\n"));
        assert!(arguments.contains("--http_timeout\n5000\n"));
        assert!(arguments.contains(&format!("--http_proxy\n{proxy}\n")));
        assert!(arguments.ends_with("https://example.test/search?q=rust\n"));
    }

    #[cfg(feature = "lightpanda")]
    #[tokio::test]
    async fn lightpanda_renderer_kills_and_reaps_a_timed_out_fetch() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("pid.txt");
        let script = format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec sleep 30\n",
            pid_file.display()
        );
        let (_executable_directory, executable) = executable_fixture(&script);
        let pool = BrowserPool::new(BrowserPoolConfig {
            provider: BrowserProvider::LightpandaExecutable(executable),
            ..BrowserPoolConfig::default()
        });
        let request = RenderRequest {
            url: Url::parse("https://example.test/").unwrap(),
            timeout_ms: 2_000,
            wait: WaitCondition::Load,
            user_agent: None,
            screenshot_path: None,
        };

        let error = pool.render(request).await.unwrap_err();
        pool.shutdown().await;

        assert_eq!(error.code, "use.browser.timeout");
        let pid = std::fs::read_to_string(pid_file).unwrap();
        let still_running = std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .output()
            .unwrap()
            .status
            .success();
        assert!(
            !still_running,
            "timed-out Lightpanda process {pid} survived"
        );
    }

    #[cfg(feature = "lightpanda")]
    #[tokio::test]
    async fn cancelling_lightpanda_render_still_kills_and_reaps_the_fetch() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("cancelled-pid.txt");
        let script = format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec sleep 30\n",
            pid_file.display()
        );
        let (_executable_directory, executable) = executable_fixture(&script);
        let pool = Arc::new(BrowserPool::new(BrowserPoolConfig {
            provider: BrowserProvider::LightpandaExecutable(executable),
            ..BrowserPoolConfig::default()
        }));
        let request = RenderRequest {
            url: Url::parse("https://example.test/").unwrap(),
            timeout_ms: 30_000,
            wait: WaitCondition::Load,
            user_agent: None,
            screenshot_path: None,
        };
        let render_pool = Arc::clone(&pool);
        let render = tokio::spawn(async move { render_pool.render(request).await });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !pid_file.is_file() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Lightpanda fixture did not start");

        render.abort();
        let _ = render.await;
        let pid = std::fs::read_to_string(pid_file).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let still_running = std::process::Command::new("kill")
                    .args(["-0", pid.trim()])
                    .output()
                    .unwrap()
                    .status
                    .success();
                if !still_running {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("cancelled Lightpanda process was not reaped");
        pool.shutdown().await;
    }

    #[cfg(feature = "lightpanda")]
    #[tokio::test]
    async fn lightpanda_renderer_rejects_unsupported_exact_user_agent_without_spawning() {
        let pool = BrowserPool::new(BrowserPoolConfig {
            provider: BrowserProvider::LightpandaExecutable("/not/spawned".into()),
            ..BrowserPoolConfig::default()
        });
        let request = RenderRequest {
            url: Url::parse("https://example.test/").unwrap(),
            timeout_ms: 1_000,
            wait: WaitCondition::Load,
            user_agent: Some("exact-agent".to_string()),
            screenshot_path: None,
        };

        let error = pool.render(request).await.unwrap_err();
        pool.shutdown().await;

        assert_eq!(error.code, "use.browser.unsupported");
        assert!(error.message.contains("user-agent"));
    }

    #[cfg(feature = "lightpanda")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn installed_lightpanda_renders_a_local_http_page_when_available() {
        use tokio::io::AsyncWriteExt;

        let Some(executable) = crate::detect_lightpanda() else {
            return;
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: 41\r\nConnection: close\r\n\r\n<html><body>runtime fixture</body></html>",
                )
                .await
                .unwrap();
        });
        let pool = BrowserPool::new(BrowserPoolConfig {
            provider: BrowserProvider::LightpandaExecutable(executable),
            ..BrowserPoolConfig::default()
        });
        let request = RenderRequest {
            url: Url::parse(&format!("http://{address}/fixture")).unwrap(),
            timeout_ms: 5_000,
            wait: WaitCondition::Load,
            user_agent: None,
            screenshot_path: None,
        };

        let rendered = pool.render(request).await;
        pool.shutdown().await;
        server.abort();

        let rendered = rendered.unwrap();
        assert!(rendered.html.contains("runtime fixture"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn discovered_chrome_renders_a_network_free_page_when_available() {
        let _guard = crate::test_support::lock_chrome_integration_test().await;
        let Some(executable) = crate::detect_chrome() else {
            return;
        };
        let pool = BrowserPool::new(BrowserPoolConfig {
            provider: BrowserProvider::ChromeExecutable(executable),
            ..BrowserPoolConfig::default()
        });
        let request = RenderRequest {
            url: Url::parse("data:text/html,<main id='fixture'>a3s-use</main>").unwrap(),
            timeout_ms: crate::test_support::CHROME_OPERATION_TIMEOUT_MS,
            wait: WaitCondition::Load,
            user_agent: Some("a3s-use-browser-test".to_string()),
            screenshot_path: None,
        };

        let rendered = pool.render(request).await;
        pool.shutdown().await;

        let rendered = rendered.unwrap();
        assert!(rendered.html.contains("a3s-use"));
        assert_eq!(rendered.content_type.as_deref(), Some("text/html"));
    }
}
