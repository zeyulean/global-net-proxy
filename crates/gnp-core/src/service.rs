//! 进程 / 服务管理 — 跨平台 (Mac launchctl / Linux systemd)
//!
//! 核心思路: 用系统服务管理器管理 sing-box, 而不是裸 spawn 子进程。
//! - macOS: launchctl load/unload ~/Library/LaunchAgents/com.gnp.sing-box.plist
//! - Linux: systemctl start/stop gnp-proxy.service
//!
//! 好处: 开机自启、崩溃自动重启 (KeepAlive/Restart=on-failure)、系统级管理。

use crate::platform::{sb_bin, sb_config, Platform};
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

/// launchd plist 标签 (macOS)
pub const LAUNCHD_LABEL: &str = "com.gnp.sing-box";
/// systemd service 名称 (Linux)
pub const SYSTEMD_SERVICE: &str = "gnp-proxy";

/// 获取 launchd plist 路径
pub fn launchd_plist() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join("Library/LaunchAgents/com.gnp.sing-box.plist")
}

/// 生成 launchd plist 内容 (macOS 开机自启)
pub fn launchd_plist_content() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>run</string>
        <string>-c</string>
        <string>{conf}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/sing-box-gnp.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/sing-box-gnp.err</string>
</dict>
</plist>
"#,
        label = LAUNCHD_LABEL,
        bin = sb_bin().display(),
        conf = sb_config().display(),
    )
}

/// 生成 systemd 单元内容 (Linux 开机自启, 系统级)
///
/// 使用系统级 systemd (/etc/systemd/system/), 需 root 安装但更可靠。
pub fn systemd_unit_content() -> String {
    format!(
        r#"[Unit]
Description=GNP Proxy (mixed only, no tun)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={bin} run -c {conf}
Restart=on-failure
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
"#,
        bin = sb_bin().display(),
        conf = sb_config().display(),
    )
}

/// Linux systemd 系统级服务单元路径 (/etc/systemd/system/gnp-proxy.service)
///
/// 用系统级 (systemctl, 不加 --user), 更可靠。
/// install 时需要 root (写 /etc/systemd/system/), 但 start/stop 用 pkexec/sudo 或 root。
pub fn systemd_system_unit_path() -> PathBuf {
    PathBuf::from("/etc/systemd/system").join(format!("{}.service", SYSTEMD_SERVICE))
}

/// 安装 Linux systemd 系统服务: 写 unit 文件 + daemon-reload + enable
///
/// 需要 root 权限 (写 /etc/systemd/system/)。
pub fn install_linux() -> Result<()> {
    let path = systemd_system_unit_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("创建 systemd 目录失败: {} (需要 root)", parent.display()))?;
    }
    std::fs::write(&path, systemd_unit_content())
        .with_context(|| format!("写入 systemd 单元失败: {} (需要 root)", path.display()))?;

    // 重新加载 systemd 守护进程
    let _ = Command::new("systemctl")
        .args(["daemon-reload"])
        .status();
    // 设为开机自启
    let _ = Command::new("systemctl")
        .args(["enable", SYSTEMD_SERVICE])
        .status();

    println!(
        "✅ Linux systemd 系统服务已安装: {}",
        path.display()
    );
    println!("   启动: sudo systemctl start {}", SYSTEMD_SERVICE);
    println!("   状态: sudo systemctl status {}", SYSTEMD_SERVICE);
    Ok(())
}

/// 启动 sing-box
pub fn start(platform: Platform) -> Result<()> {
    match platform {
        Platform::MacOs => start_macos(),
        Platform::Linux => start_linux(),
        _ => bail!("不支持的平台"),
    }
}

/// 停止 sing-box
pub fn stop(platform: Platform) -> Result<()> {
    match platform {
        Platform::MacOs => stop_macos(),
        Platform::Linux => stop_linux(),
        _ => bail!("不支持的平台"),
    }
}

/// 检查状态 (返回是否运行中)
pub fn is_running(platform: Platform) -> Result<bool> {
    match platform {
        Platform::MacOs => is_running_macos(),
        Platform::Linux => is_running_linux(),
        _ => bail!("不支持的平台"),
    }
}

// --- macOS (launchctl) ---

fn start_macos() -> Result<()> {
    let plist = launchd_plist();
    if !plist.exists() {
        std::fs::write(&plist, launchd_plist_content())
            .with_context(|| format!("写入 plist 失败: {}", plist.display()))?;
    }
    let _ = Command::new("launchctl")
        .args(["load", plist.to_str().unwrap()])
        .status()
        .context("launchctl load 失败")?;
    if !is_running_macos()? {
        bail!("sing-box 启动失败 (launchctl load 后未运行)");
    }
    Ok(())
}

fn stop_macos() -> Result<()> {
    let plist = launchd_plist();
    if plist.exists() {
        let _ = Command::new("launchctl")
            .args(["unload", plist.to_str().unwrap()])
            .status()
            .context("launchctl unload 失败")?;
    }
    let _ = Command::new("pkill").args(["-f", "sing-box run"]).status();
    Ok(())
}

fn is_running_macos() -> Result<bool> {
    let out = Command::new("launchctl")
        .args(["list"])
        .output()
        .context("launchctl list 失败")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(stdout.contains(LAUNCHD_LABEL))
}

// --- Linux (systemd) ---

fn start_linux() -> Result<()> {
    // 确保 systemd 系统服务已安装 (未安装则自动创建, 修复全新部署 install+start 失败)
    if !systemd_system_unit_path().exists() {
        install_linux()?;
    }
    let st = Command::new("systemctl")
        .args(["start", SYSTEMD_SERVICE])
        .status()
        .context("systemctl start 失败")?;
    if !st.success() {
        bail!("systemctl start {} 失败", SYSTEMD_SERVICE);
    }
    Ok(())
}

fn stop_linux() -> Result<()> {
    let _ = Command::new("systemctl")
        .args(["stop", SYSTEMD_SERVICE])
        .status();
    Ok(())
}

fn is_running_linux() -> Result<bool> {
    let out = Command::new("systemctl")
        .args(["is-active", SYSTEMD_SERVICE])
        .output()
        .context("systemctl is-active 失败")?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(stdout == "active")
}