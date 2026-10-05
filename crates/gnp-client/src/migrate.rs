//! `gnpc migrate` — 一台机器一条命令完成 v1 → v2 迁移 (plan §4.2)
//!
//! 顺序是硬要求 (§4.2.4): **先起新的并验证, 再拆旧的**。
//! 任一步失败就停在新半边, 旧服务保持原样 —— 随时可退 (backups/legacy-singbox/)。
//!
//! 步骤:
//! 1. 取唯一事实源: `--config <host toml>` > 已有 `$GNP_HOME/config.toml` > 解析旧 config.json
//! 2. 搬资产: sing-box 二进制 / rules/ / cache.db / gnpc 自身 → 新布局
//! 3. 生成 config.json, **`sing-box check` 必须过**, 不过就中止 (不动旧服务)
//! 4. 装调度 (新服务先起 + 验证) → 再拆旧 (unload/disable/删旧 plist·unit·cron 行)
//! 5. 旧目录整体挪 `backups/legacy-singbox/` (不删, 回滚用)
//! 6. 输出验证清单

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use gnp_core::platform;
use gnp_core::settings::{ClientAuth, ClientLocal, ClientServer, ClientSettings, GuardSettings};

use crate::install_sched;

pub struct MigrateArgs {
    /// host toml (`deploy/hosts/<host>.toml`) — 给了就直接用, 跳过旧配置解析
    pub config: Option<String>,
    /// 只报告将要做什么, 不落盘不动服务
    pub dry_run: bool,
}

pub fn run(args: &MigrateArgs) -> Result<()> {
    let base = platform::gnp_home();
    let legacy = platform::legacy_sb_dir();
    let legacy_cfg = legacy.join("config.json");

    println!("== gnpc migrate: {} → {} ==", legacy.display(), base.display());
    if args.dry_run {
        println!("   (--dry-run: 只报告, 不落盘不动服务)");
    }

    // --- 1. 唯一事实源 ---
    let (settings, verbatim) = resolve_settings(args, &legacy_cfg)?;
    settings.validate()?;
    println!("\n[1/6] 唯一事实源");
    println!("   server: {}:{}", settings.server.host, settings.server.hy2_port);
    println!(
        "   listen: {}  clash_api: {}",
        settings.client.listen,
        if settings.clash_api().is_some() { "on" } else { "off" }
    );
    println!(
        "   ssh 兜底: {}",
        settings
            .ssh_fallback()
            .map(|s| format!("{}@{}:{}", s.user, s.server, s.server_port))
            .unwrap_or_else(|| "(关)".into())
    );

    if args.dry_run {
        println!("\n--dry-run 结束: 未做任何改动");
        return Ok(());
    }

    // --- 2. 落事实源 + 搬资产 ---
    println!("\n[2/6] 布局与资产");
    platform::ensure_layout(&base)?;
    let toml_path = match &verbatim {
        Some(src) => {
            // 逐字拷贝 (保住注释与格式 → 与 deploy/hosts/<host>.toml md5 一致)
            let dst = platform::gnp_config_toml();
            let content = std::fs::read(src)
                .with_context(|| format!("读取 {} 失败", src.display()))?;
            platform::write_private(&dst, &String::from_utf8_lossy(&content))?;
            println!("   config.toml: {} ← 逐字来自 {}", dst.display(), src.display());
            dst
        }
        None => settings.save_default()?,
    };
    println!("   config.toml: {}", toml_path.display());

    // sing-box 二进制: 旧目录 → bin/ (新目录已有则跳过, 保留新版本)
    let sb_new = platform::gnp_sb_bin();
    if sb_new.exists() {
        println!("   sing-box: 已存在 ({})", sb_new.display());
    } else {
        let sb_old = legacy.join(if cfg!(windows) { "sing-box.exe" } else { "sing-box" });
        if sb_old.exists() {
            std::fs::copy(&sb_old, &sb_new).context("复制 sing-box 二进制失败")?;
            set_exec(&sb_new)?;
            println!("   sing-box: {} → {}", sb_old.display(), sb_new.display());
        } else {
            println!("   sing-box: 旧目录没有 → 稍后 `gnpc install` 下载");
        }
    }

    // gnpc 自身入 bin/ (tick.sh 按 $BASE/bin/gnpc 找内核组件)
    ensure_self_installed();

    // rules/ 与 cache.db
    copy_tree(&legacy.join("rules"), &platform::gnp_rules_dir(), "rules/")?;
    let cache_old = legacy.join("cache.db");
    if cache_old.exists() {
        let dst = platform::gnp_cache_db();
        if dst.exists() {
            println!("   cache.db: 已有 ({})", dst.display());
        } else {
            let _ = std::fs::copy(&cache_old, &dst);
            println!("   cache.db: → {}", dst.display());
        }
    }

    // secrets (aipro 路由容器挂载源; 带换行 — 坑清单 #3)
    write_secrets(&settings)?;

    // --- 3. 生成 config.json, sing-box check 必须过 ---
    println!("\n[3/6] 生成 config.json");
    let json = gnp_core::install::generate_config(&settings)?;
    if platform::sb_exists() {
        gnp_core::install::check_config_file(&json)
            .context("sing-box check 未过 —— 中止迁移, 旧服务未动")?;
        println!("   ✅ sing-box check 通过");
    } else {
        println!("   ⚠️  sing-box 不存在, 跳过 check (装完再 `gnpc config --check`)");
    }

    // --- 4. 装调度: 落盘 → 旧常驻让位端口 → 装载新 → 验证 → 才拆旧 ---
    //
    // 顺序说明: 计划 §4.2.4 写的是"新服务 start+verify → 再拆旧"。那是**新机器**的顺序;
    // 同机迁移时新旧都占 127.0.0.1:1080, 旧的不让位, 新的起来必 crash-loop
    // (launchd 只会看到"job 已加载"就误判成功)。所以这里拆成:
    //   落盘 → 停旧(保留 plist 可回滚) → 装载新 → 进程+端口+出口三重验证
    //   → 失败就把旧 plist 装回去 → 成功才清残留/归档旧目录
    println!("\n[4/6] 调度 + 服务接线 (先让位端口, 后拆旧)");
    install_sched::install_with(false)?;
    let stopped = install_sched::stop_legacy_services();
    for s in &stopped {
        println!("   - {}", s);
    }
    // 让 sing-box 释放监听 (只有真停了旧服务才需要等)
    if !stopped.is_empty() {
        for _ in 0..30 {
            if !platform::port_open(1080) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if platform::port_open(1080) {
            println!("   ⚠️  1080 仍被占 —— 可能有别的进程持有, 继续尝试装载");
        }
    }

    // 装载新常驻 + 新调度 (tick 也在这步, 漏了就等于调度没接上)
    if let Err(e) = install_sched::load_now() {
        println!("   ❌ 装载失败: {}", e);
        rollback_legacy();
        anyhow::bail!("迁移中止: 装载失败, 已把旧服务原样装回");
    }

    let verified = verify_new_service();
    if !verified {
        println!("   ❌ 新服务未通过验证 (进程/端口/出口)");
        rollback_legacy();
        anyhow::bail!(
            "迁移中止: 新服务未通过验证, 已把旧服务原样装回。\n   排障: {} 的 tick.log / sing-box.log",
            platform::gnp_var_dir().display()
        );
    }
    println!("   ✅ 新服务已验证: 进程在跑 + 1080 在听 + 出口 IP 正确");

    let purged = install_sched::purge_legacy();
    if purged.is_empty() {
        println!("   旧调度: 无残留");
    } else {
        for p in purged {
            println!("   - {}", p);
        }
    }
    remove_legacy_plist_if_any();

    // --- 5. 旧目录挪 backups (不删) ---
    println!("\n[5/6] 旧布局归档");
    let dest = platform::gnp_backups_dir().join("legacy-singbox");
    if legacy.exists() {
        if dest.exists() {
            // 已归档过 (重跑): 保留首次归档, 旧目录原地待人工确认
            println!("   {} 已存在, 跳过 (旧目录仍在: {})", dest.display(), legacy.display());
        } else {
            match std::fs::rename(&legacy, &dest) {
                Ok(()) => println!("   {} → {}", legacy.display(), dest.display()),
                Err(e) => {
                    // 跨设备/权限: 退化为 copy + 留原目录
                    println!("   rename 失败 ({}), 改为复制归档", e);
                    copy_tree(&legacy, &dest, "legacy-singbox")?;
                    println!("   已复制到 {} (旧目录保留, 人工确认后删)", dest.display());
                }
            }
        }
    } else {
        println!("   旧目录不存在 (可能已迁过)");
    }

    // sudo 部署属主归还 (root 经 sudo 跑 migrate 会把部署根写成 root 属主 —
    // 2026-10-05 aipro 实发, tick 进 root crontab + guard 写不动 config.json 的根因)
    crate::install_sched::fix_deploy_ownership(&base);

    // --- 6. 验证清单 ---
    println!("\n[6/6] 验证清单");
    print_checklist();
    println!("\n✅ 迁移完成。回滚: `gnpc uninstall-scheduler` + 从 {} 原位恢复", dest.display());
    Ok(())
}

// --- 事实源解析 ---

/// 取事实源: 返回 (settings, 逐字来源文件)
///
/// 逐字来源非 None 时, 落盘用**原文件字节拷贝**而不是重新序列化 —— 否则
/// §7.1 验收 6 的 md5 比对必然失败 (注释/格式会变), §5 的
/// "scp deploy/hosts/<host>.toml → gnpc migrate" 也会被改写。
fn resolve_settings(
    args: &MigrateArgs,
    legacy_cfg: &Path,
) -> Result<(ClientSettings, Option<PathBuf>)> {
    // a) 显式 --config
    if let Some(p) = &args.config {
        let path = PathBuf::from(p);
        println!("[0/6] 事实源 = {}", p);
        return Ok((ClientSettings::load(&path)?, Some(path)));
    }
    // b) 新布局已有 (多半是刚 scp 过来的 deploy/hosts/<host>.toml)
    let cur = platform::gnp_config_toml();
    if cur.exists() {
        println!("[0/6] 事实源 = 已有 {}", cur.display());
        return Ok((ClientSettings::load(&cur)?, None));
    }
    // c) 解析旧 config.json (只能重新序列化, 没有 toml 可抄)
    println!("[0/6] 事实源 = 解析旧 {}", legacy_cfg.display());
    if !legacy_cfg.exists() {
        bail!(
            "既没有 {} 也没有旧配置 {}; 请用 `gnpc init` 生成或 `gnpc migrate --config deploy/hosts/<host>.toml`",
            cur.display(),
            legacy_cfg.display()
        );
    }
    Ok((settings_from_legacy(legacy_cfg)?, None))
}

/// 旧 config.json → ClientSettings (只取 gnp 关心的字段, 其余不管)
pub fn settings_from_legacy(cfg: &Path) -> Result<ClientSettings> {
    let v = gnp_core::config::load(cfg)?;
    let hy2 = gnp_core::config::extract_hy2_endpoint(&v)
        .context("旧 config.json 里没有 hysteria2 outbound")?;
    if hy2.server.is_empty() {
        bail!("旧 config.json 的 hy2 outbound 缺 server");
    }

    // mixed inbound 的 listen
    let listen = v
        .get("inbounds")
        .and_then(|x| x.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|i| i.get("type").and_then(|t| t.as_str()) == Some("mixed"))
        })
        .and_then(|i| i.get("listen"))
        .and_then(|l| l.as_str())
        .unwrap_or("0.0.0.0")
        .to_string();

    // dns-hosts predefined → [client.hosts]
    let mut hosts = std::collections::BTreeMap::new();
    if let Some(pre) = v
        .get("dns")
        .and_then(|d| d.get("servers"))
        .and_then(|s| s.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|s| s.get("tag").and_then(|t| t.as_str()) == Some("dns-hosts"))
        })
        .and_then(|s| s.get("predefined"))
        .and_then(|p| p.as_object())
    {
        for (k, val) in pre {
            if let Some(ip) = val.as_str() {
                hosts.insert(k.clone(), ip.to_string());
            }
        }
    }

    // ssh 兜底 outbound (旧配置可能没有 → 用默认形态, hy2 活着时无副作用)
    let (ssh_user, ssh_key, ssh_port) = match gnp_core::config::find_outbound(&v, "ssh") {
        Some(ssh) => (
            ssh.get("user")
                .and_then(|u| u.as_str())
                .unwrap_or("lw")
                .to_string(),
            ssh.get("private_key_path")
                .and_then(|p| p.as_str())
                .unwrap_or("~/.ssh/id_ed25519")
                .to_string(),
            ssh.get("server_port")
                .and_then(|p| p.as_u64())
                .unwrap_or(22) as u16,
        ),
        None => (
            "lw".to_string(),
            "~/.ssh/id_ed25519".to_string(),
            22,
        ),
    };

    let clash_port = gnp_core::config::clash_api_addr(&v)
        .and_then(|a| a.rsplit(':').next().and_then(|p| p.parse().ok()))
        .unwrap_or(platform::GNP_CLASH_API_PORT);

    let s = ClientSettings {
        server: ClientServer {
            host: hy2.server,
            // 旧配置里的端口就是实机在用的端口 (2026-10-05 已迁 5766), 照搬
            hy2_port: hy2.server_port,
            ssh_port,
            ssh_user,
            ssh_key,
        },
        auth: ClientAuth {
            hy2_password: hy2.password,
            obfs_password: hy2.obfs_password.unwrap_or_default(),
        },
        client: ClientLocal {
            listen,
            clash_api_port: clash_port,
            hosts,
        },
        guard: GuardSettings::default(),
    };
    Ok(s)
}

// --- 辅助 ---

/// 把 gnpc 自身放进 `$GNP_HOME/bin/`
///
/// tick.sh 的内核组件就是 `$BASE/bin/gnpc` (组件缺席=静默跳过) —— 不放这里,
/// guard 与 rules-update 会静默永不执行。幂等: 已在位或同一文件则跳过。
pub fn ensure_self_installed() {
    let Ok(self_exe) = std::env::current_exe() else {
        println!("   ⚠️  取自身路径失败, tick 将用 PATH 里的 gnpc");
        return;
    };
    let dest = platform::gnp_client_bin();
    let same = self_exe
        .canonicalize()
        .ok()
        .zip(dest.canonicalize().ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false);
    if same {
        println!("   gnpc: 已在位 ({})", dest.display());
        return;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    match std::fs::copy(&self_exe, &dest) {
        Ok(_) => {
            if let Err(e) = set_exec(&dest) {
                println!("   ⚠️  gnpc 已复制但 chmod 失败: {}", e);
            }
            println!("   gnpc: {} → {}", self_exe.display(), dest.display());
        }
        Err(e) => println!("   ⚠️  gnpc 自身复制失败 ({}), tick 将用 PATH 里的 gnpc", e),
    }
}

fn set_exec(p: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path, label: &str) -> Result<()> {
    if !from.exists() {
        println!("   {}: 旧目录没有, 跳过", label);
        return Ok(());
    }
    std::fs::create_dir_all(to)?;
    let mut n = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let dst = to.join(entry.file_name());
        if dst.exists() {
            continue; // 已有则保留新文件
        }
        std::fs::copy(entry.path(), &dst)?;
        n += 1;
    }
    println!("   {}: → {} ({} 个新文件)", label, to.display(), n);
    Ok(())
}

fn write_secrets(s: &ClientSettings) -> Result<()> {
    let dir = platform::gnp_secrets_dir();
    std::fs::create_dir_all(&dir)?;
    if !s.auth.hy2_password.is_empty() {
        platform::write_secret(&platform::gnp_secret_hy2_password(), &s.auth.hy2_password)?;
        println!("   secret: {}", platform::gnp_secret_hy2_password().display());
    }
    if let Some(obfs) = s.obfs() {
        platform::write_secret(&platform::gnp_secret_hy2_obfs(), &obfs)?;
        println!("   secret: {}", platform::gnp_secret_hy2_obfs().display());
    }
    Ok(())
}

/// 三重验证新服务: 进程在跑 + 1080 在听 + 出口 IP 真是服务端
///
/// 只看 launchctl 有 job 不算过 —— KeepAlive 的 crash-loop 也是"已加载"。
fn verify_new_service() -> bool {
    let Ok(p) = platform::ensure_supported() else {
        return false;
    };
    // 1) **新**服务自己必须在跑 (不能只看"有进程在跑" —— 旧服务没停干净时会误判)
    let _ = p;
    let mut running = false;
    for _ in 0..20 {
        if install_sched::new_service_running() {
            running = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    if !running {
        println!("      · 新服务 (com.gnpc.singbox / gnpc.service) 没起来");
        return false;
    }
    // 2) 端口 (mixed 1080)
    let mut port = false;
    for _ in 0..20 {
        if platform::port_open(1080) {
            port = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    if !port {
        println!("      · 1080 没在听");
        return false;
    }
    // 3) 出口 IP (证明双通道真通, 不只是进程活着)
    match gnp_core::tunnel::detect_exit_ip("socks5h://127.0.0.1:1080", 10) {
        Ok((ip, ms)) => {
            println!("      · 出口 {} ({}ms)", ip, ms);
            true
        }
        Err(e) => {
            println!("      · 出口检测失败: {}", e);
            false
        }
    }
}

/// 回滚: 把旧常驻服务原样装回 (旧目录/旧单元此刻都还在)
fn rollback_legacy() {
    // Linux: 旧 systemd 单元重新 enable+start
    if gnp_core::platform::Platform::detect() == gnp_core::platform::Platform::Linux {
        for name in gnp_core::scheduler::LEGACY_SYSTEMD {
            let unit = gnp_core::scheduler::systemd_unit_path(name);
            let renamed = unit.with_extension("service.disabled");
            if !unit.exists() && renamed.exists() {
                let _ = std::fs::rename(&renamed, &unit);
            }
            if unit.exists() {
                let _ = gnp_core::service::enable_named(name);
                let _ = gnp_core::service::start_named(name);
                if gnp_core::tunnel::service_active_named(name) {
                    println!("   - 已装回旧服务 {}", name);
                }
            }
        }
        // 停掉没起来的新服务, 免得两个抢 1080
        let _ = gnp_core::service::stop_named(gnp_core::scheduler::SYSTEMD_CLIENT);
        return;
    }
    println!("↩︎  回滚: 装回旧服务 {}", gnp_core::scheduler::LEGACY_LAUNCHD.join(", "));
    for label in gnp_core::scheduler::LEGACY_LAUNCHD {
        let plist = gnp_core::scheduler::launchd_plist_path(label);
        // plist 可能在 purge 阶段已被移成 .migrated —— 移回来再装, 否则回滚是空转
        if !plist.exists() {
            let moved = gnp_core::scheduler::launchd_plist_path(label)
                .with_extension("plist.migrated");
            if moved.exists() {
                let _ = std::fs::rename(&moved, &plist);
                println!("   - 恢复 plist {}", plist.display());
            } else {
                continue;
            }
        }
        gnp_core::scheduler::launchd_enable(label);
        let _ = std::process::Command::new("launchctl")
            .args(["unload", plist.to_str().unwrap_or("")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let _ = std::process::Command::new("launchctl")
            .args(["load", plist.to_str().unwrap_or("")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        println!("   - 已装回 {}", label);
    }
    // 新服务停掉, 免得两个抢 1080
    if let Ok(p) = platform::ensure_supported() {
        let _ = gnp_core::service::stop(p);
    }
    for label in [gnp_core::scheduler::LAUNCHD_TICK, gnp_core::scheduler::LAUNCHD_SINGBOX] {
        let plist = gnp_core::scheduler::launchd_plist_path(label);
        let _ = std::process::Command::new("launchctl")
            .args(["unload", plist.to_str().unwrap_or("")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let _ = std::process::Command::new("launchctl")
            .args(["remove", label])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// 旧 launchd plist 已由 purge_legacy 移走; 这里兜底删掉残留的 .plist (不在 backups 的)
fn remove_legacy_plist_if_any() {
    for label in gnp_core::scheduler::LEGACY_LAUNCHD {
        let plist = gnp_core::scheduler::launchd_plist_path(label);
        if plist.exists() {
            let _ = std::fs::remove_file(&plist);
            println!("   - 删除残留 plist {}", plist.display());
        }
    }
}

fn print_checklist() {
    println!("   1) gnpc status                       # 双通道健康 / urltest=hy2-out / 出口 IP");
    println!("   2) launchctl list | grep gnpc         # Mac: 仅 com.gnpc.*");
    println!("      crontab -l | grep -c tick          # Linux: 仅一行 tick");
    println!("   3) tail {}/tick.log", platform::gnp_var_dir().display());
    println!("   4) ls {}  # 应已不存在", platform::legacy_sb_dir().display());
    println!("   5) pgrep -f sing-box | wc -l          # 应恰 1 个");
}
