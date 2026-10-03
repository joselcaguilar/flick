use axum::{
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode},
};
use flick_api::{ApiConfig, ApiState, ProblemJson, router};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn rejects_bad_token_host_and_origin() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            "Bearer wrong",
            "127.0.0.1:7871",
            "http://localhost:5173",
            StatusCode::UNAUTHORIZED,
        ),
        (
            "Bearer dev-token",
            "evil.example",
            "http://localhost:5173",
            StatusCode::FORBIDDEN,
        ),
        (
            "Bearer dev-token",
            "127.0.0.1:7871",
            "http://evil.example",
            StatusCode::FORBIDDEN,
        ),
    ];

    for (token, host, origin, expected) in cases {
        let response = send(request(
            "GET",
            "/api/v1/status",
            token,
            host,
            origin,
            Body::empty(),
        )?)
        .await;
        assert_eq!(response.status(), expected);
    }

    Ok(())
}

#[tokio::test]
async fn mapping_validation_returns_stable_codes() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            json!({
                "name": "Global verb",
                "gesture_id": "builtin.thumb_up",
                "target_mode": "global",
                "action": {"kind": "verb", "verb": "on"}
            }),
            "verb_requires_target",
        ),
        (
            json!({
                "name": "Untaught fan level",
                "gesture_id": "builtin.circle_cw",
                "target_mode": "anchor",
                "anchor_id": "anchor-1",
                "action": {"kind": "verb", "verb": "level_set", "level": 1}
            }),
            "level_not_taught",
        ),
    ];

    for (payload, code) in cases {
        let response = send(json_request("/api/v1/mappings", payload)?).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let problem = problem(response).await?;
        assert_eq!(problem.code, code);
    }

    Ok(())
}

#[tokio::test]
async fn mapping_gesture_patch_rejects_conflicts() -> Result<(), Box<dyn std::error::Error>> {
    let app = router(ApiState::fake(ApiConfig::dev()));
    let mut ids = Vec::new();
    for (gesture, verb) in [("builtin.thumb_up", "on"), ("builtin.thumb_down", "off")] {
        let payload = json!({
            "name": format!("Lamp {verb}"),
            "gesture_id": gesture,
            "target_mode": "anchor",
            "anchor_id": "anchor-1",
            "action": {"kind": "verb", "verb": verb}
        });
        let response = call(&app, json_request("/api/v1/mappings", payload)?).await;
        assert_eq!(response.status(), StatusCode::OK);
        let id = body_json(response).await?["id"]
            .as_str()
            .ok_or("missing id")?
            .to_owned();
        ids.push(id);
    }
    let uri = format!("/api/v1/mappings/{}", ids[1]);

    for (gesture, code) in [
        ("builtin.thumb_up", "gesture_conflict"),
        ("  ", "bad_gesture_id"),
    ] {
        let patch = json!({"gesture_id": gesture});
        let response = call(&app, json_request_with("PATCH", &uri, patch)?).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(problem(response).await?.code, code);
    }

    let patch = json!({"gesture_id": "builtin.circle_cw"});
    let response = call(&app, json_request_with("PATCH", &uri, patch)?).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await?["gesture_id"],
        "builtin.circle_cw"
    );
    Ok(())
}

#[tokio::test]
async fn anchor_area_override_patch_validates_and_clears() -> Result<(), Box<dyn std::error::Error>>
{
    let anchor = serde_json::from_value(json!({
        "id": "anchor-lamp",
        "place_id": "place-office",
        "name": "Flexo",
        "target": {"entity_id": "light.flexo"},
        "domain": "light",
        "kind": "direction",
        "verb_params": {},
        "sensitive": false,
        "sensitive_ack": false,
        "status": "ok",
        "verbs": [],
        "last_used_at": null,
        "created_at": "2026-09-28T10:00:00Z",
        "updated_at": "2026-09-28T10:00:00Z"
    }))?;
    let state = ApiState::fake(ApiConfig::dev());
    state.replace_anchors(vec![anchor]);
    let app = router(state);
    let uri = "/api/v1/anchors/anchor-lamp";

    for area in ["   ", "a\u{7}b"] {
        let patch = json!({"area_override": area});
        let response = call(&app, json_request_with("PATCH", uri, patch)?).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(problem(response).await?.code, "bad_area_id");
    }

    for (patch, expected) in [
        (json!({"area_override": "  office "}), json!("office")),
        (json!({"name": "Desk lamp"}), json!("office")),
        (json!({"area_override": null}), serde_json::Value::Null),
    ] {
        let response = call(&app, json_request_with("PATCH", uri, patch)?).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await?["area_override"], expected);
    }
    Ok(())
}

#[tokio::test]
async fn client_certificate_upload_rejects_unreadable_data()
-> Result<(), Box<dyn std::error::Error>> {
    let request = json_request_with(
        "PUT",
        "/api/v1/ha/client-certificate",
        json!({"data": "not base64!"}),
    )?;
    let response = send(request).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem(response).await?.code, "ha_cert_invalid");
    Ok(())
}

#[tokio::test]
async fn activity_query_validation_returns_stable_codes() -> Result<(), Box<dyn std::error::Error>>
{
    let cases = [
        ("/api/v1/activity?limit=0", "invalid_limit"),
        ("/api/v1/activity?limit=201", "invalid_limit"),
        ("/api/v1/activity?status=bogus", "invalid_status"),
    ];
    for (uri, code) in cases {
        let response = send(json_request_with("GET", uri, serde_json::Value::Null)?).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(problem(response).await?.code, code);
    }

    let ok = "/api/v1/activity?status=ok&include_suppressed=true";
    let response = send(json_request_with("GET", ok, serde_json::Value::Null)?).await;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(())
}

async fn send(request: Request<Body>) -> Response<Body> {
    call(&router(ApiState::fake(ApiConfig::dev())), request).await
}

async fn call(app: &axum::Router, request: Request<Body>) -> Response<Body> {
    match app.clone().oneshot(request).await {
        Ok(response) => response,
        Err(err) => match err {},
    }
}

fn request(
    method: &str,
    uri: &str,
    token: &str,
    host: &str,
    origin: &str,
    body: Body,
) -> Result<Request<Body>, axum::http::Error> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("host", host)
        .header("origin", origin)
        .header("authorization", token)
        .body(body)
}

fn json_request(
    uri: &str,
    payload: serde_json::Value,
) -> Result<Request<Body>, Box<dyn std::error::Error>> {
    json_request_with("POST", uri, payload)
}

fn json_request_with(
    method: &str,
    uri: &str,
    payload: serde_json::Value,
) -> Result<Request<Body>, Box<dyn std::error::Error>> {
    let body = Body::from(serde_json::to_vec(&payload)?);
    Ok(Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:7871")
        .header("origin", "http://localhost:5173")
        .header("authorization", "Bearer dev-token")
        .header("content-type", "application/json")
        .body(body)?)
}

async fn problem(response: Response<Body>) -> Result<ProblemJson, Box<dyn std::error::Error>> {
    let bytes = to_bytes(response.into_body(), 1_048_576).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn body_json(
    response: Response<Body>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let bytes = to_bytes(response.into_body(), 1_048_576).await?;
    Ok(serde_json::from_slice(&bytes)?)
}
