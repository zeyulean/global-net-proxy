//! gnpc probe — 主动诊断: hy2 握手测试 + UDP 载荷尺寸扫描
//!
//! 把 2026-10-05 MTU 黑洞事件两天的排查范式 (plan.md 附录) 一条命令化:
//! 1. hy2 握手测试 — 起临时 sing-box 实例 (独立端口/无 cache_file), 经真实路径 curl generate_204
//! 2. 载荷尺寸扫描 — 客户端向服务端发 N 个指定尺寸 UDP 包, ssh 读服务端
//!    /proc/net/snmp Udp InDatagrams 差分 → 判"到达 vs 被丢" → MTU 截止点
//! 3. 输出结论: 路径是否容得下 QUIC (≥1200B 初始包), 建议 hy2 / ssh 兜底
//!
//! 尺寸扫描判据 (事件证据①): 载荷 1200B 全达、1240B 起全丢 → 截止点在 1228~1268,
//! QUIC 初始包 (线上 1228B+) 落入死区, 握手无声失败。

use anyhow::{bail, Context, Result};
use serde_json::json;
use std::net::UdpSocket;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct ProbeArgs {
    pub sizes: Vec<u16>,
    pub count: u32,
    pub ssh_user: String,
    pub ssh_port: u16,
    pub skip_handshake: bool,
    pub skip_mtu: bool,
}

/// 入口
pub fn run(args: ProbeArgs) -> Result<()> {
    let cfg_path = gnp_core::platform::gnp_config_json();
    if !cfg_path.exists() {
        bail!("配置不存在: {} (先 gnpc install)", cfg_path.display());
    }
    let v = gnp_core::config::load(&cfg_path)?;
    let hy2 = gnp_core::config::extract_hy2_endpoint(&v)
        .context("配置中没有 hysteria2 outbound")?;

    println!("== gnp probe ({}:{}/UDP) ==", hy2.server, hy2.server_port);
    let mut handshake_ok = false;
    if !args.skip_handshake {
        handshake_ok = handshake_test(&hy2);
    }
    let mut mtu_ok: Option<bool> = None;
    if !args.skip_mtu {
        mtu_ok = mtu_scan(&hy2.server, hy2.server_port, &args).map(|mp| mp >= 1228);
    }

    // 结论
    println!("\n== 结论 ==");
    if args.skip_handshake {
        println!("  (握手测试已跳过)");
    } else if handshake_ok {
        println!("  ✅ hy2 握手可用 — 代理主通道健康");
    } else {
        println!("  ❌ hy2 握手失败 — 主通道不可用 (urltest 已/将自动落 ssh 兜底)");
        match mtu_ok {
            Some(false) => println!("     原因: UDP 载荷 MTU 黑洞, QUIC 初始包被丢 (见扫描)"),
            Some(true) => println!("     注: 扫描显示 MTU 可容 QUIC — 排除 MTU, 查 QoS/服务端/特征识别"),
            None => {}
        }
        println!("     人工强制: gnpc switch ssh; 恢复自动: gnpc switch auto");
    }
    Ok(())
}

/// hy2 握手测试: 临时 sing-box 实例 + 经真实路径 curl
fn handshake_test(hy2: &gnp_core::config::Hy2Endpoint) -> bool {
    println!("\n-- 1. hy2 握手测试 (临时 sing-box 实例, 独立端口) --");
    // 随机空闲端口
    let port = match std::net::TcpListener::bind("127.0.0.1:0") {
        Ok(l) => {
            let p = l.local_addr().map(|a| a.port()).unwrap_or(19800);
            drop(l);
            p
        }
        Err(_) => 19800,
    };

    // 临时配置: 独立 mixed 端口 + 生产 hy2 参数 (含 obfs), 无 cache_file/dns (避免与生产实例抢锁)
    let mut hy2_ob = json!({
        "type": "hysteria2",
        "tag": "hy2-out",
        "server": hy2.server,
        "server_port": hy2.server_port,
        "password": hy2.password,
        "tls": { "enabled": true, "insecure": true }
    });
    if let (Some(t), Some(pw)) = (&hy2.obfs_type, &hy2.obfs_password) {
        hy2_ob["obfs"] = json!({ "type": t, "password": pw });
    }
    let tmp_cfg = json!({
        "log": { "level": "fatal" },
        "inbounds": [{ "type": "mixed", "tag": "mixed-in", "listen": "127.0.0.1", "listen_port": port }],
        "outbounds": [hy2_ob, { "type": "direct", "tag": "direct" }],
        "route": { "final": "hy2-out" }
    });

    let tmp_dir = std::env::temp_dir().join(format!("gnp-probe-{}", now_ms()));
    if std::fs::create_dir_all(&tmp_dir).is_err() {
        println!("  ❌ 临时目录创建失败");
        return false;
    }
    let cfg_file = tmp_dir.join("config.json");
    let cfg_str = serde_json::to_string_pretty(&tmp_cfg).unwrap_or_default();
    if std::fs::write(&cfg_file, cfg_str).is_err() {
        println!("  ❌ 临时配置写入失败");
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return false;
    }

    // 起临时实例 (隐藏窗口: Windows)
    let mut child = {
        let mut cmd = Command::new(gnp_core::platform::gnp_sb_bin());
        cmd.args(["run", "-c"])
            .arg(&cfg_file)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                println!("  ❌ 临时 sing-box 启动失败: {}", e);
                let _ = std::fs::remove_dir_all(&tmp_dir);
                return false;
            }
        }
    };
    std::thread::sleep(Duration::from_millis(800));

    // 经真实路径探测 (generate_204 轻; 出口 IP 佐证)
    let proxy = format!("socks5h://127.0.0.1:{}", port);
    let start = Instant::now();
    let out = Command::new("curl")
        .args(["-s", "-m", "8", "-x", &proxy, "-o", "/dev/null", "-w", "%{http_code}", "https://www.gstatic.com/generate_204"])
        .output();
    let elapsed = start.elapsed().as_millis() as u64;
    let ok = matches!(&out, Ok(o) if o.status.success()
        && String::from_utf8_lossy(&o.stdout).trim().starts_with('2'));

    if ok {
        let exit_ip = Command::new("curl")
            .args(["-s", "-m", "6", "-x", &proxy, "https://ipinfo.io/ip"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        println!("  ✅ 握手+访问通: HTTP 204 ({}ms), 出口 {}", elapsed, exit_ip);
    } else {
        println!("  ❌ 握手失败 (generate_204 无响应, {}ms 超时窗)", elapsed);
    }

    // 清理 (kill + 删临时目录; 注意 /tmp 文件先删后读会扑空 — 读全在删之前)
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&tmp_dir);
    ok
}

/// 载荷尺寸扫描: 服务端 /proc/net/snmp 差分
fn mtu_scan(server: &str, server_port: u16, args: &ProbeArgs) -> Option<u16> {
    println!("\n-- 2. UDP 载荷尺寸扫描 ({}/档 × 3 轮取中位, 服务端 InDatagrams 差分) --", args.count);

    // 基线噪声 (无包窗口内的自然到达: 服务端自身 DNS 响应等)
    let noise = match read_indatagrams(server, args) {
        Some(t0) => {
            std::thread::sleep(Duration::from_millis(500));
            read_indatagrams(server, args).map(|t1| t1.saturating_sub(t0)).unwrap_or(0)
        }
        None => 0,
    };
    if noise > 0 {
        println!("  (基线噪声 ~{} 包/0.5s)", noise);
    }

    let mut results: Vec<(u16, u64)> = Vec::new();
    for size in &args.sizes {
        // 3 轮 "读 t0 → 紧接着发包 → 读 t1": 窗口收紧到 ssh 往返级 (~100ms,
        // 服务端自然 UDP 噪声 ~1 包), 取中位数 — 抗 DNS 回包突发噪声
        let mut deltas: Vec<u64> = Vec::new();
        for _ in 0..3 {
            let Some(t0) = read_indatagrams(server, args) else {
                println!(
                    "  {:>5}B: 无法读取服务端计数器 (需 ssh {}@{} 免密), 扫描中止",
                    size, args.ssh_user, server
                );
                return None;
            };
            send_datagrams(server, server_port, *size as usize, args.count);
            let Some(t1) = read_indatagrams(server, args) else { return None };
            deltas.push(t1.saturating_sub(t0));
        }
        deltas.sort_unstable();
        let median = deltas[deltas.len() / 2];
        let arrived = median >= (args.count as u64 / 2).max(1);
        results.push((*size, median));
        println!(
            "  {:>5}B: {} 中位 {}/{} 到达 (各轮: {:?})",
            size,
            if arrived { "✅" } else { "❌" },
            median,
            args.count,
            deltas
        );
    }

    // 截止点 = 最大到达档; 单调性校验: 高尺寸到达而低尺寸丢失 = 噪声污染测量
    let monotonic = results.windows(2).all(|w| w[0].1 <= w[1].1);
    let max_pass = results
        .iter()
        .filter(|(_, d)| *d >= (args.count as u64 / 2).max(1))
        .map(|(s, _)| *s)
        .max();
    println!("\n  扫描结论:");
    if !monotonic {
        println!("    ⚠️  到达数随尺寸非单调 — 服务端 UDP 噪声干扰了测量, 尺寸结论仅供参考");
        println!("       (可在服务端空闲时段重跑, 或以握手测试结果为准)");
    }
    match max_pass {
        Some(mp) if mp >= 1228 => println!("    ✅ 路径可容 QUIC (≥{}B 到达, ≥1200B 初始包无碍)", mp),
        Some(mp) => {
            println!("    ❌ MTU 截止 ~{}B: QUIC 初始包 (≥1200B) 被丢 → hy2 不可用", mp);
            println!("       建议: urltest 已自动落 ssh; 人工强制 gnpc switch ssh");
        }
        None => {
            println!("    ❌ 全档被丢 — UDP 大包全面黑洞 (或端口/服务端异常), 建议 ssh 兜底");
            println!("       佐证手段: lwtop 127.0.0.1 自环 hy2 全通 → 排除服务端");
        }
    }
    max_pass
}

/// ssh 读服务端 /proc/net/snmp → Udp InDatagrams
///
/// 注意: /proc/net/snmp 有两行 Udp: (表头 + 数值), awk /^Udp:/ 会双匹配 —
/// 这里取最后一行的第 2 个 token。
fn read_indatagrams(server: &str, args: &ProbeArgs) -> Option<u64> {
    let out = Command::new("ssh")
        .args([
            "-o", "ConnectTimeout=6",
            "-o", "BatchMode=yes",
            "-p", &args.ssh_port.to_string(),
            &format!("{}@{}", args.ssh_user, server),
            "cat /proc/net/snmp",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().filter(|l| l.starts_with("Udp:")).last()?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// 发 count 个 size 字节的 UDP 包 (载荷内容无关, 服务端丢弃, 只测到达)
fn send_datagrams(server: &str, port: u16, size: usize, count: u32) {
    let sock = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return,
    };
    if sock.connect((server, port)).is_err() {
        return;
    }
    let payload = vec![0x41u8; size];
    for _ in 0..count {
        if sock.send(&payload).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}
