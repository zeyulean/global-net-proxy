//! 系统代理管理 — 跨平台 HTTP/HTTPS/SOCKS 代理开关
//!
//! macOS: 用 networksetup 设置/取消系统代理，通过 osascript 弹管理员授权窗口。
//! Linux: 用 gsettings 设置 GNOME 系统代理；不支持时回退提示 export。
//!
//! 代理目标固定为 sing-box mixed 代理: 127.0.0.1:1080 (socks5+http)。

use anyhow::{bail, Context, Result};
use std::process::Command;

use crate::platform;

/// 默认代理地址
const PROXY_HOST: &str = "127.0.0.1";
/// 默认代理端口
const PROXY_PORT: u16 = 1080;

// ===========================================================================
//  公开 API
// ===========================================================================

/// 开启系统代理
///
/// macOS: 设置 Web Proxy / Secure Web Proxy / SOCKS Firewall Proxy，
///        通过 osascript 弹管理员授权窗口 (不存密码)。
/// Linux: gsettings 设置 GNOME 系统代理 (manual 模式)。
pub fn enable() -> Result<()> {
    let p = platform::ensure_supported()?;
    match p {
        platform::Platform::MacOs => enable_macos(),
        platform::Platform::Linux => enable_linux(),
        platform::Platform::Windows => enable_windows(),
        _ => unreachable!(),
    }
}

/// 关闭系统代理
///
/// macOS: 关闭所有代理状态。
/// Linux: gsettings 切回 'none' 模式。
pub fn disable() -> Result<()> {
    let p = platform::ensure_supported()?;
    match p {
        platform::Platform::MacOs => disable_macos(),
        platform::Platform::Linux => disable_linux(),
        platform::Platform::Windows => disable_windows(),
        _ => unreachable!(),
    }
}

/// 查看系统代理状态
///
/// macOS: 读取 networksetup 查询三类代理当前状态。
/// Linux: 读取 gsettings 当前模式。
pub fn status() -> Result<()> {
    let p = platform::ensure_supported()?;
    match p {
        platform::Platform::MacOs => status_macos(),
        platform::Platform::Linux => status_linux(),
        platform::Platform::Windows => status_windows(),
        _ => unreachable!(),
    }
}

// ===========================================================================
//  macOS 实现
// ===========================================================================

/// 检测活跃网络服务名 (如 "Wi-Fi" 或 "Ethernet")
///
/// 1. 用 `route -n get default` 找默认路由接口 (如 en0)
/// 2. 用 `networksetup -listallhardwareports` 找接口对应的硬件端口名
fn macos_active_service() -> Result<String> {
    // 1. 找默认路由接口
    let route_out = Command::new("route")
        .args(["-n", "get", "default"])
        .output()
        .context("执行 route -n get default 失败")?;
    let route_text = String::from_utf8_lossy(&route_out.stdout);

    let interface = route_text
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            trimmed.strip_prefix("interface:").map(|v| v.trim().to_string())
        })
        .context("无法检测默认路由接口")?;

    // 2. 找接口对应的硬件端口名
    let hw_out = Command::new("networksetup")
        .args(["-listallhardwareports"])
        .output()
        .context("执行 networksetup -listallhardwareports 失败")?;
    let hw_text = String::from_utf8_lossy(&hw_out.stdout);

    // 解析硬件端口列表，找匹配接口名的端口
    let mut current_port: Option<String> = None;
    for line in hw_text.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("Hardware Port:") {
            current_port = Some(name.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("Device:") {
            if rest.trim() == interface {
                return current_port.context(format!("找到接口 {} 但无对应端口名", interface));
            }
        }
    }

    // 回退: 尝试常见名称
    for candidate in &["Wi-Fi", "Ethernet", "以太网"] {
        let test = Command::new("networksetup")
            .args(["-getwebproxy", candidate])
            .output();
        if let Ok(o) = test {
            if o.status.success() {
                return Ok(candidate.to_string());
            }
        }
    }

    bail!(
        "无法确定活跃网络服务名 (接口: {}). 请手动指定",
        interface
    )
}

/// 用 osascript 以管理员权限执行 shell 命令
fn run_with_admin(command: &str) -> Result<()> {
    // osascript 的 do shell script 需要对内部双引号转义
    let escaped = command.replace('\\', "\\\\").replace('\"', "\\\"");
    let script = format!(
        "do shell script \"{}\" with administrator privileges",
        escaped
    );

    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .context("执行 osascript 失败")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // 用户取消授权
        if stderr.contains("-128") || stderr.contains("User canceled") {
            bail!("用户取消了管理员授权");
        }
        bail!("networksetup 执行失败: {}", stderr.trim());
    }

    Ok(())
}

/// macOS 开启系统代理
fn enable_macos() -> Result<()> {
    let service = macos_active_service()?;
    println!("🌐 网络服务: {}", service);

    // 构建一条组合命令，只弹一次授权窗口
    let cmd = format!(
        "networksetup -setwebproxy '{s}' {h} {p} && \
         networksetup -setsecurewebproxy '{s}' {h} {p} && \
         networksetup -setsocksfirewallproxy '{s}' {h} {p}",
        s = service,
        h = PROXY_HOST,
        p = PROXY_PORT
    );

    println!("🔑 正在请求管理员授权 (osascript 弹窗)...");
    run_with_admin(&cmd)?;

    println!(
        "✅ 系统代理已开启 ({}:{} → {})",
        PROXY_HOST, PROXY_PORT, service
    );
    Ok(())
}

/// macOS 关闭系统代理
fn disable_macos() -> Result<()> {
    let service = macos_active_service()?;
    println!("🌐 网络服务: {}", service);

    let cmd = format!(
        "networksetup -setwebproxystate '{s}' off && \
         networksetup -setsecurewebproxystate '{s}' off && \
         networksetup -setsocksfirewallproxystate '{s}' off",
        s = service
    );

    println!("🔑 正在请求管理员授权 (osascript 弹窗)...");
    run_with_admin(&cmd)?;

    println!("✅ 系统代理已关闭 ({})", service);
    Ok(())
}

/// macOS 查看代理状态
fn status_macos() -> Result<()> {
    let service = macos_active_service()?;
    println!("🌐 网络服务: {}\n", service);

    let proxies = [
        ("HTTP 代理", "-getwebproxy"),
        ("HTTPS 代理", "-getsecurewebproxy"),
        ("SOCKS 代理", "-getsocksfirewallproxy"),
    ];

    let mut all_enabled = true;
    for (name, flag) in &proxies {
        let out = Command::new("networksetup")
            .args([flag, service.as_str()])
            .output()
            .context(format!("执行 networksetup {} 失败", flag))?;
        let text = String::from_utf8_lossy(&out.stdout);
        let enabled = text.lines().any(|l| {
            l.trim().eq_ignore_ascii_case("Enabled: Yes")
        });
        if !enabled {
            all_enabled = false;
        }
        let mark = if enabled { "✅" } else { "⬛" };
        println!("{} {}: (查询无需授权)", mark, name);
        for line in text.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                println!("    {}", trimmed);
            }
        }
    }

    println!();
    if all_enabled {
        println!("📊 状态: 系统代理已全部开启 ✅");
    } else {
        println!("📊 状态: 系统代理未完全开启 ⬛");
    }
    Ok(())
}

// ===========================================================================
//  Linux 实现
// ===========================================================================

/// Linux 开启系统代理 (GNOME gsettings)
fn enable_linux() -> Result<()> {
    // 检查 gsettings 是否可用
    let has_gsettings = Command::new("which")
        .args(["gsettings"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_gsettings {
        println!("⚠️  未检测到 gsettings (非 GNOME 桌面环境)");
        println!("请手动设置环境变量:");
        println!("  export http_proxy=http://{}:{}", PROXY_HOST, PROXY_PORT);
        println!("  export https_proxy=http://{}:{}", PROXY_HOST, PROXY_PORT);
        println!("  export all_proxy=socks5://{}:{}", PROXY_HOST, PROXY_PORT);
        return Ok(());
    }

    // 设置 GNOME 系统代理
    let host_str = format!("'{}'", PROXY_HOST);
    let port_str = format!("{}", PROXY_PORT);

    let cmds = [
        ("org.gnome.system.proxy", "mode", "'manual'"),
        ("org.gnome.system.proxy.http", "host", &host_str),
        ("org.gnome.system.proxy.http", "port", &port_str),
        ("org.gnome.system.proxy.https", "host", &host_str),
        ("org.gnome.system.proxy.https", "port", &port_str),
        ("org.gnome.system.proxy.socks", "host", &host_str),
        ("org.gnome.system.proxy.socks", "port", &port_str),
    ];

    for (schema, key, value) in &cmds {
        let out = Command::new("gsettings")
            .args(["set", schema, key, value])
            .output()
            .context(format!("gsettings set {}.{} 失败", schema, key))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            bail!("gsettings set {}.{} 失败: {}", schema, key, stderr.trim());
        }
    }

    println!(
        "✅ GNOME 系统代理已开启 ({}:{})",
        PROXY_HOST, PROXY_PORT
    );
    Ok(())
}

/// Linux 关闭系统代理 (GNOME gsettings)
fn disable_linux() -> Result<()> {
    let has_gsettings = Command::new("which")
        .args(["gsettings"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_gsettings {
        println!("⚠️  未检测到 gsettings");
        println!("请手动清除环境变量:");
        println!("  unset http_proxy https_proxy all_proxy");
        return Ok(());
    }

    let out = Command::new("gsettings")
        .args(["set", "org.gnome.system.proxy", "mode", "'none'"])
        .output()
        .context("gsettings set mode none 失败")?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("关闭 GNOME 代理失败: {}", stderr.trim());
    }

    println!("✅ GNOME 系统代理已关闭");
    Ok(())
}

/// Linux 查看代理状态 (GNOME gsettings)
fn status_linux() -> Result<()> {
    let has_gsettings = Command::new("which")
        .args(["gsettings"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_gsettings {
        println!("⚠️  未检测到 gsettings (非 GNOME 桌面环境)");
        println!("当前环境变量:");
        for key in &["http_proxy", "https_proxy", "all_proxy"] {
            let val = std::env::var(key).unwrap_or_else(|_| "(未设置)".to_string());
            println!("  {} = {}", key, val);
        }
        return Ok(());
    }

    // 读取 GNOME 代理模式
    let out = Command::new("gsettings")
        .args(["get", "org.gnome.system.proxy", "mode"])
        .output()
        .context("gsettings get mode 失败")?;

    let mode = String::from_utf8_lossy(&out.stdout).trim().to_string();
    println!("== GNOME 系统代理状态 ==\n");
    println!("  模式: {}", mode);

    if mode.contains("manual") {
        println!("  状态: 已开启 ✅");
        // 显示配置详情
        for (name, schema) in [
            ("HTTP", "org.gnome.system.proxy.http"),
            ("HTTPS", "org.gnome.system.proxy.https"),
            ("SOCKS", "org.gnome.system.proxy.socks"),
        ] {
            let host = Command::new("gsettings")
                .args(["get", schema, "host"])
                .output();
            let port = Command::new("gsettings")
                .args(["get", schema, "port"])
                .output();
            if let (Ok(h), Ok(p)) = (host, port) {
                let h_val = String::from_utf8_lossy(&h.stdout).trim().to_string();
                let p_val = String::from_utf8_lossy(&p.stdout).trim().to_string();
                println!("  {}: {}:{}", name, h_val, p_val);
            }
        }
    } else {
        println!("  状态: 已关闭 ⬛");
    }

    Ok(())
}

// ===========================================================================
//  Windows 实现 (注册表 WinINET 代理 + InternetSetOption 刷新)
// ===========================================================================

/// 以 -EncodedCommand 运行 PowerShell 脚本 (UTF-16LE base64, 免引号转义地狱)
fn run_ps(script: &str) -> Result<String> {
    let b64 = crate::platform::ps_encode(script);
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &b64])
        .output()
        .context("powershell 执行失败")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        bail!("powershell 脚本失败: {}", text.trim());
    }
    Ok(text)
}

/// WinINET 刷新片段 (让浏览器等立即感知代理变化, 无需重新登录)
const PS_REFRESH: &str = r#"
Add-Type -MemberDefinition '[DllImport("wininet.dll", SetLastError=true)] public static extern bool InternetSetOption(IntPtr h, int o, IntPtr b, int l);' -Name N -Namespace W
[W.W]::InternetSetOption([IntPtr]::Zero, 39, [IntPtr]::Zero, 0) | Out-Null
[W.W]::InternetSetOption([IntPtr]::Zero, 37, [IntPtr]::Zero, 0) | Out-Null
"#;

/// Windows 开启系统代理 (当前用户, 注册表 + 刷新)
fn enable_windows() -> Result<()> {
    let script = format!(
        r#"$reg = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
Set-ItemProperty -Path $reg -Name ProxyEnable -Value 1
Set-ItemProperty -Path $reg -Name ProxyServer -Value '{h}:{p}'
Set-ItemProperty -Path $reg -Name ProxyOverride -Value 'localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.2*;172.30.*;172.31.*;192.168.*;<local>'
{refresh}
Write-Output 'OK'
"#,
        h = PROXY_HOST,
        p = PROXY_PORT,
        refresh = PS_REFRESH
    );
    run_ps(&script)?;
    println!("✅ Windows 系统代理已开启 ({}:{} → WinINET)", PROXY_HOST, PROXY_PORT);
    println!("   浏览器/系统应用即时生效; 需要走代理的 CLI 工具请设 HTTP_PROXY 环境变量");
    Ok(())
}

/// Windows 关闭系统代理
fn disable_windows() -> Result<()> {
    let script = format!(
        r#"$reg = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
Set-ItemProperty -Path $reg -Name ProxyEnable -Value 0
{refresh}
Write-Output 'OK'
"#,
        refresh = PS_REFRESH
    );
    run_ps(&script)?;
    println!("✅ Windows 系统代理已关闭");
    Ok(())
}

/// Windows 查看代理状态
fn status_windows() -> Result<()> {
    let script = r#"$reg = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
$v = Get-ItemProperty -Path $reg
Write-Output ("ProxyEnable: " + $v.ProxyEnable)
Write-Output ("ProxyServer: " + $v.ProxyServer)
"#;
    let out = run_ps(script)?;
    println!("== Windows 系统代理状态 (WinINET) ==\n");
    for line in out.lines().filter(|l| l.contains(':')) {
        let enabled = line.contains("ProxyEnable: 1");
        let mark = if enabled { "✅ 已开启" } else { "⬛ 已关闭" };
        println!("  {} {}", line.trim(), if line.starts_with("ProxyEnable") { mark } else { "" });
    }
    Ok(())
}
