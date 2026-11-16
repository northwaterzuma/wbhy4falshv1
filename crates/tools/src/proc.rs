//! Structured argv builder for controlled yt-dlp and FFmpeg execution.
//! Never invokes a shell (`cmd.exe` / `/bin/sh`), never inherits untrusted PATH or user configs,
//! and enforces `--ignore-config` and locked binary paths.

use mdm_ipc::{ErrorCode, ErrorDetail};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlledCommandSpec {
    pub executable: String,
    pub args: Vec<String>,
    pub clean_env: Vec<(String, String)>,
}

/// Constructs a hardened `yt-dlp` argv invocation bound to a verified component set and local relay.
pub fn build_ytdlp_command(
    ytdlp_bin: &Path,
    ffmpeg_dir: &Path,
    deno_bin: &Path,
    local_relay_proxy_url: &str,
    format_expr: &str,
    output_template: &str,
    page_url: &str,
    fragment_concurrency: u8,
) -> Result<ControlledCommandSpec, ErrorDetail> {
    if page_url.starts_with('-')
        || !(page_url.starts_with("https://") || page_url.starts_with("http://"))
    {
        return Err(ErrorDetail {
            code: ErrorCode::InvalidRequest,
            message: "Invalid or unsafe media page URL".to_string(),
            retryable: false,
            retry_after_ms: None,
        });
    }
    let mut args = vec![
        "--ignore-config".to_string(),
        "--no-Playlist".to_ascii_lowercase(),
        "--newline".to_string(),
        "--no-exec".to_string(),
        "--no-simulate".to_string(),
        "--print".to_string(),
        "after_move:rustcatch-output:%(filepath)j".to_string(),
        "--progress".to_string(),
        "--progress-template".to_string(),
        "download:rustcatch-bytes:%(progress.downloaded_bytes)s".to_string(),
        "--keep-video".to_string(), // Preserve merge inputs until Rust verifies final output
        "--concurrent-fragments".to_string(),
        fragment_concurrency.clamp(1, 8).to_string(),
        "--ffmpeg-location".to_string(),
        ffmpeg_dir.to_string_lossy().to_string(),
    ];
    if !local_relay_proxy_url.trim().is_empty() {
        args.push("--proxy".to_string());
        args.push(local_relay_proxy_url.to_string());
    }
    args.extend([
        "--js-runtimes".to_string(),
        format!("deno:{}", deno_bin.to_string_lossy()),
        "-f".to_string(),
        format_expr.to_string(),
        "-o".to_string(),
        output_template.to_string(),
        "--".to_string(),
        page_url.to_string(),
    ]);

    let clean_env = vec![
        (
            "DENO_INSTALL_ROOT".to_string(),
            deno_bin
                .parent()
                .unwrap_or(Path::new("."))
                .to_string_lossy()
                .to_string(),
        ),
        ("NO_COLOR".to_string(), "1".to_string()),
    ];

    Ok(ControlledCommandSpec {
        executable: ytdlp_bin.to_string_lossy().to_string(),
        args,
        clean_env,
    })
}
