//! gnpc — global-net-proxy client CLI
//!
//! 管理本机 sing-box mixed 代理 (hysteria2/QUIC 隧道)。
//! 安全原则: 只用 mixed 代理模式 (socks5+http on 1080), 不碰路由表, 零断网风险。
//!
//! v2 (plan D2/D3/D4): `config.toml` 是唯一事实源, `config.json`/服务单元/`tick.sh`
//! 全是生成物; 调度唯一入口 = `tick.sh` (每分钟)。

use std::io::IsTerminal;

mod api;
mod cleanup;
mod guard;
mod install_sched;
mod migrate;
mod probe;
mod recover;
mod register;
mod switch;
mod update_rules;

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};

use gnp_core::settings::{
    ClientAuth, ClientLocal, ClientServer, ClientSettings, GuardSettings,
};
use gnp_core::{config, install, platform, proxy, service, settings, tunnel};

/// global-net-proxy client — 管理 sing-box mixed 代理 (hysteria2/QUIC 隧道)
///
/// 安全原则: 本工具只使用 mixed 代理模式 (socks5+http on 127.0.0.1:1080),
/// 不修改系统路由表, 零断网风险。绝不用 tun 模式。
///
/// 快速开始:
///   gnpc migrate          # 从旧布局 (~/.local/share/sing-box) 一键迁到 ~/.local/gnp
///   gnpc start            # 启动代理
///   gnpc status           # 查看状态
#[derive(Parser)]
#[command(
    name = "gnpc",
    version,
    about = "global-net-proxy client (sing-box mixed 代理)",
    long_about = "管理本机 sing-box mixed 代理 (hysteria2/QUIC 隧道)。\n\
        \n\
        安全原则: 只用 mixed 代理模式 (socks5+http on 127.0.0.1:1080),\n\
        不碰路由表, 零断网风险。绝不用 tun 模式。\n\
        \n\
        部署根: ~/.local/gnp/ ($GNP_HOME 可覆盖)\n\
        - bin/{gnpc,sing-box,tick.sh}\n\
        - config.toml   ← 唯一事实源 (勿手改生成物)\n\
        - config.json   ← 生成物 (gnpc install 渲染)\n\
        - etc/tick.d/  rules/  secrets/  var/  backups/\n\
        \n\
        常用命令:\n\
        \n  \
        gnpc install-scheduler  装调度 (tick.sh + launchd/crontab) 并清旧\n  \
        gnpc start              启动代理\n  \
        gnpc stop               停止代理\n  \
        gnpc status             查看状态 (进程/端口/通道/出口IP)\n  \
        gnpc status --brief     一行摘要 (tick.sh/容器唯一数据接口)\n  \
        gnpc migrate            旧布局 → 新布局 一键迁移\n  \
        gnpc probe              诊断: hy2 握手 + MTU 扫描\n  \
        gnpc switch ssh         强制 TCP 兜底 (auto 恢复自动)\n  \
        gnpc guard              跑一次看门狗 (调度内部调用)\n  \
        gnpc config --check     校验配置 (含与 config.toml 一致性)\n  \
        gnpc proxy --on         开启系统代理 (macOS)\n\
        \n\
        详见: docs/usage.md"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 启动 sing-box 代理 (开机自启)
    #[command(long_about = "启动 sing-box 代理服务并注册为开机自启。\n\n\
        平台行为:\n  \
        macOS  → launchctl load (KeepAlive=true, 崩溃自动重启)\n  \
        Linux → systemctl start gnpc (自动创建 unit 如不存在)\n\n\
        启动后代理监听 0.0.0.0:1080 (socks5+http)。\n\n\
        示例:\n  \
        gnpc start")]
    Start,
    /// 停止 sing-box 代理
    #[command(long_about = "停止 sing-box 代理服务, 卸载开机自启并杀掉残留进程。\n\n\
        平台行为:\n  \
        macOS  → launchctl unload + pkill sing-box run\n  \
        Linux → systemctl stop gnpc\n\n\
        示例:\n  \
        gnpc stop")]
    Stop,
    /// 查看状态 (进程/端口/隧道/出口IP)
    #[command(long_about = "显示完整的代理运行状态。\n\n\
        输出内容:\n  \
        1. 安装状态 (sing-box 二进制 + config.toml + config.json)\n  \
        2. 进程状态 (是否运行中)\n  \
        3. 端口 1080 是否在监听\n  \
        4. 配置安全检查 (无 tun/strict_route, 有 mixed+hysteria2)\n  \
        5. 隧道出口 IP 和延迟 (如果运行中)\n  \
        6. 通道状态 (selector/urltest + 各通道实时延迟)\n\n\
        --brief 输出**一行**纯文本, 是 tick.sh / etc/tick.d 的唯一数据接口 (D5:\n\
        bash 永不解析 JSON):\n  \
        hy2 135ms sel=hy2-out auto=hy2-out ssh=180ms frozen=no exit=8.209.203.17\n\n\
        示例:\n  \
        gnpc status\n  \
        gnpc status --brief")]
    Status {
        /// 一行纯文本摘要 (给 bash 用)
        #[arg(long)]
        brief: bool,
    },

    /// 生成 config.toml (唯一事实源)
    #[command(long_about = "交互式/flag 生成 config.toml —— 之后所有生成物都由它渲染。\n\n\
        给全 flag 即非交互 (适合 scp 到目标机直接落盘):\n  \
        gnpc init --server 8.209.203.17 --hy2-password <密码> --obfs-password <obfs> --listen 0.0.0.0\n\n\
        只给部分 flag 时, 缺项从旧 config.json (若有) 或默认值推断。\n\
        已存在的 config.toml 不会被覆盖 (需先 --force)。\n\n\
        示例:\n  \
        gnpc init --server 8.209.203.17 --hy2-password gnp-xxx --obfs-password gnp-obfs-20261005")]
    Init {
        /// 远端服务端地址
        #[arg(long)]
        server: Option<String>,
        /// hysteria2 密码
        #[arg(long)]
        hy2_password: Option<String>,
        /// salamander obfs 密码 (服务端 inbound 强制)
        #[arg(long)]
        obfs_password: Option<String>,
        /// hy2 端口 (缺省 5766)
        #[arg(long)]
        hy2_port: Option<u16>,
        /// mixed 监听地址 (mac 127.0.0.1; 局域网 0.0.0.0)
        #[arg(long)]
        listen: Option<String>,
        /// 预定义解析, 形如 aipro.host=192.168.1.2,lwtop.host=8.209.203.17
        #[arg(long)]
        hosts: Option<String>,
        /// 写到这个路径 (缺省 $GNP_HOME/config.toml)
        #[arg(long)]
        out: Option<String>,
        /// 覆盖已存在的 config.toml
        #[arg(long)]
        force: bool,
    },

    /// 从旧布局一键迁移到新布局
    #[command(name = "migrate", long_about = "把 ~/.local/share/sing-box (v1) 迁到 ~/.local/gnp (v2), 一条命令搞定。\n\n\
        步骤 (§4.2): 取事实源 → 搬资产 → 生成 config.json (sing-box check 必须过)\n\
        → 装调度 (**新服务先起并验证, 再拆旧的**) → 旧目录挪 backups/legacy-singbox\n\
        → 输出验证清单。任一步失败: 停在新半边, 旧服务保持原样。\n\n\
        事实源优先级: --config <host toml> > 已有 $GNP_HOME/config.toml > 解析旧 config.json\n\
        (已有 config.toml 直接用 → 与 deploy/hosts/<host>.toml md5 一致)\n\n\
        示例:\n  \
        gnpc migrate\n  \
        gnpc migrate --config deploy/hosts/mac.toml\n  \
        gnpc migrate --dry-run")]
    Migrate {
        /// host toml (deploy/hosts/&lt;host&gt;.toml) — 给了就跳过旧配置解析
        #[arg(long)]
        config: Option<String>,
        /// 只报告将要做什么
        #[arg(long)]
        dry_run: bool,
    },

    /// 装/卸调度 (tick.sh + launchd/crontab, 并清旧)
    #[command(long_about = "调度唯一入口 = tick.sh, 每分钟一次 (Mac launchd com.gnpc.tick /\n\
        Linux crontab 一行 / 服务端 GNP_HOME=/opt/gnp)。幂等, 可重复跑。\n\n\
        同时清理旧调度: com.gnp.* (unload + 移走)、旧 cron 行剔除、\n\
        gnp-proxy / gnp-hy2 (--user sing-box) disable —— §7.1 验收 2/4 要求“无残留”。\n\n\
        uninstall-scheduler 只拆调度, 不停常驻 sing-box (那是 `gnpc stop`)。\n\n\
        示例:\n  \
        gnpc install-scheduler\n  \
        gnpc uninstall-scheduler")]
    InstallScheduler {
        /// 拆掉调度 (保留配置与日志)
        #[arg(long)]
        uninstall: bool,
    },
    /// 查看/校验配置
    #[command(long_about = "查看 sing-box 配置文件内容, 或校验配置是否安全。\n\n\
        校验项:\n  \
        - 无 tun/strict_route/auto_route (危险配置检测)\n  \
        - 有 mixed inbound (socks5+http)\n  \
        - 有 hysteria2 outbound\n  \
        - **与 config.toml 一致**: config.json 是 config.toml 的生成物,\n  \
                 两者对不上说明有人手改了生成物 → 重跑 `gnpc install`\n\n\
        示例:\n  \
        gnpc config --check   # 校验配置安全性 + 与 config.toml 一致性\n  \
        gnpc config --show    # 显示完整 JSON 配置")]
    Config {
        /// 显示完整配置内容
        #[arg(long)]
        show: bool,
        /// 校验配置是否正确 (无 tun/strict_route, 有 mixed+hysteria2)
        #[arg(long)]
        check: bool,
    },
    /// 隧道诊断 (兼容旧名 wg)
    #[command(alias = "wg", long_about = "显示 hysteria2/QUIC 隧道配置详情并测试连通性。\n\n\
        输出内容:\n  \
        1. 隧道配置 (远端 server:port / 密码状态)\n  \
        2. 隧道连通性 (通过 socks5h 检测出口 IP 和延迟)\n  \
        3. 代理测试 (github + google HTTP 状态码)\n\n\
        示例:\n  \
        gnpc tunnel   # 或旧名 gnpc wg")]
    Tunnel,
    /// 测试代理连通性
    #[command(long_about = "快速测试代理是否可用。\n\n\
        测试内容:\n  \
        1. 通过 socks5h://127.0.0.1:1080 检测出口 IP\n  \
        2. 测试 github (api.github.com/zen)\n  \
        3. 测试 google (www.google.com)\n\n\
        使用 socks5h (带 h) 表示 DNS 在代理端远程解析, 避免本地 DNS 污染。\n\n\
        示例:\n  \
        gnpc test")]
    Test,
    /// 主动诊断: hy2 握手测试 + UDP 载荷尺寸扫描 (MTU 黑洞定位)
    #[command(long_about = "主动诊断跨境链路, 把\"代理无声超时\"排查一条命令化。\n\n\
        两项检测 (2026-10-05 MTU 黑洞事件范式):\n  \
        1. hy2 握手测试 — 临时 sing-box 实例经真实路径 curl generate_204\n  \
        2. UDP 载荷尺寸扫描 — 向服务端发 1200/1240/1280/1332/1400B 各 N 包,\n  \
           ssh 读服务端 /proc/net/snmp Udp InDatagrams 差分 → MTU 截止点\n\n\
        输出明确结论: 路径是否容得下 QUIC (≥1200B 初始包)。\n\
        MTU 扫描需本机 ssh 密钥可免密登录服务端。\n\n\
        示例:\n  \
        gnpc probe                     # 全套诊断\n  \
        gnpc probe --skip-mtu          # 只测 hy2 握手\n  \
        gnpc probe --sizes 1200,1240,1280 --count 20")]
    Probe {
        /// 尺寸档 (逗号分隔, B)
        #[arg(long, default_value = "1200,1240,1280,1332,1400")]
        sizes: String,
        /// 每档 UDP 包数
        #[arg(long, default_value_t = 10)]
        count: u32,
        /// 读服务端计数器的 ssh 用户 (缺省取 config.toml [server].ssh_user)
        #[arg(long)]
        ssh_user: Option<String>,
        /// 服务端 ssh 端口 (缺省取 config.toml [server].ssh_port)
        #[arg(long)]
        ssh_port: Option<u16>,
        /// 跳过 hy2 握手测试
        #[arg(long)]
        skip_handshake: bool,
        /// 跳过 MTU 扫描
        #[arg(long)]
        skip_mtu: bool,
    },
    /// 手动通道切换 (manual override: auto/hy2/ssh)
    #[command(long_about = "手动切换出站通道 (urltest 的 manual override)。\n\n\
        通道:\n  \
        auto → 交还 urltest 自动选路 (默认, hy2 快则 hy2, 死则自动落 ssh)\n  \
        hy2  → 强制 hy2-out (QUIC 主通道)\n  \
        ssh  → 强制 ssh-out (TCP 兜底, 免疫 UDP MTU 黑洞)\n\n\
        实现: clash_api 热切换 (立即生效) + config selector default 写回 (重启仍生效)。\n\n\
        注意: config.json 是 config.toml 的生成物 —— 重跑 `gnpc install` 会按 toml 重渲染,\n\
        把手动选择复位成默认 (auto-out)。要固化就改 config.toml 或切回 auto。\n\n\
        示例:\n  \
        gnpc switch            # 查看当前通道\n  \
        gnpc switch ssh        # 强制 TCP 兜底\n  \
        gnpc switch auto       # 恢复自动选路")]
    Switch {
        /// 目标通道: auto | hy2 | ssh (缺省显示当前)
        target: Option<String>,
    },
    /// 通道看门狗 (退避探测 + 故障冻结 + 告警; 单 tick)
    #[command(long_about = "通道看门狗: 探测 hy2 健康, 故障自动冻结到 ssh 兜底, 恢复自动回切。\n\n\
        **单 tick 无常驻**: 由 tick.sh 每分钟调一次 (调度唯一, D3);\n\
        手动跑一次用本命令, 不要自己写 cron (会破坏调度唯一性)。\n\
        1. hy2 探测失败 → 指数退避 (backoff_base_s*2^n → backoff_cap_s, ±20% 抖动)\n  \
           掐掉重连风暴 (参数取自 config.toml [guard])\n  \
        2. 连续 freeze_after_failures 次失败 → selector 冻结到 ssh-out + 重启 sing-box\n  \
        3. 恢复探测通过 → 自动解冻交还 urltest; hy2 健康但 urltest 卡在 ssh → 强制 hy2\n  \
        4. 状态翻转告警 (GNP_ALERT_CMD 钩子 / osascript / notify-send), 全程落 var/guard.log\n\n\
        示例:\n  \
        gnpc guard                 # 手动跑一次 tick\n  \
        gnpc install-scheduler     # 装调度 (每分钟自动调它)")]
    Guard,

    /// 从 gnp.cfg 接入（peer）
    #[command(long_about = "读取 gnps gen-user 生成的 gnp.cfg，一键完成安装。\n\n\
        cfg 字段: user-name / server-ip / server-port / peer-key\n\n\
        流程 = install: 下载 sing-box → 规则集 → 生成 config.json → 装服务\n\n\
        示例:\n          gnpc peer gnp.cfg\n          gnpc peer ~/Downloads/gnp.cfg --name 自定义本机名(仅显示用)")]
    Peer {
        /// gnp.cfg 路径
        cfg: String,
    },
    /// 按 config.toml 装齐 (二进制 + 规则集 + config.json + secrets)
    #[command(long_about = "读 config.toml (唯一事实源) 生成一切: 下载 sing-box + 规则集,\n\
        渲染 config.json (双通道形态), 写 secrets, 装常驻服务。\n\n\
        事实源优先级: --config &lt;path&gt; &gt; $GNP_HOME/config.toml\n\n\
        行为步骤:\n  \
        1. 下载 sing-box (已存在则跳过)\n  \
        2. 下载规则集 (geosite-cn, geoip-cn, google, github, openai 等)\n  \
        3. 渲染 config.json: route.final → proxy-out (selector) → auto-out (urltest:\n   \
           hy2-out + ssh-out) → hy2-out (QUIC + salamander obfs) + ssh-out (TCP 兜底)\n  \
        4. 写 secrets/hy2-password + secrets/hy2-obfs (0600 带换行, aipro 容器挂载源)\n  \
        5. `sing-box check` 校验生成物\n  \
        6. Linux: 写 systemd 单元 (需 root; 无免密 sudo 时打印命令)\n\n\
        调度 (tick.sh + cron/launchd) 由 `gnpc install-scheduler` 负责 —— 两件事分开, 生命周期不同。\n\n\
        示例:\n  \
        gnpc install\n  \
        gnpc install --config deploy/hosts/mac.toml\n  \
        gnpc install --bin-only")]
    Install {
        /// config.toml 路径 (缺省 $GNP_HOME/config.toml)
        #[arg(long)]
        config: Option<String>,
        /// 只下载 sing-box 二进制 (不生成配置)
        #[arg(long)]
        bin_only: bool,
    },

    /// 自动注册新机器 (从 gitee peer 池取配置)
    #[command(long_about = "从 gitee 私有仓库的 peer 池自动取配置, 一键完成安装。\n\n\
        工作流程:\n  \
        1. 克隆 gitee 私有仓库 (需要 GITEE_TOKEN)\n  \
        2. 读取 peers/ 目录下的 peer JSON\n  \
        3. 选择一个 status=available 的 peer\n  \
        4. 标记为 used 并 push 回 gitee\n  \
        5. 生成 config.json + 下载 sing-box + 规则集\n  \
        6. 安装 systemd 服务 (Linux)\n\n\
        重要: register 完成后, 需要在 server 上执行:\n  \
        sudo gnps activate <client_id>\n\n\
        示例:\n  \
        export GITEE_TOKEN=xxxx\n  \
        gnpc register --client-id macbook\n  \
        gnpc register --list       # 查看 peer 池\n  \
        gnpc register --dry-run    # 试运行")]
    Register {
        /// client_id (可选, 默认用 hostname)
        #[arg(long)]
        client_id: Option<String>,
        /// 列出 peer 池状态
        #[arg(long)]
        list: bool,
        /// 只看会选中哪个, 不实际修改
        #[arg(long)]
        dry_run: bool,
    },
    /// 规则集日更 (下载 + 重启加载) — tick.sh 04 点窗口自动调
    #[command(name = "rules-update", alias = "update-rules", long_about = "重新下载规则集并重启 sing-box 加载。\n\n\
        正常无需手动跑: tick.sh 在 04 点窗口调本命令一次, 成功才写当日标记\n\
        (var/.rules-updated-&lt;date&gt;), 失败落 WARN 明日再试。\n\n\
        旧名 update-rules 仍可用 (alias)。旧 --install-cron / --check 已删:\n\
        调度唯一 (D3), 挂掉拉起归 `gnpc guard`。\n\n\
        示例:\n  \
        gnpc rules-update")]
    RulesUpdate,

    /// 应急清理 (参考 aipro 事故)
    #[command(long_about = "彻底清理 sing-box 所有残留。\n\n\
        参考: aipro 2026-08-10 sing-box tun 破坏路由导致完全断网事故。\n\n\
        清理步骤 (6 步):\n  \
        1. 停止 sing-box 服务 + 杀进程\n  \
        2. 禁用开机自启\n  \
        3. 清理残留 tun 接口 (gnp0, tun0)\n  \
        4. 清理策略路由 (ip rule priority 9000-9010, table 2022)\n  \
        5. 恢复主网卡默认路由\n  \
        6. 备份并禁用 sing-box 数据目录\n\n\
        注意: 此命令会彻底清除 sing-box, 之后需重新安装。\n\n\
        示例:\n  \
        gnpc cleanup")]
    Cleanup,
    /// 断网恢复 (参考 aipro 事故)
    #[command(long_about = "sing-box tun 模式破坏路由表后的网络恢复工具。\n\n\
        恢复步骤 (5 步):\n  \
        1. 停止 sing-box 服务 (破坏路由的元凶)\n  \
        2. 清理策略路由 (ip rule flush)\n  \
        3. 清理独立路由表 (table 2022/100/200)\n  \
        4. 恢复主网卡默认路由 (尝试常见网关)\n  \
        5. 清理残留 tun 接口 (gnp0, tun0) + 恢复 DNS\n\n\
        如果 recover 后仍不通, 直接 reboot 重启机器。\n\n\
        示例:\n  \
        gnpc recover")]
    Recover,
    /// CLI 环境变量代理开关 (curl/pip/npm/git 等)
    #[command(long_about = "为 CLI 工具 (curl/pip/uv/npm/git/docker 等) 输出代理环境变量。\n\n\
        这些工具不读系统代理设置, 只认环境变量:\n  \
        http_proxy / https_proxy / all_proxy → 127.0.0.1:1080\n\n\
        用法:\n          eval \"$(gnpc env --on)\"    # 当前 shell 生效\n          eval \"$(gnpc env --off)\"   # 取消\n          eval \"$(gnpc env --hook)\"  # 装入 gnp-on/gnp-off 快捷函数(放 .zshrc/.bashrc)\n\n\
        示例 (.zshrc):\n          eval \"$(~/.local/bin/gnpc env --hook)\"\n\n\
        注: 国内流量无需加 no_proxy —— sing-box 已按 geosite-cn 分流直连。")]
    Env {
        /// 输出 export 环境变量 (eval 用)
        #[arg(long)]
        on: bool,
        /// 输出 unset 环境变量 (eval 用)
        #[arg(long)]
        off: bool,
        /// 输出 gnp-on/gnp-off shell 函数 (eval 用)
        #[arg(long)]
        hook: bool,
    },
    /// 系统代理开关 (macOS 系统代理 / Linux GNOME 代理)
    #[command(long_about = "设置或取消操作系统层面的代理, 让浏览器等 GUI 程序走 sing-box。\n\n\
        平台差异:\n  \
        macOS  → networksetup + osascript 弹管理员授权 (不存密码)\n  \
                 设置 HTTP/HTTPS/SOCKS 代理为 127.0.0.1:1080\n  \
        Linux  → gsettings 设置 GNOME 系统代理 (manual 模式)\n  \
                 无 GNOME 时提示 export 环境变量\n\n\
        示例:\n  \
        gnpc proxy --on      # 开启系统代理\n  \
        gnpc proxy --off     # 关闭系统代理\n  \
        gnpc proxy --status  # 查看当前状态")]
    Proxy {
        /// 开启系统代理
        #[arg(long)]
        on: bool,
        /// 关闭系统代理
        #[arg(long)]
        off: bool,
        /// 查看代理状态
        #[arg(long)]
        status: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Start => cmd_start(),
        Commands::Stop => cmd_stop(),
        Commands::Status { brief } => {
            if brief {
                cmd_status_brief()
            } else {
                cmd_status()
            }
        }
        Commands::Init {
            server,
            hy2_password,
            obfs_password,
            hy2_port,
            listen,
            hosts,
            out,
            force,
        } => cmd_init(
            server,
            hy2_password,
            obfs_password,
            hy2_port,
            listen,
            hosts,
            out,
            force,
        ),
        Commands::Migrate { config, dry_run } => migrate::run(&migrate::MigrateArgs {
            config,
            dry_run,
        }),
        Commands::InstallScheduler { uninstall } => {
            if uninstall {
                install_sched::uninstall()
            } else {
                install_sched::install()
            }
        }
        Commands::Config { show, check } => cmd_config(show, check),
        Commands::Tunnel => cmd_tunnel(),
        Commands::Test => cmd_test(),
        Commands::Probe {
            sizes,
            count,
            ssh_user,
            ssh_port,
            skip_handshake,
            skip_mtu,
        } => {
            // ssh 参数缺省来自唯一事实源 (flag 优先)
            let st = settings::ClientSettings::load_or_default();
            let ssh_user = ssh_user.unwrap_or(st.server.ssh_user.clone());
            let ssh_port = ssh_port.unwrap_or(st.server.ssh_port);
            probe::run(probe::ProbeArgs {
                sizes: sizes
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect(),
                count,
                ssh_user,
                ssh_port,
                skip_handshake,
                skip_mtu,
            })
        }
        Commands::Switch { target } => switch::run(target),
        Commands::Guard => guard::run(),
        Commands::Peer { cfg } => cmd_peer(&cfg),
        Commands::Install { config, bin_only } => cmd_install(&config, bin_only),
        Commands::Register {
            client_id,
            list,
            dry_run,
        } => register::run(&register::RegisterArgs {
            client_id,
            list,
            dry_run,
        }),
        Commands::RulesUpdate => update_rules::cmd_update(),
        Commands::Cleanup => {
            if cfg!(windows) {
                println!("ℹ️  cleanup 针对 macOS/Linux 的 tun 路由事故; Windows 上 sing-box 只监听端口,");
                println!("   无路由劫持风险。如需卸载: gnpc stop 后删除 %USERPROFILE%\\.local\\share\\sing-box");
                Ok(())
            } else {
                cleanup::run()
            }
        }
        Commands::Recover => {
            if cfg!(windows) {
                println!("ℹ️  recover 针对 macOS/Linux 的 tun 路由事故恢复; Windows 无此风险, 无需使用。");
                Ok(())
            } else {
                recover::run()
            }
        }
        Commands::Env { on, off, hook } => cmd_env(on, off, hook),
        Commands::Proxy { on, off, status } => cmd_proxy(on, off, status),
    }
}

/// 解析 --hosts "a.host=1.2.3.4,b.host=5.6.7.8" → BTreeMap (输出确定)
fn parse_hosts(s: &str) -> std::collections::BTreeMap<String, String> {
    s.split(',')
        .filter_map(|pair| {
            let (k, v) = pair.trim().split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// 从 gnp.cfg 接入 (gnps gen-user 生成的 peer 配置)
///
/// gnp.cfg = key = value 明文: user-name / server-ip / server-port / peer-key / obfs-pass
/// (§3.9: server-port 现在是 5766, 且带 obfs-pass)
fn cmd_peer(cfg_path: &str) -> Result<()> {
    let content = std::fs::read_to_string(cfg_path)
        .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", cfg_path, e))?;

    // 极简 key = value 解析（# 注释行忽略）
    let mut kv = std::collections::HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            kv.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let server = kv.get("server-ip").context("cfg 缺少 server-ip")?.clone();
    let password = kv.get("peer-key").context("cfg 缺少 peer-key")?.clone();
    let port: u16 = kv
        .get("server-port")
        .map(|s| s.parse().unwrap_or(platform::GNP_PORT))
        .unwrap_or(platform::GNP_PORT);
    let user = kv.get("user-name").cloned().unwrap_or_else(|| "peer".into());

    println!("== gnp peer 接入 ({} → {}:{}) ==", user, server, port);

    // 落唯一事实源 (之后 install/init 都以它为准)
    let s = ClientSettings {
        server: ClientServer {
            host: server.clone(),
            hy2_port: port,
            ..Default::default()
        },
        auth: ClientAuth {
            hy2_password: password,
            // gnp.cfg 可选携带 obfs-pass (服务端 inbound 开 salamander 时必填)
            obfs_password: kv.get("obfs-pass").cloned().unwrap_or_default(),
        },
        client: ClientLocal::default(),
        guard: GuardSettings::default(),
    };
    s.validate()?;
    let toml_path = s.save_default()?;
    println!("✅ 唯一事实源: {}", toml_path.display());

    cmd_install(&None, false)?;
    println!("\n✅ peer 接入完成!");
    println!("   启动: gnpc install-scheduler && gnpc start");
    println!("   验证: gnpc status");
    Ok(())
}

/// 读唯一事实源: --config > $GNP_HOME/config.toml
fn load_settings(config: &Option<String>) -> Result<ClientSettings> {
    match config {
        Some(p) => {
            let s = settings::ClientSettings::load(std::path::Path::new(p))?;
            println!("   事实源: {} (显式)", p);
            Ok(s)
        }
        None => {
            let p = platform::gnp_config_toml();
            if !p.exists() {
                anyhow::bail!(
                    "找不到唯一事实源: {}。先 `gnpc init` 生成, 或 `gnpc install --config <host.toml>`",
                    p.display()
                );
            }
            println!("   事实源: {}", p.display());
            settings::ClientSettings::load(&p)
        }
    }
}

/// 按 config.toml 装齐
fn cmd_install(config: &Option<String>, bin_only: bool) -> Result<()> {
    let plat = platform::ensure_supported()?;
    println!("== gnpc 安装 ({}) ==", plat.as_str());
    platform::ensure_layout(&platform::gnp_home()).context("创建部署目录失败")?;

    // 0. gnpc 自身入 bin/ —— tick.sh 的内核组件是 $BASE/bin/gnpc, 缺席就静默不跑
    crate::migrate::ensure_self_installed();

    // 1. sing-box 二进制
    if !platform::sb_exists() {
        install::install_singbox(None)?;
    } else {
        println!("✅ sing-box 已存在: {}", platform::gnp_sb_bin().display());
    }
    if bin_only {
        println!("✅ 只装了二进制 (--bin-only), 未生成配置");
        return Ok(());
    }

    // 2. 事实源
    let s = load_settings(config)?;
    s.validate()?;

    // 3. 规则集
    install::install_rules()?;

    // 4. 渲染 config.json + sing-box check (坏配置不许上线)
    let json = install::generate_config(&s)?;
    install::check_config_file(&json)?;

    // 5. secrets (aipro 路由容器挂载源; 0600 + 带换行)
    write_secrets(&s)?;

    // 6. 常驻服务 (调度归 install-scheduler, 两件事生命周期不同)
    if plat == platform::Platform::Linux {
        service::install_linux()?;
    }

    // sudo 部署属主归还 (root 经 sudo 跑 install 与 migrate 同理, 见 install_sched)
    crate::install_sched::fix_deploy_ownership(&platform::gnp_home());

    println!("\n✅ 安装完成。启动: gnpc install-scheduler && gnpc start");
    Ok(())
}

/// 写 secrets (aipro-wifi-router 挂载源; 坑清单 #3: 必须带换行)
fn write_secrets(s: &ClientSettings) -> Result<()> {
    if !s.auth.hy2_password.is_empty() {
        platform::write_secret(&platform::gnp_secret_hy2_password(), &s.auth.hy2_password)?;
        println!("✅ secret: {}", platform::gnp_secret_hy2_password().display());
    }
    if let Some(obfs) = s.obfs() {
        platform::write_secret(&platform::gnp_secret_hy2_obfs(), &obfs)?;
        println!("✅ secret: {}", platform::gnp_secret_hy2_obfs().display());
    }
    Ok(())
}

/// 生成 config.toml (唯一事实源)
///
/// 非交互条件: 给了 `--server` + `--hy2-password` 就直接落盘, 缺项从旧 config.json 推断。
fn cmd_init(
    server: Option<String>,
    hy2_password: Option<String>,
    obfs_password: Option<String>,
    hy2_port: Option<u16>,
    listen: Option<String>,
    hosts: Option<String>,
    out: Option<String>,
    force: bool,
) -> Result<()> {
    let dest = out
        .map(std::path::PathBuf::from)
        .unwrap_or_else(platform::gnp_config_toml);
    if dest.exists() && !force {
        anyhow::bail!(
            "{} 已存在 (不覆盖)。改配置请编辑它再 `gnpc install`; 确要重写: --force",
            dest.display()
        );
    }

    // 已有事实源 / 旧 config.json → 起点
    let mut s = settings::ClientSettings::load_default()
        .ok()
        .flatten()
        .or_else(|| {
            let old = platform::legacy_sb_dir().join("config.json");
            if old.exists() {
                crate::migrate::settings_from_legacy(&old).ok()
            } else {
                None
            }
        })
        .unwrap_or_default();

    if let Some(v) = server {
        s.server.host = v;
    }
    if let Some(v) = hy2_password {
        s.auth.hy2_password = v;
    }
    if let Some(v) = obfs_password {
        s.auth.obfs_password = v;
    }
    if let Some(v) = hy2_port {
        s.server.hy2_port = v;
    }
    if let Some(v) = listen {
        s.client.listen = v;
    }
    if let Some(v) = hosts {
        s.client.hosts = parse_hosts(&v);
    }

    // 缺 host/密码 → 交互补 (仅在 TTY 下)
    if s.server.host.trim().is_empty() || s.auth.hy2_password.trim().is_empty() {
        if !std::io::stdin().is_terminal() {
            anyhow::bail!(
                "缺必要参数且非交互: 请给 --server <ip> 与 --hy2-password <密码>\n   例: gnpc init --server 8.209.203.17 --hy2-password gnp-xxx --obfs-password gnp-obfs-20261005"
            );
        }
        if s.server.host.trim().is_empty() {
            s.server.host = prompt("服务端地址 [8.209.203.17]", "8.209.203.17")?;
        }
        if s.auth.hy2_password.trim().is_empty() {
            s.auth.hy2_password = prompt("hy2 密码", "")?;
        }
        if s.auth.obfs_password.trim().is_empty() {
            s.auth.obfs_password = prompt("obfs 密码 (空=不用 obfs)", "")?;
        }
    }

    s.validate()?;
    s.save(&dest)?;
    println!("✅ 唯一事实源已生成: {}", dest.display());
    println!("   hy2 端口: {}  listen: {}  ssh 兜底: {}",
        s.server.hy2_port,
        s.client.listen,
        if s.ssh_fallback().is_some() { "开" } else { "关" });
    println!("\n下一步: gnpc install        # 渲染 config.json + 规则集 + 服务");
    println!("        gnpc install-scheduler   # 装调度 (tick.sh + cron/launchd)");
    Ok(())
}

fn prompt(label: &str, default: &str) -> Result<String> {
    use std::io::Write;
    print!("{}: ", label);
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let v = line.trim().to_string();
    Ok(if v.is_empty() { default.to_string() } else { v })
}

/// 启动
fn cmd_start() -> Result<()> {
    let platform = platform::ensure_supported()?;
    platform::ensure_installed()?;
    println!("♻️  启动 sing-box ({})...", platform.as_str());
    service::start(platform)?;
    println!("✅ sing-box 已启动 (socks5+http on 127.0.0.1:1080)");
    Ok(())
}

/// 停止
fn cmd_stop() -> Result<()> {
    let platform = platform::ensure_supported()?;
    println!("⏹️  停止 sing-box ({})...", platform.as_str());
    service::stop(platform)?;
    println!("✅ sing-box 已停止");
    Ok(())
}

/// 状态
fn cmd_status() -> Result<()> {
    let platform = platform::ensure_supported()?;
    println!("== gnpc 状态 ({}) ==", platform.as_str());
    println!("   (一行摘要: gnpc status --brief)");

    // 1. 二进制/配置
    println!("\n📦 安装:");
    println!(
        "  sing-box 二进制: {}",
        if platform::sb_exists() { "已安装 ✅" } else { "未安装 ❌" }
    );
    println!(
        "  唯一事实源 config.toml: {}",
        if platform::settings_exist() { "存在 ✅" } else { "缺失 ❌ (gnpc init)" }
    );
    println!(
        "  生成物 config.json: {}",
        if platform::config_exists() { "存在 ✅" } else { "缺失 ❌ (gnpc install)" }
    );
    println!("  部署根: {}", platform::gnp_home().display());

    // 2. 进程状态
    let running = service::is_running(platform)?;
    println!("\n🔄 进程:");
    println!(
        "  运行状态: {}",
        if running { "运行中 ✅" } else { "已停止" }
    );

    // 3. 端口
    let port_open = check_port(1080);
    println!(
        "  端口 1080: {}",
        if port_open { "监听中 ✅" } else { "未监听" }
    );

    // 4. 配置安全检查
    if platform::config_exists() {
        let cfg_path = platform::gnp_config_json();
        if let Ok(v) = config::load(&cfg_path) {
            let safe = config::is_safe(&v);
            let has_mixed = config::has_mixed_inbound(&v);
            let has_hy2 = config::has_hy2_endpoint(&v);
            println!("\n🔒 配置安全:");
            println!(
                "  无 tun/strict_route: {}",
                if safe { "✅" } else { "❌ 危险!" }
            );
            println!("  mixed inbound: {}", if has_mixed { "✅" } else { "❌" });
            println!("  hysteria2 outbound: {}", if has_hy2 { "✅" } else { "❌" });
        }
    }

    // 5. 隧道状态 (如果运行中)
    if running {
        println!("\n🌐 隧道:");
        match test_proxy_simple() {
            Ok((ip, ms)) => println!("  出口 IP: {} ({}ms)", ip, ms),
            Err(e) => println!("  出口检测失败: {}", e),
        }
    }

    // 6. 通道状态 (双通道配置: selector/urltest + 各通道实时延迟)
    if running {
        if let Ok(v) = config::load(&platform::gnp_config_json()) {
            if let Some(sel) = config::find_outbound(&v, "selector") {
                println!("\n🔀 通道:");
                println!("  route.final: {}", config::final_outbound(&v).unwrap_or_default());
                println!(
                    "  selector 默认: {}",
                    sel.get("default").and_then(|d| d.as_str()).unwrap_or("?")
                );
                if let Some(addr) = api::controller_addr() {
                    if let Ok(proxies) = api::proxies_map(&addr, 2) {
                        if let Some(now) = api::proxy_now(&proxies, "proxy-out") {
                            println!("  当前生效 (热): {}", now);
                        }
                        if let Some(auto) = api::proxy_now(&proxies, "auto-out") {
                            println!("  urltest 选中: {}", auto);
                        }
                        for tag in ["hy2-out", "ssh-out"] {
                            match api::proxy_delay_ms(&proxies, tag) {
                                Some(ms) => println!("  {}: {}ms ✅", tag, ms),
                                None => println!("  {}: 最近探测超时/失败 ❌", tag),
                            }
                        }
                    } else {
                        println!("  (clash_api 不可达 — sing-box 异常?)");
                    }
                } else {
                    println!("  (旧配置无 clash_api — 重刷配置后可见热状态)");
                }
            }
        }
    }

    Ok(())
}

/// `status --brief` — **一行**纯文本, tick.sh / etc/tick.d 的唯一数据接口 (D5)
///
/// bash 永不解析 JSON (D5), 所以这一行必须自足: 通道/延迟/冻结/出口。
/// 例: hy2 135ms sel=hy2-out auto=hy2-out ssh=180ms frozen=no exit=8.209.203.17
fn cmd_status_brief() -> Result<()> {
    let mut out = String::new();

    // 1) 通道与延迟 (clash_api)
    let mut sel = String::from("?");
    let mut auto = String::from("-");
    let mut hy2_ms: Option<u64> = None;
    let mut ssh_ms: Option<u64> = None;
    if let Some(addr) = api::controller_addr() {
        if let Ok(proxies) = api::proxies_map(&addr, 2) {
            sel = api::proxy_now(&proxies, "proxy-out").unwrap_or_else(|| "?".into());
            auto = api::proxy_now(&proxies, "auto-out").unwrap_or_else(|| "-".into());
            hy2_ms = api::proxy_delay_ms(&proxies, "hy2-out");
            ssh_ms = api::proxy_delay_ms(&proxies, "ssh-out");
        }
    }

    // 2) 冻结状态 (guard-state.json, Rust 侧读, bash 不碰)
    let frozen = settings_frozen();

    out.push_str(&format!(
        "hy2 {} sel={} auto={} ssh{} frozen={}",
        hy2_ms.map(|m| format!("{}ms", m)).unwrap_or_else(|| "-".into()),
        sel,
        auto,
        ssh_ms.map(|m| format!("={}ms", m)).unwrap_or_else(|| "=-".into()),
        if frozen { "yes" } else { "no" },
    ));

    // 3) 出口 IP (装了才有意义; 3s 超时免得拖慢 tick)
    if let Ok((ip, _)) = tunnel::detect_exit_ip("socks5h://127.0.0.1:1080", 3) {
        out.push_str(&format!(" exit={}", ip));
    } else {
        out.push_str(" exit=-");
    }
    println!("{}", out);
    Ok(())
}

/// guard 冻结标志 (var/guard-state.json; 坏文件当没冻结)
fn settings_frozen() -> bool {
    std::fs::read_to_string(platform::gnp_guard_state())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("frozen").and_then(|f| f.as_bool()))
        .unwrap_or(false)
}

/// 配置
fn cmd_config(show: bool, check: bool) -> Result<()> {
    let cfg_path = platform::gnp_config_json();
    if !cfg_path.exists() {
        anyhow::bail!("配置文件不存在: {}", cfg_path.display());
    }
    let v = config::load(&cfg_path)?;

    if check {
        let safe = config::is_safe(&v);
        let has_mixed = config::has_mixed_inbound(&v);
        let has_hy2 = config::has_hy2_endpoint(&v);
        println!("== 配置校验 ==");
        println!(
            "  无 tun/strict_route: {}",
            if safe { "✅" } else { "❌ 危险!" }
        );
        println!("  mixed inbound: {}", if has_mixed { "✅" } else { "❌" });
        println!(
            "  hysteria2 outbound: {}",
            if has_hy2 { "✅" } else { "❌" }
        );
        if !safe {
            anyhow::bail!("检测到危险配置 (tun/strict_route)! 请立即修复。");
        }

        // config.json 是 config.toml 的生成物 —— 对不上就是有人手改了生成物
        let mut consistent = true;
        match settings::ClientSettings::load_default() {
            Ok(Some(st)) => {
                let expect: serde_json::Value =
                    serde_json::from_str(&install::render_config(&st)?)?;
                let same = config::same_shape(&expect, &v);
                println!(
                    "  与 config.toml 一致: {}",
                    if same { "✅" } else { "❌ 生成物被手改" }
                );
                if !same {
                    consistent = false;
                    println!("     修复: gnpc install   (重新渲染 {})", cfg_path.display());
                }
            }
            Ok(None) => {
                println!("  与 config.toml 一致: ⚠️  无 config.toml (跳过; 用 gnpc init 生成)");
            }
            Err(e) => {
                consistent = false;
                println!("  与 config.toml 一致: ❌ 读不了 config.toml: {}", e);
            }
        }
        if !consistent {
            anyhow::bail!("config.json 与 config.toml 不一致 (生成物被手改?), 请重跑 gnpc install");
        }
        println!("✅ 配置安全且完整");
        return Ok(());
    }

    if show {
        println!("== 当前配置 ({}) ==", cfg_path.display());
        let pretty = serde_json::to_string_pretty(&v)?;
        println!("{}", pretty);
    }
    Ok(())
}

/// 隧道诊断 (Hysteria2/QUIC; 旧名 wg)
fn cmd_tunnel() -> Result<()> {
    let platform = platform::ensure_supported()?;
    println!("== 隧道诊断 ({}) ==", platform.as_str());

    // 读配置
    if platform::config_exists() {
        let v = config::load(&platform::gnp_config_json())?;
        if let Some(hy2) = config::extract_hy2_endpoint(&v) {
            println!("\n📋 隧道配置:");
            println!("  远端 server: {}:{}", hy2.server, hy2.server_port);
            println!(
                "  密码: 已配置 ({} 字符)",
                hy2.password.len()
            );
        }
    }

    // 检测出口
    println!("\n🌐 隧道连通性:");
    match test_proxy_simple() {
        Ok((ip, ms)) => println!("  ✅ 出口 IP: {} ({}ms)", ip, ms),
        Err(e) => println!("  ❌ {}", e),
    }

    // 测试几个网站
    println!("\n🔍 代理测试:");
    let proxy = "socks5h://127.0.0.1:1080";
    for (name, url) in [
        ("github", "https://api.github.com/zen"),
        ("google", "https://www.google.com"),
    ] {
        match tunnel::test_proxy(proxy, url, 8) {
            Ok((code, ms)) => println!("  {}: HTTP {} ({}ms)", name, code, ms),
            Err(e) => println!("  {}: 失败 ({})", name, e),
        }
    }

    Ok(())
}

/// 测试代理
fn cmd_test() -> Result<()> {
    println!(
        "== 代理连通性测试 (socks5://127.0.0.1:1080) =="
    );
    let proxy = "socks5h://127.0.0.1:1080";
    match test_proxy_simple() {
        Ok((ip, ms)) => println!("✅ 出口 IP: {} ({}ms)", ip, ms),
        Err(e) => {
            println!("❌ 代理不可用: {}", e);
            return Ok(());
        }
    }
    for (name, url) in [
        ("github", "https://api.github.com/zen"),
        ("google", "https://www.google.com"),
    ] {
        match tunnel::test_proxy(proxy, url, 8) {
            Ok((code, ms)) => println!("  {}: HTTP {} ({}ms)", name, code, ms),
            Err(e) => println!("  {}: 失败 ({})", name, e),
        }
    }
    Ok(())
}

// --- 辅助函数 ---

/// CLI 环境变量代理开关
fn cmd_env(on: bool, off: bool, hook: bool) -> Result<()> {
    let url = "http://127.0.0.1:1080".to_string();
    let no_proxy = "localhost,127.0.0.1,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,169.254.0.0/16";

    // 直接运行（输出到终端）时给引导而非裸 export —— 子进程改不了父 shell 环境
    if (on || off || hook)
        && std::io::stdout().is_terminal()
        && std::env::var("GNP_ENV_RAW").is_err()
    {
        println!("⚠️  env 的输出需要 eval 装载 —— 直接运行不会改变当前 shell：");
        if on {
            println!(r#"  eval "$(gnpc env --on)"    # 或用快捷函数: gnp-on"#);
        }
        if off {
            println!(r#"  eval "$(gnpc env --off)"   # 或用快捷函数: gnp-off"#);
        }
        if hook {
            println!(r#"  eval "$(gnpc env --hook)"  # 放入 ~/.zshrc 得到 gnp-on/gnp-off"#);
        }
        println!("   (新终端已自动加载快捷函数；当前旧会话可 source ~/.zshrc)");
        return Ok(());
    }
    if on {
        println!("export http_proxy={}", url);
        println!("export https_proxy={}", url);
        println!("export all_proxy=socks5://127.0.0.1:1080");
        println!("export HTTP_PROXY=${{http_proxy}} HTTPS_PROXY=${{https_proxy}} ALL_PROXY=${{all_proxy}}");
        println!("export no_proxy={} NO_PROXY=${{no_proxy}}", no_proxy);
        return Ok(());
    }
    if off {
        for k in ["http_proxy","https_proxy","all_proxy","HTTP_PROXY","HTTPS_PROXY","ALL_PROXY","no_proxy","NO_PROXY"] {
            println!("unset {}", k);
        }
        return Ok(());
    }
    if hook {
        println!("# gnp CLI 代理快捷函数 (由 gnpc env --hook 生成, eval 装载)");
        println!("gnp-on()  {{ eval \"$(gnpc env --on)\"; echo \"🌐 CLI 代理已开 (127.0.0.1:1080) – curl/pip/npm/git 生效\"; }}");
        println!("gnp-off() {{ eval \"$(gnpc env --off)\"; echo \"⏹️  CLI 代理已关\"; }}");
        println!("# 提示: gnp-on 只对当前 shell 及其子进程生效");
        return Ok(());
    }
    // 无参数: 显示当前状态
    println!("== gnp CLI 代理环境 ==");
    let mut any = false;
    for k in ["http_proxy","https_proxy","all_proxy"] {
        match std::env::var(k) {
            Ok(v) => { println!("  {} = {} ✅", k, v); any = true; }
            Err(_) => println!("  {} = (未设置)", k),
        }
    }
    if any {
        println!("\n当前 shell 已走代理; 取消: eval \"$(gnpc env --off)\"");
    } else {
        println!("\n当前 shell 未走代理。开启: eval \"$(gnpc env --on)\"");
        println!("永久快捷: 把 eval \"$(gnpc env --hook)\" 加入 ~/.zshrc (得到 gnp-on/gnp-off)");
    }
    Ok(())
}

/// 系统代理开关
fn cmd_proxy(on: bool, off: bool, status: bool) -> Result<()> {
    if on {
        proxy::enable()?;
    } else if off {
        proxy::disable()?;
    } else if status {
        proxy::status()?;
    } else {
        // 无参数: 显示状态和用法
        println!("== 系统代理管理 ==\n");
        let _ = proxy::status();
        println!("\n用法:");
        println!("  gnpc proxy --on      开启系统代理 (浏览器走 sing-box)");
        println!("  gnpc proxy --off     关闭系统代理");
        println!("  gnpc proxy --status  查看当前代理状态");
    }
    Ok(())
}

/// 检查端口是否监听 (跨平台: 直接 TCP 连接探测, 不依赖 lsof/netstat)
fn check_port(port: u16) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    let addr = format!("127.0.0.1:{}", port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next());
    match addr {
        Some(a) => TcpStream::connect_timeout(&a, std::time::Duration::from_millis(600)).is_ok(),
        None => false,
    }
}

/// 简单出口检测
fn test_proxy_simple() -> Result<(String, u64)> {
    tunnel::detect_exit_ip("socks5h://127.0.0.1:1080", 8)
}