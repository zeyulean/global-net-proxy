//! gnp-core — global-net-proxy 共享核心库
//!
//! 提供 client 和 server 共用的基础设施:
//! - platform: 平台检测与路径管理
//! - service:  跨平台服务管理 (launchctl/systemd)
//! - config:   sing-box config.json 解析与生成
//! - wg:       WireGuard 隧道诊断

pub mod config;
pub mod install;
pub mod platform;
pub mod service;
pub mod wg;

/// 版本常量
pub const VERSION: &str = env!("CARGO_PKG_VERSION");