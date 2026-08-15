//! 平台检测与路径管理
//!
//! gnp 跨平台支持:
//! - macOS:   用 launchctl 管理 (~/Library/LaunchAgents/com.gnp.sing-box.plist)
//! - Linux:   用 systemd 管理 (gnp-proxy.service)
//! - Windows: 用 schtasks 计划任务管理 (gnp-singbox, 开机自启 SYSTEM)
//!
//! sing-box 数据目录统一为 ~/.local/share/sing-box/

use anyhow::{bail, Result};
use std::path::PathBuf;

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

/// sing-box 数据目录
///
/// 统一使用 ~/.local/share/sing-box/ (跨平台一致, 不与 macOS 的 Application Support 混淆;
/// Windows 上为 C:\Users\<u>\.local\share\sing-box\, 三端路径一致便于文档统一)
pub fn sb_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/share/sing-box")
}

/// sing-box 二进制路径
pub fn sb_bin() -> PathBuf {
    sb_dir().join(if cfg!(windows) { "sing-box.exe" } else { "sing-box" })
}

/// sing-box 配置文件路径
pub fn sb_config() -> PathBuf {
    sb_dir().join("config.json")
}

/// 规则集目录
pub fn sb_rules_dir() -> PathBuf {
    sb_dir().join("rules")
}

/// 检查 sing-box 二进制是否存在
pub fn sb_exists() -> bool {
    sb_bin().exists()
}

/// 检查 config.json 是否存在
pub fn config_exists() -> bool {
    sb_config().exists()
}

/// 检查平台是否受支持
pub fn ensure_supported() -> Result<Platform> {
    let p = Platform::detect();
    match p {
        Platform::MacOs | Platform::Linux | Platform::Windows => Ok(p),
        _ => bail!("不支持的平台: {}", std::env::consts::OS),
    }
}

/// 断言 sing-box 已安装
pub fn ensure_installed() -> Result<()> {
    if !sb_exists() {
        bail!("sing-box 未安装! 二进制不存在: {}", sb_bin().display());
    }
    if !config_exists() {
        bail!(
            "配置文件不存在: {}. 请先运行 `gnp config` 生成配置。",
            sb_config().display()
        );
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
        .chain(std::iter::once(0u16))
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
