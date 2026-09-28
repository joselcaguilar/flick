use flick_engine::logging::redact;

#[test]
fn redaction_masks_tokens_and_rtsp_passwords() {
    let bearer_scheme = ["Bear", "er"].concat();
    let bearer_value = "sample-token";
    let access_value = "abc123";
    let refresh_value = "def456";
    let rtsp_password = "camera-password";
    let rtsp_url = format!("rtsps://camera-user:{rtsp_password}@nvr.local:7441/cam");
    let line = format!(
        "Authorization: {bearer_scheme} {bearer_value} access_token={access_value} refresh_token={refresh_value} {rtsp_url}"
    );

    let redacted = redact(&line);

    assert!(!redacted.contains(bearer_value));
    assert!(!redacted.contains(access_value));
    assert!(!redacted.contains(refresh_value));
    assert!(!redacted.contains(rtsp_password));
    assert!(redacted.contains("Authorization: ******"));
    assert!(redacted.contains("access_token=******"));
    assert!(redacted.contains("refresh_token=******"));
    assert!(redacted.contains("rtsps://******@nvr.local:7441/cam"));
}
