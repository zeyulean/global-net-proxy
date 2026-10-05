//! gnp-client guard — 通道看门狗 (退避探测 + 故障冻结 + 告警)
//!
//! 设计为 cron 每分钟调一次 (`gnp-client guard --install-cron` 安装), 单 tick 无常驻。
//!
//! 职责 (2026-10-05 MTU 事件 P1):
//! 1. hy2 健康探测 — clash_api delay (5000ms 超时)
//! 2. 指数退避 — 连续失败后探测间隔 1m→2m→4m→…→cap 1h (±20% 抖动),
//!    把"服务端重启 → 全员重连风暴"这类二次伤害掐掉 (hysteria2 社区知名 QoS 触发模式);
//!    sing-box 内部连接级重试不可配置, 退避在编排层实现并落日志 (验收④"日志里退避间隔可见")
//! 3. 故障冻结 — 连续 2 次失败 → selector 强制 ssh-out (urltest 3m 探测周期太久);
//!    恢复探测通过 → 自动解冻交还 urltest
//! 4. 告警 — 状态翻转立即告警, 持续故障 30min 限频; 渠道: GNP_ALERT_CMD > osascript(Mac)
//!    > notify-send(Linux), 始终落 guard.log

use anyhow::{bail, Context as _, Result};
use serde_json::json;
use std::process::Command;

use crate::api;
use gnp_core::platform::{self, sb_dir};

const STATE_FILE: &str = "guard-state.json";
const LOG_FILE: &str = "guard.log";
/// 连续失败几次后冻结到 ssh
const FREEZE_AFTER: u32 = 2;
/// 探测退避: 60s * 2^n, 上限 180s (= urltest 3m 探测周期)
///
/// 上限不能太高: 退避同时延迟"恢复检测"(实测 1h 上限会让回切滞后 26min);
/// cap 对齐 urltest 周期后, 恢复检测 ≤4min, 且单机 1 次/3min 的 QUIC 握手
/// 远够不成重连风暴 (风暴 = 多机 × 每连接重试, urltest 机制下不存在)。
const BACKOFF_BASE_S: u64 = 60;
const BACKOFF_CAP_S: u64 = 180;
/// 持续故障告警限频
const ALERT_REPEAT_MS: u64 = 30 * 60 * 1000;
/// 健康心跳日志间隔 (免日志爆炸)
const HEARTBEAT_MS: u64 = 60 * 60 * 1000;

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct GuardState {
    failures: u32,
    frozen: bool,
    /// 下次允许探测的时间戳 ms
    next_probe_ms: u64,
    last_alert_ms: u64,
    last_ok_ms: u64,
    last_heartbeat_ms: u64,
}

pub fn run(install_cron: bool) -> Result<()> {
    if install_cron {
        return install_cron_job();
    }
    tick()
}

// --- cron ---

fn install_cron_job() -> Result<()> {
    let self_path = std::env::current_exe()?;
    let log = sb_dir().join(LOG_FILE);
    let cron_line = format!("* * * * * {} guard >> {} 2>&1", self_path.display(), log.display());

    let existing = Command::new("crontab").args(["-l"]).output();
    let mut lines: Vec<String> = Vec::new();
    if let Ok(out) = existing {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if !line.contains("gnp-client") && !line.contains("global-net-proxy") {
                    lines.push(line.to_string());
                }
            }
        }
    }
    lines.push(cron_line.clone());

    use std::io::Write;
    let mut st = Command::new("crontab")
        .args(["-"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("crontab 启动失败")?;
    {
        let mut child = st;
        // 显式关闭 stdin — 不 drop 则 crontab 等不到 EOF, 安装会永久挂起
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(lines.join("\n").as_bytes());
        }
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!("crontab 安装失败: {}", String::from_utf8_lossy(&output.stderr));
        }
    }
    println!("✅ guard cron 已安装 (每分钟): {}", cron_line);
    Ok(())
}

// --- 单 tick ---

fn tick() -> Result<()> {
    let state_path = sb_dir().join(STATE_FILE);
    let mut st: GuardState = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let now = now_ms();

    // sing-box 存活兜底 (update-rules check 的轻量重复, guard 顺带)
    let addr = match api::controller_addr() {
        Some(a) => a,
        None => {
            // 旧配置: 无 clash_api, guard 无法工作; 进程不在就拉起
            if let Ok(p) = platform::ensure_supported() {
                if !platform::config_exists() {
                    return Ok(());
                }
                if !gnp_core::service::is_running(p)? {
                    log("WARN  sing-box 未运行 (旧配置), 尝试拉起");
                    let _ = gnp_core::service::start(p);
                }
            }
            return Ok(());
        }
    };
    let Ok(p) = platform::ensure_supported() else { return Ok(()) };
    if !gnp_core::service::is_running(p)? {
        log("WARN  sing-box 未运行, 尝试拉起");
        let _ = gnp_core::service::start(p);
        save_state(&state_path, &st);
        return Ok(());
    }

    // 退避等待: 静默跳过 (失败 tick 已记录退避计划)
    if now < st.next_probe_ms {
        return Ok(());
    }

    // hy2 探测
    let probe = api::probe_delay(&addr, "hy2-out", 5000);

    match probe {
        Ok(ms) => {
            let was_down = st.failures > 0 || st.frozen;
            if was_down {
                log(&format!(
                    "OK    hy2 恢复 ({}ms, 此前连续失败 {} 次, frozen={})", ms, st.failures, st.frozen
                ));
                if st.frozen {
                    match crate::switch::set_selector("auto-out") {
                        Ok(_) => log("OK    已解冻 → auto-out (urltest 自动选路)"),
                        Err(e) => log(&format!("ERROR 解冻失败: {}", e)),
                    }
                }
                alert("gnp 通道恢复", "hy2 握手恢复, 已自动回切 (urltest)");
            } else if now.saturating_sub(st.last_heartbeat_ms) > HEARTBEAT_MS {
                log(&format!("OK    hy2 正常 ({}ms)", ms));
            }
            st.failures = 0;
            st.frozen = false;
            st.last_ok_ms = now;
            st.last_heartbeat_ms = now;
            st.next_probe_ms = 0;

            // 严格 hy2 优先 (plan 4.3): urltest 按"最快+tolerance 300ms"选路,
            // hy2 恢复后可能因差距不足 300ms 一直留在 ssh (实测 cozepc: 155ms vs 326ms)。
            // guard 在 hy2 健康且 selector=auto-out 时把 urltest 卡在 ssh 的情况
            // 纠正为 hy2-out; 手动 switch ssh (selector≠auto-out) 不受影响。
            if !was_down {
                if let Ok(proxies) = api::api_get_json(&addr, "/proxies", 2) {
                    let sel_now = proxies.get("proxy-out").and_then(|p| p.get("now")).and_then(|n| n.as_str());
                    let auto_now = proxies.get("auto-out").and_then(|p| p.get("now")).and_then(|n| n.as_str());
                    if sel_now == Some("auto-out") && auto_now == Some("ssh-out") {
                        log("INFO  hy2 健康但 urltest 因 tolerance 停在 ssh-out → 强制 hy2-out");
                        if let Err(e) = crate::switch::set_selector("hy2-out") {
                            log(&format!("ERROR 强制 hy2 失败: {}", e));
                        }
                    }
                }
            }
        }
        Err(_) => {
            st.failures += 1;
            // 指数退避 + 20% 抖动: 60 * 2^(n-1), cap 1h
            let base = BACKOFF_BASE_S
                .saturating_mul(1u64 << (st.failures - 1).min(6))
                .min(BACKOFF_CAP_S);
            let jitter_pct = 80 + (now % 41) as u64; // 80~120%
            let backoff_s = base * jitter_pct / 100;
            st.next_probe_ms = now + backoff_s * 1000;
            log(&format!(
                "FAIL  hy2 探测失败 (连续 {} 次) → 退避 {}s 后再探",
                st.failures, backoff_s
            ));

            if st.failures >= FREEZE_AFTER && !st.frozen {
                match crate::switch::set_selector("ssh-out") {
                    Ok(_) => {
                        st.frozen = true;
                        st.last_alert_ms = now;
                        log("WARN  已冻结到 ssh-out (TCP 兜底免疫 UDP MTU 黑洞)");
                        // 实测 (2026-10-05): 长跑实例里 ssh-out 可能僵死 (直接 ssh 正常但
                        // 出站连探测都挂), 冻结后重启服务确保兜底通道干净可用
                        if let Ok(p) = platform::ensure_supported() {
                            let _ = gnp_core::service::stop(p);
                            std::thread::sleep(std::time::Duration::from_secs(1));
                            match gnp_core::service::start(p) {
                                Ok(()) => log("OK    sing-box 已重启 (确保 ssh-out 干净)"),
                                Err(e) => log(&format!("ERROR 重启失败: {}", e)),
                            }
                        }
                        alert(
                            "gnp 通道降级",
                            &format!("hy2 连续 {} 次探测失败, 已冻结到 ssh 兜底; 恢复后自动回切", st.failures),
                        );
                    }
                    Err(e) => log(&format!("ERROR 冻结失败: {}", e)),
                }
            } else if st.frozen && now.saturating_sub(st.last_alert_ms) > ALERT_REPEAT_MS {
                st.last_alert_ms = now;
                alert(
                    "gnp hy2 仍不可用",
                    &format!("已持续 {} 分钟, 流量在 ssh 兜底上", (now.saturating_sub(st.last_ok_ms)) / 60000),
                );
            }
        }
    }

    save_state(&state_path, &st);
    Ok(())
}

fn save_state(path: &std::path::Path, st: &GuardState) {
    if let Ok(s) = serde_json::to_string_pretty(&json!({
        "failures": st.failures,
        "frozen": st.frozen,
        "next_probe_ms": st.next_probe_ms,
        "last_alert_ms": st.last_alert_ms,
        "last_ok_ms": st.last_ok_ms,
        "last_heartbeat_ms": st.last_heartbeat_ms,
    })) {
        let _ = std::fs::write(path, s);
    }
}

// --- 日志与告警 ---

fn log(msg: &str) {
    let line = format!("[{}] {}", ts_readable(), msg);
    println!("{}", line);
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(sb_dir().join(LOG_FILE))
    {
        let _ = writeln!(f, "{}", line);
    }
}

fn ts_readable() -> String {
    // 秒级可读时间 (本地时区, 免 chrono 依赖: 用 date 命令)
    Command::new("date")
        .args(["+%Y-%m-%d %H:%M:%S"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| format!("{}", now_ms()))
}

/// 告警: GNP_ALERT_CMD > osascript (Mac) > notify-send (Linux); 始终落日志
fn alert(title: &str, body: &str) {
    log(&format!("ALERT {} — {}", title, body));
    if let Ok(cmd) = std::env::var("GNP_ALERT_CMD") {
        let _ = Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .env("GNP_ALERT_TITLE", title)
            .env("GNP_ALERT_BODY", body)
            .status();
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!("display notification \"{}\" with title \"{}\"", body, title);
        let _ = Command::new("osascript").args(["-e", &script]).status();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = Command::new("notify-send").args([title, body]).status();
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
