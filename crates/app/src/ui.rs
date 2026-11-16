//! IDM Classic Light UI View-Model and egui/eframe Window Bindings (UI-01 .. UI-12).

use crate::runner::{AttemptControl, StartRequest, WorkerEvent, WorkerHandle, spawn_worker};
use crate::task_coordinator::TaskCoordinator;
use mdm_ipc::{
    CallerOriginKind, CallerScope, CapabilityScope, CollisionPolicy, ControlOp, DirectoryPolicy,
    MediaIntent, MediaKind, MediaPreset, OutputPolicy, ResponseStatus, SourceSpec, SubtitlePolicy,
    TaskSnapshot, TaskState, sanitize_file_name,
};
use mdm_tools::video::extract_youtube_identity;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const DEFAULT_NETWORK_PROFILE_ID: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdmWindowId {
    Ui01Main,
    Ui02AddUrl,
    Ui03DownloadInfo,
    Ui04Progress,
    Ui05VideoSelector,
    Ui06CompletionProperties,
    Ui07Options,
    Ui08QueueScheduler,
    Ui09BrowserFloatingBar,
    Ui10BatchGrabberWizard,
    Ui11RecoveryDiagnostics,
    Ui12DesktopTrayBasket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiDialog {
    AddUrl,
    DownloadInfo,
    Options,
    QueueScheduler,
    SiteGrabber,
    TaskLog,
    About,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    OpenAddUrl,
    PrepareAddUrl(String),
    OpenDownloadInfo,
    StartQueue,
    StopQueue,
    ResumeSelected,
    StopSelected,
    CancelSelected,
    RemoveSelected,
    DeleteSelectedFiles,
    OpenOptions,
    OpenScheduler,
    OpenGrabber,
    OpenTaskLog,
    ClearLog,
    OpenAbout,
    CloseDialog,
    SelectCategory(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Progress,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub time: String,
    pub level: LogLevel,
    pub task_id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarButtonStates {
    pub can_add: bool,
    pub can_resume: bool,
    pub can_stop: bool,
    pub can_stop_all: bool,
    pub can_delete_record: bool,
    pub can_open_options: bool,
    pub can_open_scheduler: bool,
    pub can_start_queue: bool,
    pub can_stop_queue: bool,
    pub can_open_grabber: bool,
}

/// Computes IDM classic toolbar button enable/disable states based on current selection.
pub fn compute_toolbar_states(selected: &[TaskSnapshot], any_running: bool) -> ToolbarButtonStates {
    let can_resume = selected.iter().any(|t| {
        matches!(
            t.state,
            TaskState::Paused | TaskState::Failed | TaskState::Queued
        )
    });
    let can_stop = selected.iter().any(|t| {
        matches!(
            t.state,
            TaskState::Running | TaskState::Resolving | TaskState::Finishing
        )
    });
    let can_delete_record = !selected.is_empty();

    ToolbarButtonStates {
        can_add: true,
        can_resume,
        can_stop,
        can_stop_all: any_running,
        can_delete_record,
        can_open_options: true,
        can_open_scheduler: true,
        can_start_queue: true,
        can_stop_queue: true,
        can_open_grabber: true,
    }
}

pub struct RustCatchDesktopApp {
    pub coordinator: TaskCoordinator,
    pub selected_category: String,
    pub selected_task_ids: Vec<String>,
    pub add_url_input: String,
    pub close_to_tray: bool,
    pub status_message: String,
    pub open_dialog: Option<UiDialog>,
    pub pending_task_id: Option<String>,
    pub pending_url: String,
    pub pending_file_name: String,
    pub pending_directory: String,
    pub pending_category: String,
    pub pending_queue_id: String,
    pub pending_media_id: Option<String>,
    pub pending_start_now: bool,
    pub dialog_error: Option<String>,
    pub queue_running: bool,
    pub add_url_history: Vec<String>,
    pub last_task_id: Option<String>,
    pub worker: WorkerHandle,
    pub worker_events: Receiver<WorkerEvent>,
    pub log_lines: Vec<LogLine>,
    active_attempts: HashMap<String, (String, AttemptControl)>,
}

impl Default for RustCatchDesktopApp {
    fn default() -> Self {
        let (worker, worker_events) = spawn_worker();
        Self {
            coordinator: TaskCoordinator::new(),
            selected_category: "所有下载 (All Downloads)".to_string(),
            selected_task_ids: Vec::new(),
            add_url_input: String::new(),
            close_to_tray: true,
            status_message: "就绪 (Ready) | 全局速度: 0 B/s".to_string(),
            open_dialog: None,
            pending_task_id: None,
            pending_url: String::new(),
            pending_file_name: String::new(),
            pending_directory: String::new(),
            pending_category: "视频 (Video)".to_string(),
            pending_queue_id: "main".to_string(),
            pending_media_id: None,
            pending_start_now: true,
            dialog_error: None,
            queue_running: false,
            add_url_history: Vec::new(),
            last_task_id: None,
            worker,
            worker_events,
            log_lines: Vec::new(),
            active_attempts: HashMap::new(),
        }
    }
}

impl Drop for RustCatchDesktopApp {
    fn drop(&mut self) {
        for (_, control) in self.active_attempts.values() {
            control.request_stop();
        }
    }
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn safe_file_stem(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches(|c| c == '.' || c == '_');
    if cleaned.is_empty() {
        "download".to_string()
    } else {
        cleaned.to_string()
    }
}

/// Default save directory shown in UI-03: `Downloads` next to the running executable.
pub fn default_download_directory() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("Downloads")))
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "Downloads".to_string())
}

/// Maps a category-tree label to a predicate over task records.
/// Status categories use `state`; type categories use the inferred file category;
/// queue categories use `queue_id`.
pub fn task_matches_category(
    task: &crate::model::TaskRecord,
    category: &str,
) -> bool {
    if category.starts_with("所有下载") {
        return true;
    }
    if category.starts_with("未完成") {
        return !matches!(task.state, TaskState::Completed | TaskState::Cancelled);
    }
    if category.starts_with("已完成") {
        return task.state == TaskState::Completed;
    }
    if category.starts_with("主队列") {
        return task.queue_id == "main";
    }
    if category.starts_with("同步队列") {
        return task.queue_id == "sync";
    }
    task.category == category
}

fn default_media_intent() -> MediaIntent {
    MediaIntent {
        version: 1,
        kind: MediaKind::VideoAudio,
        preset: MediaPreset::HighestQuality,
        max_height: None,
        codec_preference: vec!["av1".into(), "vp9".into(), "h264".into()],
        container: "auto".into(),
        hdr: "preserve".into(),
        audio_language: "original".into(),
        subtitles: SubtitlePolicy {
            languages: vec![],
            source: "manual_preferred".into(),
            mode: "sidecar".into(),
            required: false,
        },
        allow_quality_downgrade: false,
        allow_transcode: false,
        require_exact_format: false,
        selected_format_ids: vec![],
    }
}

impl RustCatchDesktopApp {
    pub fn push_log(&mut self, level: LogLevel, task_id: &str, message: impl Into<String>) {
        let message = message.into();
        self.log_lines.push(LogLine {
            time: chrono::Local::now().format("%H:%M:%S").to_string(),
            level,
            task_id: task_id.to_string(),
            message: message.clone(),
        });
        if self.log_lines.len() > 500 {
            self.log_lines.remove(0);
        }
        if level != LogLevel::Progress {
            self.status_message = message;
        }
    }

    pub fn start_task_worker(&mut self, task_id: &str) -> bool {
        let Some(task) = self.coordinator.tasks.get(task_id) else {
            self.push_log(LogLevel::Error, task_id, "任务不存在，无法启动 worker");
            return false;
        };
        let Some(attempt_id) = task.attempt_id.clone() else {
            self.push_log(
                LogLevel::Error,
                task_id,
                "任务没有 attempt_id，无法启动 worker",
            );
            return false;
        };
        if task.worker_source_url.trim().is_empty() {
            self.push_log(
                LogLevel::Error,
                task_id,
                "任务缺少可执行的源地址，无法启动 worker",
            );
            return false;
        }
        if task.network_profile_id != DEFAULT_NETWORK_PROFILE_ID || task.context_ref.is_some() {
            let error = mdm_ipc::ErrorDetail {
                code: mdm_ipc::ErrorCode::ToolUnavailable,
                message: "当前下载执行器无法应用此任务的网络配置或授权上下文".into(),
                retryable: false,
                retry_after_ms: None,
            };
            self.record_worker_event(WorkerEvent::Failed { task_id: task_id.into(), attempt_id, error });
            return false;
        }
        let request = StartRequest {
            task_id: task.task_id.clone(),
            attempt_id: attempt_id.clone(),
            page_url: task.worker_source_url.clone(),
            file_name_hint: task.output.file_name_hint.clone(),
            output_dir: task.output.directory_ref.clone(),
            media_intent: task.media_intent.clone(),
        };
        let Some(control) = self.worker.start(request) else {
            let error = mdm_ipc::ErrorDetail {
                code: mdm_ipc::ErrorCode::InternalError,
                message: "后台下载服务不可用".into(),
                retryable: false,
                retry_after_ms: None,
            };
            self.record_worker_event(WorkerEvent::Failed { task_id: task_id.into(), attempt_id, error });
            return false;
        };
        self.active_attempts.insert(task_id.into(), (attempt_id, control));
        self.push_log(LogLevel::Info, task_id, "已提交下载任务到后台 worker");
        true
    }

    pub fn drain_worker_events(&mut self) {
        while let Ok(event) = self.worker_events.try_recv() {
            self.record_worker_event(event);
        }
    }

    pub fn record_worker_event(&mut self, event: WorkerEvent) {
        match event {
            WorkerEvent::Log { task_id, message } => {
                let level = if message.contains("stderr")
                    || message.contains("失败")
                    || message.contains("拒绝执行")
                {
                    LogLevel::Error
                } else {
                    LogLevel::Info
                };
                self.push_log(level, &task_id, message);
            }
            WorkerEvent::Progress {
                task_id,
                attempt_id,
                bytes,
            } => {
                if self
                    .coordinator
                    .apply_worker_event(&task_id, &attempt_id, bytes, false)
                    .is_ok()
                {
                    self.push_log(LogLevel::Progress, &task_id, format!("已接收 {} B", bytes));
                }
            }
            WorkerEvent::Completed {
                task_id,
                attempt_id,
            } => {
                self.clear_attempt(&task_id, &attempt_id);
                let bytes = self
                    .coordinator
                    .tasks
                    .get(&task_id)
                    .map(|task| task.bytes_received)
                    .unwrap_or(0);
                match self
                    .coordinator
                    .apply_worker_event(&task_id, &attempt_id, bytes, true)
                {
                    Ok(_) => self.push_log(LogLevel::Info, &task_id, "下载完成"),
                    Err(error) => self.push_log(
                        LogLevel::Error,
                        &task_id,
                        format!("完成事件被拒绝: {}", error.message),
                    ),
                }
            }
            WorkerEvent::Failed {
                task_id,
                attempt_id,
                error,
            } => {
                self.clear_attempt(&task_id, &attempt_id);
                if self.coordinator
                    .mark_task_failed(&task_id, &attempt_id, &error, now_epoch_ms())
                    .is_ok()
                {
                    self.push_log(LogLevel::Error, &task_id, format!("{:?}: {}", error.code, error.message));
                }
            }
            WorkerEvent::Stopped { task_id, attempt_id } => {
                self.clear_attempt(&task_id, &attempt_id);
                if self.coordinator.finish_stopping(&task_id, &attempt_id) {
                    self.push_log(LogLevel::Info, &task_id, "后台下载已停止");
                }
            }
        }
    }

    fn clear_attempt(&mut self, task_id: &str, attempt_id: &str) {
        if self.active_attempts.get(task_id).is_some_and(|(id, _)| id == attempt_id) {
            self.active_attempts.remove(task_id);
        }
    }

    pub fn ui_caller() -> CallerScope {
        CallerScope {
            os_user: "local-desktop-user".to_string(),
            origin_kind: CallerOriginKind::DesktopUi,
            instance_id: "desktop-ui".to_string(),
            profile_id: "default".to_string(),
            capabilities: vec![
                CapabilityScope::Read,
                CapabilityScope::Add,
                CapabilityScope::Control,
                CapabilityScope::DeleteFiles,
                CapabilityScope::InspectNetwork,
                CapabilityScope::ManageQueue,
                CapabilityScope::ManageLimits,
            ],
        }
    }

    /// Validates the address bar and opens the UI-03 download-info dialog.
    /// No task is created before the user confirms the download info.
    pub fn prepare_add_url(&mut self, raw_url: &str) -> bool {
        let trimmed = raw_url.trim();
        self.dialog_error = None;
        if trimmed.is_empty() {
            self.dialog_error = Some("请输入 HTTP/HTTPS 直链或视频页面地址".to_string());
            return false;
        }
        let parsed = match url::Url::parse(trimmed) {
            Ok(parsed) => parsed,
            Err(_) => {
                self.dialog_error = Some("地址格式无效".to_string());
                return false;
            }
        };
        if !matches!(parsed.scheme(), "http" | "https") {
            self.dialog_error = Some(format!(
                "不支持的协议: {}（仅支持 http/https）",
                parsed.scheme()
            ));
            return false;
        }

        let identity = extract_youtube_identity(trimmed);
        let file_name = if let Some((_, media_id)) = &identity {
            format!("youtube_{}.mp4", safe_file_stem(media_id))
        } else {
            let last_segment = parsed
                .path_segments()
                .and_then(|segments| segments.last().map(str::to_string))
                .filter(|segment| !segment.is_empty())
                .unwrap_or_else(|| "download.bin".to_string());
            let stem = safe_file_stem(&last_segment);
            if stem.contains('.') {
                stem
            } else {
                format!("{}.bin", stem)
            }
        };
        if let Err(err) = sanitize_file_name(&file_name) {
            self.dialog_error = Some(format!("{:?}: {}", err.code, err.message));
            return false;
        }

        self.add_url_input = trimmed.to_string();
        self.pending_task_id = None;
        self.pending_url = trimmed.to_string();
        self.pending_file_name = file_name;
        self.pending_directory = default_download_directory();
        self.pending_category = if identity.is_some() {
            "视频 (Video)".to_string()
        } else {
            "所有下载 (All Downloads)".to_string()
        };
        self.pending_queue_id = "main".to_string();
        self.pending_media_id = identity.map(|(_, media_id)| media_id);
        self.pending_start_now = true;
        self.open_dialog = Some(UiDialog::DownloadInfo);
        self.status_message = "地址已解析，等待确认".to_string();
        true
    }

    /// Confirms the download-info dialog. Creates a new task, or saves edits to an existing task.
    pub fn confirm_pending_download(&mut self) -> bool {
        self.dialog_error = None;
        if self.pending_url.trim().is_empty() && self.pending_task_id.is_none() {
            self.dialog_error = Some("下载地址为空，无法创建任务".to_string());
            return false;
        }
        if let Err(err) = sanitize_file_name(&self.pending_file_name) {
            self.dialog_error = Some(format!("{:?}: {}", err.code, err.message));
            return false;
        }
        let directory_ref = if self.pending_directory.trim().is_empty() {
            None
        } else {
            Some(self.pending_directory.trim().to_string())
        };

        if let Some(task_id) = self.pending_task_id.clone() {
            let Some(task) = self.coordinator.tasks.get_mut(&task_id) else {
                self.dialog_error = Some(format!("任务不存在: {}", task_id));
                return false;
            };
            task.output.file_name_hint = self.pending_file_name.clone();
            task.output.directory_ref = directory_ref;
            task.output.queue_id = Some(self.pending_queue_id.clone());
            task.revision += 1;
            self.status_message = format!("下载信息已保存: {}", task_id);
            self.open_dialog = None;
            return true;
        }

        let source = SourceSpec::PageUrl {
            url: self.pending_url.clone(),
            page_url: Some(self.pending_url.clone()),
            platform_media_id: self.pending_media_id.clone(),
        };
        let output = OutputPolicy {
            directory_policy: if directory_ref.is_some() {
                DirectoryPolicy::UserSelected
            } else {
                DirectoryPolicy::CategoryDefault
            },
            file_name_hint: self.pending_file_name.clone(),
            directory_ref,
            queue_id: Some(self.pending_queue_id.clone()),
            collision: CollisionPolicy::Rename,
        };
        let media_intent = self
            .pending_media_id
            .as_ref()
            .map(|_| default_media_intent());
        let caller = Self::ui_caller();
        let request_id = format!("ui-add-{}", Uuid::new_v4());
        let idempotency_key = format!("ui-add-url:{}", request_id);
        let response = self.coordinator.add_task(
            &caller,
            &request_id,
            &idempotency_key,
            &source,
            output,
            None,
            DEFAULT_NETWORK_PROFILE_ID.to_string(),
            media_intent,
        );
        if let Some(err) = response.error {
            self.dialog_error = Some(format!("{:?}: {}", err.code, err.message));
            return false;
        }
        let task_id = response.task_id.unwrap_or_default();
        self.selected_task_ids = vec![task_id.clone()];
        self.last_task_id = Some(task_id.clone());
        self.push_log(
            LogLevel::Info,
            &task_id,
            format!("任务已创建: {}，来源=page_url", self.pending_file_name),
        );
        if !self.add_url_history.contains(&self.pending_url) {
            self.add_url_history.push(self.pending_url.clone());
            if self.add_url_history.len() > 20 {
                self.add_url_history.remove(0);
            }
        }

        if self.pending_start_now {
            let rev = self
                .coordinator
                .tasks
                .get(&task_id)
                .map(|task| task.revision)
                .unwrap_or(1);
            let request_id = format!("ui-start-{}", Uuid::new_v4());
            let start_response = self.coordinator.control_task(
                &caller,
                &request_id,
                &task_id,
                rev,
                ControlOp::Resume,
                now_epoch_ms(),
            );
            if let Some(error) = start_response.error {
                self.push_log(
                    LogLevel::Error,
                    &task_id,
                    format!("启动失败: {:?}: {}", error.code, error.message),
                );
            } else {
                self.queue_running = true;
                self.start_task_worker(&task_id);
                self.status_message = format!(
                    "任务已提交后台 worker: {} | {}",
                    task_id, self.pending_file_name
                );
            }
        } else {
            self.status_message =
                format!("任务已加入队列: {} | {}", task_id, self.pending_file_name);
        }
        self.open_dialog = None;
        true
    }

    pub fn open_download_info_for_selected(&mut self) {
        let task_id = self
            .selected_task_ids
            .first()
            .cloned()
            .or_else(|| self.last_task_id.clone());
        let Some(task_id) = task_id else {
            self.status_message = "请先选择一个任务，再打开下载信息".to_string();
            return;
        };
        let Some(task) = self.coordinator.tasks.get(&task_id) else {
            self.status_message = format!("任务不存在: {}", task_id);
            return;
        };
        self.pending_task_id = Some(task_id.clone());
        self.pending_url = task.source_display.clone();
        self.pending_file_name = task.output.file_name_hint.clone();
        self.pending_directory = task.output.directory_ref.clone().unwrap_or_default();
        self.pending_queue_id = task
            .output
            .queue_id
            .clone()
            .unwrap_or_else(|| "main".to_string());
        self.pending_media_id = task.media_intent.as_ref().map(|_| task.task_id.clone());
        self.pending_start_now = false;
        self.dialog_error = None;
        self.open_dialog = Some(UiDialog::DownloadInfo);
        self.status_message = format!("已打开下载信息: {}", task_id);
    }

    pub fn control_selected(&mut self, op: ControlOp, label: &str) {
        if self.selected_task_ids.is_empty() {
            self.status_message = format!("请先选择任务，再执行“{}”", label);
            return;
        }
        if op == ControlOp::RemoveRecord {
            self.handle_delete_key(false, now_epoch_ms());
            return;
        }
        let ids = self.selected_task_ids.clone();
        let mut changed = 0usize;
        for id in ids {
            if self.control_download(&id, op) {
                changed += 1;
            }
        }
        self.status_message = format!("{}: {} 个任务状态已更新", label, changed);
    }

    pub fn set_queue_running(&mut self, running: bool) {
        let ids: Vec<String> = self
            .coordinator
            .tasks
            .values()
            .filter(|task| {
                if running {
                    task.state == TaskState::Queued
                } else {
                    matches!(
                        task.state,
                        TaskState::Running | TaskState::Resolving | TaskState::Finishing
                    )
                }
            })
            .map(|task| task.task_id.clone())
            .collect();
        let op = if running {
            ControlOp::Resume
        } else {
            ControlOp::Pause
        };
        let mut changed = 0usize;
        for id in ids {
            if self.control_download(&id, op) {
                changed += 1;
            }
        }
        self.queue_running = running;
        self.status_message = format!(
            "{}: {} 个任务状态已更新",
            if running {
                "开始队列"
            } else {
                "停止队列"
            },
            changed
        );
    }

    fn control_download(&mut self, task_id: &str, op: ControlOp) -> bool {
        let Some(task) = self.coordinator.tasks.get(task_id) else {
            return false;
        };
        if matches!(op, ControlOp::Pause | ControlOp::Cancel)
            && let Some((attempt_id, control)) = self.active_attempts.get(task_id)
            && task.attempt_id.as_ref() == Some(attempt_id)
            && !control.request_stop()
        {
            return false;
        }
        let response = self.coordinator.control_task(
            &Self::ui_caller(), &format!("ui-{}", Uuid::new_v4()),
            task_id, task.revision, op, now_epoch_ms(),
        );
        if response.status == ResponseStatus::Error {
            return false;
        }
        if op == ControlOp::Resume {
            self.start_task_worker(task_id);
        }
        true
    }

    /// Keyboard Delete triggers `RemoveRecord` only (never `DeleteFiles` unless explicitly confirmed with Shift+Delete).
    pub fn handle_delete_key(&mut self, shift_delete_files: bool, now_epoch_ms: u64) {
        let caller = Self::ui_caller();
        let ids = self.selected_task_ids.clone();
        let mut removed = 0usize;
        for id in ids {
            if let Some(rec) = self.coordinator.tasks.get(&id) {
                let rev = rec.revision;
                if shift_delete_files {
                    let _ = self.coordinator.control_task(
                        &caller,
                        "ui-del-files",
                        &id,
                        rev,
                        ControlOp::DeleteFiles,
                        now_epoch_ms,
                    );
                }
                let next_rev = self
                    .coordinator
                    .tasks
                    .get(&id)
                    .map(|r| r.revision)
                    .unwrap_or(rev);
                let response = self.coordinator.control_task(
                    &caller,
                    "ui-rm-rec",
                    &id,
                    next_rev,
                    ControlOp::RemoveRecord,
                    now_epoch_ms,
                );
                if response.status != ResponseStatus::Error {
                    removed += 1;
                }
            }
        }
        self.selected_task_ids.clear();
        if self
            .last_task_id
            .as_deref()
            .map(|id| !self.coordinator.tasks.contains_key(id))
            .unwrap_or(false)
        {
            self.last_task_id = None;
        }
        self.status_message = format!("已移除 {} 条任务记录（未删除下载文件）", removed);
    }

    pub fn apply_action(&mut self, action: AppAction) {
        match action {
            AppAction::OpenAddUrl => {
                self.dialog_error = None;
                self.open_dialog = Some(UiDialog::AddUrl);
                self.status_message = "已打开“添加地址”窗口".to_string();
            }
            AppAction::PrepareAddUrl(url) => {
                if !self.prepare_add_url(&url) {
                    self.open_dialog = Some(UiDialog::AddUrl);
                }
            }
            AppAction::OpenDownloadInfo => self.open_download_info_for_selected(),
            AppAction::StartQueue => self.set_queue_running(true),
            AppAction::StopQueue => self.set_queue_running(false),
            AppAction::ResumeSelected => self.control_selected(ControlOp::Resume, "恢复"),
            AppAction::StopSelected => self.control_selected(ControlOp::Pause, "停止"),
            AppAction::CancelSelected => self.control_selected(ControlOp::Cancel, "取消下载"),
            AppAction::RemoveSelected => self.control_selected(ControlOp::RemoveRecord, "移除记录"),
            AppAction::DeleteSelectedFiles => {
                self.status_message = "删除文件需要显式二次确认；此入口不直接删除文件".to_string();
            }
            AppAction::OpenOptions => {
                self.open_dialog = Some(UiDialog::Options);
                self.status_message = "已打开“选项”窗口".to_string();
            }
            AppAction::OpenScheduler => {
                self.open_dialog = Some(UiDialog::QueueScheduler);
                self.status_message = "已打开“计划任务/队列”窗口".to_string();
            }
            AppAction::OpenGrabber => {
                self.open_dialog = Some(UiDialog::SiteGrabber);
                self.status_message = "已打开“站点抓取”窗口".to_string();
            }
            AppAction::OpenTaskLog => {
                self.open_dialog = Some(UiDialog::TaskLog);
                self.status_message = format!("已打开任务日志，共 {} 行", self.log_lines.len());
            }
            AppAction::ClearLog => {
                self.log_lines.clear();
                self.status_message = "任务日志已清空".to_string();
            }
            AppAction::OpenAbout => {
                self.open_dialog = Some(UiDialog::About);
                self.status_message = "已打开“关于”窗口".to_string();
            }
            AppAction::CloseDialog => {
                self.open_dialog = None;
                self.dialog_error = None;
            }
            AppAction::SelectCategory(category) => {
                self.selected_category = category.clone();
                self.status_message = format!("当前分类: {}", category);
            }
        }
    }
}
