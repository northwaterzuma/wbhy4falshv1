//! Background media worker. Each execution owns a separate staging directory.

use crate::tool_process;
use mdm_ipc::{ErrorCode, ErrorDetail, MediaIntent, MediaKind, sanitize_file_name};
use mdm_tools::{
    ComponentLockManifest, build_ytdlp_command, classify_ytdlp_stderr, extract_weibo_visitor_target,
};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use uuid::Uuid;

const COMPONENT_LOCK_JSON: &str = include_str!("../../../COMPONENT_LOCK.json");

pub struct StartRequest {
    pub task_id: String,
    pub attempt_id: String,
    pub page_url: String,
    pub file_name_hint: String,
    pub output_dir: Option<String>,
    pub media_intent: Option<MediaIntent>,
}

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    Log {
        task_id: String,
        message: String,
    },
    Progress {
        task_id: String,
        attempt_id: String,
        bytes: u64,
    },
    Completed {
        task_id: String,
        attempt_id: String,
    },
    Failed {
        task_id: String,
        attempt_id: String,
        error: ErrorDetail,
    },
    Stopped {
        task_id: String,
        attempt_id: String,
    },
}

#[derive(Debug, Default, PartialEq, Eq)]
enum AttemptState {
    #[default]
    Running,
    StopRequested,
    Finished,
}

#[derive(Clone, Default)]
pub struct AttemptControl(Arc<Mutex<AttemptState>>);

impl AttemptControl {
    pub fn request_stop(&self) -> bool {
        let mut state = self.0.lock().unwrap();
        if *state == AttemptState::Finished {
            return false;
        }
        *state = AttemptState::StopRequested;
        true
    }

    fn stopped(&self) -> bool {
        *self.0.lock().unwrap() == AttemptState::StopRequested
    }

    fn commit(
        &self,
        publish: impl FnOnce() -> Result<PathBuf, ErrorDetail>,
    ) -> Result<PathBuf, ErrorDetail> {
        let mut state = self.0.lock().unwrap();
        if *state == AttemptState::StopRequested {
            return Err(error(ErrorCode::InvalidState, "任务已停止"));
        }
        let path = publish()?;
        *state = AttemptState::Finished;
        Ok(path)
    }

    fn finish(&self) -> bool {
        let mut state = self.0.lock().unwrap();
        let stopped = *state == AttemptState::StopRequested;
        *state = AttemptState::Finished;
        stopped
    }
}

pub struct WorkerHandle {
    start_tx: Option<Sender<(StartRequest, AttemptControl)>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl WorkerHandle {
    pub fn start(&self, request: StartRequest) -> Option<AttemptControl> {
        let control = AttemptControl::default();
        self.start_tx
            .as_ref()?
            .send((request, control.clone()))
            .ok()?;
        Some(control)
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        self.start_tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn spawn_worker() -> (WorkerHandle, Receiver<WorkerEvent>) {
    let (start_tx, start_rx) = mpsc::channel::<(StartRequest, AttemptControl)>();
    let (event_tx, event_rx) = mpsc::channel::<WorkerEvent>();
    let worker = thread::spawn(move || {
        while let Ok((request, control)) = start_rx.recv() {
            let result = handle_request(&request, &control, &event_tx);
            let task_id = request.task_id;
            let attempt_id = request.attempt_id;
            let event = if control.finish() {
                WorkerEvent::Stopped {
                    task_id,
                    attempt_id,
                }
            } else {
                match result {
                    Ok(path) => {
                        log(
                            &event_tx,
                            &task_id,
                            format!("成品已提交: {}", path.display()),
                        );
                        WorkerEvent::Completed {
                            task_id,
                            attempt_id,
                        }
                    }
                    Err(error) => WorkerEvent::Failed {
                        task_id,
                        attempt_id,
                        error,
                    },
                }
            };
            let _ = event_tx.send(event);
        }
    });
    (
        WorkerHandle {
            start_tx: Some(start_tx),
            worker: Some(worker),
        },
        event_rx,
    )
}

fn log(event_tx: &Sender<WorkerEvent>, task_id: &str, message: impl Into<String>) {
    let _ = event_tx.send(WorkerEvent::Log {
        task_id: task_id.to_string(),
        message: message.into(),
    });
}

fn error(code: ErrorCode, message: impl Into<String>) -> ErrorDetail {
    ErrorDetail {
        code,
        message: message.into(),
        retryable: false,
        retry_after_ms: None,
    }
}

fn load_manifest(root: &Path) -> Result<ComponentLockManifest, ErrorDetail> {
    let json = match fs::read_to_string(root.join("COMPONENT_LOCK.json")) {
        Ok(json) => json,
        Err(err) if err.kind() == io::ErrorKind::NotFound => COMPONENT_LOCK_JSON.to_string(),
        Err(err) => {
            return Err(error(
                ErrorCode::ToolUnavailable,
                format!("无法读取组件清单: {err}"),
            ));
        }
    };
    serde_json::from_str(&json).map_err(|_| error(ErrorCode::ToolUnavailable, "组件清单格式无效"))
}

fn install_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn component_path(
    manifest: &ComponentLockManifest,
    root: &Path,
    id: &str,
) -> Result<PathBuf, ErrorDetail> {
    manifest
        .components
        .iter()
        .find(|component| component.id == id)
        .and_then(|component| component.relative_executable.as_ref())
        .map(|relative| root.join(relative))
        .ok_or_else(|| error(ErrorCode::ToolUnavailable, format!("组件清单缺少 {id}")))
}

fn output_stem(file_name_hint: &str) -> &str {
    Path::new(file_name_hint)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(file_name_hint)
}

fn resolve_output_dir(requested: Option<&str>, root: &Path) -> PathBuf {
    match requested.map(str::trim) {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => root.join("Downloads"),
    }
}

/// The staging directory was created exclusively for this execution.
struct StagingDirectory(PathBuf);

impl StagingDirectory {
    fn new(output_dir: &Path) -> io::Result<Self> {
        let path = output_dir.join(format!(".rustcatch-{}", Uuid::new_v4()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct DownloadReport {
    artifact: Option<PathBuf>,
    ambiguous: bool,
    redirect: Option<String>,
}

impl DownloadReport {
    fn record(&mut self, line: &str) -> Option<u64> {
        if let Some(payload) = line.strip_prefix("rustcatch-output:") {
            match serde_json::from_str::<PathBuf>(payload) {
                Ok(path) if self.artifact.is_none() => self.artifact = Some(path),
                _ => self.ambiguous = true,
            }
        } else if self.redirect.is_none() {
            self.redirect = extract_weibo_visitor_target(line);
        }
        line.strip_prefix("rustcatch-bytes:")?.parse().ok()
    }

    fn staged_file(self, staging: &Path) -> Result<PathBuf, ErrorDetail> {
        let invalid = || error(ErrorCode::MergeFailed, "工具未提供唯一、有效的暂存成品");
        let path = self
            .artifact
            .filter(|_| !self.ambiguous)
            .ok_or_else(invalid)?;
        let metadata = fs::symlink_metadata(&path).map_err(|_| invalid())?;
        let canonical = fs::canonicalize(path).map_err(|_| invalid())?;
        let staging = fs::canonicalize(staging).map_err(|_| invalid())?;
        if !metadata.is_file() || metadata.len() == 0 || canonical.parent() != Some(&staging) {
            return Err(invalid());
        }
        Ok(canonical)
    }
}

fn configure_tool(command: &mut Command) {
    command.env_clear();
    for key in ["SystemRoot", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

fn verify_tracks(
    ffprobe: &Path,
    file: &Path,
    kind: Option<MediaKind>,
    control: &AttemptControl,
) -> Result<(), ErrorDetail> {
    let mut cmd = Command::new(ffprobe);
    configure_tool(&mut cmd);
    cmd.args([
        "-v",
        "error",
        "-protocol_whitelist",
        "file",
        "-show_entries",
        "stream=codec_type",
        "-of",
        "csv=p=0",
    ]);
    cmd.arg(file);
    let (mut video, mut audio) = (false, false);
    let (status, _) = tool_process::run(
        &mut cmd,
        || control.stopped(),
        |line| {
            video |= line.trim() == "video";
            audio |= line.trim() == "audio";
        },
    )
    .map_err(|_| error(ErrorCode::MergeFailed, "无法完成成品媒体校验"))?;
    let valid = match kind {
        Some(MediaKind::VideoAudio) => video && audio,
        Some(MediaKind::AudioOnly) => audio,
        Some(MediaKind::VideoOnly) => video,
        _ => video || audio,
    };
    if !status.success() || !valid {
        return Err(error(
            ErrorCode::MergeFailed,
            "成品缺少所请求的音视频轨，或媒体校验失败",
        ));
    }
    Ok(())
}

fn publish_file(
    staging_file: &Path,
    output_dir: &Path,
    file_name_hint: &str,
) -> Result<PathBuf, ErrorDetail> {
    let ext = staging_file
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty() && ext.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .ok_or_else(|| error(ErrorCode::MergeFailed, "媒体成品扩展名无效"))?;
    let stem = output_stem(file_name_hint);
    File::options()
        .write(true)
        .open(staging_file)
        .and_then(|file| file.sync_all())
        .map_err(|err| error(ErrorCode::InternalError, format!("无法同步成品: {err}")))?;
    for suffix in 0..1000 {
        let name = if suffix == 0 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem} ({suffix}).{ext}")
        };
        let target = output_dir.join(sanitize_file_name(&name)?);
        match publish_without_replacing(staging_file, &target) {
            Ok(()) => return Ok(target),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => {
                return Err(error(
                    ErrorCode::InternalError,
                    format!("无法安全提交成品: {err}"),
                ));
            }
        }
    }
    Err(error(
        ErrorCode::InvalidState,
        "有过多同名文件，请指定其他文件名",
    ))
}

#[cfg(windows)]
fn publish_without_replacing(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    let wide = |path: &Path| -> Vec<u16> { path.as_os_str().encode_wide().chain([0]).collect() };
    let source = wide(source);
    let target = wide(target);
    // The staging directory and destination share a volume. Omit REPLACE_EXISTING.
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn publish_without_replacing(source: &Path, target: &Path) -> io::Result<()> {
    // Linking into the destination directory is atomic and refuses name collisions.
    fs::hard_link(source, target)
}

fn handle_request(
    request: &StartRequest,
    control: &AttemptControl,
    event_tx: &Sender<WorkerEvent>,
) -> Result<PathBuf, ErrorDetail> {
    if control.stopped() {
        return Err(error(ErrorCode::InvalidState, "任务已停止"));
    }
    sanitize_file_name(&request.file_name_hint)?;
    let kind = request.media_intent.as_ref().map(|intent| intent.kind);
    let format = match kind {
        Some(MediaKind::AudioOnly) => "ba",
        Some(MediaKind::VideoOnly) => "bv",
        Some(MediaKind::SubtitlesOnly) => {
            return Err(error(
                ErrorCode::FormatUnavailable,
                "当前桌面执行器尚未接入单独下载字幕",
            ));
        }
        _ => "bv*+ba/b",
    };
    let root = install_root();
    let manifest = load_manifest(&root)?;
    let component_set_id = manifest.verify_ready_for_execution(&root)?;
    let ytdlp = component_path(&manifest, &root, "yt-dlp")?;
    let deno = component_path(&manifest, &root, "deno")?;
    let ffmpeg = component_path(&manifest, &root, "ffmpeg")?;
    let ffprobe = component_path(&manifest, &root, "ffprobe")?;
    let out_dir = std::path::absolute(resolve_output_dir(request.output_dir.as_deref(), &root))
        .map_err(|err| error(ErrorCode::InvalidRequest, format!("输出目录无效: {err}")))?;
    fs::create_dir_all(&out_dir)
        .map_err(|err| error(ErrorCode::InternalError, format!("无法创建输出目录: {err}")))?;
    log(
        event_tx,
        &request.task_id,
        format!("组件集 {component_set_id} 校验通过，开始下载"),
    );

    let mut url = request.page_url.clone();
    for attempt in 0..2 {
        let staging = StagingDirectory::new(&out_dir)
            .map_err(|err| error(ErrorCode::InternalError, format!("无法创建暂存目录: {err}")))?;
        let template = format!(
            "{}.%(ext)s",
            staging.0.join("media").to_string_lossy().replace('%', "%%")
        );
        let spec = build_ytdlp_command(
            &ytdlp,
            ffmpeg.parent().unwrap_or(&root),
            &deno,
            "",
            format,
            &template,
            &url,
            4,
        )?;
        let mut command = Command::new(&spec.executable);
        configure_tool(&mut command);
        command.args(&spec.args).envs(spec.clean_env);
        let mut report = DownloadReport::default();
        let (status, stderr) = tool_process::run(
            &mut command,
            || control.stopped(),
            |line| {
                if let Some(bytes) = report.record(line) {
                    let _ = event_tx.send(WorkerEvent::Progress {
                        task_id: request.task_id.clone(),
                        attempt_id: request.attempt_id.clone(),
                        bytes,
                    });
                }
            },
        )
        .map_err(|err| {
            error(
                ErrorCode::ToolUnavailable,
                format!("无法运行媒体组件: {err}"),
            )
        })?;

        if status.success() {
            let file = report.staged_file(&staging.0)?;
            verify_tracks(&ffprobe, &file, kind, control)?;
            return control.commit(|| publish_file(&file, &out_dir, &request.file_name_hint));
        }
        if let Some(redirect) = report
            .redirect
            .filter(|target| attempt == 0 && *target != url)
        {
            url = redirect;
            log(
                event_tx,
                &request.task_id,
                "检测到微博访客校验跳转，已还原媒体地址并重试一次",
            );
            continue;
        }
        return Err(classify_ytdlp_stderr(&stderr).1);
    }
    unreachable!("the second execution must return")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_component_lock_reports_tool_unavailable() {
        let (handle, events) = spawn_worker();
        handle
            .start(StartRequest {
                task_id: "test-task".into(),
                attempt_id: "test-attempt".into(),
                page_url: "https://example.invalid/video".into(),
                file_name_hint: "test.mp4".into(),
                output_dir: None,
                media_intent: None,
            })
            .unwrap();
        loop {
            match events
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
            {
                WorkerEvent::Failed { error, .. } => {
                    assert_eq!(error.code, ErrorCode::ToolUnavailable);
                    break;
                }
                WorkerEvent::Log { .. } => {}
                other => panic!("unexpected event: {other:?}"),
            }
        }
    }

    #[test]
    fn empty_output_dir_falls_back_to_downloads_subfolder() {
        let root = Path::new("install");
        assert_eq!(resolve_output_dir(Some("  "), root), root.join("Downloads"));
        assert_eq!(resolve_output_dir(Some("other"), root), Path::new("other"));
    }

    #[test]
    fn missing_and_foreign_files_do_not_pass_staging_check() {
        let parent = StagingDirectory::new(&std::env::temp_dir()).unwrap();
        let dir = StagingDirectory::new(&parent.0).unwrap();
        let foreign = parent.0.join("media.mp4");
        fs::write(&foreign, b"unrelated").unwrap();
        assert!(DownloadReport::default().staged_file(&dir.0).is_err());
        let report = DownloadReport {
            artifact: Some(foreign.clone()),
            ..Default::default()
        };
        assert!(report.staged_file(&dir.0).is_err());
        assert_eq!(fs::read(foreign).unwrap(), b"unrelated");
    }

    #[test]
    fn publishing_preserves_existing_names_and_other_task_fragments() {
        let dir = StagingDirectory::new(&std::env::temp_dir()).unwrap();
        let target = dir.0.join("example.mp4");
        let fragment = dir.0.join("example.f399.mp4");
        fs::write(&target, b"previous").unwrap();
        fs::write(&fragment, b"other task").unwrap();
        {
            let stage = StagingDirectory::new(&dir.0).unwrap();
            let media = stage.0.join("media.mp4");
            fs::write(&media, b"verified").unwrap();
            let path = publish_file(&media, &dir.0, "example.mp4").unwrap();
            assert_eq!(path, dir.0.join("example (1).mp4"));
        }
        assert_eq!(
            fs::read(dir.0.join("example (1).mp4")).unwrap(),
            b"verified"
        );
        assert_eq!(fs::read(target).unwrap(), b"previous");
        assert_eq!(fs::read(fragment).unwrap(), b"other task");
    }

    #[test]
    fn stop_and_commit_cannot_both_win() {
        let control = AttemptControl::default();
        assert!(control.request_stop());
        assert!(
            control
                .commit(|| panic!("stopped task cannot publish"))
                .is_err()
        );
        assert!(control.finish());
        let control = AttemptControl::default();
        assert!(
            control
                .commit(|| Ok(PathBuf::from("committed.mp4")))
                .is_ok()
        );
        assert!(!control.request_stop());
    }
}
