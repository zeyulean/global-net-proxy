//! sing-box config.json 解析与生成
//!
//! 只处理 gnp 关心的字段 (mixed inbound + hysteria2 outbound), 其余保持原样。

use anyhow::{Context, Result};
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

/// 从 config 提取 hysteria2 outbound 信息 (用于诊断)
#[derive(Debug, Clone)]
pub struct Hy2Endpoint {
    pub server: String,       // 远端 server 地址
    pub server_port: u16,     // 远端 server 端口 (hysteria2/QUIC 443)
    pub password: String,     // hysteria2 密码
}

/// 从 config.json 提取 hysteria2 outbound 信息 (用于诊断)
///
/// 只支持 sing-box outbound hysteria2 格式。
pub fn extract_hy2_endpoint(v: &Value) -> Option<Hy2Endpoint> {
    if let Some(outbounds) = v.get("outbounds").and_then(|o| o.as_array()) {
        for ob in outbounds {
            if ob.get("type").and_then(|t| t.as_str()) == Some("hysteria2") {
                let server = ob.get("server")?.as_str()?.to_string();
                let password = ob.get("password")?.as_str()?.to_string();
                let server_port = ob
                    .get("server_port")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(443) as u16;
                return Some(Hy2Endpoint {
                    server,
                    server_port,
                    password,
                });
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

/// 检查 config 是否有 hysteria2 outbound
pub fn has_hy2_endpoint(v: &Value) -> bool {
    v.get("outbounds")
        .and_then(|o| o.as_array())
        .map(|arr| {
            arr.iter()
                .any(|x| x.get("type").and_then(|t| t.as_str()) == Some("hysteria2"))
        })
        .unwrap_or(false)
}