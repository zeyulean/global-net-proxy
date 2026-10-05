//! 平台检测与路径管理
//!
//! gnp 跨平台支持 (v2 命名):
//! - macOS:   launchd `com.gnpc.singbox` (常驻) + `com.gnpc.tick` (调度)
//! - Linux:   systemd `gnpc.service` (客户端) / `gnps.service` (服务端) + crontab 单行 tick
//! - Windows: schtasks 计划任务 `gnpc` (开机自启)
//!
//! 部署根目录 (v2 路径收敛, 2026-10-05 plan D4):
//! - 客户端 `~/.local/gnp/`  = `$GNP_HOME` || `~/.local/gnp`
//! - 服务端 `/opt/gnp/`      = `$GNP_SERVER_HOME` || `/opt/gnp`
//!
//! 布局:
//! ```text
//! <gnp_home>/
//!   bin/{gnpc,sing-box,tick.sh}   config.toml(唯一事实源)   config.json(生成物)
//!   etc/tick.d/*.sh   rules/*.srs   secrets/   var/   backups/
//! ```

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// hy2 默认端口 (plan D4: 代码默认值 5766, toml `[server].hy2_port` 可覆盖)
pub const GNP_PORT: u16 = 5766;

/// clash_api 默认端口 (switch/status/guard 依赖)
pub const GNP_CLASH_API_PORT: u16 = 9090;

/// `$GNP_HOME` 的原值 (未设置/为空则 None)
pub fn gnp_home_from_env() -> Option<PathBuf> {
    std::env::var_os("GNP_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// 客户端部署根目录: `$GNP_HOME` || `~/.local/gnp`
pub fn gnp_home() -> PathBuf {
    gnp_home_from_env().unwrap_or_else(|| home_dir().join(".local/gnp"))
}

/// 服务端部署根目录: `$GNP_SERVER_HOME` || `/opt/gnp`
pub fn gnps_home() -> PathBuf {
    if let Some(h) = std::env::var_os("GNP_SERVER_HOME") {
        let p = PathBuf::from(h);
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    PathBuf::from("/opt/gnp")
}

/// 用户主目录
pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// "用户 home" —— GNP_HOME 显式指定时按它推导, 否则用 $HOME
///
/// sudo 跑 (`sudo GNP_HOME=/home/lwboy/.local/gnp gnpc ...`) 时 $HOME=/root,
/// 但配置里的 `~/.ssh/id_ed25519` 指的是**目标用户**的家目录。
/// 展开错了会让 sing-box check 直接 FATAL (aipro 实迁踩过)。
pub fn user_home() -> PathBuf {
    target_home().unwrap_or_else(home_dir)
}

/// 展开 `~` 前缀 (config.toml 里 ssh_key 允许写 `~/.ssh/id_ed25519`)
pub fn expand_tilde(s: &str) -> PathBuf {
    let home = user_home();
    if s == "~" {
        return home;
    }
    if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        return home.join(rest);
    }
    PathBuf::from(s)
}

/// 创建 `<root>` 下的 gnp 目录骨架 (幂等)
pub fn ensure_layout(root: &Path) -> Result<()> {
    for sub in [
        "bin",
        "etc/tick.d",
        "rules",
        "var",
        "secrets",
        "backups",
    ] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    Ok(())
}

// --- 客户端派生路径 ---

pub fn gnp_bin_dir() -> PathBuf {
    gnp_home().join("bin")
}
/// sing-box 二进制 (与 gnpc/tick.sh 同目录)
pub fn gnp_sb_bin() -> PathBuf {
    gnp_bin_dir().join(if cfg!(windows) { "sing-box.exe" } else { "sing-box" })
}
pub fn gnp_client_bin() -> PathBuf {
    gnp_bin_dir().join(if cfg!(windows) { "gnpc.exe" } else { "gnpc" })
}
pub fn gnp_server_bin() -> PathBuf {
    gnp_bin_dir().join(if cfg!(windows) { "gnps.exe" } else { "gnps" })
}
pub fn gnp_tick_sh() -> PathBuf {
    gnp_bin_dir().join("tick.sh")
}
/// 唯一事实源 (0600)
pub fn gnp_config_toml() -> PathBuf {
    gnp_home().join("config.toml")
}
/// 生成物 (勿手改)
pub fn gnp_config_json() -> PathBuf {
    gnp_home().join("config.json")
}
pub fn gnp_var_dir() -> PathBuf {
    gnp_home().join("var")
}
pub fn gnp_cache_db() -> PathBuf {
    gnp_var_dir().join("cache.db")
}
pub fn gnp_rules_dir() -> PathBuf {
    gnp_home().join("rules")
}
pub fn gnp_secrets_dir() -> PathBuf {
    gnp_home().join("secrets")
}
pub fn gnp_backups_dir() -> PathBuf {
    gnp_home().join("backups")
}
pub fn gnp_tick_d() -> PathBuf {
    gnp_home().join("etc/tick.d")
}
pub fn gnp_tick_log() -> PathBuf {
    gnp_var_dir().join("tick.log")
}
pub fn gnp_guard_state() -> PathBuf {
    gnp_var_dir().join("guard-state.json")
}
pub fn gnp_guard_log() -> PathBuf {
    gnp_var_dir().join("guard.log")
}
pub fn gnp_singbox_log() -> PathBuf {
    gnp_var_dir().join("sing-box.log")
}
pub fn gnp_singbox_err() -> PathBuf {
    gnp_var_dir().join("sing-box.err")
}
/// hy2 密码 secret (aipro 路由容器挂载源; 必须带换行 — 坑清单 #3)
pub fn gnp_secret_hy2_password() -> PathBuf {
    gnp_secrets_dir().join("hy2-password")
}
pub fn gnp_secret_hy2_obfs() -> PathBuf {
    gnp_secrets_dir().join("hy2-obfs")
}

// --- 服务端派生路径 ---

pub fn gnps_bin_dir() -> PathBuf {
    gnps_home().join("bin")
}
pub fn gnps_sb_bin() -> PathBuf {
    gnps_bin_dir().join("sing-box")
}
pub fn gnps_tick_sh() -> PathBuf {
    gnps_bin_dir().join("tick.sh")
}
pub fn gnps_config_toml() -> PathBuf {
    gnps_home().join("config.toml")
}
pub fn gnps_config_json() -> PathBuf {
    gnps_home().join("config.json")
}
pub fn gnps_certs_dir() -> PathBuf {
    gnps_home().join("certs")
}
pub fn gnps_cert_crt() -> PathBuf {
    gnps_certs_dir().join("server.crt")
}
pub fn gnps_cert_key() -> PathBuf {
    gnps_certs_dir().join("server.key")
}
pub fn gnps_var_dir() -> PathBuf {
    gnps_home().join("var")
}
pub fn gnps_backups_dir() -> PathBuf {
    gnps_home().join("backups")
}
pub fn gnps_pending_dir() -> PathBuf {
    gnps_home().join("pending-users")
}
pub fn gnps_tick_d() -> PathBuf {
    gnps_home().join("etc/tick.d")
}

// --- 旧布局 (migrate 探测用) ---

/// v1 客户端数据目录 `~/.local/share/sing-box/` (migrate 的迁移源)
///
/// **GNP_HOME 显式指定时按同一 home 推导**, 而不是按当前进程的 `$HOME`:
/// `sudo GNP_HOME=/home/lwboy/.local/gnp gnpc migrate` 时 $HOME=/root,
/// 按 $HOME 找会跑到 /root/.local/share/sing-box (不存在) → 资产搬不过去。
pub fn legacy_sb_dir() -> PathBuf {
    if let Some(base) = gnp_home_from_env() {
        // 标准布局 <home>/.local/gnp → 旧布局 <home>/.local/share/sing-box
        if let Some(local) = base.parent() {
            if local.file_name().and_then(|n| n.to_str()) == Some(".local") {
                if let Some(home) = local.parent() {
                    return home.join(".local/share/sing-box");
                }
            }
        }
        // 非标准布局: 就近找
        return base.join("share/sing-box");
    }
    home_dir().join(".local/share/sing-box")
}

/// GNP_HOME 指向的"用户 home" (即 <home>/.local 的上一级); 推不出来返回 None
pub fn target_home() -> Option<PathBuf> {
    let base = gnp_home_from_env()?;
    let local = base.parent()?;
    if local.file_name().and_then(|n| n.to_str()) == Some(".local") {
        return local.parent().map(|h| h.to_path_buf());
    }
    None
}

/// v1 服务端目录 `/opt/gnp-quic/` (lwtop)
pub fn legacy_server_dir() -> PathBuf {
    PathBuf::from("/opt/gnp-quic")
}

// --- 存在性断言 ---

/// sing-box 二进制是否存在
pub fn sb_exists() -> bool {
    gnp_sb_bin().exists()
}

/// config.json 是否存在
pub fn config_exists() -> bool {
    gnp_config_json().exists()
}

/// config.toml 是否存在 (唯一事实源)
pub fn settings_exist() -> bool {
    gnp_config_toml().exists()
}

/// 平台枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
    Other,
}

impl Platform {
    pub fn detect() -> Self {
        match std::env::consts::OS {
            "macos" => Platform::MacOs,
            "linux" => Platform::Linux,
            "windows" => Platform::Windows,
            _ => Platform::Other,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::MacOs => "macos",
            Platform::Linux => "linux",
            Platform::Windows => "windows",
            Platform::Other => "other",
        }
    }
}

/// 检查平台是否受支持
pub fn ensure_supported() -> Result<Platform> {
    let p = Platform::detect();
    match p {
        Platform::MacOs | Platform::Linux | Platform::Windows => Ok(p),
        _ => bail!("不支持的平台: {}", std::env::consts::OS),
    }
}

/// 断言 sing-box 已安装 (二进制 + 生成物 config.json)
pub fn ensure_installed() -> Result<()> {
    if !sb_exists() {
        bail!("sing-box 未安装! 二进制不存在: {}", gnp_sb_bin().display());
    }
    if !config_exists() {
        bail!(
            "配置不存在: {}。请先运行 `gnpc install` 生成。",
            gnp_config_json().display()
        );
    }
    Ok(())
}

/// 把路径写进 secret 文件 (0600, **带换行** — 坑清单 #3: read 无换行返回非零)
pub fn write_secret(path: &Path, value: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("创建 secrets 目录失败: {}", parent.display()))?;
    }
    let body = if value.ends_with('\n') {
        value.to_string()
    } else {
        format!("{}\n", value)
    };
    std::fs::write(path, body)
        .with_context(|| format!("写 secret 失败: {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("设置 0600 失败: {}", path.display()))?;
    }
    Ok(())
}

/// config.toml 权限 0600 (含内联密码, D2)
pub fn write_private(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, content)
        .with_context(|| format!("写 {} 失败", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).ok();
    }
    Ok(())
}

/// PowerShell -EncodedCommand 编码 (UTF-16LE + base64)
///
/// 免引号转义地狱: 任意脚本编码后经 `powershell -NoProfile -EncodedCommand <b64>` 执行
pub fn ps_encode(script: &str) -> String {
    const TBL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let utf16: Vec<u8> = script
        .encode_utf16()
        .flat_map(|c| c.to_le_bytes())
        .collect();
    let mut out = String::new();
    for chunk in utf16.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TBL[(n >> 18) as usize & 63] as char);
        out.push(TBL[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TBL[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TBL[n as usize & 63] as char } else { '=' });
    }
    out
}

/// 本地端口是否有东西在听 (跨平台, 直接 TCP 探测, 不依赖 lsof/netstat)
pub fn port_open(port: u16) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    match format!("127.0.0.1:{}", port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next())
    {
        Some(a) => TcpStream::connect_timeout(&a, std::time::Duration::from_millis(600)).is_ok(),
        None => false,
    }
}
