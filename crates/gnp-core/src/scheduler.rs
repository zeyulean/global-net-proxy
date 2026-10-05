//! 调度资产 — 统一 tick 入口 + 服务单元 + launchd/crontab 接线 (plan D3)
//!
//! **repo 是可编辑事实源** (`deploy/scheduler/`), 下面 `include_str!` 把它编进二进制;
//! 改完 scheduler 文件必须重编 gnpc/gnps 才生效。
//!
//! 调度唯一性 (plan §7.1 验收 2):
//! - Mac    → launchd `com.gnpc.tick` (StartInterval=60) 跑 `tick.sh`
//! - Linux  → crontab **一行** `* * * * * <base>/bin/tick.sh`
//! - 服务端 → 同一份 tick.sh, `GNP_HOME=/opt/gnp` + crontab 一行
//!
//! sing-box 常驻本体是独立 KeepAlive 服务 (`com.gnpc.singbox` / `gnpc.service`),
//! **不与 tick 合并** (生命周期不同, 合并已被否决)。

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::platform;

// --- 嵌入资产 ---

pub const TICK_SH: &str = include_str!("../../../deploy/scheduler/tick.sh");
pub const PLIST_SINGBOX: &str = include_str!("../../../deploy/scheduler/com.gnpc.singbox.plist");
pub const PLIST_TICK: &str = include_str!("../../../deploy/scheduler/com.gnpc.tick.plist");
pub const UNIT_CLIENT: &str = include_str!("../../../deploy/scheduler/gnpc.service");
pub const UNIT_SERVER: &str = include_str!("../../../deploy/scheduler/gnps.service");
pub const CRON: &str = include_str!("../../../deploy/scheduler/gnpc.cron");
pub const TICK_D_GNPS_HEALTH: &str = include_str!("../../../deploy/scheduler/etc/tick.d/gnps-health.sh");

// --- 名字 (v2) ---

/// Mac 常驻 sing-box launchd 标签
pub const LAUNCHD_SINGBOX: &str = "com.gnpc.singbox";
/// Mac tick (跑 tick.sh) launchd 标签
pub const LAUNCHD_TICK: &str = "com.gnpc.tick";
/// Linux 客户端 systemd 服务名
pub const SYSTEMD_CLIENT: &str = "gnpc";
/// Linux 服务端 systemd 服务名
pub const SYSTEMD_SERVER: &str = "gnps";
/// Windows 计划任务名
pub const WIN_TASK: &str = "gnpc";

/// 旧名字 — 迁移时必须清干净 (§7.1 验收 4: 无残留)
pub const LEGACY_LAUNCHD: &[&str] = &["com.gnp.sing-box", "com.gnp.guard"];
/// 旧 systemd 单元 (系统级 gnp-proxy / gnp-hy2 + aipro 的 --user sing-box)
pub const LEGACY_SYSTEMD: &[&str] = &["gnp-proxy", "gnp-hy2"];
/// aipro 旧的用户级单元名
pub const LEGACY_SYSTEMD_USER: &[&str] = &["sing-box"];

// --- 模板渲染 ---

/// `{{KEY}}` 占位替换 (资产里刻意不用 sed/python, 避免跨平台引号地狱)
pub fn render(tpl: &str, vars: &[(&str, &str)]) -> String {
    let mut out = tpl.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{}}}}}", k), v);
    }
    out
}

/// 未替换的占位符 → 报错 (防资产与代码漂移: 忘了填 {{CONFIG}} 就装出坏服务)
pub fn assert_rendered(s: &str) -> Result<()> {
    if let Some(i) = s.find("{{") {
        let rest = &s[i..];
        let end = rest.find("}}").map(|e| i + e + 2).unwrap_or(s.len());
        anyhow::bail!("模板占位符未替换: {}", &s[i..end.min(i + 40)]);
    }
    Ok(())
}

/// 客户端 launchd plist 变量组 (base = $GNP_HOME)
pub fn client_vars(base: &Path) -> Vec<(String, String)> {
    vec![
        ("SB_BIN".into(), base.join("bin/sing-box").display().to_string()),
        ("CONFIG".into(), base.join("config.json").display().to_string()),
        ("SB_LOG".into(), base.join("var/sing-box.log").display().to_string()),
        ("SB_ERR".into(), base.join("var/sing-box.err").display().to_string()),
        ("TICK_SH".into(), base.join("bin/tick.sh").display().to_string()),
        (
            "TICK_LAUNCHD_LOG".into(),
            base.join("var/tick-launchd.log").display().to_string(),
        ),
    ]
}

/// 服务端变量组 (base = $GNP_SERVER_HOME)
pub fn server_vars(base: &Path) -> Vec<(String, String)> {
    vec![
        ("SB_BIN".into(), base.join("bin/sing-box").display().to_string()),
        ("CONFIG".into(), base.join("config.json").display().to_string()),
        ("TICK_SH".into(), base.join("bin/tick.sh").display().to_string()),
    ]
}

fn render_owned(tpl: &str, vars: Vec<(String, String)>) -> Result<String> {
    let refs: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let out = render(tpl, &refs);
    assert_rendered(&out)?;
    Ok(out)
}

pub fn client_plist_singbox(base: &Path) -> Result<String> {
    render_owned(PLIST_SINGBOX, client_vars(base))
}
pub fn client_plist_tick(base: &Path) -> Result<String> {
    render_owned(PLIST_TICK, client_vars(base))
}
pub fn client_unit(base: &Path) -> Result<String> {
    render_owned(UNIT_CLIENT, client_vars(base))
}
pub fn server_unit(base: &Path) -> Result<String> {
    render_owned(UNIT_SERVER, server_vars(base))
}

/// crontab 单行 (客户端: 直接路径; 服务端: 显式 GNP_HOME, tick.sh 默认按 $HOME 找 BASE)
pub fn client_cron_line(base: &Path) -> String {
    format!("* * * * * {}", base.join("bin/tick.sh").display())
}
pub fn server_cron_line(base: &Path) -> String {
    format!("* * * * * GNP_HOME={} {}", base.display(), base.join("bin/tick.sh").display())
}
pub fn client_cron(base: &Path) -> String {
    render(CRON, &[("CRON_CMD", &client_cron_line(base))])
}
pub fn server_cron(base: &Path) -> String {
    render(CRON, &[("CRON_CMD", &server_cron_line(base))])
}

// --- 落盘 ---

pub fn write_executable(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("创建目录失败: {}", parent.display()))?;
    }
    std::fs::write(path, content).with_context(|| format!("写 {} 失败", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).ok();
    }
    Ok(())
}

/// 写 tick.sh 到 `<base>/bin/tick.sh` (可执行)
pub fn write_tick_script(base: &Path) -> Result<PathBuf> {
    let dest = base.join("bin/tick.sh");
    write_executable(&dest, TICK_SH)?;
    Ok(dest)
}

/// launchd plist 路径 (~/Library/LaunchAgents/<label>.plist)
pub fn launchd_plist_path(label: &str) -> PathBuf {
    platform::home_dir()
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", label))
}

/// systemd 单元路径 (/etc/systemd/system/<name>.service)
pub fn systemd_unit_path(name: &str) -> PathBuf {
    PathBuf::from("/etc/systemd/system").join(format!("{}.service", name))
}

// --- crontab (坑清单 #4: 必须用文件参数或显式关 stdin) ---

/// 读当前 crontab 行 (无 crontab 时空)
pub fn read_crontab() -> Vec<String> {
    match Command::new("crontab").args(["-l"]).output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// 写 crontab (Rust 端显式 `stdin.take()` 关管道; 不关则永久挂起)
pub fn write_crontab(lines: &[String]) -> Result<()> {
    use std::io::Write;
    let content = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    let mut st = Command::new("crontab")
        .args(["-"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("crontab 启动失败")?;
    if let Some(mut stdin) = st.stdin.take() {
        let _ = stdin.write_all(content.as_bytes());
    }
    let out = st.wait_with_output().context("crontab 写入失败")?;
    if !out.status.success() {
        anyhow::bail!("crontab 安装失败: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(())
}

/// 判定某行 crontab 是否属于 gnp (旧/新都算)
pub fn is_gnp_cron_line(line: &str) -> bool {
    line.contains("gnpc")
        || line.contains("gnp-client")
        || line.contains("gnp-server")
        || line.contains("gnps")
        || line.contains("global-net-proxy")
        || line.contains("tick.sh")
        || line.contains("update-rules")
        // 旧服务端布局 (/opt/gnp-quic) 与旧服务名
        || line.contains("gnp-hy2")
        || line.contains("gnp-quic")
}

/// 剔除所有 gnp 旧行 (保留用户其它 cron)
pub fn purge_gnp_cron_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|l| !is_gnp_cron_line(l))
        .cloned()
        .collect()
}

// --- 旧调度清理 (migrate / uninstall-scheduler) ---

/// 卸载并移走旧 launchd 单元 (com.gnp.*)
pub fn remove_legacy_launchd() -> Vec<String> {
    let mut done = Vec::new();
    for label in LEGACY_LAUNCHD {
        let plist = launchd_plist_path(label);
        if !plist.exists() {
            continue;
        }
        let _ = Command::new("launchctl")
            .args(["unload", "-w", plist.to_str().unwrap_or("")])
            .status();
        let _ = Command::new("launchctl")
            .args(["remove", label])
            .status();
        let bak = plist.with_extension("plist.migrated");
        match std::fs::rename(&plist, &bak) {
            Ok(_) => done.push(format!("{} → {}", plist.display(), bak.display())),
            Err(e) => done.push(format!("{} 移走失败: {}", plist.display(), e)),
        }
    }
    done
}

/// 禁用旧 systemd 单元 (系统级 gnp-proxy / gnp-hy2)
pub fn disable_legacy_systemd() -> Vec<String> {
    let mut done = Vec::new();
    for name in LEGACY_SYSTEMD {
        let out = Command::new("systemctl")
            .args(["disable", "--now", name])
            .output();
        let ok = out.as_ref().map(|o| o.status.success()).unwrap_or(false);
        let unit = systemd_unit_path(name);
        if ok && unit.exists() {
            let bak = unit.with_extension("service.disabled");
            let _ = std::fs::rename(&unit, &bak);
            let _ = Command::new("systemctl").args(["daemon-reload"]).status();
            done.push(format!("{} disabled → {}", name, bak.display()));
        }
    }
    done
}

/// 禁用旧 systemd **用户级** 单元 (aipro 的 --user sing-box)
pub fn disable_legacy_systemd_user(user: &str) -> Vec<String> {
    let mut done = Vec::new();
    for name in LEGACY_SYSTEMD_USER {
        let out = Command::new("systemctl")
            .args(["--user", "disable", "--now", name])
            .env("XDG_RUNTIME_DIR", format!("/run/user/{}", uid_of(user)))
            .output();
        if let Ok(o) = out {
            if o.status.success() {
                done.push(format!("--user {} disabled", name));
            }
        }
    }
    done
}

fn uid_of(user: &str) -> u32 {
    Command::new("id")
        .args(["-u", user])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_sh_is_bash32_safe_and_self_contained() {
        // D3/D5 + 坑清单 #5: bash 永不解析 JSON, 不用 jq; 不用 bash 4 才有的语法
        // (Mac 自带 bash 3.2: 无关联数组/大小写展开/mapfile)
        // 只查代码行 —— 注释里可以出现这些词 (tick.sh 头注释正是在声明禁用它们)
        let code: String = TICK_SH
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            "jq ",
            "declare -A",
            "local -A",
            "mapfile",
            "readarray",
            ",,", // ${var,,}
            ",^", // ${var^^}
        ] {
            assert!(
                !code.contains(forbidden),
                "tick.sh 代码不该出现 bash4 特性/JSON 解析: {:?}",
                forbidden
            );
        }
        // tick.sh 自带 BASE 推导 + log 上限 + 两内核组件 + tick.d 循环
        assert!(TICK_SH.contains("GNP_HOME:-$HOME/.local/gnp"));
        assert!(TICK_SH.contains("gnpc\" guard"));
        assert!(TICK_SH.contains("gnpc\" rules-update"));
        assert!(TICK_SH.contains("etc/tick.d/*.sh"));
        // 组件缺席=静默, 组件失败=WARN —— 必须靠 if/else 区分。
        // 踩过的坑: `[ -x ] && cmd || WARN` 分不清"缺席"与"失败", 服务端机器
        // (只有 gnps, 没有 gnpc) 会每分钟刷一条 WARN, 违反 §7.1 验收 3。
        let joined: String = code
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let q = "\"";
        let if_guard = format!("if [ -x {q}$BASE/bin/gnpc{q} ]; then");
        assert!(
            joined.contains(&if_guard),
            "guard 组件必须用 if/else 判缺席, 否则缺席也会 WARN"
        );
        assert!(joined.contains(&format!("|| echo {q}WARN gnpc guard 非零退出{q}")));
        // 旧写法: `[ -x ... ] && "$BASE/bin/gnpc" guard ... || echo WARN` (缺席也 WARN)
        assert!(
            !joined.contains(&format!(
                "[ -x {q}$BASE/bin/gnpc{q} ] && {q}$BASE/bin/gnpc{q} guard"
            )),
            "别用 &&|| 链挂 guard: 缺席与失败会被混为一谈"
        );
    }

    #[test]
    fn client_templates_render_without_placeholders() {
        let base = PathBuf::from("/Users/lwboy/.local/gnp");
        let sb = client_plist_singbox(&base).unwrap();
        assert!(sb.contains("com.gnpc.singbox"));
        assert!(sb.contains("/Users/lwboy/.local/gnp/bin/sing-box"));
        assert!(sb.contains("/Users/lwboy/.local/gnp/config.json"));
        assert!(sb.contains("/Users/lwboy/.local/gnp/var/sing-box.log"));
        assert!(sb.contains("KeepAlive"));

        let tick = client_plist_tick(&base).unwrap();
        assert!(tick.contains("com.gnpc.tick"));
        assert!(tick.contains("<string>/bin/bash</string>"));
        assert!(tick.contains("/Users/lwboy/.local/gnp/bin/tick.sh"));
        assert!(tick.contains("StartInterval"));
        assert!(tick.contains("tick-launchd.log"));
        // tick 不是常驻 → 不该有 KeepAlive
        assert!(!tick.contains("KeepAlive"));

        let unit = client_unit(&base).unwrap();
        assert!(unit.contains("Description=GNP Client"));
        assert!(unit.contains(
            "ExecStart=/Users/lwboy/.local/gnp/bin/sing-box run -c /Users/lwboy/.local/gnp/config.json"
        ));
        assert!(unit.contains("WantedBy=multi-user.target"));

        let sunit = server_unit(Path::new("/opt/gnp")).unwrap();
        assert!(sunit.contains("Description=GNP Server"));
        assert!(sunit.contains("ExecStart=/opt/gnp/bin/sing-box run -c /opt/gnp/config.json"));
    }

    #[test]
    fn cron_lines_are_single_line() {
        let c = client_cron_line(Path::new("/home/lwboy/.local/gnp"));
        assert_eq!(c, "* * * * * /home/lwboy/.local/gnp/bin/tick.sh");
        // 服务端必须显式带 GNP_HOME (tick.sh 默认 $HOME/.local/gnp 对 /opt/gnp 是错的)
        let s = server_cron_line(Path::new("/opt/gnp"));
        assert_eq!(s, "* * * * * GNP_HOME=/opt/gnp /opt/gnp/bin/tick.sh");
        for line in [client_cron(Path::new("/home/lw/.local/gnp")), server_cron(Path::new("/opt/gnp"))] {
            let cron_lines: Vec<&str> = line
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
                .collect();
            assert_eq!(cron_lines.len(), 1, "crontab 只允许一行: {:?}", cron_lines);
        }
    }

    #[test]
    fn missing_placeholder_is_a_hard_error() {
        assert!(assert_rendered("ExecStart=/opt/gnp/bin/sing-box run").is_ok());
        let e = assert_rendered("ExecStart={{SB_BIN}} run").unwrap_err().to_string();
        assert!(e.contains("SB_BIN"), "{}", e);
    }

    #[test]
    fn legacy_names_cover_old_deployments() {
        assert!(LEGACY_LAUNCHD.contains(&"com.gnp.sing-box"));
        assert!(LEGACY_LAUNCHD.contains(&"com.gnp.guard"));
        assert!(LEGACY_SYSTEMD.contains(&"gnp-proxy"));
        assert!(LEGACY_SYSTEMD.contains(&"gnp-hy2"));
        assert!(LEGACY_SYSTEMD_USER.contains(&"sing-box"));
    }

    #[test]
    fn cron_purge_keeps_foreign_lines() {
        let lines = vec![
            "0 3 * * * /usr/bin/backup.sh".to_string(),
            "* * * * * /home/lwboy/.local/bin/gnp-client guard >> /tmp/g.log".to_string(),
            "*/5 * * * * /home/lwboy/.local/gnp/bin/tick.sh".to_string(),
            "0 4 * * * /home/lwboy/.local/bin/gnp-client update-rules check".to_string(),
            "@reboot /opt/gnp-quic/gnp-hy2".to_string(),
        ];
        let kept = purge_gnp_cron_lines(&lines);
        assert_eq!(kept, vec!["0 3 * * * /usr/bin/backup.sh".to_string()]);
    }

    #[test]
    fn gnps_health_component_fails_loud() {
        // 组件失败必须非零退出 (tick.sh 落 WARN); 不能静默成功
        assert!(TICK_D_GNPS_HEALTH.contains("exit 1"));
        assert!(TICK_D_GNPS_HEALTH.contains("systemctl is-active gnps"));
    }
}
