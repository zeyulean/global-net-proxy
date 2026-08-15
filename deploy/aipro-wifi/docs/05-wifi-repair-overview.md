# aipro WiFi 修复总览 — 方法论、git 流程与踩坑清单

> 汇总 2026-08-12 ~ 08-15 三轮修复（8821cu/88x2bu/aic8800 全部阵亡于 hiusbc）。
> 一句话结论：**OrangePi AIpro 20T 的 hiusbc USB 主控无法可靠驱动任何 USB WiFi 网卡，
> WiFi 需求用外接路由器方案，aipro 只做透明代理网关。**

## 一、全局判断框架（这次踩坑换来的）

修 WiFi 驱动前，先按顺序确认四件事，任何一件不过就止损：

1. **网卡真实型号**：lsusb 的 Product 名会骗人（"802.11ac NIC" 不等于 8821CU），
   查 USB ID → 芯片。0bda:c820 = RTL8812BU，**不是** 8821CU（两个坑都因它）
2. **官方是否宣称支持**：搜 "板子型号 + wifi" 确认官方镜像是否有驱动路径；
   官方支持 ≠ 一定可用（AIpro 官方宣传 WiFi5 但实际 lockup）
3. **平台 USB 控制器是什么**：私有控制器（hiusbc/dwc3 变种）= 高危，
   WiFi 驱动崩溃先怀疑平台而不是驱动
4. **崩溃有没有 Oops**：无 Oops 的静默复位/lockup = 平台层（xHCI/中断/watchdog），
   驱动层补丁救不了，尽早止损

## 二、aipro 上的 git 处理流程（本次实践，可复用）

驱动调试必须 git 管理，每轮实验可回溯可复现：

```bash
# 1. 源码放 NVMe（不动系统目录），初始化
mkdir -p /mnt/disk/lwboy/projects/drivers/<name> && cd <name>
git init && git add -A
git -c user.name=<agent> -c user.email=<mail> commit -qm "initial import"

# 2. 每轮实验一个 commit（补丁+打点一起进，消息带实验编号）
#    实验编号 E1..En 连续递增，方便日志对照
git add -A && git commit -qm "E17: skip rf_calib (calib done by load_fw patch)"

# 3. 对照实验用 git checkout <hash> -- <file> 回退单文件，不整仓回退
git checkout d0a7573 -- drivers/aic8800/aic8800_fdrv/rwnx_main.c

# 4. logs/EXPERIMENTS.md 记录每轮：假设→改动→结果→结论
# 5. 编译命令模板（跨模块符号依赖）：
cd drivers/aic8800/aic8800_fdrv
make -C /lib/modules/5.10.0+/build M=$PWD \
  KBUILD_EXTRA_SYMBOLS=<path>/aic_load_fw/Module.symvers modules
```

要点：
- **先 commit 再装模块**——崩溃重启后 ko 可能丢（make 中断产生 0 字节 ko，
  insmod 报 "Invalid parameters"，make clean 全重编可解）
- 装模块固定流程：`cp xxx.ko /lib/modules/.../aic8800/ && depmod -a`，
  之前 **md5sum 校验源 ko 与已装 ko 一致**再测（本会话曾因忘 cp 装了旧版白测一轮）
- blacklist（/etc/modprobe.d/）保证开机不自动加载实验模块——**这是反复崩溃下
  机器始终能重启回来的唯一原因**

## 三、踩过的坑（按疼痛程度排序）

1. **8821cu.ko 加载 → 整机 hard lockup**（08-12 与 08-15 各一次）
   板载 0bda:c820 是 RTL8812BU，8821cu 当 8821C 初始化 → 死锁。
   甚至 insmod 符号解析失败（Unknown symbol）也能触发崩溃。
   **教训：加载任何 rtl 88xx 驱动前先核对 USB ID 对应的真实芯片**

2. **崩溃后干等自愈 vs 物理重启**：bbox watchdog 有时 30s 复位有时永不触发。
   探测循环 ssh ConnectTimeout=4 + sleep 4；超过 ~5 分钟不自愈就别等了，
   让用户物理重启（用户在场时提前说明）

3. **printk 打点改变时序**：加 KERN_ALERT 打点后崩溃点会漂移（延迟掩盖竞态窗口），
   "打点后不崩"≠"修好了"。定位时以多轮一致的最后打点为准

4. **Python 脚本 patch 内嵌 ssh heredoc 的转义地狱**：
   `ssh host "python3 << EOF ... EOF"` 内层引号/反斜杠必然出错。
   正确姿势：python 脚本 scp 过去再 `ssh host "python3 /tmp/x.py"`

5. **make 失败会删 ko**：assert 失败/编译错误后 aic8800_fdrv.ko 消失，
   若没注意继续测 = 测的旧模块。测试前 ls -la 确认 ko 存在 + md5 比对

6. **Git checkout 单文件 vs 整仓**：回退实验用 `checkout <hash> -- <file>`，
   避免把其他文件的后续补丁一起卷走

7. **Mac↔aipro 传文件**：同一局域网直连 scp 即可（曾绕道 lwtop/ningsure 完全多余）；
   scp 大文件被 180s timeout 截断 → 文件不完整（tar unexpected EOF），
   传完 **md5 比对**两端

8. **watcher 抓现场**：崩溃时 dmesg buffer 会丢，需要 systemd 服务持续
   `dmesg -w` 追加写文件 + sync 落盘（/home/lwboy/aic8800/aic_dmesg_follow.sh）
   否则每次重启现场就没了

9. **换物理口之前先看拓扑**：`readlink /sys/bus/usb/devices/usbN` 查控制器归属。
   所有口汇同一控制器时换口毫无意义（AIpro 外口全在 hiusbc3+hub 后面）

10. **strings 挖二进制驱动**：无源码的 ko 用 `strings xxx.ko | grep -i 关键词`
    能挖出大量自证信息（本次靠它发现 "StartXfer and LPM request conflict"）

## 四、诊断工具箱（本次验证有效的命令组合）

```bash
# USB 拓扑: 哪个控制器、几层 hub
lsusb; readlink /sys/bus/usb/devices/usb3
for d in /sys/bus/usb/devices/*/idVendor; do p=$(dirname $d); echo "$(basename $p): $(cat $p/idVendor):$(cat $p/idProduct)"; done

# 控制器能力: maxchild/模式
cat /sys/bus/usb/devices/usb1/maxchild        # 物理口数
ls /sys/devices/platform/a5180000.hiusbc3/    # 控制器绑定

# 中断分布（绑核参考）
grep xhci /proc/interrupts; cat /proc/irq/154/smp_affinity_list

# 二进制驱动自白
strings /var/davinci/driver/drv_hiusbc.ko | grep -iE "conflict|lpm|error"

# 崩溃自愈探测循环（Mac 侧）
for i in $(seq 1 60); do r=$(ssh -o ConnectTimeout=4 aipro "echo ok" 2>&1|tail -1); [ "$r" = "ok" ] && break; sleep 4; done
```

## 五、状态与后续

- aipro 现状：LAN/docker 正常，aic 模块 blacklist，**绿联 AX900 请勿再插着做实验**
  （插着没事，但别加载驱动）；板载 RTL8812BU 勿加载 8821cu
- 绿联 AX900 本体完好，用于 Mac/标准 Linux 免驱直用
- WiFi 需求走外接路由器（README 路线 4），aipro 做透明代理网关
- 三轮事故全记录：docs/02（8821cu lockup）、docs/03（88x2bu NULL Oops）、
  docs/04（aic8800 全程日志）
