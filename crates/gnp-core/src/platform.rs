//! 平台检测与路径管理
//!
//! gnp 跨平台支持:
//! - macOS: 用 launchctl 管理 (~/Library/LaunchAgents/com.gnp.sing-box.plist)
//! - Linux: 用 systemd 管理 (gnp-proxy.service)
//!
//! sing-box 数据目录统一为 ~/.local/share/sing-box/

use anyhow::{bail, Result};
use std::path::PathBuf;

/// 平台枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
    Other,
}

impl Platform {
    pub fn detect() -> Self {
        match std::env::consts::OS {
            "macos" => Platform::MacOs,
            "linux" => Platform::Linux,
            _ => Platform::Other,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::MacOs => "macos",
            Platform::Linux => "linux",
            Platform::Other => "other",
        }
    }
}

/// sing-box 数据目录
///
/// 统一使用 ~/.local/share/sing-box/ (跨平台一致, 不与 macOS 的 Application Support 混淆)
pub fn sb_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/share/sing-box")
}

/// sing-box 二进制路径
pub fn sb_bin() -> PathBuf {
    sb_dir().join("sing-box")
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
        Platform::MacOs | Platform::Linux => Ok(p),
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