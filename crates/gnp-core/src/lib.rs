//! gnp-core — global-net-proxy 共享核心库
//!
//! 提供 client 和 server 共用的基础设施:
//! - platform: 平台检测与路径管理
//! - service:  跨平台服务管理 (launchctl/systemd)
//! - config:   sing-box config.json 解析与生成
//! - tunnel:   Hysteria2 (QUIC) 隧道诊断 (旧名 wg, 保留兼容别名)

pub mod config;
pub mod install;
pub mod platform;
pub mod proxy;
pub mod service;
pub mod tunnel;

/// 兼容别名: wg 时代 (WireGuard) 的模块名, 现已全面切换 Hysteria2/QUIC
pub use tunnel as wg;

/// 版本常量
pub const VERSION: &str = env!("CARGO_PKG_VERSION");