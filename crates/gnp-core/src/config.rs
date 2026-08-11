//! sing-box config.json 解析与生成
//!
//! 只处理 gnp 关心的字段 (mixed inbound + wg endpoint), 其余保持原样。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// 解析 config.json 为通用 JSON
pub fn load(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("读取配置失败: {}", path.display()))?;
    let v: Value = serde_json::from_str(&content)
        .with_context(|| format!("解析配置 JSON 失败: {}", path.display()))?;
    Ok(v)
}

/// 保存 config.json
pub fn save(path: &Path, v: &Value) -> Result<()> {
    let content = serde_json::to_string_pretty(v)
        .with_context(|| format!("序列化配置失败: {}", path.display()))?;
    std::fs::write(path, content)
        .with_context(|| format!("写入配置失败: {}", path.display()))?;
    Ok(())
}

/// 从 config 提取 wg endpoint 信息
#[derive(Debug, Clone)]
pub struct WgEndpoint {
    pub address: String,       // 本机 wg IP (如 10.0.0.4/32)
    pub private_key: String,   // 本机私钥
    pub peer_address: String,  // 远端 server 地址
    pub peer_public_key: String, // 远端 server 公钥
    pub peer_port: u16,        // 远端 server 端口 (wg 端口)
    pub mtu: u64,
}

/// 从 config.json 提取 wg outbound 信息 (用于诊断)
///
/// 兼容两种格式: 1.12 outbound wireguard 和 1.13 endpoint wireguard
pub fn extract_wg_endpoint(v: &Value) -> Option<WgEndpoint> {
    // 优先尝试 outbounds (1.12 格式)
    if let Some(outbounds) = v.get("outbounds").and_then(|o| o.as_array()) {
        for ob in outbounds {
            if ob.get("type").and_then(|t| t.as_str()) == Some("wireguard") {
                let address = ob
                    .get("local_address")?
                    .as_array()?
                    .first()?
                    .as_str()?
                    .to_string();
                let private_key = ob.get("private_key")?.as_str()?.to_string();
                return Some(WgEndpoint {
                    address,
                    private_key,
                    peer_address: ob.get("server")?.as_str()?.to_string(),
                    peer_public_key: ob.get("peer_public_key")?.as_str()?.to_string(),
                    peer_port: ob
                        .get("server_port")
                        .and_then(|x| x.as_u64())
                        .unwrap_or(1194) as u16,
                    mtu: ob.get("mtu").and_then(|m| m.as_u64()).unwrap_or(1280),
                });
            }
        }
    }
    // 回退: endpoints (1.13 格式)
    if let Some(endpoints) = v.get("endpoints").and_then(|e| e.as_array()) {
        for ep in endpoints {
            if ep.get("type").and_then(|t| t.as_str()) == Some("wireguard") {
                let address = ep
                    .get("address")?
                    .as_array()?
                    .first()?
                    .as_str()?
                    .to_string();
                let private_key = ep.get("private_key")?.as_str()?.to_string();
                let peers = ep.get("peers")?.as_array()?;
                if let Some(p) = peers.first() {
                    return Some(WgEndpoint {
                        address,
                        private_key,
                        peer_address: p.get("address")?.as_str()?.to_string(),
                        peer_public_key: p.get("public_key")?.as_str()?.to_string(),
                        peer_port: p
                            .get("port")
                            .and_then(|x| x.as_u64())
                            .unwrap_or(1194) as u16,
                        mtu: ep.get("mtu").and_then(|m| m.as_u64()).unwrap_or(1280),
                    });
                }
            }
        }
    }
    None
}

/// 检查 config 是否安全 (无 tun / strict_route / auto_route)
pub fn is_safe(v: &Value) -> bool {
    let s = serde_json::to_string(v).unwrap_or_default();
    !(s.contains("strict_route") || s.contains("auto_route"))
}

/// 检查 config 是否有 mixed inbound
pub fn has_mixed_inbound(v: &Value) -> bool {
    v.get("inbounds")
        .and_then(|ib| ib.as_array())
        .map(|arr| arr.iter().any(|x| x.get("type").and_then(|t| t.as_str()) == Some("mixed")))
        .unwrap_or(false)
}

/// 检查 config 是否有 wg outbound/endpoint
///
/// 兼容 1.12 outbound 格式和 1.13 endpoint 格式
pub fn has_wg_endpoint(v: &Value) -> bool {
    // 1.12 outbound 格式
    let has_outbound = v
        .get("outbounds")
        .and_then(|o| o.as_array())
        .map(|arr| {
            arr.iter()
                .any(|x| x.get("type").and_then(|t| t.as_str()) == Some("wireguard"))
        })
        .unwrap_or(false);
    // 1.13 endpoint 格式
    let has_endpoint = v
        .get("endpoints")
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .any(|x| x.get("type").and_then(|t| t.as_str()) == Some("wireguard"))
        })
        .unwrap_or(false);
    has_outbound || has_endpoint
}