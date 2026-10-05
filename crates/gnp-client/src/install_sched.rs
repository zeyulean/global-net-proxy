//! 调度安装 / 卸载 + 旧调度清理 (plan §3.6, D3)
//!
//! 唯一调度入口 = `tick.sh`。本模块负责:
//! 1. 落 `bin/tick.sh` (资产来自 `deploy/scheduler/`, `include_str!` 编进二进制)
//! 2. 接线: Mac launchd `com.gnpc.tick` / Linux crontab **一行**
//! 3. 清旧: `com.gnp.*` unload+移走、旧 cron 行剔除、`gnp-proxy`/`gnp-hy2` disable
//!
//! 常驻 sing-box 服务是独立 KeepAlive 单元, **不与 tick 合并** (生命周期不同)。
//! 但 macOS 上两个 launchd plist 一起写 (都在 LaunchAgents), 便于一次装齐。

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use gnp_core::platform::{self, Platform};
use gnp_core::scheduler;

/// 装调度 (幂等; 可重复跑)
pub fn install() -> Result<()> {
    install_with(true)
}

/// `load=false` = 只落盘 (tick.sh + plist/cron 文件), 不装载/不接线
///
/// migrate 用这个两段式: 先落盘 → 停旧常驻让出 1080 → 再装载新服务。
/// (同机同端口, 旧的不让位, 新的起来就是 crash-loop)
pub fn install_with(load: bool) -> Result<()> {
    let base = platform::gnp_home();
    platform::ensure_layout(&base)?;
    println!("== 安装调度 ({}) ==", Platform::detect().as_str());
    println!("   部署根: {}", base.display());

    // 1) tick.sh (唯一调度入口)
    let tick = scheduler::write_tick_script(&base)?;
    println!("✅ tick.sh: {}", tick.display());

    if !load {
        println!("-- 只落盘, 不装载 (--no-load)");
        match Platform::detect() {
            Platform::MacOs => write_plists(&base)?,
            Platform::Linux => write_cron(&base)?,
            _ => {}
        }
        return Ok(());
    }

    // 2) 接线 + 常驻服务
    match Platform::detect() {
        Platform::MacOs => install_macos(&base)?,
        Platform::Linux => install_linux(&base)?,
        Platform::Windows => {
            println!(
                "⚠️  Windows 本次不装 tick (D6: 计划任务只管常驻 sing-box, 无 bash 调度器)。"
            );
            println!("   手动等效命令: gnpc guard   (单 tick, 退避/冻结/告警逻辑一致)");
        }
        _ => bail!("不支持的平台"),
    }

    // 3) 清旧 (幂等: 没有旧东西就什么都不打)
    let cleaned = purge_legacy();
    if cleaned.is_empty() {
        println!("✅ 无旧调度残留 (com.gnp.* / 旧 cron 行 / gnp-proxy / gnp-hy2 都不存在)");
    } else {
        println!("✅ 已清理旧调度:");
        for c in cleaned {
            println!("   - {}", c);
        }
    }
    Ok(())
}

/// 卸调度 (只拆调度, 不停常驻 sing-box —— 那是 `gnpc stop` 的事)
pub fn uninstall() -> Result<()> {
    let base = platform::gnp_home();
    println!("== 卸载调度 ==");
    match Platform::detect() {
        Platform::MacOs => {
            for label in [scheduler::LAUNCHD_TICK, scheduler::LAUNCHD_SINGBOX] {
                let plist = scheduler::launchd_plist_path(label);
                let _ = Command::new("launchctl").args(["unload", "-w", plist.to_str().unwrap_or("")]).status();
                let _ = Command::new("launchctl").args(["remove", label]).status();
                if plist.exists() {
                    match std::fs::remove_file(&plist) {
                        Ok(_) => println!("   - 卸载并删除 {}", plist.display()),
                        Err(e) => println!("   ⚠️  删除 {} 失败: {}", plist.display(), e),
                    }
                }
            }
        }
        Platform::Linux => {
            let lines = scheduler::read_crontab();
            let kept = scheduler::purge_gnp_cron_lines(&lines);
            if kept.len() == lines.len() {
                println!("   - crontab 无 gnp 行, 无需改");
            } else {
                scheduler::write_crontab(&kept)?;
                println!("   - crontab 已剔除 gnp 行 (保留 {} 条其它任务)", kept.len());
            }
        }
        Platform::Windows => {
            let _ = Command::new("schtasks")
                .args(["/Delete", "/TN", scheduler::WIN_TASK, "/F"])
                .status();
            println!("   - 已删计划任务 {}", scheduler::WIN_TASK);
        }
        _ => bail!("不支持的平台"),
    }
    let tick = base.join("bin/tick.sh");
    if tick.exists() {
        let _ = std::fs::remove_file(&tick);
        println!("   - 删除 {}", tick.display());
    }
    println!("✅ 调度已卸载 (配置与日志保留在 {})", base.join("var").display());
    Ok(())
}

// --- macOS ---

/// 只写两个 plist (不 launchctl load)
fn write_plists(base: &Path) -> Result<()> {
    let agents = platform::home_dir().join("Library/LaunchAgents");
    std::fs::create_dir_all(&agents)?;
    std::fs::create_dir_all(base.join("var"))?;
    let sb = scheduler::launchd_plist_path(scheduler::LAUNCHD_SINGBOX);
    scheduler::write_executable(&sb, &scheduler::client_plist_singbox(base)?)?;
    let tick = scheduler::launchd_plist_path(scheduler::LAUNCHD_TICK);
    scheduler::write_executable(&tick, &scheduler::client_plist_tick(base)?)?;
    Ok(())
}

/// 装载两个 plist (先 unload 保证内容生效)
pub fn load_plists() -> Result<()> {
    for (label, log) in [
        (scheduler::LAUNCHD_SINGBOX, "常驻 sing-box"),
        (scheduler::LAUNCHD_TICK, "tick 60s"),
    ] {
        let plist = scheduler::launchd_plist_path(label);
        let _ = Command::new("launchctl")
            .args(["unload", "-w", plist.to_str().unwrap_or("")])
            .status();
        let _ = Command::new("launchctl")
            .args(["remove", label])
            .status();
        Command::new("launchctl")
            .args(["load", plist.to_str().unwrap_or("")])
            .status()
            .with_context(|| format!("launchctl load {} 失败", plist.display()))?;
        println!("✅ launchd {} ({}): {}", label, log, plist.display());
    }
    Ok(())
}

fn install_macos(base: &Path) -> Result<()> {
    write_plists(base)?;
    load_plists()
}

/// 只停旧 launchd 常驻 (保留 plist 文件 → 可回滚); migrate 在让位端口时用
pub fn stop_legacy_singbox() -> Vec<String> {
    let mut done = Vec::new();
    for label in scheduler::LEGACY_LAUNCHD {
        let plist = scheduler::launchd_plist_path(label);
        let listed = Command::new("launchctl")
            .args(["list"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(label))
            .unwrap_or(false);
        if !listed && !plist.exists() {
            continue;
        }
        let _ = Command::new("launchctl")
            .args(["unload", "-w", plist.to_str().unwrap_or("")])
            .status();
        let _ = Command::new("launchctl").args(["remove", label]).status();
        // 文件留着 (purge_legacy 才移走) —— verify 失败要能原样装回来
        done.push(format!("{} 已停 (plist 保留待回滚)", label));
    }
    if !done.is_empty() {
        let _ = Command::new("pkill").args(["-f", "sing-box run"]).status();
    }
    done
}

// --- Linux ---

fn write_cron(base: &Path) -> Result<()> {
    // crontab: 剔旧 + 加一行 (坑清单 #4: Rust 端显式关 stdin)
    let mut lines = scheduler::purge_gnp_cron_lines(&scheduler::read_crontab());
    lines.push(scheduler::client_cron_line(base));
    scheduler::write_crontab(&lines)?;
    println!("✅ crontab 单行: {}", scheduler::client_cron_line(base));
    Ok(())
}

fn install_linux(base: &Path) -> Result<()> {
    write_cron(base)?;

    // 常驻服务 = 系统级 gnpc.service (需 root; 无免密 sudo 时只打印命令)
    let unit = scheduler::client_unit(base)?;
    let unit_path = scheduler::systemd_unit_path(scheduler::SYSTEMD_CLIENT);
    let wrote = std::fs::write(&unit_path, &unit).is_ok();
    if wrote {
        let _ = Command::new("systemctl").args(["daemon-reload"]).status();
        let _ = Command::new("systemctl").args(["enable", "gnpc"]).status();
        println!("✅ systemd {}: {}", scheduler::SYSTEMD_CLIENT, unit_path.display());
    } else if write_unit_via_sudo(base) {
        println!("✅ systemd {} (经 sudo 写入)", scheduler::SYSTEMD_CLIENT);
    } else {
        println!("⚠️  写 {} 需要 root —— 请手动执行:", unit_path.display());
        println!("   sudo tee {} >/dev/null <<< '{}'", unit_path.display(), unit.replace('\n', "\\n"));
        println!("   sudo systemctl daemon-reload && sudo systemctl enable {}", scheduler::SYSTEMD_CLIENT);
    }
    Ok(())
}

/// root 或免密 sudo 时写 unit (aipro 的 sudo 要密码 → 返回 false 让用户手动)
fn write_unit_via_sudo(base: &Path) -> bool {
    let unit = match scheduler::client_unit(base) {
        Ok(u) => u,
        Err(_) => return false,
    };
    let unit_path = scheduler::systemd_unit_path(scheduler::SYSTEMD_CLIENT);
    // 免密探测: 直接问 sudo 能不能免交互执行
    let ok = Command::new("sudo")
        .args(["-n", "true"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return false;
    }
    use std::io::Write;
    let mut child = match Command::new("sudo")
        .arg("tee")
        .arg(&unit_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    if let Some(mut sin) = child.stdin.take() {
        let _ = sin.write_all(unit.as_bytes());
    }
    match child.wait() {
        Ok(st) if st.success() => {
            let _ = Command::new("sudo").args(["systemctl", "daemon-reload"]).status();
            let _ = Command::new("sudo").args(["systemctl", "enable", "gnpc"]).status();
            true
        }
        _ => false,
    }
}

// --- 旧调度清理 ---

/// 卸载旧 launchd + 剔旧 cron + disable 旧 systemd (幂等)
pub fn purge_legacy() -> Vec<String> {
    let mut done = Vec::new();
    match Platform::detect() {
        Platform::MacOs => done.extend(scheduler::remove_legacy_launchd()),
        Platform::Linux => {
            let lines = scheduler::read_crontab();
            let kept = scheduler::purge_gnp_cron_lines(&lines);
            if kept.len() != lines.len() {
                if scheduler::write_crontab(&kept).is_ok() {
                    done.push(format!(
                        "crontab 剔除旧 gnp 行 {} 条",
                        lines.len() - kept.len()
                    ));
                }
            }
            done.extend(scheduler::disable_legacy_systemd());
            // aipro 的旧 --user sing-box (需要该用户自己的会话)
            if let Some(user) = current_user() {
                done.extend(scheduler::disable_legacy_systemd_user(&user));
            }
        }
        _ => {}
    }
    done
}

fn current_user() -> Option<String> {
    std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("LOGNAME").ok())
        .filter(|s| !s.is_empty())
}
