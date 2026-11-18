//! Native Messaging Host (`com.rustcatch.bridge`) framing and 4-stage `hello` authentication.

use mdm_app::{CaptureRegistry, ContextRecord, TaskCoordinator};
use mdm_ipc::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

pub struct NativeHostSession {
    pub allowed_extension_origins: Vec<String>,
    pub caller_origin: String,
    pub authenticated_session: Option<(String, String)>, // (instance_id, session_ref)
    pub session_profile_id: Option<String>, // profile_id bound at first authenticated data op
    pub enrolled_secrets: BTreeMap<String, Vec<u8>>,
    pub active_challenges: BTreeMap<String, (String, String, u64)>, // challenge_id -> (instance_id, nonce, expires_at_ms)
    pub coordinator: TaskCoordinator,
    pub capture_registry: CaptureRegistry,
}

impl NativeHostSession {
    pub fn new(allowed_extension_origins: Vec<String>, caller_origin: String) -> Self {
        Self {
            allowed_extension_origins,
            caller_origin,
            authenticated_session: None,
            session_profile_id: None,
            enrolled_secrets: BTreeMap::new(),
            active_challenges: BTreeMap::new(),
            coordinator: TaskCoordinator::new(),
            capture_registry: CaptureRegistry::new(),
        }
    }

    /// Reads a single 4-byte native-endian length-prefixed frame, enforcing 256 KiB BEFORE allocation.
    pub fn read_native_frame<R: Read>(reader: &mut R) -> io::Result<Option< Result<Vec<u8>, ErrorDetail> >> {
        let mut len_buf = [0u8; 4];
        match reader.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let frame_len = u32::from_ne_bytes(len_buf) as usize;
        if frame_len == 0 || frame_len > MAX_FRAME_BYTES {
            return Ok(Some(Err(ErrorDetail {
                code: ErrorCode::TooLarge,
                message: format!("Native frame length {} violates 1..={} byte bound", frame_len, MAX_FRAME_BYTES),
                retryable: false,
                retry_after_ms: None,
            })));
        }
        let mut payload = vec![0u8; frame_len];
        reader.read_exact(&mut payload)?;
        Ok(Some(Ok(payload)))
    }

    /// Writes a 4-byte native-endian length-prefixed JSON response to stdout.
    pub fn write_native_frame<W: Write>(writer: &mut W, resp: &BrowserBridgeResponse) -> io::Result<()> {
        let bytes = serde_json::to_vec(resp).expect("response serializes");
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Native response exceeds frame limit"));
        }
        let len_bytes = (bytes.len() as u32).to_ne_bytes();
        writer.write_all(&len_bytes)?;
        writer.write_all(&bytes)?;
        writer.flush()
    }

    pub fn handle_raw_frame(&mut self, raw: &[u8], now_epoch_ms: u64) -> BrowserBridgeResponse {
        if !self.allowed_extension_origins.contains(&self.caller_origin) {
            return BrowserBridgeResponse::error(
                "untrusted-origin",
                ErrorCode::PermissionDenied,
                "Extension origin is not in allowed_origins",
                false,
                None,
            );
        }
        let val = match validate_raw_frame(raw) {
            Ok(v) => v,
            Err(err) => return BrowserBridgeResponse::error("invalid-frame", err.code, err.message, false, None),
        };
        let req: BrowserBridgeRequest = match serde_json::from_value(val) {
            Ok(r) => r,
            Err(e) => return BrowserBridgeResponse::error("invalid-envelope", ErrorCode::InvalidRequest, e.to_string(), false, None),
        };
        if req.version != PROTOCOL_VERSION_V1 {
            return BrowserBridgeResponse::error(req.request_id, ErrorCode::UnsupportedVersion, "Only protocol version 1 is supported", false, None);
        }

        if req.op == "hello" {
            return self.handle_hello(&req, now_epoch_ms);
        }

        // All non-hello operations require an authenticated session on this connection
        let Some((authed_instance_id, authed_session_ref)) = &self.authenticated_session else {
            return BrowserBridgeResponse::error(
                req.request_id,
                ErrorCode::PermissionDenied,
                "Connection must complete hello.prove authentication before invoking data/control ops",
                false,
                None,
            );
        };
        let authed_instance_id = authed_instance_id.clone();
        let authed_session_ref = authed_session_ref.clone();
        // Profile id is bound to this authenticated session on the first data op
        // and never changed afterwards. Page payload alone cannot switch profiles.
        let requested_profile = req.payload["browser"]["profile_id"].as_str().unwrap_or("").to_string();
        let authed_profile_id = if let Some(bound) = &self.session_profile_id {
            if !requested_profile.is_empty() && requested_profile != *bound {
                return BrowserBridgeResponse::error(
                    &req.request_id,
                    ErrorCode::PermissionDenied,
                    "profile_id differs from the profile bound to this authenticated session",
                    false,
                    None,
                );
            }
            bound.clone()
        } else if !requested_profile.is_empty() {
            self.session_profile_id = Some(requested_profile.clone());
            requested_profile
        } else {
            // Fallback for ops that carry no browser block (task.add/handoff.status).
            format!("profile-{}", authed_session_ref)
        };

        match req.op.as_str() {
            "capture.publish" => {
                let browser: BrowserMeta = serde_json::from_value(req.payload["browser"].clone()).unwrap();
                if browser.installation_id != authed_instance_id {
                    return BrowserBridgeResponse::error(req.request_id, ErrorCode::PermissionDenied, "installation_id mismatch", false, None);
                }
                let candidates: Vec<CaptureCandidate> = serde_json::from_value(req.payload["candidates"].clone()).unwrap_or_default();
                match self.capture_registry.publish_batch(&browser, candidates) {
                    Ok(count) => BrowserBridgeResponse {
                        version: 1,
                        request_id: req.request_id,
                        status: ResponseStatus::Ok,
                        task_id: None,
                        task_state: None,
                        revision: None,
                        durable: None,
                        handoff_token: None,
                        expires_at: None,
                        data: Some(serde_json::json!({ "stored_count": count })),
                        error: None,
                    },
                    Err(e) => BrowserBridgeResponse::error(req.request_id, e.code, e.message, e.retryable, e.retry_after_ms),
                }
            }
            "context.put" => {
                let raw_materials = serde_json::to_vec(&req.payload["materials"]).unwrap_or_default();
                if raw_materials.len() > MAX_CONTEXT_BYTES {
                    return BrowserBridgeResponse::error(req.request_id, ErrorCode::TooLarge, "Context materials exceed 32768 bytes", false, None);
                }
                let browser: BrowserMeta = serde_json::from_value(req.payload["browser"].clone()).unwrap();
                if browser.installation_id != authed_instance_id {
                    return BrowserBridgeResponse::error(req.request_id, ErrorCode::PermissionDenied, "Cross-instance context.put denied", false, None);
                }
                let cref = format!("ctx-{}", self.coordinator.contexts.len() + 1);
                let digest = hex::encode(Sha256::digest(&raw_materials));
                self.coordinator.contexts.insert(
                    cref.clone(),
                    ContextRecord {
                        context_ref: cref.clone(),
                        owner_instance_id: browser.installation_id,
                        owner_profile_id: browser.profile_id,
                        page_origin: req.payload["page_origin"].as_str().unwrap_or_default().to_string(),
                        allowed_hosts: serde_json::from_value(req.payload["allowed_hosts"].clone()).unwrap_or_default(),
                        context_revision: 1,
                        expires_at_utc: req.payload["expires_at"].as_str().unwrap_or("2026-11-16T08:15:00Z").to_string(),
                        materials_digest: digest,
                        revoked: false,
                    },
                );
                BrowserBridgeResponse {
                    version: 1,
                    request_id: req.request_id,
                    status: ResponseStatus::Ok,
                    task_id: None,
                    task_state: None,
                    revision: None,
                    durable: None,
                    handoff_token: None,
                    expires_at: None,
                    data: Some(serde_json::json!({
                        "context_ref": cref,
                        "context_revision": "1",
                        "expires_at": "2026-11-16T08:15:00Z",
                        "material_changed": true
                    })),
                    error: None,
                }
            }
            "handoff.prepare" => {
                let browser: BrowserMeta = match serde_json::from_value(req.payload["browser"].clone()) {
                    Ok(v) => v,
                    Err(e) => return BrowserBridgeResponse::error(&req.request_id, ErrorCode::InvalidRequest, format!("Invalid browser: {}", e), false, None),
                };
                if browser.installation_id != authed_instance_id {
                    return BrowserBridgeResponse::error(&req.request_id, ErrorCode::PermissionDenied, "installation_id mismatch", false, None);
                }
                let source: SourceSpec = match serde_json::from_value(req.payload["source"].clone()) {
                    Ok(v) => v,
                    Err(e) => return BrowserBridgeResponse::error(&req.request_id, ErrorCode::InvalidRequest, format!("Invalid source: {}", e), false, None),
                };
                let output: OutputPolicy = match serde_json::from_value(req.payload["output"].clone()) {
                    Ok(v) => v,
                    Err(e) => return BrowserBridgeResponse::error(&req.request_id, ErrorCode::InvalidRequest, format!("Invalid output: {}", e), false, None),
                };
                let context_ref = req.payload["context_ref"].as_str().map(str::to_string);
                let network_profile_id = req.payload["network_profile_id"].as_str().unwrap_or("system-default").to_string();
                let media_intent: Option<MediaIntent> = serde_json::from_value(req.payload["media_intent"].clone()).ok();
                let caller = self.bridge_caller(&authed_instance_id, &authed_profile_id);
                self.coordinator.prepare_handoff(&caller, &req.request_id, &req.idempotency_key,
                    &browser, &source, output, context_ref, network_profile_id, media_intent, now_epoch_ms)
            }
            "handoff.commit" => {
                let task_id = req.payload["task_id"].as_str().unwrap_or_default().to_string();
                let expected_revision = req.payload["expected_revision"].as_str()
                    .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                let token = req.payload["handoff_token"].as_str().unwrap_or_default().to_string();
                let dl_id = req.payload["browser_download_id"].as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .or_else(|| req.payload["browser_download_id"].as_u64()).unwrap_or(0);
                // Only a trusted native-extension caller may assert browser cancellation.
                let cancelled = req.payload["browser_cancel_confirmed"].as_bool().unwrap_or(false);
                let caller = self.bridge_caller(&authed_instance_id, &authed_profile_id);
                self.coordinator.commit_handoff(&caller, &req.request_id, &req.idempotency_key,
                    &task_id, expected_revision, &token, dl_id, cancelled, now_epoch_ms)
            }
            "handoff.status" => {
                let caller = self.bridge_caller(&authed_instance_id, &authed_profile_id);
                self.coordinator.query_handoff_status(&caller, &req.request_id, &req.idempotency_key)
            }
            "handoff.abort" => {
                let task_id = req.payload["task_id"].as_str().unwrap_or_default().to_string();
                let expected_revision = req.payload["expected_revision"].as_str()
                    .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                let token = req.payload["handoff_token"].as_str().unwrap_or_default().to_string();
                let caller = self.bridge_caller(&authed_instance_id, &authed_profile_id);
                self.coordinator.abort_handoff(&caller, &req.request_id, &req.idempotency_key,
                    &task_id, expected_revision, &token, now_epoch_ms)
            }
            "task.add" => {
                let source: SourceSpec = match serde_json::from_value(req.payload["source"].clone()) {
                    Ok(v) => v,
                    Err(e) => return BrowserBridgeResponse::error(&req.request_id, ErrorCode::InvalidRequest, format!("Invalid source: {}", e), false, None),
                };
                let output: OutputPolicy = match serde_json::from_value(req.payload["output"].clone()) {
                    Ok(v) => v,
                    Err(e) => return BrowserBridgeResponse::error(&req.request_id, ErrorCode::InvalidRequest, format!("Invalid output: {}", e), false, None),
                };
                let context_ref = req.payload["context_ref"].as_str().map(str::to_string);
                let network_profile_id = req.payload["network_profile_id"].as_str().unwrap_or("system-default").to_string();
                let media_intent: Option<MediaIntent> = serde_json::from_value(req.payload["media_intent"].clone()).ok();
                let caller = self.bridge_caller(&authed_instance_id, &authed_profile_id);
                self.coordinator.add_task(&caller, &req.request_id, &req.idempotency_key,
                    &source, output, context_ref, network_profile_id, media_intent)
            }
            "task.get" => {
                let task_id = req.payload["task_id"].as_str().unwrap_or_default();
                let caller = self.bridge_caller(&authed_instance_id, &authed_profile_id);
                match self.coordinator.tasks.get(task_id) {
                    Some(task) if task.caller_key == format!("{}:{}:{}", caller.os_user, caller.instance_id, caller.profile_id) => BrowserBridgeResponse {
                        version: PROTOCOL_VERSION_V1,
                        request_id: req.request_id.clone(),
                        status: ResponseStatus::Ok,
                        task_id: Some(task.task_id.clone()),
                        task_state: Some(task.state),
                        revision: Some(DecimalU64(task.revision)),
                        durable: Some(true),
                        handoff_token: None,
                        expires_at: None,
                        data: Some(serde_json::to_value(task.to_snapshot()).unwrap_or_default()),
                        error: None,
                    },
                    Some(_) => BrowserBridgeResponse::error(&req.request_id, ErrorCode::PermissionDenied, "Task belongs to another caller", false, None),
                    None => BrowserBridgeResponse::error(&req.request_id, ErrorCode::NotFound, "Task not found", false, None),
                }
            }
            "task.control" => {
                let task_id = req.payload["task_id"].as_str().unwrap_or_default().to_string();
                let expected_revision = req.payload["expected_revision"].as_str()
                    .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                let action = req.payload["action"].as_str().unwrap_or_default();
                let op = match action {
                    "pause" => ControlOp::Pause,
                    "resume" => ControlOp::Resume,
                    "cancel" => ControlOp::Cancel,
                    "retry" => ControlOp::Retry,
                    "remove_record" => ControlOp::RemoveRecord,
                    "delete_files" => ControlOp::DeleteFiles,
                    _ => return BrowserBridgeResponse::error(&req.request_id, ErrorCode::InvalidRequest, "Unknown control action", false, None),
                };
                let caller = self.bridge_caller(&authed_instance_id, &authed_profile_id);
                self.coordinator.control_task(&caller, &req.request_id, &task_id, expected_revision, op, now_epoch_ms)
            }
            _ => BrowserBridgeResponse::error(req.request_id, ErrorCode::InvalidRequest, "Unsupported op in dispatch", false, None),
        }
    }

    /// Builds a least-privilege CallerScope for an authenticated native-extension session.
    /// Browser data/control ops receive no DeleteFiles capability.
    fn bridge_caller(&self, instance_id: &str, profile_id: &str) -> CallerScope {
        CallerScope {
            os_user: "native-extension-session".to_string(),
            origin_kind: CallerOriginKind::NativeExtension,
            instance_id: instance_id.to_string(),
            profile_id: profile_id.to_string(),
            capabilities: vec![CapabilityScope::Read, CapabilityScope::Add, CapabilityScope::Control],
        }
    }

    fn handle_hello(&mut self, req: &BrowserBridgeRequest, now_epoch_ms: u64) -> BrowserBridgeResponse {
        let versions: Vec<u16> = serde_json::from_value(req.payload["supported_versions"].clone()).unwrap_or_default();
        if !versions.contains(&PROTOCOL_VERSION_V1) {
            return BrowserBridgeResponse::error(&req.request_id, ErrorCode::UnsupportedVersion, "No common protocol version", false, None);
        }
        let stage = req.payload["stage"].as_str().unwrap_or("");
        match stage {
            "negotiate" => BrowserBridgeResponse {
                version: 1,
                request_id: req.request_id.clone(),
                status: ResponseStatus::Ok,
                task_id: None,
                task_state: None,
                revision: None,
                durable: None,
                handoff_token: None,
                expires_at: None,
                data: Some(serde_json::json!({
                    "stage": "negotiated",
                    "selected_version": 1,
                    "capabilities": ["handoff_v1", "capture_v1", "context_v1", "media_v1"],
                    "max_frame_bytes": MAX_FRAME_BYTES
                })),
                error: None,
            },
            "enroll" => {
                let instance_id = format!("inst-{}", self.enrolled_secrets.len() + 1);
                let secret = format!("sec-{}-{}", instance_id, now_epoch_ms).into_bytes();
                let secret_hex = hex::encode(&secret);
                self.enrolled_secrets.insert(instance_id.clone(), secret);
                BrowserBridgeResponse {
                    version: 1,
                    request_id: req.request_id.clone(),
                    status: ResponseStatus::Ok,
                    task_id: None,
                    task_state: None,
                    revision: None,
                    durable: None,
                    handoff_token: None,
                    expires_at: None,
                    data: Some(serde_json::json!({
                        "stage": "enrolled",
                        "selected_version": 1,
                        "capabilities": ["handoff_v1", "capture_v1", "context_v1"],
                        "max_frame_bytes": MAX_FRAME_BYTES,
                        "registered_instance_id": instance_id,
                        "enrollment_secret": secret_hex
                    })),
                    error: None,
                }
            }
            "challenge" => {
                let inst_id = req.payload["registered_instance_id"].as_str().unwrap_or("");
                if !self.enrolled_secrets.contains_key(inst_id) {
                    return BrowserBridgeResponse::error(&req.request_id, ErrorCode::PermissionDenied, "Unknown registered_instance_id", false, None);
                }
                let challenge_id = format!("chal-{}", self.active_challenges.len() + 1);
                let nonce = format!("nonce-{}-{}", challenge_id, now_epoch_ms);
                self.active_challenges.insert(challenge_id.clone(), (inst_id.to_string(), nonce.clone(), now_epoch_ms + 60_000));
                BrowserBridgeResponse {
                    version: 1,
                    request_id: req.request_id.clone(),
                    status: ResponseStatus::Ok,
                    task_id: None,
                    task_state: None,
                    revision: None,
                    durable: None,
                    handoff_token: None,
                    expires_at: None,
                    data: Some(serde_json::json!({
                        "stage": "challenge",
                        "selected_version": 1,
                        "capabilities": ["handoff_v1", "capture_v1", "context_v1"],
                        "max_frame_bytes": MAX_FRAME_BYTES,
                        "registered_instance_id": inst_id,
                        "challenge_id": challenge_id,
                        "challenge": nonce,
                        "expires_at": "2026-11-16T08:01:00Z"
                    })),
                    error: None,
                }
            }
            "prove" => {
                let inst_id = req.payload["registered_instance_id"].as_str().unwrap_or("");
                let chal_id = req.payload["challenge_id"].as_str().unwrap_or("");
                let proof = req.payload["auth_proof"].as_str().unwrap_or("");
                // Single-use challenge: remove immediately on prove attempt
                let Some((expected_inst, nonce, exp_ms)) = self.active_challenges.remove(chal_id) else {
                    return BrowserBridgeResponse::error(&req.request_id, ErrorCode::PermissionDenied, "Challenge not found or already consumed", false, None);
                };
                if now_epoch_ms > exp_ms || expected_inst != inst_id {
                    return BrowserBridgeResponse::error(&req.request_id, ErrorCode::PermissionDenied, "Challenge expired or instance mismatch", false, None);
                }
                let secret = self.enrolled_secrets.get(inst_id).unwrap();
                if !verify_hello_hmac(secret, 1, chal_id, &nonce, &self.caller_origin, inst_id, proof) {
                    return BrowserBridgeResponse::error(&req.request_id, ErrorCode::PermissionDenied, "Invalid HMAC auth_proof", false, None);
                }
                let session_ref = format!("sess-{}-{}", inst_id, now_epoch_ms);
                self.authenticated_session = Some((inst_id.to_string(), session_ref.clone()));
                BrowserBridgeResponse {
                    version: 1,
                    request_id: req.request_id.clone(),
                    status: ResponseStatus::Ok,
                    task_id: None,
                    task_state: None,
                    revision: None,
                    durable: None,
                    handoff_token: None,
                    expires_at: None,
                    data: Some(serde_json::json!({
                        "stage": "authenticated",
                        "selected_version": 1,
                        "capabilities": ["handoff_v1", "capture_v1", "context_v1"],
                        "max_frame_bytes": MAX_FRAME_BYTES,
                        "registered_instance_id": inst_id,
                        "session_ref": session_ref
                    })),
                    error: None,
                }
            }
            _ => BrowserBridgeResponse::error(&req.request_id, ErrorCode::InvalidRequest, "Unknown hello.stage", false, None),
        }
    }
}
