/// HTTP-level integration tests for the veiled registry API.
///
/// Each test spins up an in-memory router (no network socket needed) and drives
/// it with axum's `oneshot` helper.
use axum::{body::Body, http::{Request, StatusCode}};
use http_body_util::BodyExt;
use tower::ServiceExt;

use veiled::registry::{db::Db, server::{AppState, build_router}};

/// Build a fresh router backed by an in-memory SQLite database.
fn test_router() -> axum::Router {
    let db = Db::open(":memory:").expect("in-memory DB");
    let store = db.load_store(8).expect("load_store");
    let state = AppState::new(store, db);
    build_router(state)
}

/// Send a request and return (status, body-as-string).
async fn send(router: axum::Router, req: Request<Body>) -> (StatusCode, String) {
    let resp = router.oneshot(req).await.expect("oneshot failed");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body collect").to_bytes();
    let body = String::from_utf8_lossy(&bytes).into_owned();
    (status, body)
}

// ─── /api/v1/sets ──────────────────────────────────────────────────────────

#[tokio::test]
async fn list_sets_initial() {
    let router = test_router();
    let req = Request::builder()
        .uri("/api/v1/sets")
        .body(Body::empty())
        .unwrap();
    let (status, body) = send(router, req).await;
    assert_eq!(status, StatusCode::OK);
    // One empty set seeded on start.
    assert!(body.contains("\"id\":0"), "body={body}");
    assert!(body.contains("\"size\":0"), "body={body}");
}

#[tokio::test]
async fn get_set_not_found() {
    let router = test_router();
    let req = Request::builder()
        .uri("/api/v1/sets/99")
        .body(Body::empty())
        .unwrap();
    let (status, _) = send(router, req).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ─── /api/v1/register ──────────────────────────────────────────────────────

fn register_body(commitment: &str, nullifier: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/v1/register")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"commitment":"{commitment}","nullifier":"{nullifier}"}}"#
        )))
        .unwrap()
}

const NULLIFIER: &str = "539e8b76cd9eb5026f5ef732e1519862ff11a24ac867c129624c5060e958789d";
// 33-byte compressed EC point (66 hex chars)
const COMMITMENT: &str = "02094e64861e58a60d432b4ea9118cde96fe4838abd848b99ef3ad234ff8abf15e";

#[tokio::test]
async fn register_success() {
    let router = test_router();
    let req = register_body(COMMITMENT, NULLIFIER);
    let (status, body) = send(router, req).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert!(body.contains("\"set_id\":0"), "body={body}");
    assert!(body.contains("\"index\":0"), "body={body}");
}

#[tokio::test]
async fn register_invalid_hex_returns_400() {
    let router = test_router();
    let req = register_body("not-hex", NULLIFIER);
    let (status, _) = send(router, req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn register_duplicate_nullifier_returns_409() {
    // Two registrations for the same nullifier must fail with 409.
    let db = Db::open(":memory:").expect("in-memory DB");
    let store = db.load_store(8).expect("load_store");
    let state = AppState::new(store, db);
    let router = build_router(state);

    let (s1, _) = send(router.clone(), register_body(COMMITMENT, NULLIFIER)).await;
    assert_eq!(s1, StatusCode::OK);

    // Different commitment (33 bytes = 66 hex chars), same nullifier.
    let commitment2 = "031111111111111111111111111111111111111111111111111111111111111111";
    let (s2, _) = send(router, register_body(commitment2, NULLIFIER)).await;
    assert_eq!(s2, StatusCode::CONFLICT);
}

#[tokio::test]
async fn register_fills_set_and_rolls_over() {
    let db = Db::open(":memory:").expect("in-memory DB");
    // Use capacity 2 so rollover happens quickly.
    let store = db.load_store(2).expect("load_store");
    let state = AppState::new(store, db);
    let router = build_router(state);

    // commitments are 33-byte compressed EC points (66 hex chars)
    let pairs: &[(&str, &str)] = &[
        ("02aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
         "1111111111111111111111111111111111111111111111111111111111111111"),
        ("02bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
         "2222222222222222222222222222222222222222222222222222222222222222"),
        ("02cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
         "3333333333333333333333333333333333333333333333333333333333333333"),
    ];

    for (cm, nul) in pairs {
        let (s, _) = send(router.clone(), register_body(cm, nul)).await;
        assert_eq!(s, StatusCode::OK);
    }

    // After 3 registrations with capacity 2: set 0 is full (2 entries),
    // set 1 has 1 entry.
    let req = Request::builder().uri("/api/v1/sets").body(Body::empty()).unwrap();
    let (_, body) = send(router, req).await;
    assert!(body.contains("\"id\":1"), "expected set 1 to exist, body={body}");
}

// ─── /api/v1/has ───────────────────────────────────────────────────────────

fn has_body(pub_key: &str, name: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/v1/has")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"pub_key":"{pub_key}","name":"{name}"}}"#
        )))
        .unwrap()
}

const PUB_KEY: &str = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";

#[tokio::test]
async fn has_not_present() {
    let router = test_router();
    let req = has_body(PUB_KEY, "alice");
    let (status, body) = send(router, req).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert!(body.contains("\"present\":false"), "body={body}");
    // The nullifier is always returned regardless of presence.
    assert!(body.contains("\"nullifier\":"), "body={body}");
}

#[tokio::test]
async fn has_present_after_register() {
    let db = Db::open(":memory:").expect("in-memory DB");
    let store = db.load_store(8).expect("load_store");
    let state = AppState::new(store, db);
    let router = build_router(state);

    // Register alice's nullifier.
    let (s, _) = send(router.clone(), register_body(COMMITMENT, NULLIFIER)).await;
    assert_eq!(s, StatusCode::OK);

    // has(PUB_KEY, "alice") — nullifier matches NULLIFIER above.
    let (status, body) = send(router, has_body(PUB_KEY, "alice")).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert!(body.contains("\"present\":true"), "body={body}");
}

#[tokio::test]
async fn has_invalid_pub_key_returns_400() {
    let router = test_router();
    let req = has_body("nothex", "alice");
    let (status, _) = send(router, req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ─── Bob/Alice payment scenario ────────────────────────────────────────────
//
// Bob wants to send Alice money.  Alice gives Bob her pub_key + name.
// Bob uses two endpoints:
//   1. POST /api/v1/has   — confirms Alice is registered (gets her nullifier)
//   2. POST /api/v1/verify — confirms Alice controls the commitment (owns blinding key)
//
// This test exercises step 1 fully and step 2 structurally (without running
// the slow ZK prove; a real proof is tested in veiled-core's proof unit tests).

#[tokio::test]
async fn bob_checks_alice_is_registered_before_payment() {
    let db = Db::open(":memory:").expect("in-memory DB");
    let store = db.load_store(8).expect("load_store");
    let state = AppState::new(store, db);
    let router = build_router(state);

    // Alice registered (commitment + nullifier already on-chain).
    let (s, _) = send(router.clone(), register_body(COMMITMENT, NULLIFIER)).await;
    assert_eq!(s, StatusCode::OK);

    // Bob: step 1 — checks Alice is registered using her pub_key + name.
    let (status, body) = send(router.clone(), has_body(PUB_KEY, "alice")).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert!(body.contains("\"present\":true"), "Alice must be registered: body={body}");

    // The /has response returns the nullifier Bob needs for the next step.
    let has_resp: serde_json::Value = serde_json::from_str(&body).unwrap();
    let nullifier_from_has = has_resp["nullifier"].as_str().unwrap();
    assert_eq!(nullifier_from_has, NULLIFIER, "nullifier from /has must match the registered one");

    // Bob: step 2 — submits a proof using the nullifier returned by /has.
    // (Dummy proof — will be valid=false, but confirms the endpoint accepts
    //  the nullifier from /has and routes correctly to set 0.)
    let dummy_proof = "00".repeat(878);
    let (status, body) = send(router, verify_body(nullifier_from_has, 0, &dummy_proof)).await;
    assert_eq!(status, StatusCode::OK, "verify must respond 200: body={body}");
    // A dummy proof fails cryptographic verification — that is expected here.
    // A real proof (generated by `veiled prove`) would return valid=true.
    assert!(body.contains("\"valid\":false"), "body={body}");
}

// ─── /api/v1/verify ────────────────────────────────────────────────────────

fn verify_body(nullifier: &str, set_id: u64, proof: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/v1/verify")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"nullifier":"{nullifier}","set_id":{set_id},"proof":"{proof}"}}"#
        )))
        .unwrap()
}

#[tokio::test]
async fn verify_invalid_hex_returns_400() {
    let router = test_router();
    let req = verify_body(NULLIFIER, 0, "not-hex");
    let (status, _) = send(router, req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn verify_wrong_proof_length_returns_400() {
    let router = test_router();
    // 877 bytes (1754 hex chars) — one byte short.
    let short_proof = "aa".repeat(877);
    let req = verify_body(NULLIFIER, 0, &short_proof);
    let (status, body) = send(router, req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
}

#[tokio::test]
async fn verify_set_not_found_returns_404() {
    let router = test_router();
    // Correct-length proof (all zeros — will fail verification, but set 99 doesn't exist).
    let dummy_proof = "00".repeat(878);
    let req = verify_body(NULLIFIER, 99, &dummy_proof);
    let (status, _) = send(router, req).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn verify_invalid_proof_returns_valid_false() {
    let db = Db::open(":memory:").expect("in-memory DB");
    let store = db.load_store(8).expect("load_store");
    let state = AppState::new(store, db);
    let router = build_router(state);

    // Register a commitment so set 0 has one member.
    let (s, _) = send(router.clone(), register_body(COMMITMENT, NULLIFIER)).await;
    assert_eq!(s, StatusCode::OK);

    // Submit an all-zeros (structurally valid but cryptographically wrong) proof.
    let dummy_proof = "00".repeat(878);
    let (status, body) = send(router, verify_body(NULLIFIER, 0, &dummy_proof)).await;
    assert_eq!(status, StatusCode::OK, "body={body}");
    assert!(body.contains("\"valid\":false"), "body={body}");
}
