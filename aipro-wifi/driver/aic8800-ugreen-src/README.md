# aic8800-ugreen 源码归档（绿联 AX900 / AIC8800D80）

> 作用：aipro 上 `/mnt/disk/lwboy/projects/drivers/aic8800-ugreen` 的完整备份。
> 该 git 仓丢失/损坏时，从这里可完整恢复（源码 + 全部 22 轮实验历史）。

## 内容

| 文件 | 说明 |
|---|---|
| `aic8800-ugreen-tree.tar.gz` (1.3M) | 源码树快照（无 .git/编译产物），解压即用 |
| `aic8800-ugreen.bundle` (39M) | 完整 git 历史（git clone 即恢复全部 commit） |
| `EXPERIMENTS.md` | E1-E22 实验记录（也可从 bundle 恢复） |

## 恢复

```bash
# 源码 + 历史：
git clone aic8800-ugreen.bundle aic8800-ugreen     # 含全部 commit
# 或只要快照：
tar xzf aic8800-ugreen-tree.tar.gz
```

## 重编关键点（详见 ../../docs/04）

```bash
cd drivers/aic8800/aic8800_fdrv
grep -n "CONFIG_USB_BT" Makefile        # 必须为 y（D80 三接口复合形态）
make -C /lib/modules/5.10.0+/build ARCH=arm64 M=$PWD \
  KBUILD_EXTRA_SYMBOLS=<树内>/aic_load_fw/Module.symvers modules
# 产物 md5 应为 2396b6f4c2bc358f19bda87102158583（与 artifacts/aic8800 一致）
```

历史注记：初版导入（a8ec713）已含 8d80/8d88/368b ID、USB_BT=n、list-init 等修改；
E22（15cfb8c）把 USB_BT 改回 y 才最终可用。上游原始来源：绿联 AX900 随附驱动 V1.4。
