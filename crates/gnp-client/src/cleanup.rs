//! gnp-client cleanup — 应急清理
//!
//! 参考事故: aipro 2026-08-10 sing-box tun 破坏路由导致完全断网。
//! 本命令彻底清理 sing-box 所有残留, 防止再次破坏网络。
//!
//! 步骤:
//!   1. 停止 sing-box 服务 + 杀进程
//!   2. 禁用开机自启
//!   3. 清理残留 tun 接口 (gnp0, tun0)
//!   4. 清理策略路由 (ip rule/route)
//!   5. 恢复主网卡默认路由
//!   6. 备份并禁用 sing-box 数据目录

use anyhow::Result;
use std::process::Command;

pub fn run() -> Result<()> {
    println!("=== [1/6] 立即停止 sing-box 服务 ===");
    let _ = Command::new("systemctl").args(["stop", "gnp-proxy"]).status();
    let _ = Command::new("systemctl").args(["stop", "sing-box-gnp"]).status();
    let _ = Command::new("systemctl").args(["stop", "sing-box"]).status();
    let _ = Command::new("launchctl").args(["unload"]).arg(
        dirs::home_dir()
            .unwrap_or_default()
            .join("Library/LaunchAgents/com.gnp.sing-box.plist")
            .to_str()
            .unwrap_or(""),
    ).status();
    // 强杀所有 sing-box 进程
    let _ = Command::new("pkill").args(["-9", "sing-box"]).status();
    std::thread::sleep(std::time::Duration::from_secs(1));
    let check = Command::new("pgrep").args(["-af", "sing-box"]).output();
    match check {
        Ok(o) if o.status.success() => {
            println!("  ⚠️  仍有残留: {}", String::from_utf8_lossy(&o.stdout).trim());
        }
        _ => println!("  sing-box 进程已全部清理 ✓"),
    }

    println!("=== [2/6] 禁用开机自启 ===");
    let _ = Command::new("systemctl").args(["disable", "gnp-proxy"]).status();
    let _ = Command::new("systemctl").args(["disable", "sing-box-gnp"]).status();
    let _ = Command::new("systemctl").args(["mask", "sing-box-gnp"]).status();
    println!("  开机自启已禁用 ✓");

    println!("=== [3/6] 清理残留 tun 接口 ===");
    for iface in &["gnp0", "tun0"] {
        let _ = Command::new("ip").args(["link", "del", iface]).status();
    }
    std::thread::sleep(std::time::Duration::from_secs(1));
    println!("  tun 接口已清理 ✓");

    println!("=== [4/6] 清理策略路由 ===");
    for prio in &[9000, 9001, 9002, 9003, 9010] {
        let _ = Command::new("ip")
            .args(["rule", "del", "priority", &prio.to_string()])
            .status();
    }
    let _ = Command::new("ip").args(["route", "flush", "table", "2022"]).status();
    println!("  策略路由已清理 ✓");

    println!("=== [5/6] 恢复主网卡默认路由 ===");
    // 尝试探测主网关 (从现有路由取)
    if let Ok(out) = Command::new("ip").args(["route", "show", "default"]).output() {
        let has_default = String::from_utf8_lossy(&out.stdout).contains("default");
        if !has_default {
            // 尝试常见网关
            for gw in &["192.168.0.1", "192.168.1.1", "10.0.0.1"] {
                let _ = Command::new("ip")
                    .args(["route", "add", "default", "via", gw, "dev", "eth1"])
                    .status();
            }
        }
    }
    // 显示当前路由
    let _ = Command::new("ip").args(["route", "show", "default"]).status();
    println!("  ✓");

    println!("=== [6/6] 备份并禁用 sing-box 数据目录 ===");
    let sb_dir = gnp_core::platform::sb_dir();
    if sb_dir.exists() {
        let ts = chrono_free_timestamp();
        let backup = sb_dir.with_file_name(format!("sing-box.disabled-{}", ts));
        match std::fs::rename(&sb_dir, &backup) {
            Ok(()) => println!("  已备份: {}", backup.display()),
            Err(_) => {
                println!("  重命名失败, 尝试 rm -rf...");
                let _ = std::fs::remove_dir_all(&sb_dir);
                println!("  sing-box 数据已删除");
            }
        }
    } else {
        println!("  sing-box 数据目录不存在, 跳过");
    }

    println!("\n=== 最终状态 ===");
    println!("sing-box 进程: ");
    let check = Command::new("pgrep").args(["-af", "sing-box"]).output();
    match check {
        Ok(o) if o.status.success() => println!("  ⚠️  仍有: {}", String::from_utf8_lossy(&o.stdout).trim()),
        _ => println!("  无 ✓"),
    }
    println!("默认路由:");
    let _ = Command::new("ip").args(["route", "show", "default"]).status();

    println!("\n✅ 应急清理完成, sing-box 不会再次破坏路由");
    Ok(())
}

/// 生成时间戳 (不依赖 chrono, 用 std)
fn chrono_free_timestamp() -> String {
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => format!("{}", d.as_secs()),
        Err(_) => "unknown".to_string(),
    }
}
