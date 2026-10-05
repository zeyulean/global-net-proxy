//! config.toml — 唯一事实源 (plan D2)
//!
//! 客户端 `~/.local/gnp/config.toml` / 服务端 `/opt/gnp/config.toml`。
//! `config.json`、服务单元、`tick.sh`、路由容器 secrets **全是生成物**。
//!
//! 设计约束 (plan §1.3):
//! - 字段即 schema, 缺省值写进 `Default` (hy2 端口 `GNP_PORT`=5766)
//! - `~` 由 `ssh_key_expanded()` 展开, toml 里可写 `~/.ssh/id_ed25519`
//! - **config.json 内不放注释** (不依赖 sing-box 对未知字段的容忍度);
//!   "勿手改" 提示写在 toml 头部注释 + 文档里
//!
//! 示例 (客户端):
//! ```toml
//! [server]
//! host = "8.209.203.17"
//! hy2_port = 5766
//! ssh_port = 22
//! ssh_user = "lw"
//! ssh_key  = "~/.ssh/id_ed25519"
//!
//! [auth]
//! hy2_password  = "..."
//! obfs_password = "..."
//!
//! [client]
//! listen = "127.0.0.1"
//! clash_api_port = 9090
//!
//! [client.hosts]
//! aipro.host = "192.168.1.2"
//!
//! [guard]
//! freeze_after_failures = 2
//! backoff_base_s = 60
//! backoff_cap_s  = 180
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::install::SshFallback;
use crate::platform::{self, GNP_CLASH_API_PORT, GNP_PORT};

// --- 缺省值 ---

fn default_hy2_port() -> u16 {
    GNP_PORT
}
fn default_ssh_port() -> u16 {
    22
}
fn default_ssh_user() -> String {
    "lw".to_string()
}
fn default_ssh_key() -> String {
    "~/.ssh/id_ed25519".to_string()
}
fn default_clash_api_port() -> u16 {
    GNP_CLASH_API_PORT
}
fn default_listen() -> String {
    "0.0.0.0".to_string()
}
fn default_server_listen() -> String {
    "::".to_string()
}
fn default_freeze_after() -> u32 {
    2
}
fn default_backoff_base() -> u64 {
    60
}
fn default_backoff_cap() -> u64 {
    180
}

// --- 客户端 ---

/// 客户端配置 (唯一事实源)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientSettings {
    #[serde(default)]
    pub server: ClientServer,
    #[serde(default)]
    pub auth: ClientAuth,
    #[serde(default)]
    pub client: ClientLocal,
    #[serde(default)]
    pub guard: GuardSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientServer {
    /// 远端服务端地址 (IP 或域名)
    #[serde(default)]
    pub host: String,
    /// hysteria2 端口 = `GNP_PORT` (5766), 可覆盖
    #[serde(default = "default_hy2_port")]
    pub hy2_port: u16,
    /// ssh 兜底通道端口
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    /// ssh 兜底用户; **空串 = 关闭 ssh 兜底 (单通道旧形态)**
    #[serde(default = "default_ssh_user")]
    pub ssh_user: String,
    /// ssh 私钥; 允许 `~` 前缀
    #[serde(default = "default_ssh_key")]
    pub ssh_key: String,
}

impl Default for ClientServer {
    fn default() -> Self {
        Self {
            host: String::new(),
            hy2_port: default_hy2_port(),
            ssh_port: default_ssh_port(),
            ssh_user: default_ssh_user(),
            ssh_key: default_ssh_key(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientAuth {
    /// hysteria2 密码
    #[serde(default)]
    pub hy2_password: String,
    /// salamander obfs 密码; 空串 = 不带 obfs (须与服务端 inbound 一致)
    #[serde(default)]
    pub obfs_password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientLocal {
    /// mixed 监听地址 (mac 单机自用 = 127.0.0.1; 局域网服务机 = 0.0.0.0)
    #[serde(default = "default_listen")]
    pub listen: String,
    /// clash_api 端口; **0 = 关闭** (switch/status/guard 依赖它)
    #[serde(default = "default_clash_api_port")]
    pub clash_api_port: u16,
    /// 可选: 预定义解析 + 路由直连 (仅 mac 用)
    #[serde(default)]
    pub hosts: BTreeMap<String, String>,
}

impl Default for ClientLocal {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            clash_api_port: default_clash_api_port(),
            hosts: BTreeMap::new(),
        }
    }
}

/// guard 参数 (freeze / 退避) — 2026-10-05 实测教训: cap 180s 而非 1h
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardSettings {
    /// 连续失败几次后冻结到 ssh
    #[serde(default = "default_freeze_after")]
    pub freeze_after_failures: u32,
    /// 退避基数秒 (60 * 2^(n-1))
    #[serde(default = "default_backoff_base")]
    pub backoff_base_s: u64,
    /// 退避上限秒 (对齐 urltest 3m; 再大会拖慢恢复回切)
    #[serde(default = "default_backoff_cap")]
    pub backoff_cap_s: u64,
}

impl Default for GuardSettings {
    fn default() -> Self {
        Self {
            freeze_after_failures: default_freeze_after(),
            backoff_base_s: default_backoff_base(),
            backoff_cap_s: default_backoff_cap(),
        }
    }
}

impl ClientSettings {
    /// 从 toml 读 (唯一事实源)
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("读取 config.toml 失败: {}", path.display()))?;
        toml::from_str(&text)
            .with_context(|| format!("解析 config.toml 失败: {}", path.display()))
    }

    /// 读默认位置 `$GNP_HOME/config.toml`; 不存在返回 None
    pub fn load_default() -> Result<Option<Self>> {
        let p = platform::gnp_config_toml();
        if !p.exists() {
            return Ok(None);
        }
        Ok(Some(Self::load(&p)?))
    }

    /// 读默认位置, 缺则用 `Default` (便于 status/guard 在未初始化时也不炸)
    pub fn load_or_default() -> Self {
        Self::load_default()
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    /// 写 toml (0600, 带 "勿手改生成物" 头部注释)
    pub fn save(&self, path: &Path) -> Result<()> {
        let body = toml::to_string_pretty(self).context("序列化 config.toml 失败")?;
        platform::write_private(path, &format!("{}{}", client_header(), body))
    }

    /// 存到默认位置 `$GNP_HOME/config.toml`
    pub fn save_default(&self) -> Result<PathBuf> {
        let p = platform::gnp_config_toml();
        self.save(&p)?;
        Ok(p)
    }

    /// ssh 兜底参数; `ssh_user` 为空 = 单通道 (旧形态)
    pub fn ssh_fallback(&self) -> Option<SshFallback> {
        if self.server.ssh_user.trim().is_empty() {
            return None;
        }
        Some(SshFallback {
            server: self.server.host.clone(),
            server_port: self.server.ssh_port,
            user: self.server.ssh_user.clone(),
            private_key_path: platform::expand_tilde(&self.server.ssh_key),
        })
    }

    /// obfs 密码; 空串 = 不带 obfs
    pub fn obfs(&self) -> Option<String> {
        let p = self.auth.obfs_password.trim();
        if p.is_empty() {
            None
        } else {
            Some(p.to_string())
        }
    }

    /// clash_api 端口; `clash_api_port = 0` → None (关闭)
    pub fn clash_api(&self) -> Option<u16> {
        if self.client.clash_api_port == 0 {
            None
        } else {
            Some(self.client.clash_api_port)
        }
    }

    /// 预定义解析 [(域名, IP)] (BTreeMap → 已按域名排序, 输出确定)
    pub fn hosts(&self) -> Vec<(String, String)> {
        self.client
            .hosts
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// 必要字段是否齐 (install 前置检查)
    pub fn validate(&self) -> Result<()> {
        if self.server.host.trim().is_empty() {
            anyhow::bail!("config.toml: [server].host 不能为空");
        }
        if self.auth.hy2_password.trim().is_empty() {
            anyhow::bail!("config.toml: [auth].hy2_password 不能为空");
        }
        if self.server.hy2_port == 0 {
            anyhow::bail!("config.toml: [server].hy2_port 不能为 0");
        }
        Ok(())
    }
}

fn client_header() -> String {
    format!(
        "# gnpc 客户端配置 — 唯一事实源 (勿手改生成物)\n\
         #\n\
         # 本文件是唯一事实源。config.json / 服务单元 / tick.sh / 容器 secrets\n\
         # 全部由 `gnpc install --config <本文件>` 生成, **勿手改**。\n\
         # 改这里 → 重跑 `gnpc install` → 重启服务。\n\
         #\n\
         # ssh_user 置空 = 关闭 ssh 兜底 (单通道旧形态); clash_api_port = 0 = 关闭 clash_api。\n\
         #\n\
         # 缺省端口 hy2 = {} (GNP_PORT, 代码默认值)。\n\n",
        GNP_PORT
    )
}

// --- 服务端 ---

/// 服务端配置 (唯一事实源)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSettings {
    #[serde(default)]
    pub server: ServerListen,
    #[serde(default)]
    pub auth: ServerAuth,
    #[serde(default)]
    pub users: Vec<ServerUser>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerListen {
    /// inbound 监听地址
    #[serde(default = "default_server_listen")]
    pub listen: String,
    /// hysteria2 端口 = `GNP_PORT` (5766), 可覆盖
    #[serde(default = "default_hy2_port")]
    pub hy2_port: u16,
}

impl Default for ServerListen {
    fn default() -> Self {
        Self {
            listen: default_server_listen(),
            hy2_port: default_hy2_port(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerAuth {
    /// salamander obfs 密码 (客户端须一致)
    #[serde(default)]
    pub obfs_password: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerUser {
    /// 用户名 (可选; sing-box 允许无名用户)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub password: String,
}

impl ServerSettings {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("读取服务端 config.toml 失败: {}", path.display()))?;
        toml::from_str(&text)
            .with_context(|| format!("解析服务端 config.toml 失败: {}", path.display()))
    }

    pub fn load_default() -> Result<Option<Self>> {
        let p = platform::gnps_config_toml();
        if !p.exists() {
            return Ok(None);
        }
        Ok(Some(Self::load(&p)?))
    }

    pub fn load_or_default() -> Self {
        Self::load_default().ok().flatten().unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let body = toml::to_string_pretty(self).context("序列化服务端 config.toml 失败")?;
        platform::write_private(path, &format!("{}{}", server_header(), body))
    }

    pub fn save_default(&self) -> Result<PathBuf> {
        let p = platform::gnps_config_toml();
        self.save(&p)?;
        Ok(p)
    }

    /// obfs; 空串 = 不带 obfs
    pub fn obfs(&self) -> Option<String> {
        let p = self.auth.obfs_password.trim();
        if p.is_empty() {
            None
        } else {
            Some(p.to_string())
        }
    }

    /// 追加一个用户 (gen-user)
    pub fn add_user(&mut self, name: Option<&str>, password: &str) -> Result<()> {
        if self
            .users
            .iter()
            .any(|u| u.password == password)
        {
            anyhow::bail!("密码冲突, 拒绝写入");
        }
        self.users.push(ServerUser {
            name: name.map(|s| s.to_string()),
            password: password.to_string(),
        });
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if self.server.hy2_port == 0 {
            anyhow::bail!("config.toml: [server].hy2_port 不能为 0");
        }
        if self.users.is_empty() {
            anyhow::bail!("config.toml: 至少要有一个 [[users]] (否则无人能连)");
        }
        Ok(())
    }
}

fn server_header() -> String {
    format!(
        "# gnps 服务端配置 — 唯一事实源 (勿手改生成物)\n\
         #\n\
         # config.json / gnps.service / tick.sh 由 `gnps install --config <本文件>` 生成, **勿手改**。\n\
         # 加用户: `gnps gen-user --name <名>` (追加 [[users]] 后自动重新渲染 + 重启)。\n\
         #\n\
         # 缺省端口 hy2 = {} (GNP_PORT, 代码默认值)。\n\
         # 端口改动必须 ufw + 云安全组双侧, 且 ufw 持久化。\n\n",
        GNP_PORT
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_toml(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("gnp-settings-test-{}-{}", std::process::id(), name));
        p
    }

    fn sample() -> ClientSettings {
        let mut hosts = BTreeMap::new();
        hosts.insert("aipro.host".to_string(), "192.168.1.2".to_string());
        hosts.insert("lwtop.host".to_string(), "8.209.203.17".to_string());
        ClientSettings {
            server: ClientServer {
                host: "8.209.203.17".to_string(),
                hy2_port: 5766,
                ssh_port: 22,
                ssh_user: "lw".to_string(),
                ssh_key: "~/.ssh/id_ed25519".to_string(),
            },
            auth: ClientAuth {
                hy2_password: "gnp-quic-test-password".to_string(),
                obfs_password: "gnp-obfs-20261005".to_string(),
            },
            client: ClientLocal {
                listen: "127.0.0.1".to_string(),
                clash_api_port: 9090,
                hosts,
            },
            guard: GuardSettings::default(),
        }
    }

    #[test]
    fn defaults_match_plan() {
        let s = ClientSettings::default();
        // GNP_PORT=5766 是代码默认值 (§1.3/D4)
        assert_eq!(s.server.hy2_port, 5766);
        assert_eq!(s.server.ssh_port, 22);
        assert_eq!(s.server.ssh_user, "lw");
        assert_eq!(s.server.ssh_key, "~/.ssh/id_ed25519");
        assert_eq!(s.client.clash_api_port, 9090);
        assert_eq!(s.client.listen, "0.0.0.0");
        // guard 实测教训值 (§7.3 #8)
        assert_eq!(s.guard.freeze_after_failures, 2);
        assert_eq!(s.guard.backoff_base_s, 60);
        assert_eq!(s.guard.backoff_cap_s, 180);
        // 服务端缺省
        let srv = ServerSettings::default();
        assert_eq!(srv.server.hy2_port, 5766);
        assert_eq!(srv.server.listen, "::");
    }

    #[test]
    fn toml_round_trip() {
        let p = tmp_toml("roundtrip");
        let orig = sample();
        orig.save(&p).expect("save");
        let back = ClientSettings::load(&p).expect("load");
        assert_eq!(orig, back);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn toml_round_trip_minimal_only_required_fields() {
        let p = tmp_toml("minimal");
        // 只写 [server].host + [auth].hy2_password: 其余全部走缺省
        let text = "[server]\nhost = \"1.2.3.4\"\n\n[auth]\nhy2_password = \"pw\"\n";
        std::fs::write(&p, text).unwrap();
        let s = ClientSettings::load(&p).expect("load minimal");
        assert_eq!(s.server.host, "1.2.3.4");
        assert_eq!(s.server.hy2_port, GNP_PORT);
        assert_eq!(s.guard.backoff_cap_s, 180);
        assert!(s.validate().is_ok());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn tilde_expansion() {
        let s = sample();
        let ssh = s.ssh_fallback().expect("ssh fallback on");
        let home = platform::home_dir();
        assert_eq!(ssh.private_key_path, home.join(".ssh/id_ed25519"));
        // 绝对路径原样
        assert_eq!(
            platform::expand_tilde("/root/.ssh/id_ed25519"),
            PathBuf::from("/root/.ssh/id_ed25519")
        );
        assert_eq!(platform::expand_tilde("~"), home);
    }

    #[test]
    fn empty_ssh_user_disables_fallback() {
        let mut s = sample();
        s.server.ssh_user = String::new();
        assert!(s.ssh_fallback().is_none());
        let p = tmp_toml("nossh");
        s.save(&p).unwrap();
        assert!(ClientSettings::load(&p).unwrap().ssh_fallback().is_none());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn obfs_and_clash_api_toggles() {
        let mut s = sample();
        assert_eq!(s.obfs().as_deref(), Some("gnp-obfs-20261005"));
        assert_eq!(s.clash_api(), Some(9090));
        s.auth.obfs_password = String::new();
        s.client.clash_api_port = 0;
        assert!(s.obfs().is_none());
        assert!(s.clash_api().is_none());
    }

    #[test]
    fn server_round_trip_and_add_user() {
        let p = tmp_toml("server");
        let mut srv = ServerSettings {
            server: ServerListen {
                listen: "::".to_string(),
                hy2_port: 5766,
            },
            auth: ServerAuth {
                obfs_password: "gnp-obfs-20261005".to_string(),
            },
            users: vec![ServerUser {
                name: None,
                password: "gnp-quic-test-password".to_string(),
            }],
        };
        srv.add_user(Some("mac-peertest"), "gnp-acaba449a5f42c8248d2f412d1817e46")
            .unwrap();
        // 重复密码必须被拒
        assert!(srv
            .add_user(Some("dup"), "gnp-acaba449a5f42c8248d2f412d1817e46")
            .is_err());
        srv.save(&p).unwrap();
        let back = ServerSettings::load(&p).expect("load");
        assert_eq!(srv, back);
        assert_eq!(back.users.len(), 2);
        assert_eq!(back.users[1].name.as_deref(), Some("mac-peertest"));
        assert!(back.validate().is_ok());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn header_marks_generated_files() {
        let p = tmp_toml("header");
        sample().save(&p).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("# gnpc 客户端配置"));
        assert!(text.contains("勿手改"));
        // 头部注释必须是合法 toml (load 不炸)
        assert!(ClientSettings::load(&p).is_ok());
        let _ = std::fs::remove_file(&p);
    }

    /// repo 里的 `deploy/hosts/*.toml` 是各机唯一事实源 (D2, §7.1 验收 6 md5 比对),
    /// 它们必须能被本 schema 解析且通过校验 — 资产与代码漂移在这里就被拦住。
    fn hosts_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/hosts")
    }

    #[test]
    fn deploy_host_assets_match_schema() {
        let dir = hosts_dir();
        let clients = ["mac", "aipro", "lwmate", "cozepc"];
        for name in clients {
            let p = dir.join(format!("{}.toml", name));
            assert!(p.exists(), "缺资产: {}", p.display());
            let s = ClientSettings::load(&p)
                .unwrap_or_else(|e| panic!("{} 解析失败: {}", p.display(), e));
            s.validate()
                .unwrap_or_else(|e| panic!("{} 校验失败: {}", p.display(), e));
            assert_eq!(
                s.server.hy2_port, GNP_PORT,
                "{}: hy2 端口必须等于 GNP_PORT=5766",
                name
            );
            assert_eq!(s.server.host, "8.209.203.17", "{}: 服务端 IP", name);
            assert_eq!(
                s.auth.obfs_password, "gnp-obfs-20261005",
                "{}: obfs 密码全网统一",
                name
            );
            assert!(!s.auth.hy2_password.is_empty(), "{}: hy2 密码内联", name);
            assert_eq!(s.client.clash_api_port, GNP_CLASH_API_PORT, "{}", name);
        }

        // 只有 mac 是单机自用 + 唯一带 hosts 预定义解析的主机 (§1.3)
        let mac = ClientSettings::load(&dir.join("mac.toml")).unwrap();
        assert_eq!(mac.client.listen, "127.0.0.1");
        assert_eq!(mac.client.hosts.get("aipro.host").map(String::as_str), Some("192.168.1.2"));
        assert_eq!(mac.client.hosts.len(), 3);
        for name in ["aipro", "lwmate", "cozepc"] {
            let s = ClientSettings::load(&dir.join(format!("{}.toml", name))).unwrap();
            assert_eq!(s.client.listen, "0.0.0.0", "{}: 局域网服务机", name);
            assert!(s.client.hosts.is_empty(), "{}: 不该有 hosts 段", name);
        }
        // cozepc 是 root, 私钥走绝对路径 (其余用 ~ 展开)
        let coz = ClientSettings::load(&dir.join("cozepc.toml")).unwrap();
        assert_eq!(coz.server.ssh_key, "/root/.ssh/id_ed25519");
        assert_eq!(
            coz.ssh_fallback().unwrap().private_key_path,
            PathBuf::from("/root/.ssh/id_ed25519")
        );

        // 服务端: 8 个用户, 端口 5766, obfs salamander 强制
        let srv = ServerSettings::load(&dir.join("lwtop-server.toml")).unwrap();
        srv.validate().unwrap();
        assert_eq!(srv.server.hy2_port, GNP_PORT);
        assert_eq!(srv.server.listen, "::");
        assert_eq!(srv.obfs().as_deref(), Some("gnp-obfs-20261005"));
        assert_eq!(srv.users.len(), 8, "实机同步 8 个用户");
        // 四台客户端的密码都必须在服务端用户表里 (否则部署即断网)
        for name in clients {
            let c = ClientSettings::load(&dir.join(format!("{}.toml", name))).unwrap();
            assert!(
                srv.users.iter().any(|u| u.password == c.auth.hy2_password),
                "{}: hy2 密码 {} 不在服务端 [[users]] 里",
                name,
                c.auth.hy2_password
            );
        }
    }
}
