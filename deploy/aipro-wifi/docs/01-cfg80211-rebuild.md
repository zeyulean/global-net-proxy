# 补编 cfg80211 + mac80211 内核模块（已验证可行）

**目标**：5.10.0+ 内核 `CONFIG_CFG80211=m / MAC80211=m`，但 OrangePi 出厂 modules 包**漏装** cfg80211.ko/mac80211.ko，导致 8821cu.ko 加载报 `Unknown symbol cfg80211_*`。本流程补编这两个模块。

**已验证**：编出的 .ko `vermagic` 精确为 `5.10.0+ SMP mod_unload aarch64`，`insmod` 后稳定加载，SSH 全程通畅。

## 为什么安全

- `CONFIG_MODVERSIONS is not set` → 不校验符号 CRC，gcc 版本差异零风险（原内核华为 Do-Compiler gcc 7.3，本机 gcc 11.4，无影响）
- `CONFIG_MODULE_SIG_FORCE is not set` → 不强制签名，自编 .ko 无需 OrangePi 私钥
- vermagic 复现：headers Makefile `VERSION=5 PATCHLEVEL=10 SUBLEVEL=0`（主线 5.10），`echo + > .scmversion` 复现 `5.10.0+` 的 `+`
- `CONFIG_RFKILL=y`（built-in）→ vmlinux 已导出 `rfkill_alloc`，**但 `/lib/modules` 残留 rfkill.ko**，`modprobe` 会拉它冲突 → 必须 `insmod` 绕过依赖解析

## 流程（对应 `bash/build-cfg80211.sh`）

```bash
# 1. 下载主线 linux-5.10（tuna，112MB）
curl -fL -o /tmp/linux-5.10.tar.xz \
  https://mirrors.tuna.tsinghua.edu.cn/kernel/v5.x/linux-5.10.tar.xz

# 2. 解压
cd /tmp && tar -xf linux-5.10.tar.xz && cd linux-5.10

# 3. 套当前内核 config + 复现 vermagic "+"
zcat /proc/config.gz > .config
echo "+" > .scmversion
make ARCH=arm64 olddefconfig

# 4. 验证 kernelrelease（必须 5.10.0+）
make ARCH=arm64 kernelrelease

# 5. 编译
make ARCH=arm64 -j"$(nproc)" modules_prepare
make ARCH=arm64 M=net/wireless  modules   # → net/wireless/cfg80211.ko
make ARCH=arm64 M=net/mac80211  modules   # → net/mac80211/mac80211.ko

# 6. 安装（需 sudo）
sudo install -D -m644 net/wireless/cfg80211.ko /lib/modules/5.10.0+/kernel/net/wireless/cfg80211.ko
sudo install -D -m644 net/mac80211/mac80211.ko /lib/modules/5.10.0+/kernel/net/mac80211/mac80211.ko
sudo depmod -a 5.10.0+
```

## 加载（关键：用 insmod 不用 modprobe）

```bash
# modprobe cfg80211 会拉 rfkill.ko → "exports duplicate symbol rfkill_alloc (owned by kernel)"
sudo insmod /lib/modules/5.10.0+/kernel/net/wireless/cfg80211.ko
sudo insmod /lib/modules/5.10.0+/kernel/net/mac80211/mac80211.ko
lsmod | grep -E 'cfg80211|mac80211'   # 应见两者，mac80211 used by cfg80211
```

## 验证清单

- [ ] `make kernelrelease` = `5.10.0+`
- [ ] `modinfo cfg80211.ko | grep vermagic` = `5.10.0+ SMP mod_unload aarch64`
- [ ] `modinfo mac80211.ko | grep vermagic` 同上
- [ ] insmod 后 SSH 仍通畅（cfg80211 只注册 genl 子系统，不接管网卡，零断连风险）

## 注意

- `M=` 编译 warn `Module.symvers missing`——无害（MODVERSIONS 未设，不校验）
- `/lib/modules/5.10.0+/kernel/net/rfkill/rfkill.ko` 是 OrangePi 残留（RFKILL 已 =y），用 insmod 不触发即可
- 本流程**不涉及 8821cu**；加载 8821cu 是另一回事且当前会 hard lockup
