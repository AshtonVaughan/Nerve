//! `nerve` CLI.
//!
//! The binary is split into two roles:
//!
//! * `nerve start` boots the daemon in-process.
//! * Every other subcommand is a thin WebSocket client that talks to a running
//!   daemon over `--host:--port`.
//!
//! The client side is intentionally simple — it speaks the exact same protocol
//! as the SDKs, with no privileged escape hatches.

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use nerve_core::config::DaemonConfig;
use nerve_core::Runtime;
use nerve_protocol::{AnyAction, ClientMessage, LowLevelAction, MouseButton, ServerMessage};
use serde_json::json;
use tracing_subscriber::EnvFilter;

mod client;

use client::CliClient;

#[derive(Debug, Parser)]
#[command(name = "nerve", version, about = "Nerve: the body for AI agents.")]
struct Cli {
    /// Daemon WebSocket host.
    #[arg(long, default_value = "127.0.0.1", env = "NERVE_HOST", global = true)]
    host: String,
    /// Daemon WebSocket port.
    #[arg(long, default_value_t = 8765, env = "NERVE_PORT", global = true)]
    port: u16,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Start the daemon in the foreground.
    Start {
        /// Run with dry-run safety policy.
        #[arg(long)]
        dry_run: bool,
    },
    /// Stop the running daemon (sends emergency-stop, then session_stop).
    Stop,
    /// Report whether the daemon is reachable.
    Status,
    /// Inspect the local machine and print what's wired up.
    Doctor,
    /// Print daemon capabilities as JSON.
    Capabilities,
    /// Pretty-print a single observation.
    Observe {
        #[arg(long, default_value_t = false)]
        with_screenshot: bool,
        #[arg(long, default_value_t = false)]
        with_ui_tree: bool,
    },
    /// Capture the primary screen to a PNG file.
    Screenshot {
        #[arg(long, short = 'o', default_value = "nerve-screenshot.png")]
        out: String,
    },
    /// Click at absolute coordinates.
    Click {
        #[arg(long)]
        x: i32,
        #[arg(long)]
        y: i32,
        #[arg(long, default_value = "left")]
        button: String,
    },
    /// Click the first element matching the given text.
    #[command(name = "click-text")]
    ClickText {
        text: String,
        #[arg(long)]
        app: Option<String>,
    },
    /// Type text into whatever window is focused.
    Type {
        text: String,
        #[arg(long)]
        delay_ms: Option<u64>,
    },
    /// Send a hotkey combo (e.g. `nerve hotkey ctrl+s`).
    Hotkey {
        combo: String,
    },
    /// Read the clipboard.
    #[command(name = "clipboard-get")]
    ClipboardGet,
    /// Write to the clipboard.
    #[command(name = "clipboard-set")]
    ClipboardSet { text: String },
    /// Tail the action log for a session.
    Logs {
        #[arg(long)]
        session: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Replay a session through the daemon.
    Replay {
        session: String,
        #[arg(long, default_value_t = 1.0)]
        speed: f32,
    },
    /// Print the current daemon config.
    Config,
    /// Engage emergency stop on the running daemon.
    #[command(name = "emergency-stop")]
    EmergencyStop,
}

#[tokio::main]
async fn main() -> Result<()> {
    install_tracing();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Start { dry_run } => start_daemon(dry_run).await,
        Cmd::Stop => stop(&cli.host, cli.port).await,
        Cmd::Status => status(&cli.host, cli.port).await,
        Cmd::Doctor => doctor(&cli.host, cli.port).await,
        Cmd::Capabilities => capabilities(&cli.host, cli.port).await,
        Cmd::Observe { with_screenshot, with_ui_tree } => {
            observe(&cli.host, cli.port, with_screenshot, with_ui_tree).await
        }
        Cmd::Screenshot { out } => screenshot(&cli.host, cli.port, &out).await,
        Cmd::Click { x, y, button } => click(&cli.host, cli.port, x, y, &button).await,
        Cmd::ClickText { text, app } => click_text(&cli.host, cli.port, text, app).await,
        Cmd::Type { text, delay_ms } => type_text(&cli.host, cli.port, text, delay_ms).await,
        Cmd::Hotkey { combo } => hotkey(&cli.host, cli.port, &combo).await,
        Cmd::ClipboardGet => clipboard_get(&cli.host, cli.port).await,
        Cmd::ClipboardSet { text } => clipboard_set(&cli.host, cli.port, text).await,
        Cmd::Logs { session, limit } => logs(&cli.host, cli.port, session, limit).await,
        Cmd::Replay { session, speed } => replay(&cli.host, cli.port, session, speed).await,
        Cmd::Config => config_cmd().await,
        Cmd::EmergencyStop => emergency_stop(&cli.host, cli.port).await,
    }
}

fn install_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,nerve_core=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

async fn start_daemon(dry_run: bool) -> Result<()> {
    let mut config = DaemonConfig::default();
    if dry_run {
        config.default_policy.dry_run = true;
    }
    let runtime = Runtime::new(config)?;

    // Catch Ctrl-C to flush logs and ensure emergency-stop is broadcast.
    let runtime_for_ctrlc = runtime.clone();
    ctrlc::set_handler(move || {
        eprintln!("\n[nerve] ctrl-c received, broadcasting emergency stop");
        runtime_for_ctrlc.engage_emergency_stop();
        std::process::exit(0);
    })
    .ok();

    runtime.start().await
}

async fn stop(host: &str, port: u16) -> Result<()> {
    let mut c = CliClient::connect(host, port).await?;
    c.session_start().await?;
    c.send(ClientMessage::EmergencyStop { request_id: "cli-stop".into() })
        .await?;
    println!("emergency stop sent");
    Ok(())
}

async fn status(host: &str, port: u16) -> Result<()> {
    match CliClient::connect(host, port).await {
        Ok(_) => {
            println!("daemon: reachable at ws://{host}:{port}");
            Ok(())
        }
        Err(e) => {
            println!("daemon: not reachable ({e})");
            std::process::exit(1);
        }
    }
}

async fn doctor(host: &str, port: u16) -> Result<()> {
    use sysinfo::System;
    let mut sys = System::new_all();
    sys.refresh_all();
    println!("OS:           {}", System::name().unwrap_or_else(|| "?".into()));
    println!("Kernel:       {}", System::kernel_version().unwrap_or_else(|| "?".into()));
    println!("Arch:         {}", std::env::consts::ARCH);
    println!("Hostname:     {}", hostname::get().ok().and_then(|h| h.into_string().ok()).unwrap_or_else(|| "?".into()));
    #[cfg(target_os = "linux")]
    {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            println!("Display:      Wayland (limited)");
        } else if std::env::var("DISPLAY").is_ok() {
            println!("Display:      X11");
        } else {
            println!("Display:      headless / not detected");
        }
    }

    match CliClient::connect(host, port).await {
        Ok(mut c) => {
            c.session_start().await?;
            let caps = c.capabilities().await?;
            println!("Daemon:       reachable at ws://{host}:{port}");
            println!("Backend:      sc={} input={} ax={} cb={}",
                caps.backends.screen_capture,
                caps.backends.input,
                caps.backends.accessibility,
                caps.backends.clipboard,
            );
            println!("Capabilities: screen={} input={} ax_tree={} clipboard={} semantic={} ocr={} wayland_limited={}",
                caps.screen_capture,
                caps.input_control,
                caps.accessibility_tree,
                caps.clipboard,
                caps.semantic_actions,
                caps.ocr,
                caps.wayland_limited,
            );
            if caps.missing_permissions.is_empty() {
                println!("Missing:      none reported");
            } else {
                println!("Missing:");
                for p in caps.missing_permissions {
                    println!("  - {p}");
                }
            }
        }
        Err(e) => {
            println!("Daemon:       NOT reachable at ws://{host}:{port} ({e})");
            println!("              run `nerve start` first.");
            std::process::exit(1);
        }
    }
    println!();
    println!("Recommended fixes:");
    #[cfg(target_os = "macos")]
    println!("  - Grant Screen Recording and Accessibility to your terminal/IDE in System Settings > Privacy & Security.");
    #[cfg(target_os = "linux")]
    println!("  - Prefer X11 sessions; Wayland input is limited unless uinput is available.");
    #[cfg(target_os = "windows")]
    println!("  - Run the daemon with the same integrity level as the apps you want to control (no elevated/UAC targets from a regular user shell).");
    Ok(())
}

async fn capabilities(host: &str, port: u16) -> Result<()> {
    let mut c = CliClient::connect(host, port).await?;
    c.session_start().await?;
    let caps = c.capabilities().await?;
    println!("{}", serde_json::to_string_pretty(&caps)?);
    Ok(())
}

async fn observe(host: &str, port: u16, with_screenshot: bool, with_ui_tree: bool) -> Result<()> {
    let mut c = CliClient::connect(host, port).await?;
    c.session_start().await?;
    let obs = c.observation(with_screenshot, with_ui_tree).await?;
    // Drop the base64 payload from the printed output unless explicitly asked.
    let mut value = serde_json::to_value(&obs)?;
    if !with_screenshot {
        if let Some(screen) = value.get_mut("screen") {
            if let Some(obj) = screen.as_object_mut() {
                obj.remove("screenshot_base64");
            }
        }
    }
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

async fn screenshot(host: &str, port: u16, out: &str) -> Result<()> {
    let mut c = CliClient::connect(host, port).await?;
    c.session_start().await?;
    let obs = c.observation(true, false).await?;
    let b64 = obs
        .screen
        .screenshot_base64
        .ok_or_else(|| anyhow!("daemon did not include screenshot payload"))?;
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
    std::fs::write(out, bytes).with_context(|| format!("write {}", out))?;
    println!("wrote {} ({} x {})", out, obs.screen.width, obs.screen.height);
    Ok(())
}

async fn click(host: &str, port: u16, x: i32, y: i32, button: &str) -> Result<()> {
    let button = match button.to_ascii_lowercase().as_str() {
        "left" => MouseButton::Left,
        "right" => MouseButton::Right,
        "middle" => MouseButton::Middle,
        other => return Err(anyhow!("unknown button: {other}")),
    };
    let mut c = CliClient::connect(host, port).await?;
    c.session_start().await?;
    let result = c
        .execute(AnyAction::Low(LowLevelAction::Click { x, y, button }))
        .await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn click_text(host: &str, port: u16, text: String, app: Option<String>) -> Result<()> {
    let mut c = CliClient::connect(host, port).await?;
    c.session_start().await?;
    let result = c
        .execute(AnyAction::Semantic(
            nerve_protocol::SemanticAction::ClickElementByText { text, app },
        ))
        .await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn type_text(host: &str, port: u16, text: String, delay_ms: Option<u64>) -> Result<()> {
    let mut c = CliClient::connect(host, port).await?;
    c.session_start().await?;
    let result = c
        .execute(AnyAction::Low(LowLevelAction::TypeText { text, delay_ms }))
        .await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn hotkey(host: &str, port: u16, combo: &str) -> Result<()> {
    let keys: Vec<String> = combo.split('+').map(|s| s.trim().to_string()).collect();
    let mut c = CliClient::connect(host, port).await?;
    c.session_start().await?;
    let result = c
        .execute(AnyAction::Low(LowLevelAction::Hotkey { keys }))
        .await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn clipboard_get(host: &str, port: u16) -> Result<()> {
    let mut c = CliClient::connect(host, port).await?;
    c.session_start().await?;
    let result = c
        .execute(AnyAction::Low(LowLevelAction::ClipboardGet))
        .await?;
    let text = result
        .data
        .and_then(|v| v.get("text").and_then(|t| t.as_str().map(|s| s.to_string())))
        .unwrap_or_default();
    println!("{}", text);
    Ok(())
}

async fn clipboard_set(host: &str, port: u16, text: String) -> Result<()> {
    let mut c = CliClient::connect(host, port).await?;
    c.session_start().await?;
    let result = c
        .execute(AnyAction::Low(LowLevelAction::ClipboardSet { text }))
        .await?;
    if !result.ok {
        return Err(anyhow!(result.error.unwrap_or_else(|| "clipboard set failed".into())));
    }
    println!("ok");
    Ok(())
}

async fn logs(host: &str, port: u16, session: Option<String>, limit: usize) -> Result<()> {
    let mut c = CliClient::connect(host, port).await?;
    c.session_start().await?;
    let entries = c.action_log(session, Some(limit)).await?;
    for e in entries {
        println!(
            "{} [{:?}] {:?} ok={} window={:?}",
            e.timestamp.to_rfc3339(),
            e.safety_decision,
            e.result.method,
            e.result.ok,
            e.active_window_after,
        );
    }
    Ok(())
}

async fn replay(host: &str, port: u16, session: String, speed: f32) -> Result<()> {
    let mut c = CliClient::connect(host, port).await?;
    c.session_start().await?;
    c.send(ClientMessage::ReplaySession {
        request_id: "cli-replay".into(),
        session_id: session.clone(),
        speed: Some(speed),
    })
    .await?;
    loop {
        let msg = c.next().await?;
        match msg {
            ServerMessage::ReplayProgress { step, total, entry, .. } => {
                println!("{}/{} {:?}", step + 1, total, entry.result.method);
            }
            ServerMessage::ReplayComplete { .. } => {
                println!("replay complete");
                break;
            }
            ServerMessage::Error { code, message, .. } => {
                return Err(anyhow!("replay error: {code} {message}"));
            }
            _ => {}
        }
    }
    Ok(())
}

async fn config_cmd() -> Result<()> {
    let cfg = DaemonConfig::default();
    println!("{}", serde_json::to_string_pretty(&json!({
        "default": cfg,
        "log_dir": nerve_core::config::default_log_dir(),
    }))?);
    Ok(())
}

async fn emergency_stop(host: &str, port: u16) -> Result<()> {
    let mut c = CliClient::connect(host, port).await?;
    c.session_start().await?;
    c.send(ClientMessage::EmergencyStop { request_id: "cli-estop".into() })
        .await?;
    println!("emergency stop signalled");
    Ok(())
}
