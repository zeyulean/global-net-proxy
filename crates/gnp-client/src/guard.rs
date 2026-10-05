//! gnpc guard — 通道看门狗 (退避探测 + 故障冻结 + 告警)
//!
//! **单 tick 无常驻**: 由 tick.sh 每分钟调一次 (调度唯一, D3); 手动跑用 `gnpc guard`,
//! 不要自己写 cron (会破坏调度唯一性)。参数取自 config.toml `[guard]`, 状态/日志落 `var/`。
//!
//! 职责 (2026-10-05 MTU 事件 P1):
//! 1. hy2 健康探测 — clash_api delay (5000ms 超时)
//! 2. 指数退避 — 连续失败后探测间隔 backoff_base_s→2^n→cap (默认 60s→180s, ±20% 抖动),
//!    把"服务端重启 → 全员重连风暴"这类二次伤害掐掉 (hysteria2 社区知名 QoS 触发模式);
//!    sing-box 内部连接级重试不可配置, 退避在编排层实现并落日志 (验收④"日志里退避间隔可见")
//! 3. 故障冻结 — 连续 freeze_after_failures 次失败 → selector 强制 ssh-out
//!    (urltest 3m 探测周期太久);
//!    恢复探测通过 → 自动解冻交还 urltest
//! 4. 告警 — 状态翻转立即告警, 持续故障 30min 限频; 渠道: GNP_ALERT_CMD > osascript(Mac)
//!    > notify-send(Linux), 始终落 guard.log

use anyhow::Result;
use serde_json::json;
use std::process::Command;

use crate::api;
use gnp_core::platform;
use gnp_core::settings::GuardSettings;

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

/// 入口: 跑一次 tick (调度由 tick.sh 负责, 本命令不装 cron)
pub fn run() -> Result<()> {
    tick()
}

/// guard 参数: config.toml `[guard]`, 读不到用缺省 (实测教训值)
fn guard_settings() -> GuardSettings {
    gnp_core::settings::ClientSettings::load_or_default().guard
}

// --- 单 tick ---

fn tick() -> Result<()> {
    let g = guard_settings();
    // var/ 一定要先在 (服务刚起时可能还没有)
    if let Some(parent) = platform::gnp_var_dir().parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::create_dir_all(platform::gnp_var_dir()).ok();
    let state_path = platform::gnp_guard_state();
    let mut st: GuardState = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let now = now_ms();

    // sing-box 存活兜底 (guard 顺带, 不额外等 rules-update)
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

            // 一致性自愈 (任何来源的不一致都能收敛, 手动 pin 不碰):
            // - 半冻结残留: 热选=ssh-out 但 config default=auto-out 且未 frozen
            //   (2026-10-05 aipro 实发: set_selector 半失败 —— 热切成功、config 写回失败,
            //    旧版 frozen=false → 恢复/防粘滞路径都不收拾, 永久卡 ssh-out)
            // - urltest 粘滞: selector=auto-out 而 urltest 因 tolerance 卡 ssh-out (cozepc 实测)
            if !st.frozen {
                if let Ok(proxies) = api::proxies_map(&addr, 2) {
                    let sel_now = api::proxy_now(&proxies, "proxy-out");
                    let auto_now = api::proxy_now(&proxies, "auto-out");
                    let cfg_default = selector_default_in_config();
                    match heal_action(sel_now.as_deref(), auto_now.as_deref(), cfg_default.as_deref()) {
                        Some("auto-out") => {
                            log("INFO  半冻结残留 (热=ssh-out, config=auto-out) → 回 auto-out");
                            if let Err(e) = crate::switch::set_selector("auto-out") {
                                log(&format!("ERROR 残留修复失败: {}", e));
                            }
                        }
                        Some("hy2-out") => {
                            log("INFO  hy2 健康但 urltest 因 tolerance 停在 ssh-out → 强制 hy2-out");
                            if let Err(e) = crate::switch::set_selector("hy2-out") {
                                log(&format!("ERROR 强制 hy2 失败: {}", e));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Err(_) => {
            st.failures += 1;
            // 指数退避 + 20% 抖动: 60 * 2^(n-1), cap 1h
            // cap 不能太高: 退避同时延迟"恢复检测" (实测 1h 上限让回切滞后 26min);
            // cap 对齐 urltest 3m 周期后恢复检测 ≤4min
            let base = g
                .backoff_base_s
                .saturating_mul(1u64 << (st.failures - 1).min(6))
                .min(g.backoff_cap_s);
            let jitter_pct = 80 + (now % 41) as u64; // 80~120%
            let backoff_s = base * jitter_pct / 100;
            st.next_probe_ms = now + backoff_s * 1000;
            log(&format!(
                "FAIL  hy2 探测失败 (连续 {} 次) → 退避 {}s 后再探",
                st.failures, backoff_s
            ));

            if st.failures >= g.freeze_after_failures && !st.frozen {
                // 先记账再动作 (2026-10-05 aipro 实发): set_selector 半失败时旧版不标
                // frozen, 恢复路径 (if st.frozen) 和防粘滞都不会收拾 → 永久卡 ssh-out。
                // 标了 frozen, 恢复探测一通过就一定尝试回 auto-out。
                st.frozen = true;
                st.last_alert_ms = now;
                match crate::switch::set_selector("ssh-out") {
                    Ok(_) => log("WARN  已冻结到 ssh-out (TCP 兜底免疫 UDP MTU 黑洞)"),
                    Err(e) => log(&format!(
                        "ERROR 冻结切换失败 (urltest 仍会自行落到 ssh-out): {}",
                        e
                    )),
                }
                // 实测 (2026-10-05): 长跑实例里 ssh-out 可能僵死 (直接 ssh 正常但
                // 出站连探测都挂), 冻结后重启服务确保兜底通道干净可用 —— 切换成败都要试
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
        .open(platform::gnp_guard_log())
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

// --- 一致性自愈 (纯逻辑, 可单测) ---

/// 读 config.json 里 selector 的 default (重启后的权威值)
fn selector_default_in_config() -> Option<String> {
    let v = gnp_core::config::load(&platform::gnp_config_json()).ok()?;
    gnp_core::config::find_outbound(&v, "selector")?
        .get("default")?
        .as_str()
        .map(|s| s.to_string())
}

/// 健康态收敛决策:
/// - (热=ssh-out, config=auto-out)   → 回 auto-out (半冻结残留, 任何来源)
/// - (热=auto-out, urltest=ssh-out)  → 强制 hy2-out (tolerance 粘滞, cozepc 实测)
/// - 手动 switch ssh (热=ssh-out 且 config=ssh-out) → 不碰 (用户意图)
/// - config 读不到 → 保守不碰 (无法与手动 pin 区分)
fn heal_action(
    sel_now: Option<&str>,
    auto_now: Option<&str>,
    cfg_default: Option<&str>,
) -> Option<&'static str> {
    if matches!((sel_now, cfg_default), (Some("ssh-out"), Some("auto-out"))) {
        return Some("auto-out");
    }
    if matches!((sel_now, auto_now), (Some("auto-out"), Some("ssh-out"))) {
        return Some("hy2-out");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::heal_action;

    /// 2026-10-05 aipro 实发: 热切成功+config 写回失败 → 永久卡 ssh-out
    #[test]
    fn half_frozen_leftover_recovers_to_auto() {
        assert_eq!(
            heal_action(Some("ssh-out"), Some("hy2-out"), Some("auto-out")),
            Some("auto-out")
        );
    }

    /// 手动 `gnpc switch ssh` 热态与 config 同为 ssh-out —— 用户意图, 不碰
    #[test]
    fn manual_ssh_pin_is_respected() {
        assert_eq!(heal_action(Some("ssh-out"), Some("ssh-out"), Some("ssh-out")), None);
    }

    /// urltest tolerance 粘滞 (cozepc 实测: hy2 155ms vs ssh 326ms 不切)
    #[test]
    fn urltest_stickiness_forces_hy2() {
        assert_eq!(
            heal_action(Some("auto-out"), Some("ssh-out"), Some("auto-out")),
            Some("hy2-out")
        );
    }

    /// config 不可读 → 无法区分手动 pin, 保守不碰
    #[test]
    fn unreadable_config_is_conservative() {
        assert_eq!(heal_action(Some("ssh-out"), Some("hy2-out"), None), None);
    }

    #[test]
    fn healthy_state_is_untouched() {
        assert_eq!(
            heal_action(Some("auto-out"), Some("hy2-out"), Some("auto-out")),
            None
        );
    }
}
