//! Lightpanda command-line rendering for the typed page renderer contract.

use std::process::{ExitStatus, Stdio};
use std::time::Instant;

use a3s_use_core::{UseError, UseResult};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::task::JoinHandle;
use tracing::warn;

use crate::{BrowserPool, RenderRequest, RenderedPage, WaitCondition};

const STDERR_DIAGNOSTIC_LIMIT: usize = 4 * 1024;

impl BrowserPool {
    pub(crate) async fn render_with_lightpanda(
        &self,
        request: RenderRequest,
    ) -> UseResult<RenderedPage> {
        validate_supported_request(&request)?;
        self.ensure_open()?;

        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + request.timeout();
        let _permit = tokio::time::timeout_at(deadline, self.tab_semaphore().acquire())
            .await
            .map_err(|_| render_timeout(&request))?
            .map_err(|error| super::pool::browser_error(format!("Tab limit is closed: {error}")))?;
        self.ensure_open()?;

        let executable = tokio::time::timeout_at(deadline, self.lightpanda_executable())
            .await
            .map_err(|_| render_timeout(&request))??;
        let proxy = self.lightpanda_proxy_url().map(str::to_string);
        let html = run_fetch_command(&executable, &request, proxy.as_deref(), deadline).await?;
        apply_post_fetch_wait(&request.wait, deadline, &request).await?;

        Ok(RenderedPage {
            requested_url: request.url.clone(),
            final_url: request.url,
            status: None,
            content_type: Some("text/html".to_string()),
            html,
            elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            artifacts: Vec::new(),
        })
    }
}

fn validate_supported_request(request: &RenderRequest) -> UseResult<()> {
    if request.screenshot_path.is_some() {
        return Err(unsupported(
            "Lightpanda command-line rendering does not support screenshots.",
        ));
    }
    if request.user_agent.is_some() {
        return Err(unsupported(
            "Lightpanda command-line rendering cannot apply an exact user-agent override.",
        ));
    }
    if matches!(request.wait, WaitCondition::Selector { .. }) {
        return Err(unsupported(
            "Lightpanda command-line rendering does not support selector waits.",
        ));
    }
    Ok(())
}

async fn run_fetch_command(
    executable: &std::path::Path,
    request: &RenderRequest,
    proxy: Option<&str>,
    deadline: tokio::time::Instant,
) -> UseResult<String> {
    let timeout_ms = request.timeout_ms.max(1).to_string();
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("fetch")
        .args(["--dump", "html"])
        .args(["--http_connect_timeout", &timeout_ms])
        .args(["--http_timeout", &timeout_ms])
        .args(["--log_level", "error"]);
    if let Some(proxy) = proxy {
        command.args(["--http_proxy", proxy]);
    }
    command
        .arg(request.url.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command.spawn().map_err(|error| {
        super::pool::browser_error(format!(
            "Failed to spawn Lightpanda ({}): {error}",
            executable.display()
        ))
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        super::pool::browser_error("Failed to capture Lightpanda stdout".to_string())
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        super::pool::browser_error("Failed to capture Lightpanda stderr".to_string())
    })?;
    let mut process = FetchProcess::new(
        child,
        tokio::spawn(read_all(stdout)),
        tokio::spawn(read_all(stderr)),
    );

    let status = match tokio::time::timeout_at(deadline, process.child_mut()?.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            process.terminate().await;
            return Err(super::pool::browser_error(format!(
                "Failed while waiting for Lightpanda: {error}"
            )));
        }
        Err(_) => {
            process.terminate().await;
            return Err(render_timeout(request));
        }
    };
    process.mark_reaped();

    let stdout = join_output(process.take_stdout()?, "stdout", deadline, request).await?;
    let stderr = join_output(process.take_stderr()?, "stderr", deadline, request).await?;
    decode_fetch_output(status, stdout, stderr, proxy)
}

struct FetchProcess {
    child: Option<tokio::process::Child>,
    stdout: Option<JoinHandle<std::io::Result<Vec<u8>>>>,
    stderr: Option<JoinHandle<std::io::Result<Vec<u8>>>>,
}

impl FetchProcess {
    fn new(
        child: tokio::process::Child,
        stdout: JoinHandle<std::io::Result<Vec<u8>>>,
        stderr: JoinHandle<std::io::Result<Vec<u8>>>,
    ) -> Self {
        Self {
            child: Some(child),
            stdout: Some(stdout),
            stderr: Some(stderr),
        }
    }

    fn child_mut(&mut self) -> UseResult<&mut tokio::process::Child> {
        self.child
            .as_mut()
            .ok_or_else(|| process_state_error("child process"))
    }

    fn mark_reaped(&mut self) {
        self.child.take();
    }

    fn take_stdout(&mut self) -> UseResult<JoinHandle<std::io::Result<Vec<u8>>>> {
        self.stdout
            .take()
            .ok_or_else(|| process_state_error("stdout reader"))
    }

    fn take_stderr(&mut self) -> UseResult<JoinHandle<std::io::Result<Vec<u8>>>> {
        self.stderr
            .take()
            .ok_or_else(|| process_state_error("stderr reader"))
    }

    async fn terminate(&mut self) {
        crate::cleanup::finish_child_cleanup(self.child.take()).await;
        self.abort_readers();
    }

    fn abort_readers(&mut self) {
        if let Some(task) = self.stdout.take() {
            task.abort();
        }
        if let Some(task) = self.stderr.take() {
            task.abort();
        }
    }
}

fn process_state_error(component: &str) -> UseError {
    super::pool::browser_error(format!(
        "Lightpanda process state lost its {component} before rendering completed"
    ))
}

impl Drop for FetchProcess {
    fn drop(&mut self) {
        self.abort_readers();
        let Some(child) = self.child.take() else {
            return;
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    let _ = crate::cleanup::kill_and_reap_child(Some(child)).await;
                });
            }
            Err(error) => warn!("Cannot schedule Lightpanda process cleanup: {error}"),
        }
    }
}

async fn read_all(mut pipe: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    pipe.read_to_end(&mut output).await?;
    Ok(output)
}

async fn join_output(
    mut task: JoinHandle<std::io::Result<Vec<u8>>>,
    stream: &str,
    deadline: tokio::time::Instant,
    request: &RenderRequest,
) -> UseResult<Vec<u8>> {
    let joined = match tokio::time::timeout_at(deadline, &mut task).await {
        Ok(joined) => joined,
        Err(_) => {
            task.abort();
            return Err(render_timeout(request));
        }
    };
    joined
        .map_err(|error| {
            super::pool::browser_error(format!("Lightpanda {stream} reader failed: {error}"))
        })?
        .map_err(|error| {
            super::pool::browser_error(format!("Failed to read Lightpanda {stream}: {error}"))
        })
}

fn decode_fetch_output(
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    proxy: Option<&str>,
) -> UseResult<String> {
    if !status.success() {
        let diagnostic = bounded_diagnostic(&stderr, proxy);
        let suffix = if diagnostic.is_empty() {
            String::new()
        } else {
            format!(": {diagnostic}")
        };
        return Err(super::pool::browser_error(format!(
            "Lightpanda fetch exited with {status}{suffix}"
        )));
    }

    let html = String::from_utf8(stdout).map_err(|error| {
        super::pool::browser_error(format!("Lightpanda returned non-UTF-8 HTML: {error}"))
    })?;
    if html.trim().is_empty() {
        return Err(super::pool::browser_error(
            "Lightpanda returned an empty HTML document".to_string(),
        ));
    }
    Ok(html)
}

fn bounded_diagnostic(stderr: &[u8], proxy: Option<&str>) -> String {
    let start = stderr.len().saturating_sub(STDERR_DIAGNOSTIC_LIMIT);
    let mut diagnostic = String::from_utf8_lossy(&stderr[start..]).trim().to_string();
    if let Some(proxy) = proxy {
        diagnostic = diagnostic.replace(proxy, "<redacted-proxy>");
    }
    diagnostic
}

async fn apply_post_fetch_wait(
    wait: &WaitCondition,
    deadline: tokio::time::Instant,
    request: &RenderRequest,
) -> UseResult<()> {
    let delay = match wait {
        WaitCondition::NetworkIdle { idle_ms } => Some(*idle_ms),
        WaitCondition::Delay { ms } => Some(*ms),
        WaitCondition::Load | WaitCondition::DomContentLoaded | WaitCondition::Selector { .. } => {
            None
        }
    };
    if let Some(delay) = delay {
        tokio::time::timeout_at(
            deadline,
            tokio::time::sleep(std::time::Duration::from_millis(delay)),
        )
        .await
        .map_err(|_| render_timeout(request))?;
    }
    Ok(())
}

fn render_timeout(request: &RenderRequest) -> UseError {
    UseError::new(
        "use.browser.timeout",
        format!(
            "Browser rendering exceeded {} ms.",
            request.timeout().as_millis()
        ),
    )
}

fn unsupported(message: impl Into<String>) -> UseError {
    UseError::new("use.browser.unsupported", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn diagnostics_are_bounded_and_redact_the_proxy() {
        let proxy = "http://user:secret@proxy.example:8080";
        let mut stderr = vec![b'x'; STDERR_DIAGNOSTIC_LIMIT + 200];
        stderr.extend_from_slice(proxy.as_bytes());

        let diagnostic = bounded_diagnostic(&stderr, Some(proxy));

        assert!(diagnostic.len() <= STDERR_DIAGNOSTIC_LIMIT + "<redacted-proxy>".len());
        assert!(!diagnostic.contains("secret"));
        assert!(diagnostic.contains("<redacted-proxy>"));
    }
}
