use mdm_ipc::{
    CURRENT_SCHEMA_VERSION, DecimalU64, MediaIntent, OutputPolicy, SizeKind, SourceSpec, TaskPhase,
    TaskSnapshot, TaskState,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub schema_version: u32,
    pub task_id: String,
    pub caller_key: String,
    pub idempotency_key: String,
    pub creation_digest: String,
    pub source_kind: String,
    pub source_display: String,
    /// Raw page URL kept out of snapshots and logs; only the worker uses it.
    #[serde(skip)]
    pub worker_source_url: String,
    pub protected_source_ref: String,
    pub output: OutputPolicy,
    pub queue_id: String,
    pub category: String,
    pub state: TaskState,
    pub phase: Option<TaskPhase>,
    pub reason: Option<String>,
    pub attempt_id: Option<String>,
    pub revision: u64,
    pub last_seq: u64,
    pub bytes_received: u64,
    pub total_bytes: Option<u64>,
    pub size_kind: SizeKind,
    pub media_intent: Option<MediaIntent>,
    pub context_ref: Option<String>,
    pub network_profile_id: String,
    pub component_set_id: Option<String>,
    pub retry_count: u32,
    pub max_retries: u32,
    pub not_before_epoch_ms: Option<u64>,
    pub created_at_utc: String,
    pub error_code: Option<String>,
    pub owned_artifact_paths: Vec<String>,
}

impl TaskRecord {
    pub fn to_snapshot(&self) -> TaskSnapshot {
        let (total_bytes, progress_ratio) = match (self.size_kind, self.total_bytes) {
            (SizeKind::Unknown, _) | (_, None) => (None, None),
            (_, Some(0)) => (Some(DecimalU64(0)), Some(0.0)),
            (_, Some(tot)) => {
                let ratio = (self.bytes_received as f64 / tot as f64).clamp(0.0, 1.0);
                (Some(DecimalU64(tot)), Some(ratio))
            }
        };
        TaskSnapshot {
            task_id: self.task_id.clone(),
            revision: DecimalU64(self.revision),
            attempt_id: self.attempt_id.clone(),
            state: self.state,
            phase: self.phase,
            reason: self.reason.clone(),
            display_name: self.output.file_name_hint.clone(),
            source_display: self.source_display.clone(),
            bytes_received: DecimalU64(self.bytes_received),
            total_bytes,
            size_kind: self.size_kind,
            progress_ratio,
            component_set_id: self.component_set_id.clone(),
            error_code: self.error_code.clone(),
        }
    }

    pub fn new_queued(
        task_id: String,
        caller_key: String,
        idempotency_key: String,
        creation_digest: String,
        source: &SourceSpec,
        output: OutputPolicy,
        context_ref: Option<String>,
        network_profile_id: String,
        media_intent: Option<MediaIntent>,
        initial_state: TaskState,
    ) -> Self {
        let (source_kind, raw_url) = match source {
            SourceSpec::DirectHttp { url, .. } => ("direct_http", url.as_str()),
            SourceSpec::PageUrl { url, .. } => ("page_url", url.as_str()),
            SourceSpec::Hls { url, .. } => ("hls", url.as_str()),
            SourceSpec::Dash { url, .. } => ("dash", url.as_str()),
            SourceSpec::CapturedFile { url, .. } => ("captured_file", url.as_str()),
        };
        let redacted = if source_kind == "direct_http" {
            crate::task_store::non_sensitive_url(raw_url)
        } else {
            mdm_ipc::redact_sensitive_url(raw_url)
        };
        let category = infer_category(&output.file_name_hint, media_intent.is_some());
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            task_id: task_id.clone(),
            caller_key,
            idempotency_key,
            creation_digest,
            source_kind: source_kind.to_string(),
            source_display: redacted,
            worker_source_url: raw_url.to_string(),
            protected_source_ref: format!("src-ref:{}", task_id),
            queue_id: output
                .queue_id
                .clone()
                .unwrap_or_else(|| "main".to_string()),
            output,
            category,
            state: initial_state,
            phase: if initial_state == TaskState::HandoffPending {
                None
            } else {
                Some(TaskPhase::Resolve)
            },
            reason: None,
            attempt_id: None,
            revision: 1,
            last_seq: 0,
            bytes_received: 0,
            total_bytes: None,
            size_kind: SizeKind::Unknown,
            media_intent,
            context_ref,
            network_profile_id,
            component_set_id: None,
            retry_count: 0,
            max_retries: 3,
            not_before_epoch_ms: None,
            created_at_utc: "2026-11-16T08:00:00Z".to_string(),
            error_code: None,
            owned_artifact_paths: Vec::new(),
        }
    }
}

pub fn infer_category(file_name: &str, is_media: bool) -> String {
    if is_media {
        return "视频 (Video)".to_string();
    }
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "ts" => "视频 (Video)".to_string(),
        "mp3" | "m4a" | "flac" | "wav" | "opus" | "ogg" => "音乐 (Music)".to_string(),
        "zip" | "rar" | "7z" | "tar" | "gz" | "xz" => "压缩包 (Compressed)".to_string(),
        "exe" | "msi" | "dmg" | "deb" | "rpm" | "appimage" => "程序 (Programs)".to_string(),
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" => {
            "文档 (Documents)".to_string()
        }
        _ => "常规 (General)".to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffState {
    Pending,
    Committed,
    Aborted,
    Expired,
    AwaitingUser,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffRecord {
    pub caller_key: String,
    pub idempotency_key: String,
    pub creation_digest: String,
    pub task_id: String,
    pub state: HandoffState,
    pub handoff_token_hash: String,
    pub expires_at_epoch_ms: u64,
    pub browser_download_id: u64,
    pub committed_revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRecord {
    pub context_ref: String,
    pub owner_instance_id: String,
    pub owner_profile_id: String,
    pub page_origin: String,
    pub allowed_hosts: Vec<String>,
    pub context_revision: u64,
    pub expires_at_utc: String,
    pub materials_digest: String,
    pub revoked: bool,
}
