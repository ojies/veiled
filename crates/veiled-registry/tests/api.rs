/// HTTP-level integration tests for the veiled-registry API.
///
/// Each test spins up an in-memory router (no network socket needed) and drives
/// it with axum's `oneshot` helper.
use axum::{body::Body, http::{Request, StatusCode}};
use http_body_util::BodyExt;
use tower::ServiceExt;

use veiled_registry::{db::Db, server::{AppState, build_router}};

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
const COMMITMENT: &str = "094e64861e58a60d432b4ea9118cde96fe4838abd848b99ef3ad234ff8abf15e";

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

    // Different commitment, same nullifier.
    let commitment2 = "1111111111111111111111111111111111111111111111111111111111111111";
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

    let pairs: &[(&str, &str)] = &[
        ("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
         "1111111111111111111111111111111111111111111111111111111111111111"),
        ("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
         "2222222222222222222222222222222222222222222222222222222222222222"),
        ("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
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
