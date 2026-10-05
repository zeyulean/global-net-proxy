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
    pub obfs_type: Option<String>,     // salamander (须与服务端 inbound 一致)
    pub obfs_password: Option<String>,
}

/// 从 config.json 提取 hysteria2 outbound 信息 (用于诊断)
///
/// 只支持 sing-box outbound hysteria2 格式。
pub fn extract_hy2_endpoint(v: &Value) -> Option<Hy2Endpoint> {
    find_outbound(v, "hysteria2").map(|ob| {
        let server = ob.get("server").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let password = ob.get("password").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let server_port = ob
            .get("server_port")
            .and_then(|x| x.as_u64())
            .unwrap_or(443) as u16;
        let obfs = ob.get("obfs");
        Hy2Endpoint {
            server,
            server_port,
            password,
            obfs_type: obfs
                .and_then(|o| o.get("type"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            obfs_password: obfs
                .and_then(|o| o.get("password"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
        }
    })
}

/// 按 type 或 tag 在 outbounds 里找第一个匹配项
pub fn find_outbound<'a>(v: &'a Value, type_or_tag: &str) -> Option<&'a Value> {
    v.get("outbounds")
        .and_then(|o| o.as_array())?
        .iter()
        .find(|ob| {
            ob.get("type").and_then(|t| t.as_str()) == Some(type_or_tag)
                || ob.get("tag").and_then(|t| t.as_str()) == Some(type_or_tag)
        })
}

/// find_outbound 的可变版本 (改写 selector default 等字段用)
pub fn find_outbound_mut<'a>(v: &'a mut Value, type_or_tag: &str) -> Option<&'a mut Value> {
    v.get_mut("outbounds")
        .and_then(|o| o.as_array_mut())?
        .iter_mut()
        .find(|ob| {
            ob.get("type").and_then(|t| t.as_str()) == Some(type_or_tag)
                || ob.get("tag").and_then(|t| t.as_str()) == Some(type_or_tag)
        })
}

/// route.final (当前生效出站 tag)
pub fn final_outbound(v: &Value) -> Option<String> {
    v.get("route")
        .and_then(|r| r.get("final"))
        .and_then(|f| f.as_str())
        .map(|s| s.to_string())
}

/// clash_api external_controller (如 "127.0.0.1:9090"); 未开启返回 None
pub fn clash_api_addr(v: &Value) -> Option<String> {
    v.get("experimental")
        .and_then(|e| e.get("clash_api"))
        .and_then(|c| c.get("external_controller"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
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