//! 进程 / 服务管理 — 跨平台 (Mac launchd / Linux systemd / Windows schtasks)
//!
//! v2 命名 (plan §1.1): 常驻 sing-box = `com.gnpc.singbox` / `gnpc.service` / 计划任务 `gnpc`。
//! 调度 (tick.sh) 是**另一件事**, 见 [`crate::scheduler`]; 本模块只管常驻本体。
//!
//! 好处: 开机自启、崩溃自动重启 (KeepAlive/Restart=on-failure)、系统级管理。

use crate::platform::{gnp_bin_dir, gnp_config_json, gnp_sb_bin, Platform};
use crate::scheduler;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

/// launchd plist 标签 (macOS 常驻 sing-box)
pub const LAUNCHD_LABEL: &str = scheduler::LAUNCHD_SINGBOX;
/// systemd service 名称 (Linux 客户端)
pub const SYSTEMD_SERVICE: &str = scheduler::SYSTEMD_CLIENT;

/// 获取 launchd plist 路径
pub fn launchd_plist() -> PathBuf {
    scheduler::launchd_plist_path(scheduler::LAUNCHD_SINGBOX)
}

/// 生成 launchd plist 内容 (macOS 开机自启)
///
/// 资产在 `deploy/scheduler/com.gnpc.singbox.plist`, 由 gnpc `include_str!` 嵌入。
pub fn launchd_plist_content() -> Result<String> {
    let base = crate::platform::gnp_home();
    scheduler::client_plist_singbox(&base)
}

/// 生成 systemd 单元内容 (Linux 客户端, 系统级)
pub fn systemd_unit_content() -> Result<String> {
    let base = crate::platform::gnp_home();
    scheduler::client_unit(&base)
}

/// Linux systemd 系统级服务单元路径 (/etc/systemd/system/gnpc.service)
pub fn systemd_system_unit_path() -> PathBuf {
    scheduler::systemd_unit_path(scheduler::SYSTEMD_CLIENT)
}

/// 安装 Linux systemd 系统服务: 写 unit 文件 + daemon-reload + enable
///
/// 需要 root 权限 (写 /etc/systemd/system/)。
pub fn install_linux() -> Result<()> {
    install_linux_named(SYSTEMD_SERVICE, &systemd_unit_content()?)
}

/// 同上, 但服务名与单元内容由调用方给 (服务端 gnps 复用)
pub fn install_linux_named(name: &str, unit: &str) -> Result<()> {
    write_unit_privileged(name, unit)?;
    let _ = systemctl_run(&["daemon-reload"]);
    let _ = systemctl_run(&["enable", name]);
    println!(
        "✅ Linux systemd 系统服务已安装: {}",
        scheduler::systemd_unit_path(name).display()
    );
    Ok(())
}

/// 启动 sing-box
pub fn start(platform: Platform) -> Result<()> {
    match platform {
        Platform::MacOs => start_macos(),
        Platform::Linux => start_linux(),
        Platform::Windows => start_windows(),
        _ => bail!("不支持的平台"),
    }
}

/// 停止 sing-box
pub fn stop(platform: Platform) -> Result<()> {
    match platform {
        Platform::MacOs => stop_macos(),
        Platform::Linux => stop_linux(),
        Platform::Windows => stop_windows(),
        _ => bail!("不支持的平台"),
    }
}

/// 检查状态 (返回是否运行中)
pub fn is_running(platform: Platform) -> Result<bool> {
    match platform {
        Platform::MacOs => is_running_macos(),
        Platform::Linux => is_running_linux(),
        Platform::Windows => is_running_windows(),
        _ => bail!("不支持的平台"),
    }
}

// --- systemd 通用 (客户端/服务端同名接口) ---

/// 有免密 sudo 就用 sudo 跑 systemctl, 否则直接跑
///
/// 客户端的 crontab/tick 归**用户** (plan §1.4: 调度路径在 /home/<user>/.local/gnp),
/// 但常驻服务是系统级 gnpc.service —— 写 unit 和 start 都要 root。
/// 于是这里做成"能直连就直连, 直连不行借免密 sudo"; 两者都不行返回 None,
/// 由调用方打印精确命令, 而不是整个迁移失败。
fn systemctl_run(args: &[&str]) -> Option<std::process::ExitStatus> {
    if let Ok(st) = Command::new("systemctl").args(args).status() {
        if st.success() {
            return Some(st);
        }
    }
    // 免密探测 (非交互; aipro 的 sudo 要密码 → 这里直接放弃, 走提示分支)
    let ok = Command::new("sudo")
        .args(["-n", "true"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    let mut full: Vec<&str> = vec!["systemctl"];
    full.extend_from_slice(args);
    Command::new("sudo").args(&full).status().ok()
}

/// `systemctl start <name>` (需 root 或免密 sudo)
pub fn start_named(name: &str) -> Result<()> {
    match systemctl_run(&["start", name]) {
        Some(st) if st.success() => Ok(()),
        _ => bail!("systemctl start {} 失败 (需要 root 或免密 sudo)", name),
    }
}

pub fn stop_named(name: &str) -> Result<()> {
    let _ = systemctl_run(&["stop", name]);
    Ok(())
}

pub fn enable_named(name: &str) -> Result<()> {
    let _ = systemctl_run(&["daemon-reload"]);
    let _ = systemctl_run(&["enable", name]);
    Ok(())
}

/// 有没有 root/免密 sudo (决定要不要打印"请手动执行")
pub fn has_privilege() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok())
        .map(|u| u == 0)
        .unwrap_or(false)
        || Command::new("sudo")
            .args(["-n", "true"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
}

/// 把 unit 内容写进 /etc/systemd/system/<name>.service (root 或免密 sudo)
pub fn write_unit_privileged(name: &str, unit: &str) -> Result<()> {
    let path = crate::scheduler::systemd_unit_path(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if std::fs::write(&path, unit).is_ok() {
        return Ok(());
    }
    use std::io::Write;
    let mut child = Command::new("sudo")
        .arg("tee")
        .arg(&path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("sudo tee 启动失败 (需要免密 sudo)")?;
    if let Some(mut sin) = child.stdin.take() {
        let _ = sin.write_all(unit.as_bytes());
    }
    let st = child.wait().context("sudo tee 等待失败")?;
    if !st.success() {
        bail!("写 {} 失败 (需要 root 或免密 sudo)", path.display());
    }
    Ok(())
}

pub fn is_active_named(name: &str) -> Result<bool> {
    let out = Command::new("systemctl")
        .args(["is-active", name])
        .output()
        .with_context(|| format!("systemctl is-active {} 失败", name))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim() == "active")
}

// --- macOS (launchctl) ---

fn start_macos() -> Result<()> {
    let plist = launchd_plist();
    if !plist.exists() {
        if let Some(parent) = plist.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        // var/ 必须先建, 否则 launchd 的 StandardOutPath 落不了地
        std::fs::create_dir_all(crate::platform::gnp_var_dir()).ok();
        std::fs::write(&plist, launchd_plist_content()?)
            .with_context(|| format!("写入 plist 失败: {}", plist.display()))?;
    }
    // 已加载过先 unload, 保证新内容生效 (幂等)
    // 注意: launchctl 对"未装载/切换中"的 job 会打 EIO 到 stderr, 那是常态不是故障;
    // 一律静音, 否则会经 tick.sh 落进 var/tick.log 冒充告警。
    let _ = Command::new("launchctl")
        .args(["unload", plist.to_str().unwrap_or("")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = Command::new("launchctl")
        .args(["load", plist.to_str().unwrap_or("")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if !is_running_macos()? {
        bail!("sing-box 启动失败 (launchctl load 后未运行)");
    }
    Ok(())
}

fn stop_macos() -> Result<()> {
    let plist = launchd_plist();
    if plist.exists() {
        let _ = Command::new("launchctl")
            .args(["unload", plist.to_str().unwrap_or("")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
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
    // 确保系统服务已安装 (未安装则自动创建, 修复全新部署 install+start 失败)
    if !systemd_system_unit_path().exists() {
        install_linux()?;
    }
    start_named(SYSTEMD_SERVICE)
}

fn stop_linux() -> Result<()> {
    stop_named(SYSTEMD_SERVICE)
}

fn is_running_linux() -> Result<bool> {
    is_active_named(SYSTEMD_SERVICE)
}

// --- Windows (schtasks 计划任务 + taskkill/tasklist) ---

/// 计划任务名 (开机自启; 与 Linux 服务同名 `gnpc`)
const WIN_TASK: &str = scheduler::WIN_TASK;

/// 计划任务是否存在
fn win_task_exists() -> bool {
    Command::new("schtasks")
        .args(["/Query", "/TN", WIN_TASK])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 生成隐形启动 VBS (Windows 控制台程序直接被计划任务拉起会在桌面弹黑窗;
/// wscript Run(...,0) 完全无窗口, 且保持免管理员设计)
fn win_hidden_vbs_path() -> PathBuf {
    gnp_bin_dir().join("gnp-run-hidden.vbs")
}

fn win_write_hidden_vbs() -> Result<PathBuf> {
    let vbs = win_hidden_vbs_path();
    let content = format!(
        "CreateObject(\"WScript.Shell\").Run \"\"\"{}\" run -c \"\"{}\"\"\", 0, False",
        gnp_sb_bin().display(),
        gnp_config_json().display()
    );
    if let Some(parent) = vbs.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&vbs, content)
        .with_context(|| format!("写入隐形启动 VBS 失败: {}", vbs.display()))?;
    Ok(vbs)
}

/// 创建计划任务 (当前用户登录时自启; 无需管理员权限)
///
/// 注: 不用 /RU SYSTEM —— 那需要管理员权限创建; 桌面 Windows 场景
/// ONLOGON(当前用户) 已够用, 且代理写 HKCU 也与用户会话一致。
/// 通过 wscript VBS 隐形启动, 避免桌面弹黑窗 (2026-09-16 lwwin 实测)。
/// vmwin 默认 shell 是 cmd 不是 PowerShell, 保持这条路径。
fn win_task_create() -> Result<()> {
    let vbs = win_write_hidden_vbs()?;
    let tr = format!("wscript.exe \"{}\"", vbs.display());
    let out = Command::new("schtasks")
        .args(["/Create", "/TN", WIN_TASK, "/SC", "ONLOGON", "/TR", &tr, "/F"])
        .output()
        .context("schtasks /Create 失败")?;
    if !out.status.success() {
        bail!(
            "创建计划任务失败: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// Windows 启动: 确保任务存在 → schtasks /Run → 轮询进程
fn start_windows() -> Result<()> {
    if !win_task_exists() {
        win_task_create()?;
        println!("✅ 已创建计划任务 {} (开机自启)", WIN_TASK);
    }
    Command::new("schtasks")
        .args(["/Run", "/TN", WIN_TASK])
        .output()
        .context("schtasks /Run 失败")?;
    // 轮询等待进程出现 (最多 ~8s)
    for _ in 0..16 {
        if is_running_windows()? {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    bail!("sing-box 启动失败 (计划任务已触发但进程未出现, 用 gnpc status 检查配置)");
}

/// Windows 停止: 结束任务 + 杀进程
fn stop_windows() -> Result<()> {
    let _ = Command::new("schtasks")
        .args(["/End", "/TN", WIN_TASK])
        .output();
    let _ = Command::new("taskkill")
        .args(["/F", "/IM", "sing-box.exe"])
        .output();
    Ok(())
}

/// Windows 进程检测
fn is_running_windows() -> Result<bool> {
    let out = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq sing-box.exe", "/NH"])
        .output()
        .context("tasklist 失败")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(text.to_lowercase().contains("sing-box.exe"))
}
