use std::time::Instant;

use anyhow::{Context, Result, bail};
use realtime::client::RealtimeClient;
use tokio::io::{AsyncBufReadExt, BufReader};

fn read_arg_or_env(args: &[String], index: usize, env_key: &str, default: &str) -> String {
    args.get(index)
        .cloned()
        .or_else(|| std::env::var(env_key).ok())
        .unwrap_or_else(|| default.to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let ws_url = read_arg_or_env(
        &args,
        1,
        "REALTIME_WS_URL",
        "ws://127.0.0.1:3000/api/v1/realtime/socket",
    );
    let token = read_arg_or_env(&args, 2, "REALTIME_TOKEN", "");
    let channel_name = read_arg_or_env(&args, 3, "REALTIME_CHANNEL", "echo:lobby");

    if token.trim().is_empty() {
        bail!("missing token: pass arg2 or REALTIME_TOKEN");
    }

    println!("connecting to {ws_url}");
    let client = RealtimeClient::connect(&ws_url, token.trim())
        .await
        .map_err(anyhow::Error::msg)?;

    client.on_messages(|channel, message| {
        println!("[on_messages channel={channel}] {message}");
    });

    let target_channel = channel_name.clone();
    client.on_message(&channel_name, move |message| {
        println!("[on_message channel={target_channel}] {message}");
    });

    client
        .join(&channel_name)
        .await
        .map_err(anyhow::Error::msg)?;
    println!("connected and joined `{channel_name}`. Type messages, `/leave` to exit.");

    run_stdin_loop(&client, &channel_name).await
}

async fn run_stdin_loop(client: &RealtimeClient, channel_name: &str) -> Result<()> {
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut counter: u64 = 0;
    let started = Instant::now();

    while let Some(line) = lines.next_line().await.context("stdin read failed")? {
        let line = line.trim();
        if line.eq_ignore_ascii_case("/leave") {
            client
                .leave(channel_name)
                .await
                .map_err(anyhow::Error::msg)?;
            break;
        }
        if line.is_empty() {
            continue;
        }

        counter += 1;
        client
            .send(
                channel_name,
                serde_json::json!({
                    "text": line,
                    "seq": counter,
                    "uptime_ms": started.elapsed().as_millis(),
                }),
            )
            .await
            .map_err(anyhow::Error::msg)?;
    }

    Ok(())
}
