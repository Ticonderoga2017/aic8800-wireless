# LicheeRV-Nano-Build 中 aic8800 完整功能对 Linux 内核子系统的依赖清单

本文档基于对 **LicheeRV-Nano-Build** 中 `osdrv/extdrv/wireless/aic8800` 的代码扫描，列出 aic8800 完整功能（WiFi FDRV + BSP + 可选 BTLPM）所依赖的、**aic8800 外部**的 Linux 内核子系统与 API。

---

## 1. 依赖总览（按子系统）

| 内核子系统 / 目录 | 主要头文件 / API | 用途 |
|-------------------|------------------|------|
| **MMC/SDIO** | `linux/mmc/sdio.h`, `sdio_func.h`, `sdio_ids.h`, `card.h`, `host.h` | SDIO 总线：claim_host、readb/writeb、readsb/writesb、set_block_size、enable_func、claim_irq |
| **无线栈 (cfg80211/mac80211)** | `net/cfg80211.h`, `net/mac80211.h` | 无线设备注册、扫描/连接、管理帧、雷达、TDLS、testmode |
| **网络设备** | `linux/netdevice.h`, `linux/etherdevice.h`, `linux/if_arp.h`, `net/ip.h`, `linux/udp.h` | net_device、register_netdevice、skb、队列、xmit |
| **固件加载** | `linux/firmware.h` | request_firmware / release_firmware（固件与配置文件） |
| **内存/分配** | `linux/slab.h`, `linux/vmalloc.h`, `linux/dma-mapping.h`, `linux/dmapool.h` | kmalloc、vmalloc、dma 缓冲区、skb 相关 |
| **网络包** | `linux/skbuff.h` | skb 分配/释放/队列、线性化、copy |
| **同步/线程** | `linux/spinlock.h`, `linux/mutex.h`, `linux/semaphore.h`, `linux/completion.h`, `linux/kthread.h`, `linux/sched.h` | 锁、完成量、内核线程、调度 |
| **中断** | `linux/interrupt.h` | 申请/释放 IRQ、底半部 |
| **电源/唤醒** | `linux/pm_wakeirq.h`, `linux/pm_wakeup.h`, `linux/pm_runtime.h`, `linux/suspend.h` | 唤醒锁、Wake IRQ、运行时 PM |
| **平台/设备** | `linux/platform_device.h`, `linux/device.h`, `linux/module.h` | 模块、设备模型 |
| **GPIO** | `linux/gpio.h`, `linux/of_gpio.h` | 复位、电源、唤醒脚（含设备树） |
| **RFKill** | `linux/rfkill.h`, 平台 `rfkill-wlan.h` | WiFi 块/解块（部分平台） |
| **调试/用户接口** | `linux/debugfs.h`, `linux/proc_fs.h`, `linux/uaccess.h`, `linux/fs.h` | debugfs、proc、ioctl、trace |
| **Netlink** | `linux/netlink.h`, `linux/rtnetlink.h`, `net/netlink.h` | 管理/配置、rtnetlink |
| **其他** | `linux/ieee80211.h`, `linux/wireless.h`, `linux/random.h`, `linux/list.h`, `linux/bitops.h`, `linux/version.h` | 802.11 定义、ioctl、随机数、链表、版本 |

可选或平台相关：

- **USB**：`linux/usb.h`（CONFIG_USB_SUPPORT 时 aicwf_usb）
- **PCI**：`linux/pci.h`（rwnx_pci / 平台抽象）
- **蓝牙/串口**：`linux/serial_core.h`、wakelock 等（aic8800_btlpm）
- **平台特定**：`linux/amlogic/aml_gpio_consumer.h`、`mach/jzmmc.h` 等

---

## 2. 按模块的依赖细分

### 2.1 aic8800_fdrv（WiFi 全功能驱动）

- **MMC/SDIO**：`aicwf_sdio.c` — `sdio_claim_host`、`sdio_release_host`、`sdio_readb`/`sdio_writeb`、`sdio_readsb`/`sdio_writesb`、`sdio_set_block_size`、`sdio_enable_func`、`sdio_claim_irq`/`sdio_release_irq`、`sdio_register_driver`（与 `linux/mmc/*` 全对应）。
- **cfg80211/mac80211**：`rwnx_main.c`、`rwnx_tx.c`、`rwnx_rx.c`、`rwnx_msg_tx.c`、`rwnx_radar.c`、`rwnx_tdls.c`、`rwnx_testmode.c` 等 — `cfg80211_register_netdevice`、`register_netdevice`、`cfg80211_*` 管理/扫描/连接/雷达/CAC、`ieee80211_*`、NL80211、testmode。
- **网络**：`net_device`、`skb`、`dev_queue_xmit`、`alloc_skb`/`dev_kfree_skb`、`netdev_alloc_skb`、`skb_queue_*`、`__skb_*` 等（收发、队列、转发）。
- **固件**：`rwnx_platform.c`、`rwnx_cfgfile.c`、`aicwf_compat_8800dc.c`、`aicwf_compat_8800d80.c` — `request_firmware`/`release_firmware`（fmac、patch、config 等）。
- **同步/线程**：spinlock、mutex、completion、semaphore、kthread、workqueue（命令队列、TX/RX 线程、IPC）。
- **电源/唤醒**：`pm_wakeirq`、`pm_wakeup`、部分平台 `wakelock`、`CONFIG_WIFI_SUSPEND_FOR_LINUX` 时 proc 节点。
- **调试**：debugfs、proc、ioctl（wireless 扩展、私有命令）。
- **Netlink**：`aicwf_manager.c` — 自定义 netlink 协议（如 band steering 等管理功能）。

### 2.2 aic8800_bsp（板级支持）

- **MMC/SDIO**：`aicsdio.c` — 与 FDRV 相同的 `linux/mmc/*` API，用于枚举、F1/F2 使能、块大小、IPC 收发。
- **固件**：`aic_bsp_driver.c` — `request_firmware`/`release_firmware`（BSP 侧固件名/路径）。
- **同步/线程**：completion、semaphore、kthread（与 FDRV 的 TX/RX 线程、命令完成对接）。
- **平台/模块**：`module.h`、`init.h`、`inetdevice.h`（初始化、网络通知）。
- **可选**：`rfkill-wlan.h`、`aml_gpio_consumer.h` 等平台 GPIO/RFKill。

### 2.3 aic8800_btlpm（蓝牙低功耗模块，可选）

- **RFKill**：`rfkill.c` — `rfkill`、GPIO、clk、device、of_gpio。
- **电源/唤醒**：`lpm.c`、`aic8800_btlpm.c` — `pm_wakeirq`、`wakelock`、`serial_core`、timer、workqueue、notifier、proc。

### 2.4 用户空间与根文件系统

- **固件路径**：Kconfig 默认 `/usr/lib/firmware/aic8800_sdio/...`；Buildroot 包 `aic8800-sdio-firmware` 提供固件文件。
- **脚本**：`buildroot/board/cvitek/SG200X/overlay/etc/init.d/S25wifimod` 等，用于加载模块/固件路径。

---

## 3. 内核配置需求（概念性）

要使 aic8800 完整工作，内核需启用（或等效）：

- **CONFIG_MMC**、**CONFIG_MMC_SDIO**、SDIO 主机驱动（如 SDHCI 或平台 SDIO 控制器）。
- **CONFIG_CFG80211**、**CONFIG_MAC80211**（fullmac 模式，CONFIG_RWNX_FULLMAC=y）。
- **CONFIG_FW_LOADER**（request_firmware）。
- **CONFIG_NET**、**CONFIG_WIRELESS**、网络栈与 net_device。
- **CONFIG_GPIOLIB** / **CONFIG_OF**（若使用设备树 GPIO）。
- 平台相关：RFKill、PM wake、debugfs、proc、netlink 等按需。

（具体 Kconfig 以 LicheeRV 工程与 defconfig 为准。）

---

## 4. 与 wireless（StarryOS）的对应关系

| Linux 子系统 | wireless 中已有/计划 | 说明 |
|--------------|----------------------|------|
| MMC/SDIO     | **mmc crate + bsp**  | MmcHost、SdioFunc、BspSdioHost、BspSdioFuncRef；CMD52/53、4-bit、claim、CCCR 使能已对齐 |
| 固件加载     | **bsp fw_load**      | 嵌入或从存储加载固件，无 request_firmware |
| 网络设备/skb | **未实现**          | 无 net_device、无 skb；若要做完整 WiFi 栈需自实现或简化数据路径 |
| cfg80211/mac80211 | **ieee80211 crate** | 完整复刻：Cfg80211Ops、Hw、Channel/SupportedBand、ScanRequest/ConnectParams/KeyParams/StationInfo/BeaconData 等；fdrv 基于 ieee80211 提供 WiphyOps MVP 子集 |
| 同步/线程    | **部分**            | spinlock、mutex、线程（axtask）、completion 语义可自实现 |
| 中断         | **软中断/轮询**      | 无 PLIC SDIO IRQ 时用 timer 轮询 |
| GPIO/电源    | **bsp gpio**         | 上电、复位、可选唤醒 |
| RFKill、debugfs、proc、netlink | **未实现** | 非必须可裁剪 |

结论：**aic8800 完整功能**在 LicheeRV 上依赖除 aic8800 代码外的 **MMC、cfg80211/mac80211、网络栈、固件加载、内存/同步/中断/电源、以及可选 GPIO/RFKill/调试/Netlink** 等内核子系统；当前 wireless 已覆盖 MMC/SDIO 与 BSP 侧固件与 GPIO，**网络设备与 cfg80211/mac80211 为最大缺口**，若目标为“完整 WiFi 栈”需在无 Linux 环境下自设计替代或大幅裁剪 FDRV。

---

## 5. 参考代码位置（LicheeRV-Nano-Build）

- aic8800 源码：`osdrv/extdrv/wireless/aic8800/`（aic8800_bsp、aic8800_fdrv、aic8800_btlpm）。
- 内核侧仅通过 Kconfig/Makefile 引用上述路径（`linux_5.10/drivers/net/wireless/` 下 aic8800 的 Kconfig/Makefile），**无 aic8800 代码在内核树内**。
- 固件包：Buildroot `package/aic8800-sdio-firmware`；默认固件路径见 aic8800 Kconfig `AIC_FW_PATH`。
