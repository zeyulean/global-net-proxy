//! gnp-client update-rules — 规则集更新 + sing-box 守护
//!
//! 三种模式:
//!   update   — 重启 sing-box 加载最新 remote rule-set
//!   check    — 检查 sing-box 是否运行, 挂了就重启
//!   cron     — 安装/管理 cron 任务 (每天检查)

use anyhow::{Context, Result};
use std::process::Command;

/// update: 强制更新规则集 (重启 sing-box 即可)
pub fn cmd_update() -> Result<()> {
    let conf = gnp_core::platform::sb_config();
    if !conf.exists() {
        println!("⚠️  未安装 sing-box, 跳过");
        return Ok(());
    }
    println!("触发规则集更新...");

    // 重启 sing-box (remote rule-set 在启动时拉取)
    if sb_process_running() {
        kill_sb_process();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    // 用服务管理器重启
    let platform = gnp_core::platform::ensure_supported()?;
    if gnp_core::platform::config_exists() {
        let _ = gnp_core::service::start(platform);
    }

    println!("规则集更新完成 (sing-box 已重启加载最新 geosite/geoip)");
    Ok(())
}

/// check: 检查 sing-box 是否运行, 挂了就拉起来
pub fn cmd_check() -> Result<()> {
    let running = sb_process_running();

    if running {
        println!("sing-box 运行中 ✓");
        return Ok(());
    }

    println!("⚠️  sing-box 未运行, 尝试重启...");
    let platform = gnp_core::platform::ensure_supported()?;
    match gnp_core::service::start(platform) {
        Ok(()) => println!("✅ sing-box 已重启"),
        Err(e) => println!("⚠️  重启失败: {}", e),
    }
    Ok(())
}

/// 安装 cron 任务 (每天凌晨 4 点检查)
pub fn cmd_install_cron() -> Result<()> {
    // 获取 gnp-client 自身路径
    let self_path = std::env::current_exe()
        .context("无法获取 gnp-client 路径")?;
    let self_str = self_path.to_str()
        .context("路径含非 UTF-8")?;

    let log_path = gnp_core::platform::sb_dir().join("cron.log");
    let cron_line = format!(
        "0 4 * * * {} update-rules check >> {} 2>&1",
        self_str,
        log_path.display()
    );

    // 读取现有 crontab, 过滤旧条目
    let existing = Command::new("crontab")
        .args(["-l"])
        .output();
    let mut lines: Vec<String> = Vec::new();
    if let Ok(out) = existing {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if !line.contains("gnp-client") && !line.contains("global-net-proxy") {
                    lines.push(line.to_string());
                }
            }
        }
    }
    lines.push(cron_line.clone());

    // 写入新 crontab
    let content = lines.join("\n");
    let mut st = Command::new("crontab")
        .args(["-"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("crontab 命令失败")?;
    if let Some(ref mut stdin) = st.stdin {
        use std::io::Write;
        let _ = stdin.write_all(content.as_bytes());
    }
    let output = st.wait_with_output().context("crontab 写入失败")?;
    if !output.status.success() {
        anyhow::bail!("crontab 安装失败: {}", String::from_utf8_lossy(&output.stderr));
    }

    println!("cron 已安装: {}", cron_line);
    println!("每天 04:00 检查 sing-box 常驻 + 规则集更新");
    Ok(())
}

/// sing-box 进程是否在跑 (跨平台)
fn sb_process_running() -> bool {
    if cfg!(windows) {
        Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq sing-box.exe", "/NH"])
            .output()
            .map(|o| {
                format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
                    .to_lowercase()
                    .contains("sing-box.exe")
            })
            .unwrap_or(false)
    } else {
        Command::new("pgrep")
            .args(["-f", "sing-box run"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// 杀掉 sing-box 进程 (跨平台)
fn kill_sb_process() {
    if cfg!(windows) {
        let _ = Command::new("taskkill").args(["/F", "/IM", "sing-box.exe"]).status();
    } else {
        let _ = Command::new("pkill").args(["-f", "sing-box run"]).status();
    }
}
