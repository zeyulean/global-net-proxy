//! gnp rules-update — 规则集日更 (下载 + 重启加载)
//!
//! 唯一调用方是 tick.sh 的 04 点窗口 (`gnpc rules-update`), 成功才写当日标记;
//! 失败 → tick.sh 落 WARN, 明日再试。手动跑同一条命令。
//!
//! 旧 `update-rules --install-cron` / `--check` 已删: 调度唯一 (D3),
//! 挂掉拉起归 `gnpc guard`。

use anyhow::{Context, Result};
use std::process::Command;

use gnp_core::install;
use gnp_core::platform;

/// 重新下载规则集 + 重启 sing-box (remote/local rule-set 在启动时加载)
pub fn cmd_update() -> Result<()> {
    let conf = platform::gnp_config_json();
    if !conf.exists() {
        anyhow::bail!(
            "配置不存在: {} (先 `gnpc install`)",
            conf.display()
        );
    }
    println!("更新规则集...");

    // 1) 重新下载 (download_rule_tmp 成功才替换, 失败保留现有有效规则)
    install::install_rules()?;

    // 2) 重启加载 (先杀再由服务管理器拉起, 保证新实例)
    if sb_process_running() {
        kill_sb_process();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    let plat = platform::ensure_supported()?;
    let _ = gnp_core::service::start(plat);

    println!("✅ 规则集已更新 (sing-box 已重启加载)");
    Ok(())
}

/// sing-box 进程是否在跑 (跨平台)
fn sb_process_running() -> bool {
    if cfg!(windows) {
        Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq sing-box.exe", "/NH"])
            .output()
            .map(|o| {
                format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                )
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
        let _ = Command::new("taskkill")
            .args(["/F", "/IM", "sing-box.exe"])
            .status();
    } else {
        let _ = Command::new("pkill").args(["-f", "sing-box run"]).status();
    }
}

/// 保留: 供 `gnpc start` 前自检用 (macOS 无 launchd 时的兜底)
#[allow(dead_code)]
pub fn ensure_running() -> Result<()> {
    if sb_process_running() {
        return Ok(());
    }
    let plat = platform::ensure_supported().context("平台不支持")?;
    gnp_core::service::start(plat)
}
