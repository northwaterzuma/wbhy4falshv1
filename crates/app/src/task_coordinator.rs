//! Single authoritative writer for TaskRecord, HandoffRecord, ContextRecord, and tombstones.

use crate::model::{ContextRecord, HandoffRecord, HandoffState, TaskRecord};
use mdm_ipc::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Default, Clone)]
pub struct TaskCoordinator {
    pub schema_version: u32,
    pub tasks: BTreeMap<String, TaskRecord>,
    pub handoffs_by_idem: BTreeMap<(String, String), HandoffRecord>,
    pub task_by_idem: BTreeMap<(String, String), (String, String)>, // (caller_key, idem_key) -> (task_id, creation_digest)
    pub tombstones: BTreeMap<(String, String), u64>,                 // (caller_key, idem_key) -> deleted_at_epoch_ms
    pub op_dedup: BTreeMap<(String, String, String), BrowserBridgeResponse>, // (caller_key, op, request_id)
    pub contexts: BTreeMap<String, ContextRecord>,
}

impl TaskCoordinator {
    pub fn new() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            tasks: BTreeMap::new(),
            handoffs_by_idem: BTreeMap::new(),
            task_by_idem: BTreeMap::new(),
            tombstones: BTreeMap::new(),
            op_dedup: BTreeMap::new(),
            contexts: BTreeMap::new(),
        }
    }

    fn caller_key(caller: &CallerScope) -> String {
        format!("{}:{}:{}", caller.os_user, caller.instance_id, caller.profile_id)
    }

    fn hash_token(token: &str) -> String {
        hex::encode(Sha256::digest(token.as_bytes()))
    }

    fn handoff_time(epoch_ms: u64) -> String {
        chrono::DateTime::from_timestamp_millis(epoch_ms.min(i64::MAX as u64) as i64)
            .unwrap_or(chrono::DateTime::UNIX_EPOCH).to_rfc3339()
    }

    pub(crate) fn expire_handoffs(&mut self, now: u64) -> bool {
        let mut changed = false;
        for handoff in self.handoffs_by_idem.values_mut() {
            if handoff.state != HandoffState::Pending || now <= handoff.expires_at_epoch_ms {
                continue;
            }
            handoff.state = HandoffState::AwaitingUser;
            if let Some(task) = self.tasks.get_mut(&handoff.task_id) {
                task.state = TaskState::AwaitingUser;
                task.reason = Some("handoff_reconcile".into());
                task.revision += 1;
            }
            changed = true;
        }
        changed
    }

    /// Two-phase browser handoff Step 1: `handoff.prepare`.
    /// Creates `HandoffPending` task and durable ledger entry with ZERO network side-effects.
    pub fn prepare_handoff(
        &mut self,
        caller: &CallerScope,
        request_id: &str,
        idempotency_key: &str,
        browser: &BrowserMeta,
        source: &SourceSpec,
        output: OutputPolicy,
        context_ref: Option<String>,
        network_profile_id: String,
        media_intent: Option<MediaIntent>,
        now_epoch_ms: u64,
    ) -> BrowserBridgeResponse {
        let ckey = Self::caller_key(caller);
        let op_key = (ckey.clone(), "handoff.prepare".to_string(), request_id.to_string());
        if let Some(cached) = self.op_dedup.get(&op_key) {
            return cached.clone();
        }

        // Reject if tombstone exists (cannot resurrect aborted/removed handoff with same idempotency_key)
        if self.tombstones.contains_key(&(ckey.clone(), idempotency_key.to_string())) {
            return BrowserBridgeResponse::error(
                request_id,
                ErrorCode::IdempotencyConflict,
                "Idempotency key belongs to an aborted or removed tombstone record",
                false,
                None,
            );
        }

        // Validate replayability: browser intercept only accepts GET + KnownReplayable + non-empty evidence
        match source {
            SourceSpec::DirectHttp {
                method,
                replayability,
                replayability_evidence_ref,
                ..
            } => {
                if method != "GET" || *replayability != Replayability::KnownReplayable || replayability_evidence_ref.is_none() {
                    return BrowserBridgeResponse::error(
                        request_id,
                        ErrorCode::NotReplayable,
                        "Automatic browser intercept requires GET with known_replayable and evidence ref",
                        false,
                        None,
                    );
                }
            }
            _ => {
                return BrowserBridgeResponse::error(
                    request_id,
                    ErrorCode::NotReplayable,
                    "Automatic browser intercept only applies to direct_http GET downloads",
                    false,
                    None,
                );
            }
        }

        let Some(dl_id) = browser.download_id else {
            return BrowserBridgeResponse::error(
                request_id,
                ErrorCode::InvalidRequest,
                "handoff.prepare requires integer browser.download_id",
                false,
                None,
            );
        };

        if let Err(err) = sanitize_file_name(&output.file_name_hint) {
            return BrowserBridgeResponse::error(request_id, err.code, err.message, false, None);
        }

        let digest = compute_creation_digest(
            &serde_json::to_value(source).unwrap_or_default(),
            &serde_json::to_value(&output).unwrap_or_default(),
            &serde_json::to_value(&media_intent).unwrap_or_default(),
        );

        let idem_tuple = (ckey.clone(), idempotency_key.to_string());
        if let Some(existing_handoff) = self.handoffs_by_idem.get(&idem_tuple) {
            if existing_handoff.creation_digest != digest {
                return BrowserBridgeResponse::error(
                    request_id,
                    ErrorCode::IdempotencyConflict,
                    "Same idempotency_key submitted with conflicting creation payload",
                    false,
                    None,
                );
            }
            if existing_handoff.state == HandoffState::Aborted {
                return BrowserBridgeResponse::error(
                    request_id,
                    ErrorCode::InvalidState,
                    "Handoff was already aborted and cannot be re-prepared",
                    false,
                    None,
                );
            }
            let Some(task) = self.tasks.get(&existing_handoff.task_id) else {
                return BrowserBridgeResponse::error(request_id, ErrorCode::NotFound, "Task record was removed", false, None);
            };
            if existing_handoff.state == HandoffState::Committed {
                return self.query_handoff_status(caller, request_id, idempotency_key);
            }
            if existing_handoff.state != HandoffState::Pending || now_epoch_ms > existing_handoff.expires_at_epoch_ms {
                return BrowserBridgeResponse::error(request_id, ErrorCode::LeaseExpired, "Handoff lease expired", false, None);
            }
            let token = format!("htok-{}-{}", existing_handoff.task_id, existing_handoff.caller_key);
            let resp = BrowserBridgeResponse {
                version: PROTOCOL_VERSION_V1,
                request_id: request_id.to_string(),
                status: ResponseStatus::Prepared,
                task_id: Some(task.task_id.clone()),
                task_state: Some(task.state),
                revision: Some(DecimalU64(task.revision)),
                durable: Some(true),
                handoff_token: Some(token),
                expires_at: Some(Self::handoff_time(existing_handoff.expires_at_epoch_ms)),
                data: None,
                error: None,
            };
            self.op_dedup.insert(op_key, resp.clone());
            return resp;
        }

        let task_id = format!("task-{}", Uuid::new_v4());

        let token = format!("htok-{}-{}", task_id, ckey);
        let expires = now_epoch_ms.saturating_add(120_000);
        let handoff = HandoffRecord {
            caller_key: ckey.clone(),
            idempotency_key: idempotency_key.to_string(),
            creation_digest: digest.clone(),
            task_id: task_id.clone(),
            state: HandoffState::Pending,
            handoff_token_hash: Self::hash_token(&token),
            expires_at_epoch_ms: expires,
            browser_download_id: dl_id,
            committed_revision: None,
        };

        let task = TaskRecord::new_queued(
            task_id.clone(),
            ckey,
            idempotency_key.to_string(),
            digest.clone(),
            source,
            output,
            context_ref,
            network_profile_id,
            media_intent,
            TaskState::HandoffPending,
        );

        self.tasks.insert(task_id.clone(), task);
        self.handoffs_by_idem.insert(idem_tuple.clone(), handoff);
        self.task_by_idem.insert(idem_tuple, (task_id.clone(), digest));

        let resp = BrowserBridgeResponse {
            version: PROTOCOL_VERSION_V1,
            request_id: request_id.to_string(),
            status: ResponseStatus::Prepared,
            task_id: Some(task_id),
            task_state: Some(TaskState::HandoffPending),
            revision: Some(DecimalU64(1)),
            durable: Some(true),
            handoff_token: Some(token),
            expires_at: Some(Self::handoff_time(expires)),
            data: None,
            error: None,
        };
        self.op_dedup.insert(op_key, resp.clone());
        resp
    }

    /// Two-phase browser handoff Step 2: `handoff.commit`.
    /// If already committed for the same idempotency_key, returns the persisted committed result
    /// even if the lease has since expired or revision has advanced!
    pub fn commit_handoff(
        &mut self,
        caller: &CallerScope,
        request_id: &str,
        idempotency_key: &str,
        task_id: &str,
        expected_revision: u64,
        handoff_token: &str,
        browser_download_id: u64,
        browser_cancel_confirmed: bool,
        now_epoch_ms: u64,
    ) -> BrowserBridgeResponse {
        let ckey = Self::caller_key(caller);
        let op_key = (ckey.clone(), "handoff.commit".to_string(), request_id.to_string());
        if let Some(cached) = self.op_dedup.get(&op_key) {
            return cached.clone();
        }

        if !browser_cancel_confirmed {
            return BrowserBridgeResponse::error(
                request_id,
                ErrorCode::InvalidRequest,
                "handoff.commit requires browser_cancel_confirmed == true from trusted extension",
                false,
                None,
            );
        }

        let idem_tuple = (ckey, idempotency_key.to_string());
        let Some(handoff) = self.handoffs_by_idem.get_mut(&idem_tuple) else {
            return BrowserBridgeResponse::error(request_id, ErrorCode::NotFound, "Handoff record not found", false, None);
        };

        if handoff.task_id != task_id || handoff.browser_download_id != browser_download_id {
            return BrowserBridgeResponse::error(request_id, ErrorCode::InvalidRequest, "Mismatched task_id or browser_download_id", false, None);
        }
        if handoff.handoff_token_hash != Self::hash_token(handoff_token) {
            return BrowserBridgeResponse::error(request_id, ErrorCode::PermissionDenied, "Invalid handoff_token", false, None);
        }

        // Replay of an already committed handoff returns the persisted committed result
        if handoff.state == HandoffState::Committed {
            let Some(task) = self.tasks.get(task_id) else {
                return BrowserBridgeResponse::error(request_id, ErrorCode::AlreadyCommitted, "Handoff already committed; task record removed", false, None);
            };
            let resp = BrowserBridgeResponse {
                version: PROTOCOL_VERSION_V1,
                request_id: request_id.to_string(),
                status: ResponseStatus::Queued,
                task_id: Some(task.task_id.clone()),
                task_state: Some(task.state),
                revision: Some(DecimalU64(handoff.committed_revision.unwrap_or(task.revision))),
                durable: Some(true),
                handoff_token: None,
                expires_at: None,
                data: None,
                error: None,
            };
            self.op_dedup.insert(op_key, resp.clone());
            return resp;
        }

        if handoff.state == HandoffState::Aborted {
            return BrowserBridgeResponse::error(
                request_id,
                ErrorCode::InvalidState,
                "Handoff was already aborted; cannot commit",
                false,
                None,
            );
        }

        let Some(task) = self.tasks.get_mut(task_id) else {
            return BrowserBridgeResponse::error(request_id, ErrorCode::NotFound, "Task record was removed", false, None);
        };
        if handoff.state != HandoffState::Pending || now_epoch_ms > handoff.expires_at_epoch_ms {
            let already_expired = handoff.state == HandoffState::AwaitingUser;
            handoff.state = HandoffState::AwaitingUser;
            task.state = TaskState::AwaitingUser;
            task.reason = Some("handoff_reconcile".to_string());
            if !already_expired {
                task.revision += 1;
            }
            return BrowserBridgeResponse::error(
                request_id,
                ErrorCode::LeaseExpired,
                "Handoff lease expired before commit; moved to AwaitingUser(reason=handoff_reconcile)",
                false,
                None,
            );
        }

        if task.revision != expected_revision {
            return BrowserBridgeResponse::error(
                request_id,
                ErrorCode::RevisionConflict,
                format!("Expected revision {}, actual {}", expected_revision, task.revision),
                false,
                None,
            );
        }

        task.state = TaskState::Queued;
        task.phase = Some(TaskPhase::Resolve);
        task.revision += 1;
        handoff.state = HandoffState::Committed;
        handoff.committed_revision = Some(task.revision);

        let resp = BrowserBridgeResponse {
            version: PROTOCOL_VERSION_V1,
            request_id: request_id.to_string(),
            status: ResponseStatus::Queued,
            task_id: Some(task.task_id.clone()),
            task_state: Some(TaskState::Queued),
            revision: Some(DecimalU64(task.revision)),
            durable: Some(true),
            handoff_token: None,
            expires_at: None,
            data: None,
            error: None,
        };
        self.op_dedup.insert(op_key, resp.clone());
        resp
    }

    /// Aborts a pending handoff (`handoff.abort`). Returns `AlreadyCommitted` if already committed.
    pub fn abort_handoff(
        &mut self,
        caller: &CallerScope,
        request_id: &str,
        idempotency_key: &str,
        task_id: &str,
        expected_revision: u64,
        handoff_token: &str,
        now_epoch_ms: u64,
    ) -> BrowserBridgeResponse {
        let ckey = Self::caller_key(caller);
        let idem_tuple = (ckey.clone(), idempotency_key.to_string());
        let Some(handoff) = self.handoffs_by_idem.get_mut(&idem_tuple) else {
            return BrowserBridgeResponse::error(request_id, ErrorCode::NotFound, "Handoff not found", false, None);
        };
        if handoff.task_id != task_id || handoff.handoff_token_hash != Self::hash_token(handoff_token) {
            return BrowserBridgeResponse::error(request_id, ErrorCode::PermissionDenied, "Invalid handoff_token or task_id", false, None);
        }
        if handoff.state == HandoffState::Committed {
            return BrowserBridgeResponse::error(
                request_id,
                ErrorCode::AlreadyCommitted,
                "Handoff is already committed; refusing to abort running task or resume browser",
                false,
                None,
            );
        }
        let Some(task) = self.tasks.get_mut(task_id) else {
            return BrowserBridgeResponse::error(request_id, ErrorCode::NotFound, "Task record was removed", false, None);
        };
        if handoff.state == HandoffState::Aborted {
            return BrowserBridgeResponse {
                version: PROTOCOL_VERSION_V1,
                request_id: request_id.to_string(),
                status: ResponseStatus::Aborted,
                task_id: Some(task.task_id.clone()),
                task_state: Some(TaskState::Cancelled),
                revision: Some(DecimalU64(task.revision)),
                durable: Some(true),
                handoff_token: None,
                expires_at: None,
                data: None,
                error: None,
            };
        }
        if task.revision != expected_revision {
            return BrowserBridgeResponse::error(request_id, ErrorCode::RevisionConflict, "Revision conflict on abort", false, None);
        }
        handoff.state = HandoffState::Aborted;
        task.state = TaskState::Cancelled;
        task.phase = None;
        task.revision += 1;
        self.tombstones.insert(idem_tuple, now_epoch_ms);

        BrowserBridgeResponse {
            version: PROTOCOL_VERSION_V1,
            request_id: request_id.to_string(),
            status: ResponseStatus::Aborted,
            task_id: Some(task.task_id.clone()),
            task_state: Some(TaskState::Cancelled),
            revision: Some(DecimalU64(task.revision)),
            durable: Some(true),
            handoff_token: None,
            expires_at: None,
            data: None,
            error: None,
        }
    }

    /// Queries handoff status (`handoff.status`) without creating a task if unknown.
    pub fn query_handoff_status(&self, caller: &CallerScope, request_id: &str, idempotency_key: &str) -> BrowserBridgeResponse {
        let ckey = Self::caller_key(caller);
        let idem_tuple = (ckey, idempotency_key.to_string());
        let Some(handoff) = self.handoffs_by_idem.get(&idem_tuple) else {
            return BrowserBridgeResponse::error(request_id, ErrorCode::NotFound, "No handoff found for idempotency_key", false, None);
        };
        let task = self.tasks.get(&handoff.task_id);
        BrowserBridgeResponse {
            version: PROTOCOL_VERSION_V1,
            request_id: request_id.to_string(),
            status: ResponseStatus::Ok,
            task_id: Some(handoff.task_id.clone()),
            task_state: task.map(|task| task.state),
            revision: task.map(|task| DecimalU64(task.revision)),
            durable: Some(true),
            handoff_token: None,
            expires_at: None,
            data: Some(serde_json::json!({
                "handoff_state": handoff.state,
                "expires_at": Self::handoff_time(handoff.expires_at_epoch_ms),
                "browser_download_id": handoff.browser_download_id
            })),
            error: None,
        }
    }

    /// Local recovery command `RecoverHandoff` for `AwaitingUser(reason=handoff_reconcile)`.
    pub fn recover_handoff(&mut self, cmd: &RecoverHandoff, now_epoch_ms: u64) -> Result<u64, ErrorDetail> {
        let Some(task) = self.tasks.get_mut(&cmd.task_id) else {
            return Err(ErrorDetail {
                code: ErrorCode::NotFound,
                message: "Task not found".to_string(),
                retryable: false,
                retry_after_ms: None,
            });
        };
        let idem_tuple = (task.caller_key.clone(), task.idempotency_key.clone());
        let Some(handoff) = self.handoffs_by_idem.get_mut(&idem_tuple) else {
            return Err(ErrorDetail {
                code: ErrorCode::InvalidState,
                message: "Task is not a browser handoff task".to_string(),
                retryable: false,
                retry_after_ms: None,
            });
        };
        if matches!(handoff.state, HandoffState::Committed | HandoffState::Aborted) {
            return Err(ErrorDetail {
                code: ErrorCode::InvalidState,
                message: "Cannot recover an already committed or aborted handoff".to_string(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        if task.revision != cmd.expected_revision.0 {
            return Err(ErrorDetail {
                code: ErrorCode::RevisionConflict,
                message: "Revision mismatch in RecoverHandoff".to_string(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        match cmd.decision {
            HandoffDecision::ContinueDesktop => {
                if !cmd.browser_cancel_confirmed {
                    return Err(ErrorDetail {
                        code: ErrorCode::InvalidState,
                        message: "ContinueDesktop requires browser cancellation confirmation".to_string(),
                        retryable: false,
                        retry_after_ms: None,
                    });
                }
                handoff.state = HandoffState::Committed;
                task.state = TaskState::Queued;
                task.phase = Some(TaskPhase::Resolve);
                task.reason = None;
                task.revision += 1;
                handoff.committed_revision = Some(task.revision);
            }
            HandoffDecision::Abort => {
                handoff.state = HandoffState::Aborted;
                task.state = TaskState::Cancelled;
                task.phase = None;
                task.reason = None;
                task.revision += 1;
                self.tombstones.insert(idem_tuple, now_epoch_ms);
            }
        }
        Ok(task.revision)
    }

    /// Direct explicit `task.add` (no browser download ownership transfer).
    pub fn add_task(
        &mut self,
        caller: &CallerScope,
        request_id: &str,
        idempotency_key: &str,
        source: &SourceSpec,
        output: OutputPolicy,
        context_ref: Option<String>,
        network_profile_id: String,
        media_intent: Option<MediaIntent>,
    ) -> BrowserBridgeResponse {
        if !caller.capabilities.contains(&CapabilityScope::Add) {
            return BrowserBridgeResponse::error(request_id, ErrorCode::PermissionDenied, "Caller lacks 'add' capability", false, None);
        }
        let ckey = Self::caller_key(caller);
        if self.tombstones.contains_key(&(ckey.clone(), idempotency_key.to_string())) {
            return BrowserBridgeResponse::error(request_id, ErrorCode::IdempotencyConflict, "Idempotency key is tombstoned", false, None);
        }
        if let Err(err) = sanitize_file_name(&output.file_name_hint) {
            return BrowserBridgeResponse::error(request_id, err.code, err.message, false, None);
        }
        // Validate context ownership if context_ref is provided
        if let Some(cref) = &context_ref {
            let Some(ctx) = self.contexts.get(cref) else {
                return BrowserBridgeResponse::error(request_id, ErrorCode::ContextExpired, "Unknown context_ref", false, None);
            };
            if ctx.revoked || ctx.owner_instance_id != caller.instance_id || ctx.owner_profile_id != caller.profile_id {
                return BrowserBridgeResponse::error(
                    request_id,
                    ErrorCode::PermissionDenied,
                    "Cross-profile or revoked context_ref rejected",
                    false,
                    None,
                );
            }
        }

        let digest = compute_creation_digest(
            &serde_json::to_value(source).unwrap_or_default(),
            &serde_json::to_value(&output).unwrap_or_default(),
            &serde_json::to_value(&media_intent).unwrap_or_default(),
        );
        let idem_tuple = (ckey.clone(), idempotency_key.to_string());
        if let Some((existing_id, existing_digest)) = self.task_by_idem.get(&idem_tuple) {
            if existing_digest != &digest {
                return BrowserBridgeResponse::error(
                    request_id,
                    ErrorCode::IdempotencyConflict,
                    "Conflicting creation semantics for existing idempotency_key",
                    false,
                    None,
                );
            }
            let existing = self.tasks.get(existing_id).unwrap();
            return BrowserBridgeResponse {
                version: PROTOCOL_VERSION_V1,
                request_id: request_id.to_string(),
                status: ResponseStatus::Accepted,
                task_id: Some(existing.task_id.clone()),
                task_state: Some(existing.state),
                revision: Some(DecimalU64(existing.revision)),
                durable: Some(true),
                handoff_token: None,
                expires_at: None,
                data: None,
                error: None,
            };
        }

        let task_id = format!("task-{}", Uuid::new_v4());
        let task = TaskRecord::new_queued(
            task_id.clone(),
            ckey,
            idempotency_key.to_string(),
            digest.clone(),
            source,
            output,
            context_ref,
            network_profile_id,
            media_intent,
            TaskState::Queued,
        );
        self.tasks.insert(task_id.clone(), task);
        self.task_by_idem.insert(idem_tuple, (task_id.clone(), digest));

        BrowserBridgeResponse {
            version: PROTOCOL_VERSION_V1,
            request_id: request_id.to_string(),
            status: ResponseStatus::Accepted,
            task_id: Some(task_id),
            task_state: Some(TaskState::Queued),
            revision: Some(DecimalU64(1)),
            durable: Some(true),
            handoff_token: None,
            expires_at: None,
            data: None,
            error: None,
        }
    }

    /// Executes control operations (`Pause`, `Resume`, `Cancel`, `Retry`, `RemoveRecord`, `DeleteFiles`).
    /// Strictly enforces:
    /// - Repeat no-op check (`Paused` + `Pause`, `Cancelled` + `Cancel`) BEFORE revision conflict check.
    /// - `Resume`/`Retry` cannot enqueue `HandoffPending` or `AwaitingUser(reason=handoff_reconcile)`.
    /// - `RemoveRecord` never deletes user files; `DeleteFiles` requires explicit `DeleteFiles` capability and only touches `owned_artifact_paths`.
    pub fn control_task(
        &mut self,
        caller: &CallerScope,
        request_id: &str,
        task_id: &str,
        expected_revision: u64,
        op: ControlOp,
        now_epoch_ms: u64,
    ) -> BrowserBridgeResponse {
        if op == ControlOp::DeleteFiles && !caller.capabilities.contains(&CapabilityScope::DeleteFiles) {
            return BrowserBridgeResponse::error(
                request_id,
                ErrorCode::PermissionDenied,
                "Caller does not have DeleteFiles capability",
                false,
                None,
            );
        }
        if !caller.capabilities.contains(&CapabilityScope::Control) {
            return BrowserBridgeResponse::error(
                request_id,
                ErrorCode::PermissionDenied,
                "Caller does not have Control capability",
                false,
                None,
            );
        }

        let Some(task) = self.tasks.get_mut(task_id) else {
            return BrowserBridgeResponse::error(request_id, ErrorCode::NotFound, "Task not found", false, None);
        };

        // Repeat control idempotency BEFORE revision guard
        if (op == ControlOp::Pause && matches!(task.state, TaskState::Pausing | TaskState::Paused))
            || (op == ControlOp::Cancel && matches!(task.state, TaskState::Cancelling | TaskState::Cancelled))
        {
            return BrowserBridgeResponse {
                version: PROTOCOL_VERSION_V1,
                request_id: request_id.to_string(),
                status: ResponseStatus::NotModified,
                task_id: Some(task.task_id.clone()),
                task_state: Some(task.state),
                revision: Some(DecimalU64(task.revision)),
                durable: Some(true),
                handoff_token: None,
                expires_at: None,
                data: None,
                error: None,
            };
        }

        // Guard pending handoffs against ordinary Resume/Retry
        if matches!(op, ControlOp::Resume | ControlOp::Retry)
            && (task.state == TaskState::HandoffPending
                || (task.state == TaskState::AwaitingUser && task.reason.as_deref() == Some("handoff_reconcile")))
        {
            return BrowserBridgeResponse::error(
                request_id,
                ErrorCode::InvalidState,
                "Cannot Resume or Retry an uncommitted browser handoff; use commit or RecoverHandoff",
                false,
                None,
            );
        }

        if task.revision != expected_revision {
            return BrowserBridgeResponse::error(
                request_id,
                ErrorCode::RevisionConflict,
                format!("Expected revision {}, actual {}", expected_revision, task.revision),
                false,
                None,
            );
        }

        let active = matches!(task.state, TaskState::Resolving | TaskState::Running | TaskState::Finishing);
        let stopping = matches!(task.state, TaskState::Pausing | TaskState::Cancelling);
        match op {
            ControlOp::Pause => {
                if task.state.is_terminal() || stopping || task.state == TaskState::HandoffPending {
                    return BrowserBridgeResponse::error(request_id, ErrorCode::InvalidState, "Cannot pause this task state", false, None);
                }
                task.state = if active && task.attempt_id.is_some() {
                    TaskState::Pausing
                } else {
                    TaskState::Paused
                };
                task.revision += 1;
            }
            ControlOp::Resume | ControlOp::Retry => {
                if !matches!(task.state, TaskState::Queued | TaskState::Paused | TaskState::Failed) {
                    return BrowserBridgeResponse::error(request_id, ErrorCode::InvalidState, "Task is not ready to start", false, None);
                }
                let attempt = format!("att-{}", Uuid::new_v4());
                task.attempt_id = Some(attempt);
                task.state = TaskState::Running;
                task.phase = Some(TaskPhase::Transfer);
                if task.source_kind != "direct_http" {
                    task.bytes_received = 0;
                }
                task.error_code = None;
                task.reason = None;
                task.revision += 1;
            }
            ControlOp::Cancel => {
                if task.state == TaskState::Completed {
                    return BrowserBridgeResponse::error(request_id, ErrorCode::InvalidState, "Completed task cannot be cancelled", false, None);
                }
                let idem_tuple = (task.caller_key.clone(), task.idempotency_key.clone());
                if let Some(h) = self.handoffs_by_idem.get_mut(&idem_tuple) {
                    if h.state != HandoffState::Committed {
                        h.state = HandoffState::Aborted;
                        self.tombstones.insert(idem_tuple, now_epoch_ms);
                    }
                }
                if (active || stopping) && task.attempt_id.is_some() {
                    task.state = TaskState::Cancelling;
                } else {
                    task.state = TaskState::Cancelled;
                    task.phase = None;
                    task.attempt_id = None;
                }
                task.revision += 1;
            }
            ControlOp::RemoveRecord => {
                if active || stopping {
                    return BrowserBridgeResponse::error(request_id, ErrorCode::InvalidState, "Stop the task before removing it", false, None);
                }
                let idem_tuple = (task.caller_key.clone(), task.idempotency_key.clone());
                if let Some(h) = self.handoffs_by_idem.get_mut(&idem_tuple) {
                    if h.state != HandoffState::Committed {
                        h.state = HandoffState::Aborted;
                    }
                }
                self.tombstones.insert(idem_tuple, now_epoch_ms);
                let removed = self.tasks.remove(task_id).unwrap();
                return BrowserBridgeResponse {
                    version: PROTOCOL_VERSION_V1,
                    request_id: request_id.to_string(),
                    status: ResponseStatus::Ok,
                    task_id: Some(removed.task_id),
                    task_state: Some(TaskState::Cancelled),
                    revision: Some(DecimalU64(removed.revision + 1)),
                    durable: Some(true),
                    handoff_token: None,
                    expires_at: None,
                    data: None,
                    error: None,
                };
            }
            ControlOp::DeleteFiles => {
                if active || stopping {
                    return BrowserBridgeResponse::error(request_id, ErrorCode::InvalidState, "Stop the task before deleting files", false, None);
                }
                for p in &task.owned_artifact_paths {
                    if !p.contains("..") {
                        let _ = std::fs::remove_file(p);
                    }
                }
                task.owned_artifact_paths.clear();
                task.revision += 1;
            }
        }

        let task = self.tasks.get(task_id).unwrap();
        BrowserBridgeResponse {
            version: PROTOCOL_VERSION_V1,
            request_id: request_id.to_string(),
            status: ResponseStatus::Ok,
            task_id: Some(task.task_id.clone()),
            task_state: Some(task.state),
            revision: Some(DecimalU64(task.revision)),
            durable: Some(true),
            handoff_token: None,
            expires_at: None,
            data: None,
            error: None,
        }
    }

    /// Applies worker progress or completion event only if `attempt_id` matches the active attempt.
    /// High-frequency progress updates increment `last_seq` without bumping `revision` (preventing UI control revision conflicts),
    /// and never regress `Completed` tasks or committed bytes.
    pub fn apply_worker_event(
        &mut self,
        task_id: &str,
        attempt_id: &str,
        new_bytes: u64,
        mark_completed: bool,
    ) -> Result<(u64, u64), ErrorDetail> {
        let Some(task) = self.tasks.get_mut(task_id) else {
            return Err(ErrorDetail {
                code: ErrorCode::NotFound,
                message: "Task not found".to_string(),
                retryable: false,
                retry_after_ms: None,
            });
        };
        if !matches!(task.state, TaskState::Resolving | TaskState::Running | TaskState::Finishing) {
            return Err(ErrorDetail {
                code: ErrorCode::InvalidState,
                message: "Task is not accepting progress or completion events".to_string(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        if task.attempt_id.as_deref() != Some(attempt_id) {
            return Err(ErrorDetail {
                code: ErrorCode::RevisionConflict,
                message: "Dropping event from stale attempt_id".to_string(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        if task.source_kind == "direct_http" || new_bytes > task.bytes_received {
            task.bytes_received = new_bytes;
        }
        task.last_seq += 1;
        if mark_completed {
            task.phase = Some(TaskPhase::Commit);
            task.state = TaskState::Completed;
            task.revision += 1;
        }
        Ok((task.revision, task.last_seq))
    }

    pub(crate) fn finish_stopping(&mut self, task_id: &str, attempt_id: &str) -> bool {
        let Some(task) = self.tasks.get_mut(task_id) else {
            return false;
        };
        if task.attempt_id.as_deref() != Some(attempt_id) {
            return false;
        }
        task.state = match task.state {
            TaskState::Pausing => TaskState::Paused,
            TaskState::Cancelling => TaskState::Cancelled,
            _ => return false,
        };
        task.attempt_id = None;
        task.phase = None;
        task.revision += 1;
        true
    }

    /// Marks the active attempt as failed after the worker reports a terminal error.
    /// Stale attempts and terminal tasks are ignored so late events cannot corrupt state.
    pub fn mark_task_failed(
        &mut self,
        task_id: &str,
        attempt_id: &str,
        error: &ErrorDetail,
        now_epoch_ms: u64,
    ) -> Result<u64, ErrorDetail> {
        let Some(task) = self.tasks.get_mut(task_id) else {
            return Err(ErrorDetail {
                code: ErrorCode::NotFound,
                message: "Task not found".to_string(),
                retryable: false,
                retry_after_ms: None,
            });
        };
        if !matches!(task.state, TaskState::Resolving | TaskState::Running | TaskState::Finishing) {
            return Err(ErrorDetail {
                code: ErrorCode::InvalidState,
                message: "Task is not accepting failure events".to_string(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        if task.attempt_id.as_deref() != Some(attempt_id) {
            return Err(ErrorDetail {
                code: ErrorCode::RevisionConflict,
                message: "Dropping failure event from stale attempt_id".to_string(),
                retryable: false,
                retry_after_ms: None,
            });
        }
        task.state = TaskState::Failed;
        task.phase = None;
        task.reason = Some(error.message.clone());
        task.error_code = Some(format!("{:?}", error.code));
        task.revision += 1;
        task.not_before_epoch_ms = error.retry_after_ms.map(|ms| now_epoch_ms.saturating_add(ms));
        Ok(task.revision)
    }
}
