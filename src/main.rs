// src/main.rs
mod canon;
mod types;

use axum::{
    routing::{get, post},
    Json, Router,
};
use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD as B64, engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use rand::RngCore;
use serde_json::json;
use sqlx::{PgPool, Row};
use std::{net::{IpAddr, SocketAddr, ToSocketAddrs}, time::Duration as StdDuration};
use tokio::net::TcpListener;
use tower_http::{cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer};
use tracing::{error, info};
use types::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt().with_env_filter("info").init();

    let db = PgPool::connect(&std::env::var("DATABASE_URL")?).await?;

    let db_clone = db.clone();
    tokio::spawn(async move { health_worker(db_clone).await });

    let app = Router::new()
        .route("/api/registry/challenge", post(challenge))
        .route("/api/registry/register", post(register))
        .route("/api/registry/heartbeat", post(heartbeat))
        .route("/api/nodes", get(list_nodes))
        .with_state(db)
        .layer(CorsLayer::permissive())
        .layer(TimeoutLayer::new(StdDuration::from_secs(10)))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = SocketAddr::new("0.0.0.0".parse().unwrap(), 8080);
    info!("registry listening on {addr}");
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// ---------- API HANDLERS ---------- //

async fn challenge(
    axum::extract::State(db): axum::extract::State<PgPool>,
    Json(req): Json<ChallengeReq>,
) -> Result<Json<ChallengeRes>, (StatusCode, String)> {
    if req.pubkey_b64.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "pubkey_b64 required".into()));
    }

    let nonce = gen_nonce();
    let exp: DateTime<Utc> = Utc::now() + Duration::minutes(5);

    sqlx::query("INSERT INTO challenges (nonce, pubkey_b64, expires_at) VALUES ($1,$2,$3)")
        .bind(&nonce)
        .bind(&req.pubkey_b64)
        .bind(exp)
        .execute(&db)
        .await
        .map_err(internal)?;

    Ok(Json(ChallengeRes {
        nonce,
        expires_at: exp.to_rfc3339(),
    }))
}

async fn register(
    axum::extract::State(db): axum::extract::State<PgPool>,
    Json(req): Json<RegisterReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use core::convert::TryFrom;

    let row = sqlx::query("SELECT pubkey_b64, expires_at FROM challenges WHERE nonce=$1")
        .bind(&req.nonce)
        .fetch_optional(&db)
        .await
        .map_err(internal)?;
    let Some(row) = row else {
        return Err((StatusCode::BAD_REQUEST, "invalid/expired nonce".into()));
    };
    let chall_pub: String = row.get("pubkey_b64");
    let chall_exp: DateTime<Utc> = row.get("expires_at");
    if chall_exp < Utc::now() {
        return Err((StatusCode::BAD_REQUEST, "expired nonce".into()));
    }
    if chall_pub != req.pubkey_b64 {
        return Err((StatusCode::BAD_REQUEST, "pubkey mismatch".into()));
    }

    let canon = canon::canonical_json_string(&req.payload);
    let message = [canon.as_bytes(), req.nonce.as_bytes()].concat();

    let sig_bytes = B64.decode(&req.signature_b64).map_err(badreq)?;
    let sig = Signature::from_slice(&sig_bytes)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid signature: {e}")))?;
    let vk_bytes = B64.decode(&req.pubkey_b64).map_err(badreq)?;
    let vk = VerifyingKey::try_from(&vk_bytes[..])
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid pubkey: {e}")))?;
    vk.verify(&message, &sig)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "bad signature".into()))?;

    let name = req.payload.get("name").and_then(|v| v.as_str()).ok_or(bad("name"))?;
    let host = req.payload.get("host").and_then(|v| v.as_str()).ok_or(bad("host"))?;
    let api = req
        .payload
        .get("api_base_url")
        .and_then(|v| v.as_str())
        .ok_or(bad("api_base_url"))?;
    let proto = req
        .payload
        .get("protocol_version")
        .and_then(|v| v.as_str())
        .ok_or(bad("protocol_version"))?;
    let features = req.payload.get("features").cloned().unwrap_or(json!({}));
    let email = req
        .payload
        .get("contact_email")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let ip = resolve_ip(host).await.ok();
    println!("Resolved IP for host {host}: {:?}", ip);
    let ip_parsed: IpAddr = match ip {
        Some(ref ip_str) => ip_str.parse().map_err(|_| bad("could not parse resolved IP"))?,
        None => return Err(bad("could not resolve host")),
    };

    if let Some(row) = sqlx::query("SELECT pubkey FROM nodes WHERE host=$1")
        .bind(host)
        .fetch_optional(&db)
        .await
        .map_err(internal)?
    {
        let existing_pubkey: Vec<u8> = row.get("pubkey");
        let new_pubkey = B64.decode(&req.pubkey_b64).map_err(badreq)?;
        if existing_pubkey != new_pubkey {
            return Err((StatusCode::FORBIDDEN, "host already registered with another key".into()));
        }
    }
    sqlx::query(
        r#"
        INSERT INTO nodes (name, host, ip, api_base_url, pubkey, protocol_version, features, contact_email, status)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'unknown')
        ON CONFLICT(host) DO UPDATE
          SET name=EXCLUDED.name,
              ip=EXCLUDED.ip,
              api_base_url=EXCLUDED.api_base_url,
              pubkey=EXCLUDED.pubkey,
              protocol_version=EXCLUDED.protocol_version,
              features=EXCLUDED.features,
              contact_email=EXCLUDED.contact_email
        "#,
    )
    .bind(name)
    .bind(host)
    .bind(ip_parsed)
    .bind(api)
    .bind(B64.decode(&req.pubkey_b64).map_err(badreq)?)
    .bind(proto)
    .bind(features)
    .bind(email)
    .execute(&db)
    .await
    .map_err(internal)?;

    sqlx::query("DELETE FROM challenges WHERE nonce=$1")
        .bind(&req.nonce)
        .execute(&db)
        .await
        .ok();

    Ok(Json(json!({"ok": true})))
}

async fn heartbeat(
    axum::extract::State(db): axum::extract::State<PgPool>,
    Json(req): Json<HeartbeatReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use core::convert::TryFrom;

    let message = [req.host.as_bytes(), req.nonce.as_bytes()].concat();

    let sig_bytes = B64.decode(&req.signature_b64).map_err(badreq)?;
    let sig = Signature::from_slice(&sig_bytes)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid signature: {e}")))?;
    let vk_bytes = B64.decode(&req.pubkey_b64).map_err(badreq)?;
    let vk = VerifyingKey::try_from(&vk_bytes[..])
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid pubkey: {e}")))?;

    vk.verify(&message, &sig)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "bad signature".into()))?;

    let now = Utc::now();
    sqlx::query("UPDATE nodes SET last_seen_at=$1, status='online' WHERE host=$2")
        .bind(now)
        .bind(&req.host)
        .execute(&db)
        .await
        .map_err(internal)?;

    Ok(Json(json!({"ok": true})))
}

async fn list_nodes(
    axum::extract::State(db): axum::extract::State<PgPool>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let rows = sqlx::query(
        "SELECT name, host, ip::text AS ip, api_base_url, protocol_version, features,
                country_code, country_name, last_seen_at, last_latency_ms, status
         FROM nodes
         ORDER BY status DESC, name ASC",
    )
    .fetch_all(&db)
    .await
    .map_err(internal)?;

    let nodes: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "name": r.get::<String,_>("name"),
                "host": r.get::<String,_>("host"),
                "ip": r.get::<Option<String>,_>("ip"),
                "api_base_url": r.get::<String,_>("api_base_url"),
                "protocol_version": r.get::<String,_>("protocol_version"),
                "features": r.get::<serde_json::Value,_>("features"),
                "country_code": r.get::<Option<String>,_>("country_code"),
                "country_name": r.get::<Option<String>,_>("country_name"),
                "last_seen_at": r.get::<Option<DateTime<Utc>>,_>("last_seen_at"),
                "last_latency_ms": r.get::<Option<i32>,_>("last_latency_ms"),
                "status": r.get::<String,_>("status"),
            })
        })
        .collect();

    Ok(Json(json!({ "nodes": nodes })))
}


fn gen_nonce() -> String {
    let mut b = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}

fn bad(s: &'static str) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, format!("missing/invalid {}", s))
}
fn badreq<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}
fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    error!("{e}");
    (StatusCode::INTERNAL_SERVER_ERROR, "internal".into())
}

async fn resolve_ip(host: &str) -> anyhow::Result<String> {
    let addr = format!("{host}:0");
    let ip = addr
        .to_socket_addrs()?
        .next()
        .ok_or(anyhow::anyhow!("no dns"))?
        .ip();
    Ok(ip.to_string())
}


async fn health_worker(db: PgPool) {
    let client = reqwest::Client::new();
    let timeout_ms: u64 = std::env::var("HEALTH_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    loop {
        if let Err(e) = tick_health(&db, &client, timeout_ms).await {
            error!("health tick error: {e}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
}

async fn tick_health(
    db: &PgPool,
    client: &reqwest::Client,
    timeout_ms: u64,
) -> anyhow::Result<()> {
    let nodes = sqlx::query("SELECT host, api_base_url, ip::text FROM nodes")
        .fetch_all(db)
        .await?;

    for row in nodes {
        let host: String = row.get("host");
        let api: String = row.get("api_base_url");
        let ip: Option<String> = row.get("ip");
        println!("Checking health for node {host} at {api}");
        // Measure latency
        let start = std::time::Instant::now();
        let res = client
            .get(format!("{api}/health"))
            .timeout(StdDuration::from_millis(timeout_ms))
            .send()
            .await;

        let (status, latency) = match res {
            Ok(r) if r.status().is_success() => ("online", Some(start.elapsed().as_millis() as i32)),
            _ => ("offline", None),
        };

        // GeoIP
        let (cc, cn) = if let Some(ref ip) = ip {
            match client
                .get(
                    std::env::var("GEOIP_URL")
                        .unwrap_or("https://ipapi.co/{ip}/json/".into())
                        .replace("{ip}", ip),
                )
                .timeout(StdDuration::from_secs(3))
                .send()
                .await
            {
                Ok(r) => {
                    let j = r.json::<serde_json::Value>().await.unwrap_or_default();
                    (
                        j.get("country").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        j.get("country_name").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    )
                }
                Err(_) => (None, None),
            }
        } else {
            (None, None)
        };

        sqlx::query(
            r#"
            UPDATE nodes
            SET status=$1,
                last_latency_ms=$2,
                last_seen_at = CASE WHEN $1='online' THEN now() ELSE last_seen_at END,
                country_code = COALESCE($3, country_code),
                country_name = COALESCE($4, country_name)
            WHERE host=$5
            "#,
        )
        .bind(status)
        .bind(latency)
        .bind(cc)
        .bind(cn)
        .bind(&host)
        .execute(db)
        .await?;
    }
    Ok(())
}
