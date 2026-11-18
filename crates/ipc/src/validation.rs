use crate::task_api::*;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Deserializer, de};
use serde_json::Value;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Validates raw UTF-8 JSON bytes against 256 KiB limit, max depth 16, and duplicate keys.
pub fn validate_raw_frame(raw: &[u8]) -> Result<serde_json::Value, ErrorDetail> {
    if raw.len() > MAX_FRAME_BYTES {
        return Err(ErrorDetail {
            code: ErrorCode::TooLarge,
            message: format!("Frame size {} exceeds maximum {} bytes", raw.len(), MAX_FRAME_BYTES),
            retryable: false,
            retry_after_ms: None,
        });
    }
    let text = std::str::from_utf8(raw).map_err(|_| ErrorDetail {
        code: ErrorCode::InvalidRequest,
        message: "Frame is not valid UTF-8".to_string(),
        retryable: false,
        retry_after_ms: None,
    })?;

    check_depth(text)?;

    serde_json::from_str::<UniqueJson>(text).map(|value| value.0).map_err(|e| ErrorDetail {
        code: ErrorCode::InvalidRequest,
        message: format!("Malformed JSON: {}", e),
        retryable: false,
        retry_after_ms: None,
    })
}

/// Checks nesting depth <= 16 and rejects duplicate object keys at the same level.
pub fn check_depth_and_duplicate_keys(json_str: &str) -> Result<(), ErrorDetail> {
    validate_raw_frame(json_str.as_bytes()).map(|_| ())
}

fn check_depth(json_str: &str) -> Result<(), ErrorDetail> {
    let bytes = json_str.as_bytes();
    let mut i = 0;
    let mut depth: usize = 0;
    let mut in_string = false;
    let mut escape = false;

    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else {
            match b {
                b'"' => in_string = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > MAX_JSON_DEPTH {
                        return Err(ErrorDetail {
                            code: ErrorCode::InvalidRequest,
                            message: format!("JSON nesting depth exceeds {}", MAX_JSON_DEPTH),
                            retryable: false,
                            retry_after_ms: None,
                        });
                    }
                }
                b'}' | b']' => {
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        i += 1;
    }
    Ok(())
}

struct UniqueJson(Value);

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = UniqueJson;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut entries: A) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((name, value)) = entries.next_entry::<String, UniqueJson>()? {
                    if values.insert(name, value.0).is_some() {
                        return Err(de::Error::custom("duplicate JSON key"));
                    }
                }
                Ok(UniqueJson(Value::Object(values)))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut entries: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = entries.next_element::<UniqueJson>()? {
                    values.push(value.0);
                }
                Ok(UniqueJson(Value::Array(values)))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Null))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

/// Validates HTTP header value against CR/LF/NUL injection and 8192-byte limit.
pub fn validate_header_value(name: &str, value: &str) -> Result<(), ErrorDetail> {
    const ALLOWED_HEADERS: &[&str] = &["User-Agent", "Referer", "Origin", "Authorization"];
    if !ALLOWED_HEADERS.contains(&name) {
        return Err(ErrorDetail {
            code: ErrorCode::InvalidRequest,
            message: format!("Disallowed context header: {}", name),
            retryable: false,
            retry_after_ms: None,
        });
    }
    if value.len() > 8192 || value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
        return Err(ErrorDetail {
            code: ErrorCode::InvalidRequest,
            message: format!("Invalid characters or length in header {}", name),
            retryable: false,
            retry_after_ms: None,
        });
    }
    Ok(())
}

/// Sanitizes output file names against Windows reserved names, ADS (:), control chars, and traversal.
pub fn sanitize_file_name(hint: &str) -> Result<String, ErrorDetail> {
    let trimmed = hint.trim().trim_matches('.');
    if trimmed.is_empty() || trimmed.len() > 240 {
        return Err(ErrorDetail {
            code: ErrorCode::InvalidRequest,
            message: "Invalid file name hint length".to_string(),
            retryable: false,
            retry_after_ms: None,
        });
    }
    if trimmed.contains("..") || trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains(':') {
        return Err(ErrorDetail {
            code: ErrorCode::InvalidRequest,
            message: "Path traversal or ADS colon in file name hint".to_string(),
            retryable: false,
            retry_after_ms: None,
        });
    }
    if trimmed.bytes().any(|b| b < 0x20) {
        return Err(ErrorDetail {
            code: ErrorCode::InvalidRequest,
            message: "Control characters in file name hint".to_string(),
            retryable: false,
            retry_after_ms: None,
        });
    }
    let stem = trimmed.split('.').next().unwrap_or(trimmed).to_ascii_uppercase();
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL",
        "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        return Ok(format!("_{}", trimmed));
    }
    Ok(trimmed.to_string())
}

/// Computes canonical HMAC-SHA256 proof for Native Host 4-stage hello handshake.
pub fn compute_hello_hmac(
    secret: &[u8],
    version: u16,
    challenge_id: &str,
    nonce: &str,
    native_origin: &str,
    instance_id: &str,
) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key size");
    let canonical = format!(
        "rustcatch-hello-v{}:{}:{}:{}:{}",
        version, challenge_id, nonce, native_origin, instance_id
    );
    mac.update(canonical.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Verifies HMAC-SHA256 proof in constant time.
pub fn verify_hello_hmac(
    secret: &[u8],
    version: u16,
    challenge_id: &str,
    nonce: &str,
    native_origin: &str,
    instance_id: &str,
    proof_hex: &str,
) -> bool {
    let Ok(proof_bytes) = hex::decode(proof_hex) else {
        return false;
    };
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key size");
    let canonical = format!(
        "rustcatch-hello-v{}:{}:{}:{}:{}",
        version, challenge_id, nonce, native_origin, instance_id
    );
    mac.update(canonical.as_bytes());
    mac.verify_slice(&proof_bytes).is_ok()
}

/// Redacts sensitive query parameters, tokens, and credentials from URLs and diagnostic strings.
pub fn redact_sensitive_url(raw_url: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(raw_url) else {
        return "[invalid-url]".to_string();
    };
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    let sensitive_keys = ["sig", "signature", "token", "expire", "key", "auth", "hdnea", "policy"];
    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| {
            if sensitive_keys.iter().any(|sk| k.eq_ignore_ascii_case(sk)) {
                (k.into_owned(), "REDACTED".to_string())
            } else {
                (k.into_owned(), v.into_owned())
            }
        })
        .collect();
    if !pairs.is_empty() {
        parsed.query_pairs_mut().clear().extend_pairs(pairs);
    }
    parsed.to_string()
}

/// Computes deterministic semantic digest for idempotency conflict detection.
pub fn compute_creation_digest(source_json: &serde_json::Value, output_json: &serde_json::Value, media_intent_json: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_string(source_json).unwrap_or_default().as_bytes());
    hasher.update(b"|");
    hasher.update(serde_json::to_string(output_json).unwrap_or_default().as_bytes());
    hasher.update(b"|");
    hasher.update(serde_json::to_string(media_intent_json).unwrap_or_default().as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decimal_u64_bounds() {
        let max_str = "\"18446744073709551615\"";
        let parsed: DecimalU64 = serde_json::from_str(max_str).unwrap();
        assert_eq!(parsed.0, u64::MAX);

        let overflow_str = "\"18446744073709551616\"";
        assert!(serde_json::from_str::<DecimalU64>(overflow_str).is_err());

        let leading_zero = "\"01\"";
        assert!(serde_json::from_str::<DecimalU64>(leading_zero).is_err());
    }

    #[test]
    fn test_frame_size_and_depth() {
        let oversized = vec![b' '; MAX_FRAME_BYTES + 1];
        assert_eq!(validate_raw_frame(&oversized).unwrap_err().code, ErrorCode::TooLarge);

        let deep_json = "{".repeat(17) + &"}".repeat(17);
        assert_eq!(validate_raw_frame(deep_json.as_bytes()).unwrap_err().code, ErrorCode::InvalidRequest);
    }

    #[test]
    fn test_hmac_handshake_verification() {
        let secret = b"0123456789abcdef0123456789abcdef";
        let proof = compute_hello_hmac(secret, 1, "chal-1", "nonce-abc", "chrome-extension://ext123", "inst-1");
        assert!(verify_hello_hmac(secret, 1, "chal-1", "nonce-abc", "chrome-extension://ext123", "inst-1", &proof));
        assert!(!verify_hello_hmac(secret, 1, "chal-1", "nonce-abc", "chrome-extension://other", "inst-1", &proof));
    }

    #[test]
    fn test_filename_sanitization_and_redaction() {
        assert!(sanitize_file_name("../secret.txt").is_err());
        assert!(sanitize_file_name("file.txt:ads").is_err());
        assert_eq!(sanitize_file_name("CON.txt").unwrap(), "_CON.txt");
        let redacted = redact_sensitive_url("https://video.example.invalid/stream.mp4?id=10&sig=SECRET123&token=TOK");
        assert!(redacted.contains("sig=REDACTED"));
        assert!(!redacted.contains("SECRET123"));
    }
}
    #[test]
    fn duplicate_json_keys_are_rejected_at_every_depth() {
        assert!(validate_raw_frame(br#"{"op":"hello","op":"task.add"}"#).is_err());
        assert!(validate_raw_frame(br#"{"payload":{"id":1,"\u0069d":2}}"#).is_err());
        assert!(validate_raw_frame(br#"{"a":{"id":1},"b":{"id":2}}"#).is_ok());
    }
