//! gnp-client recover — 断网恢复
//!
//! aipro 事故 (2026-08-10) 后遗留的断网恢复工具。
//! sing-box tun 模式接管系统路由表导致断网, 本命令恢复网络。
//!
//! 步骤:
//!   1. 停止 sing-box 服务 (破坏路由的元凶)
//!   2. 清理策略路由 (ip rule flush)
//!   3. 清理独立路由表 (table 2022/100/200)
//!   4. 恢复主网卡默认路由
//!   5. 清理残留 tun 接口
//!   6. 恢复系统 DNS

use anyhow::Result;
use std::process::Command;

pub fn run() -> Result<()> {
    println!("[1/5] 停止 sing-box 服务 (破坏路由的元凶)");
    let _ = Command::new("systemctl").args(["stop", "gnp-proxy"]).status();
    let _ = Command::new("systemctl")
        .args(["stop", "sing-box-gnp"])
        .status();
    let _ = Command::new("systemctl")
        .args(["disable", "sing-box-gnp"])
        .status();
    let _ = Command::new("launchctl")
        .args(["unload"])
        .arg(
            dirs::home_dir()
                .unwrap_or_default()
                .join("Library/LaunchAgents/com.gnp.sing-box.plist")
                .to_str()
                .unwrap_or(""),
        )
        .status();

    println!("[2/5] 清理 sing-box 添加的策略路由");
    let _ = Command::new("ip").args(["rule", "flush"]).status();

    println!("[3/5] 清理 sing-box 添加的独立路由表");
    for table in &["2022", "100", "200"] {
        let _ = Command::new("ip")
            .args(["route", "flush", "table", table])
            .status();
    }

    println!("[4/5] 恢复主网卡默认路由");
    // 尝试常见网关/网卡组合
    for (gw, dev) in &[
        ("192.168.0.1", "eth1"),
        ("192.168.0.1", "eth0"),
        ("192.168.1.1", "eth0"),
        ("10.0.0.1", "eth0"),
    ] {
        let _ = Command::new("ip")
            .args(["route", "add", "default", "via", gw, "dev", dev])
            .status();
    }

    println!("[5/5] 清理残留 tun 接口");
    for iface in &["gnp0", "tun0"] {
        let _ = Command::new("ip")
            .args(["link", "del", iface])
            .status();
    }

    // 恢复 DNS
    println!("\n--- 恢复系统 DNS ---");
    let _ = Command::new("sh")
        .args(["-c", "echo 'nameserver 223.5.5.5' > /etc/resolv.conf"])
        .status();
    let _ = Command::new("sh")
        .args(["-c", "echo 'nameserver 119.29.29.29' >> /etc/resolv.conf"])
        .status();

    println!("\n=== 验证 ===");
    let _ = Command::new("ip").args(["route", "show", "default"]).status();

    println!("\n✅ 网络应已恢复。若仍不通, 直接重启: reboot");
    Ok(())
}
