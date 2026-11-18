use mdm_app::{browser_bridge, data_dir};
use mdm_ipc::*;
use rustcatch_nmhost::NativeHostSession;
use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Default)]
struct HostConnection {
    negotiated: bool,
    profile_key: Option<[u8; 32]>,
    instance_id: String,
    challenge: Option<(String, String, Instant)>,
    authenticated: bool,
}

impl HostConnection {
    fn handle(&mut self, root: &Path, origin: &str, bytes: &[u8]) -> BrowserBridgeResponse {
        let Some(request) = validate_raw_frame(bytes)
            .ok()
            .and_then(|value| serde_json::from_value::<BrowserBridgeRequest>(value).ok())
        else {
            return fail(
                "invalid-frame",
                ErrorCode::InvalidRequest,
                "无效的浏览器请求",
            );
        };
        if request.request_id.is_empty() || request.request_id.len() > 128 {
            return fail(
                "invalid-request",
                ErrorCode::InvalidRequest,
                "无效的请求标识",
            );
        }
        let id = request.request_id.clone();
        if !browser_bridge::configured_origin(root, origin) {
            return fail(&id, ErrorCode::PermissionDenied, "扩展来源未注册");
        }
        if request.version != PROTOCOL_VERSION_V1 {
            return fail(&id, ErrorCode::UnsupportedVersion, "协议版本不兼容");
        }
        if request.op != "hello" {
            if self.authenticated {
                return browser_bridge::forward(root, origin, &self.instance_id, request)
                    .unwrap_or_else(|_| {
                        fail(
                            &id,
                            ErrorCode::ToolUnavailable,
                            "RustCatch 桌面下载器未运行或未响应",
                        )
                    });
            }
            return fail(&id, ErrorCode::PermissionDenied, "扩展须先完成身份验证");
        }
        self.hello(origin, &request)
    }

    fn hello(&mut self, origin: &str, request: &BrowserBridgeRequest) -> BrowserBridgeResponse {
        let id = &request.request_id;
        let payload = &request.payload;
        let instance_matches =
            payload["registered_instance_id"].as_str() == Some(&self.instance_id);
        let rejected = || fail(id, ErrorCode::PermissionDenied, "扩展身份验证失败");
        let mut data = match payload["stage"].as_str() {
            Some("negotiate")
                if !self.negotiated
                    && payload["supported_versions"]
                        .as_array()
                        .is_some_and(|versions| versions.iter().any(|v| v.as_u64() == Some(1))) =>
            {
                self.negotiated = true;
                serde_json::json!({"stage": "negotiated"})
            }
            Some("enroll") if self.negotiated && self.profile_key.is_none() => {
                let Some(key) = payload["installation_key"].as_str().and_then(|raw| {
                    if raw.len() != 64 {
                        return None;
                    }
                    hex::decode(raw).ok()?.try_into().ok()
                }) else {
                    return rejected();
                };
                let mut digest = Sha256::new();
                digest.update(origin.as_bytes());
                digest.update(key);
                self.instance_id = hex::encode(digest.finalize());
                self.profile_key = Some(key);
                serde_json::json!({"stage": "enrolled", "registered_instance_id": self.instance_id})
            }
            Some("challenge")
                if self.profile_key.is_some() && instance_matches && !self.authenticated =>
            {
                let challenge = Uuid::new_v4().to_string();
                let nonce = Uuid::new_v4().simple().to_string();
                self.challenge = Some((challenge.clone(), nonce.clone(), Instant::now()));
                serde_json::json!({
                    "stage": "challenge", "registered_instance_id": self.instance_id,
                    "challenge_id": challenge, "challenge": nonce
                })
            }
            Some("prove") if instance_matches && !self.authenticated => {
                let Some((challenge, nonce, issued)) = self.challenge.take() else {
                    return rejected();
                };
                let Some(key) = self.profile_key.take() else {
                    return rejected();
                };
                let proof = payload["auth_proof"].as_str().unwrap_or("");
                if proof.len() != 64
                    || issued.elapsed() > Duration::from_secs(60)
                    || payload["challenge_id"].as_str() != Some(&challenge)
                    || !verify_hello_hmac(
                        &key,
                        1,
                        &challenge,
                        &nonce,
                        origin,
                        &self.instance_id,
                        proof,
                    )
                {
                    return rejected();
                }
                self.authenticated = true;
                serde_json::json!({"stage": "authenticated", "session_ref": Uuid::new_v4().to_string(), "registered_instance_id": self.instance_id})
            }
            _ => return rejected(),
        };
        data["selected_version"] = 1.into();
        data["capabilities"] = serde_json::json!([
            "task.add",
            "handoff.prepare",
            "handoff.status",
            "handoff.commit",
            "handoff.abort"
        ]);
        data["max_frame_bytes"] = MAX_FRAME_BYTES.into();
        BrowserBridgeResponse {
            version: 1,
            request_id: id.to_owned(),
            status: ResponseStatus::Ok,
            task_id: None,
            task_state: None,
            revision: None,
            durable: None,
            handoff_token: None,
            expires_at: None,
            data: Some(data),
            error: None,
        }
    }
}

fn fail(request_id: &str, code: ErrorCode, message: &str) -> BrowserBridgeResponse {
    BrowserBridgeResponse::error(request_id, code, message, false, None)
}

fn main() -> io::Result<()> {
    let root = data_dir().ok_or_else(|| io::Error::other("RustCatch data folder unavailable"))?;
    let mut origin = std::env::args().nth(1).unwrap_or_default();
    if !origin.ends_with('/') {
        origin.push('/');
    }
    let mut reader = io::stdin().lock();
    let mut writer = io::stdout().lock();
    let mut session = HostConnection::default();
    while let Some(frame) = NativeHostSession::read_native_frame(&mut reader)? {
        match frame {
            Ok(data) => NativeHostSession::write_native_frame(
                &mut writer,
                &session.handle(&root, &origin, &data),
            )?,
            Err(error) => {
                NativeHostSession::write_native_frame(
                    &mut writer,
                    &fail("invalid-frame", error.code, "无效的消息长度"),
                )?;
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod live_tests {
    use super::*;

    fn hello(stage: &str, fields: serde_json::Value) -> BrowserBridgeRequest {
        let mut payload = fields;
        payload["stage"] = stage.into();
        BrowserBridgeRequest {
            version: 1,
            request_id: stage.into(),
            idempotency_key: stage.into(),
            op: "hello".into(),
            payload,
        }
    }

    #[test]
    fn proves_profile_ownership_per_connection() {
        let origin = "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/";
        let mut live = HostConnection::default();
        let result = live.hello(
            origin,
            &hello("negotiate", serde_json::json!({"supported_versions": [1]})),
        );
        assert_eq!(result.status, ResponseStatus::Ok);
        let key = [5u8; 32];
        let result = live.hello(
            origin,
            &hello(
                "enroll",
                serde_json::json!({"installation_key": hex::encode(key)}),
            ),
        );
        let instance = result.data.unwrap()["registered_instance_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let result = live.hello(
            origin,
            &hello(
                "challenge",
                serde_json::json!({"registered_instance_id": instance}),
            ),
        );
        let data = result.data.unwrap();
        let challenge = data["challenge_id"].as_str().unwrap();
        let nonce = data["challenge"].as_str().unwrap();
        let proof = compute_hello_hmac(&key, 1, challenge, nonce, origin, &instance);
        let prove = hello(
            "prove",
            serde_json::json!({
                "registered_instance_id": instance, "challenge_id": challenge, "auth_proof": proof
            }),
        );
        assert_eq!(live.hello(origin, &prove).status, ResponseStatus::Ok);
        assert_eq!(live.hello(origin, &prove).status, ResponseStatus::Error);
        assert_eq!(
            HostConnection::default().hello(origin, &prove).status,
            ResponseStatus::Error
        );
    }
}
