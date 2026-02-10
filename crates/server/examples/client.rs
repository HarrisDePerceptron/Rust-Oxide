use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode};
use rust_oxide::routes::API_PREFIX;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct RegisterRequest {
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_in: usize,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    status: u16,
    message: String,
    data: T,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Defaults assume your axum server is running locally on :3000
    let base = std::env::var("BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".to_string());
    let api_base = format!("{base}{API_PREFIX}");
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "user@example.com".to_string());
    let pass = std::env::var("PASSWORD").unwrap_or_else(|_| "password123".to_string());
    let admin = std::env::var("ADMIN_EMAIL").unwrap_or_else(|_| "admin@example.com".to_string());
    let admin_pass =
        std::env::var("ADMIN_PASSWORD").unwrap_or_else(|_| "adminpassword".to_string());

    let http = Client::new();

    // 1) /api/v1/public
    call_get(&http, &format!("{api_base}/public"), None).await?;

    // 2) /api/v1/register (ignore 409)
    let _ = register(&http, &format!("{api_base}/register"), &user, &pass).await;

    // 3) /api/v1/login -> token pair
    let tokens = login(&http, &format!("{api_base}/login"), &user, &pass).await?;
    println!(
        "\nGot {} access token ({}s, first 24 chars): {}…",
        tokens.token_type,
        tokens.expires_in,
        tokens.access_token.chars().take(24).collect::<String>()
    );

    // 4) /api/v1/me (JWT protected)
    call_get(&http, &format!("{api_base}/me"), Some(&tokens.access_token)).await?;

    // 5) /api/v1/refresh -> new access token
    let refreshed = refresh(&http, &format!("{api_base}/refresh"), &tokens.refresh_token).await?;
    println!(
        "Refreshed {} access token ({}s, first 24 chars): {}…",
        refreshed.token_type,
        refreshed.expires_in,
        refreshed.access_token.chars().take(24).collect::<String>()
    );

    // 6) Admin flow (login as seeded admin and hit /api/v1/admin/stats)
    println!("\n==> Admin flow");
    let admin_tokens = login(&http, &format!("{api_base}/login"), &admin, &admin_pass).await?;
    call_get(
        &http,
        &format!("{api_base}/admin/stats"),
        Some(&admin_tokens.access_token),
    )
    .await?;

    Ok(())
}

async fn register(http: &Client, url: &str, email: &str, password: &str) -> Result<()> {
    println!("\n==> POST {url} (register)");

    let resp = http
        .post(url)
        .json(&RegisterRequest {
            email: email.to_string(),
            password: password.to_string(),
        })
        .send()
        .await
        .context("register request failed")?;

    println!("Status: {}", resp.status());
    if resp.status() == StatusCode::CONFLICT {
        println!("User already exists; continuing");
    }
    Ok(())
}

async fn login(http: &Client, url: &str, email: &str, password: &str) -> Result<TokenResponse> {
    println!("\n==> POST {url}");

    let resp = http
        .post(url)
        .json(&LoginRequest {
            email: email.to_string(),
            password: password.to_string(),
        })
        .send()
        .await
        .context("login request failed")?;

    let status = resp.status();
    let text = resp.text().await.context("reading login body failed")?;

    println!("Status: {status}");
    println!("Body: {text}");

    if status != StatusCode::OK {
        bail!("login failed with status {status}");
    }

    let envelope: ApiResponse<TokenResponse> =
        serde_json::from_str(&text).context("parsing token response failed")?;
    println!(
        "Envelope: status={} message={}",
        envelope.status, envelope.message
    );
    Ok(envelope.data)
}

async fn refresh(http: &Client, url: &str, refresh_token: &str) -> Result<TokenResponse> {
    println!("\n==> POST {url} (refresh)");

    let resp = http
        .post(url)
        .json(&serde_json::json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .context("refresh request failed")?;

    let status = resp.status();
    let text = resp.text().await.context("reading refresh body failed")?;
    println!("Status: {status}");
    println!("Body: {text}");

    if status != StatusCode::OK {
        bail!("refresh failed with status {status}");
    }

    let envelope: ApiResponse<TokenResponse> =
        serde_json::from_str(&text).context("parsing refresh response failed")?;
    println!(
        "Envelope: status={} message={}",
        envelope.status, envelope.message
    );
    Ok(envelope.data)
}

async fn call_get(http: &Client, url: &str, bearer: Option<&str>) -> Result<()> {
    println!("\n==> GET {url}");

    let mut req = http.get(url);
    if let Some(tok) = bearer {
        req = req.bearer_auth(tok);
    }

    let resp = req.send().await.context("GET request failed")?;
    let status = resp.status();
    let text = resp.text().await.context("reading GET body failed")?;

    println!("Status: {status}");
    println!("Body: {text}");
    Ok(())
}
