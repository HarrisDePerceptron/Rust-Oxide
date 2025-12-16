use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Defaults assume your axum server is running locally on :3000
    let base = std::env::var("BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".to_string());
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "admin".to_string());
    let pass = std::env::var("PASSWORD").unwrap_or_else(|_| "admin".to_string());

    let http = Client::new();

    // 1) /public
    call_get(&http, &format!("{base}/public"), None).await?;

    // 2) /login -> token
    let token = login(&http, &format!("{base}/login"), &user, &pass).await?;
    println!(
        "\nGot token (first 24 chars): {}…",
        token.chars().take(24).collect::<String>()
    );

    // 3) /me (JWT protected)
    call_get(&http, &format!("{base}/me"), Some(&token)).await?;

    // 4) /admin/stats (role protected)
    call_get(&http, &format!("{base}/admin/stats"), Some(&token)).await?;

    Ok(())
}

async fn login(http: &Client, url: &str, username: &str, password: &str) -> Result<String> {
    println!("\n==> POST {url}");

    let resp = http
        .post(url)
        .json(&LoginRequest {
            username: username.to_string(),
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

    let tr: TokenResponse = serde_json::from_str(&text).context("parsing token response failed")?;
    Ok(tr.access_token)
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
