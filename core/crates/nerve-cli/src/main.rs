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
        /// Override the config path.
        #[arg(long)]
        config: Option<std::path::PathBuf>,
        /// Daemonize: fork into the background and write a pid file.
        #[arg(long)]
        daemonize: bool,
        /// Path to the pid file when daemonizing.
        #[arg(long, default_value = ".nerve.pid")]
        pid_file: std::path::PathBuf,
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
    /// Manage daemon configuration files (write/show/edit).
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Manage the daemon auth token (rotate/show).
    Token {
        #[command(subcommand)]
        cmd: TokenCmd,
    },
    /// Register the daemon as a system service.
    Service {
        #[command(subcommand)]
        cmd: ServiceCmd,
    },
    /// Generate shell completion scripts.
    Completion {
        /// Target shell.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Engage emergency stop on the running daemon.
    #[command(name = "emergency-stop")]
    EmergencyStop,
}

#[derive(Debug, Subcommand)]
enum ConfigCmd {
    /// Print the resolved daemon config as TOML.
    Show,
    /// Write a default config file to the platform default location.
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Print the path the daemon will pick up.
    Path,
}

#[derive(Debug, Subcommand)]
enum TokenCmd {
    /// Generate a new random auth token and write it to the config file.
    Rotate,
    /// Print the current auth token (if any).
    Show,
    /// Remove the auth token from the config file.
    Clear,
}

#[derive(Debug, Subcommand)]
enum ServiceCmd {
    /// Install a service unit / launchd plist / Windows service.
    Install,
    /// Uninstall the service.
    Uninstall,
    /// Print the service status, if known.
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    install_tracing();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Start { dry_run, config, daemonize, pid_file } => {
            start_daemon(dry_run, config, daemonize, pid_file).await
        }
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
        Cmd::Config { cmd } => config_cmd(cmd).await,
        Cmd::Token { cmd } => token_cmd(cmd).await,
        Cmd::Service { cmd } => service_cmd(cmd).await,
        Cmd::Completion { shell } => completion_cmd(shell),
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

async fn start_daemon(
    dry_run: bool,
    config_path: Option<std::path::PathBuf>,
    daemonize: bool,
    pid_file: std::path::PathBuf,
) -> Result<()> {
    let (mut config, resolved_path) = DaemonConfig::resolve(config_path.as_deref());
    if dry_run {
        config.default_policy.dry_run = true;
    }
    if let Some(p) = resolved_path {
        tracing::info!("loaded config from {}", p.display());
    } else {
        tracing::info!("using built-in default config (no config file found)");
    }

    if daemonize {
        // Fork/spawn into the background and write a pid file. We
        // intentionally re-exec ourselves with the env var
        // NERVE_DAEMON_CHILD=1 so the child shares the exact same binary.
        if std::env::var("NERVE_DAEMON_CHILD").is_err() {
            let exe = std::env::current_exe()?;
            let mut args: Vec<String> = std::env::args().collect();
            // Strip `--daemonize` from passthrough args.
            args.retain(|a| a != "--daemonize");
            let mut cmd = std::process::Command::new(exe);
            cmd.args(args.iter().skip(1));
            cmd.env("NERVE_DAEMON_CHILD", "1");
            #[cfg(unix)]
            unsafe {
                use std::os::unix::process::CommandExt;
                cmd.pre_exec(|| {
                    libc_setsid();
                    Ok(())
                });
            }
            let child = cmd.spawn()?;
            std::fs::write(&pid_file, child.id().to_string())?;
            println!("nerve daemon started pid={} ({})", child.id(), pid_file.display());
            return Ok(());
        }
        // Child path: nothing else to do; just continue into the runtime.
        std::fs::write(&pid_file, std::process::id().to_string()).ok();
    }

    let runtime = Runtime::new(config)?;

    // Catch Ctrl-C to flush logs and broadcast emergency-stop.
    let runtime_for_ctrlc = runtime.clone();
    ctrlc::set_handler(move || {
        eprintln!("\n[nerve] ctrl-c received, flushing and stopping");
        runtime_for_ctrlc.engage_emergency_stop();
        runtime_for_ctrlc.shutdown();
        std::process::exit(0);
    })
    .ok();

    runtime.start().await
}

#[cfg(unix)]
fn libc_setsid() {
    // Detach from the controlling tty so the daemon survives terminal close.
    unsafe { libc::setsid() };
}

#[cfg(not(unix))]
fn libc_setsid() {}

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

async fn config_cmd(cmd: ConfigCmd) -> Result<()> {
    match cmd {
        ConfigCmd::Show => {
            let (cfg, source) = DaemonConfig::resolve(None);
            if let Some(p) = source {
                println!("# loaded from {}", p.display());
            } else {
                println!("# no config file found; showing built-in defaults");
            }
            println!("{}", cfg.to_toml_pretty()?);
        }
        ConfigCmd::Init { force } => {
            let path = nerve_core::config::default_config_path()
                .ok_or_else(|| anyhow!("could not resolve a config path on this OS"))?;
            if path.exists() && !force {
                return Err(anyhow!("{} already exists; pass --force to overwrite", path.display()));
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let cfg = DaemonConfig::default();
            std::fs::write(&path, cfg.to_toml_pretty()?)?;
            println!("wrote {}", path.display());
        }
        ConfigCmd::Path => {
            if let Some(p) = nerve_core::config::default_config_path() {
                println!("{}", p.display());
            } else {
                return Err(anyhow!("no config directory resolved for this OS"));
            }
        }
    }
    Ok(())
}

async fn token_cmd(cmd: TokenCmd) -> Result<()> {
    let path = nerve_core::config::default_config_path()
        .ok_or_else(|| anyhow!("no config dir on this OS"))?;
    let (mut cfg, _src) = DaemonConfig::resolve(Some(&path));
    match cmd {
        TokenCmd::Rotate => {
            let token = generate_token();
            cfg.auth_token = Some(token.clone());
            persist_config(&path, &cfg)?;
            println!("rotated. new token (export NERVE_AUTH_TOKEN before running clients):");
            println!("{token}");
        }
        TokenCmd::Show => match cfg.auth_token.as_deref() {
            Some(t) => println!("{t}"),
            None => println!("(no auth token configured)"),
        },
        TokenCmd::Clear => {
            cfg.auth_token = None;
            persist_config(&path, &cfg)?;
            println!("cleared");
        }
    }
    Ok(())
}

fn persist_config(path: &std::path::Path, cfg: &DaemonConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, cfg.to_toml_pretty()?)?;
    Ok(())
}

fn generate_token() -> String {
    // 192-bit random, base32-encoded for terminal-friendliness.
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut state: u128 = now ^ 0xa5a5_a5a5_a5a5_a5a5_a5a5_a5a5_a5a5_a5a5;
    let mut out = String::with_capacity(48);
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    for _ in 0..48 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let idx = ((state >> 64) as u64 & 31) as usize;
        out.push(ALPHA[idx] as char);
    }
    out
}

async fn service_cmd(cmd: ServiceCmd) -> Result<()> {
    match cmd {
        ServiceCmd::Install => install_service(),
        ServiceCmd::Uninstall => uninstall_service(),
        ServiceCmd::Status => status_service(),
    }
}

#[cfg(target_os = "linux")]
fn install_service() -> Result<()> {
    let exe = std::env::current_exe()?;
    let unit = format!(
        "[Unit]\nDescription=Nerve agent runtime\nAfter=network.target\n\n[Service]\nExecStart={} start\nRestart=on-failure\nUser=%i\nEnvironment=\"RUST_LOG=info\"\n\n[Install]\nWantedBy=default.target\n",
        exe.display()
    );
    let path = dirs::config_dir()
        .ok_or_else(|| anyhow!("no config dir"))?
        .join("systemd/user/nerve.service");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, unit)?;
    println!("wrote {}", path.display());
    println!("enable + start with: systemctl --user enable --now nerve.service");
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_service() -> Result<()> {
    let exe = std::env::current_exe()?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.nerve.daemon</string>
  <key>ProgramArguments</key><array>
    <string>{}</string>
    <string>start</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
</dict>
</plist>
"#,
        exe.display()
    );
    let home = dirs::home_dir().ok_or_else(|| anyhow!("no home dir"))?;
    let path = home.join("Library/LaunchAgents/dev.nerve.daemon.plist");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, plist)?;
    println!("wrote {}", path.display());
    println!("load with: launchctl load -w {}", path.display());
    Ok(())
}

#[cfg(target_os = "windows")]
fn install_service() -> Result<()> {
    let exe = std::env::current_exe()?;
    println!(
        "Run as Administrator:\n  sc.exe create NerveDaemon binPath= \"{} start\" start= auto",
        exe.display()
    );
    println!("then:  sc.exe start NerveDaemon");
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn install_service() -> Result<()> {
    Err(anyhow!("service install not supported on this OS"))
}

#[cfg(target_os = "linux")]
fn uninstall_service() -> Result<()> {
    let path = dirs::config_dir()
        .ok_or_else(|| anyhow!("no config dir"))?
        .join("systemd/user/nerve.service");
    if path.exists() {
        std::fs::remove_file(&path)?;
        println!("removed {}", path.display());
    } else {
        println!("not installed");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall_service() -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("no home dir"))?;
    let path = home.join("Library/LaunchAgents/dev.nerve.daemon.plist");
    if path.exists() {
        std::fs::remove_file(&path)?;
        println!("removed {}", path.display());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn uninstall_service() -> Result<()> {
    println!("Run as Administrator: sc.exe delete NerveDaemon");
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn uninstall_service() -> Result<()> {
    Err(anyhow!("service uninstall not supported on this OS"))
}

fn status_service() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let out = std::process::Command::new("systemctl")
            .args(["--user", "status", "nerve.service"])
            .output()
            .map_err(|e| anyhow!("systemctl: {e}"))?;
        std::io::Write::write_all(&mut std::io::stdout(), &out.stdout).ok();
        std::io::Write::write_all(&mut std::io::stderr(), &out.stderr).ok();
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("launchctl")
            .args(["list", "dev.nerve.daemon"])
            .output()
            .map_err(|e| anyhow!("launchctl: {e}"))?;
        std::io::Write::write_all(&mut std::io::stdout(), &out.stdout).ok();
        std::io::Write::write_all(&mut std::io::stderr(), &out.stderr).ok();
    }
    #[cfg(target_os = "windows")]
    {
        let out = std::process::Command::new("sc.exe")
            .args(["query", "NerveDaemon"])
            .output()
            .map_err(|e| anyhow!("sc.exe: {e}"))?;
        std::io::Write::write_all(&mut std::io::stdout(), &out.stdout).ok();
        std::io::Write::write_all(&mut std::io::stderr(), &out.stderr).ok();
    }
    Ok(())
}

fn completion_cmd(shell: clap_complete::Shell) -> Result<()> {
    use clap::CommandFactory;
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
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
