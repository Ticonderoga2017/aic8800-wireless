# LicheeRV-Nano-Build WiFi 模块内核功能分析与 StarryOS 移植指南

本文档分析 LicheeRV-Nano-Build 中 AIC8800 WiFi 模块在内核侧的全部功能，说明各功能作用、互动机制与实现方式，并给出在 StarryOS/wireless crate 下复刻这些实现机制的具体方案。

---

## 1. 概述

### 1.1 源码位置

- **LicheeRV-Nano-Build WiFi 模块路径**：`LicheeRV-Nano-Build/osdrv/extdrv/wireless/aic8800/`
- **目标移植位置**：`StarryOS/wireless/`（含 `driver/bsp`、`driver/fdrv` 等）

### 1.2 模块划分

| 子目录 | 角色 | 对应 StarryOS |
|--------|------|----------------|
| **aic8800_bsp** | 板级支持：固件、SDIO 底层、命令管理、平台抽象 | `wireless/driver/bsp` |
| **aic8800_fdrv** | 全功能驱动：cfg80211、net_device、IPC、数据路径、私有命令 | `wireless/driver/fdrv` |
| **aic8800_btlpm** | 蓝牙低功耗/射频控制（可选） | 暂不纳入 wireless 范畴 |

内核侧“WiFi 功能”主要指：**BSP 提供的硬件/固件/总线抽象** + **FDRV 提供的 802.11/cfg80211 与网络栈集成**。二者共同构成从 SDIO 到 `wlan0` 的完整数据与控制路径。

---

## 2. 内核功能总览

### 2.1 功能清单（按层次）

| 层次 | 功能块 | 主要文件/接口 | 作用简述 |
|------|--------|----------------|----------|
| **BSP** | 固件管理 | aic_bsp_main.c, aic_bsp_driver.c, aicwf_firmware_array | 多芯片/多模式固件表、加载路径、request_firmware/文件读取、MD5 |
| | 平台与电源 | aic_bsp_main.c, aicsdio.c | platform_driver、sysfs(cpmode/hwinfo/fwdebug)、aicbsp_set_subsys |
| | SDIO 底层 | aicsdio.c, aicsdio_txrxif.c | 探测、claim_host、读写、中断、时钟/相位 |
| | 命令管理层 | aic_bsp_driver.c (rwnx_cmd_mgr) | LMAC 消息队列、req/cfm 配对、超时、rwnx_set_cmd_tx |
| | 固件下载与启动 | aic_bsp_driver.c | DBG_MEM_*、START_APP、patch/calib、aicwifi_init/aicbt_init |
| | 预留内存 | aic_bsp_driver.c | skb 预分配池（CONFIG_RESV_MEM_SUPPORT） |
| **FDRV** | cfg80211 集成 | rwnx_main.c | wiphy_new、cfg80211_ops、bands/channels/bitrates、iface_combination |
| | 虚拟接口与 net_device | rwnx_main.c | add_virtual_intf、ndo_*、register_netdevice |
| | 扫描/连接/AP | rwnx_main.c, rwnx_cmds.c | scan/connect/disconnect、start_ap/stop_ap、beacon |
| | 密钥与站管理 | rwnx_main.c | add_key/del_key、add_station/del_station、get_station |
| | 数据路径 TX/RX | rwnx_tx.c, rwnx_rx.c, aicwf_sdio.c | 802.3→802.11、队列、skb、sdio 聚合收发 |
| | IPC 主机 | ipc_host.c, ipc_shared.h | 与固件 LMAC 的消息格式、描述符、流控 |
| | 消息发送/接收 | rwnx_msg_tx.c, rwnx_msg_rx.c | 上层→LMAC 命令、E2A 消息回调、cmd_mgr.msgind |
| | 私有命令 | aic_priv_cmd.c | Android WiFi 扩展 ioctl、RFTEST/厂商命令解析 |
| | Vendor/nl80211 | aic_vendor.c | nl80211 vendor 子命令、OUI、扩展能力 |
| | 其他特性 | rwnx_radar, rwnx_mesh, rwnx_tdls, rwnx_bfmer 等 | DFS、Mesh、TDLS、Beamforming 等可选 |

下文按 **BSP**、**FDRV 与内核无线栈**、**数据路径与总线**、**用户接口** 四块展开，并说明它们之间的互动与在 StarryOS 下的复刻要点。

---

## 3. BSP 层功能详解

### 3.1 固件管理（Firmware Management）

**位置**：`aic8800_bsp/aic_bsp_main.c`、`aic_bsp_driver.c`（rwnx_load_firmware 等）

**作用**：

- 维护按 **芯片型号**（8801/8800DC/8800D80/8800D80x2）和 **工作模式**（WORK/TEST）的固件表：`aicbsp_firmware_list`。
- 每条记录包含：`wl_fw`（WiFi 固件）、`bt_adid`/`bt_patch`/`bt_table`（蓝牙）、可选 `bt_ext_patch`。
- 加载方式：`request_firmware()` 或 文件路径（如 `AICBSP_FW_PATH`）+ `filp_open`/`kernel_read`，或 `CONFIG_FIRMWARE_ARRAY` 编译进内核。
- 校验：MD5 计算并打印，不强制失败。
- 提供 **cpmode** sysfs 与模块参数 `testmode`，用于切换正常/射频测试等模式。

**实现要点**：

- 全局 `aicbsp_info`（cpmode/hwinfo/fwlog_en/irqf）、`aicbsp_firmware_list` 与芯片 rev 检测结果共同决定最终要加载的固件名。
- 与“固件下载”配合：加载到内存后由 BSP 通过 DBG_MEM_BLOCK_WRITE_REQ 等写入设备 RAM。

**移植到 StarryOS**：

- 在 `wireless/driver/bsp` 中保留“固件表”抽象（如 `firmware.rs` 中的 `AicBspFirmware`、`AicBspCpMode`）。
- 固件来源改为：StarryOS 可用的非 volatile 存储或内置 slice，提供“按 (chip_id, cpmode) 返回固件名或二进制”的接口。
- 不做 request_firmware，由上层在初始化时把固件 buffer 交给 BSP 的下载逻辑。

### 3.2 平台与电源（Platform & Power）

**位置**：`aic_bsp_main.c`（platform_driver、sysfs）、`aicsdio.c`（aicbsp_set_subsys）

**作用**：

- 注册 `platform_driver`（name `aic_bsp`）和 `platform_device`（name `aic-bsp`），创建 sysfs 组 `aicbsp_info`：cpmode、hwinfo、fwdebug。
- `aicbsp_set_subsys(subsys, state)`：统一管理 AIC_BLUETOOTH / AIC_WIFI 电源。首次上电时：平台 power_on → SDIO 初始化 → `aicbsp_driver_fw_init()`（固件下载与启动）→ 可选 release SDIO；下电时顺序相反。
- 使用 `mutex aicbsp_power_lock` 防止并发上下电。

**实现要点**：

- BSP 与 FDRV 通过“先 BSP 加载固件并初始化 SDIO，再 FDRV 注册 netdev/cfg80211”的顺序协作；部分平台（如 Rockchip）在 BSP 里直接调用 `aicbsp_sdio_init` 和 `aicbsp_driver_fw_init`。

**移植到 StarryOS**：

- 在 `wireless/driver/bsp` 的 `platform` 模块中抽象“电源/子系统的开关”（如 `aicbsp_set_subsys`），由 StarryOS 的 HAL 或 BSP 实现真实上电/下电。
- sysfs 等价物：可通过 StarryOS 的配置或调试接口暴露 cpmode/hwinfo/fwdebug，不必强求与 Linux sysfs 一一对应。

### 3.3 SDIO 底层（SDIO Low-Level）

**位置**：`aic8800_bsp/aicsdio.c`、`aicsdio_txrxif.c`

**作用**：

- 探测：匹配 SDIO vendor/device ID（8801/8800DC/8800D80/8800D80x2），区分子芯片与 func（如 func2）。
- 提供 `aicwf_sdio_readb/writeb`、func2 的读写、块读写；`sdio_claim_host`/`sdio_release_host` 保护。
- 时钟、相位由 `aicbsp_get_feature()` 提供给上层（如 FDRV 初始化 SDIO 时钟）。
- 中断：注册 sdio_irq，在中断里通知 FDRV 收包或事件（通过 aicwf_bus 的 completion/work）。
- 可选：GPIO wakeup、OOB 中断等。

**实现要点**：

- BSP 导出 `struct aic_sdio_dev` 及读写接口；FDRV 的 `sdio_host` 或 `aicwf_sdio.c` 使用这些接口做实际 SDIO 传输，并与 `aicwf_bus`（tx/rx 线程、cmd_buf）配合。

**移植到 StarryOS**：

- `wireless/driver/bsp/sdio.rs` 中已抽象 `SdioOps`、`ProductId`、时钟/相位等；需在具体平台上实现“块读写、中断/轮询收包”，并与 FDRV 的 `SdioHost` 对接，保证语义与 Linux 的 claim_host/读写一致（至少保证单线程或锁内使用）。

### 3.4 命令管理层（Command Manager）

**位置**：`aic8800_bsp/aic_bsp_driver.c`（rwnx_cmd_mgr_*、rwnx_send_msg、rwnx_set_cmd_tx）

**作用**：

- **rwnx_cmd_mgr**：维护待处理命令队列（list）、最大队列长度、超时（RWNX_80211_CMD_TIMEOUT_MS）。
- 上层调用 `rwnx_send_msg(sdiodev, msg_params, reqcfm, reqid, cfm)`：分配 `rwnx_cmd`，入队，若需 cfm 则等待 `complete`；实际发送通过 `rwnx_set_cmd_tx(dev, lmac_msg, len)` 把消息拷贝到 `bus->cmd_buf` 并调用 `aicwf_bus_txmsg()`。
- 固件侧 E2A 确认通过 **rwnx_rx_handle_msg** 回调到 **cmd_mgr.msgind**，根据 msg->id 匹配 reqid，填充 cfm 并 complete，完成一次“请求-确认”。
- DBG 类（DBG_MEM_READ/WRITE/BLOCK_WRITE、DBG_START_APP_REQ）均通过此路径发送，用于固件下载与启动。

**实现要点**：

- 所有与 LMAC 的同步命令（如读寄存器、写内存、启动 CPU）都走 cmd_mgr，保证顺序与超时；异步事件（如 RX 数据、统计）走 E2A 的其它回调，不占 cmd 队列。

**移植到 StarryOS**：

- 在 `wireless/driver/bsp` 中实现“命令队列 + 单线程或锁保护 + 超时”：发送端把 lmac_msg 写入总线，接收端在 E2A 解析处根据 id 找到等待的 request 并完成。可与现有 `cmd.rs`（LmacMsgHeader、TaskId 等）结合，在无 `struct completion` 的环境下用条件变量或 async 的 oneshot 代替。

### 3.5 固件下载与启动（FW Download & Start）

**位置**：`aic_bsp_driver.c`（rwnx_plat_bin_fw_upload_android、aicwifi_init、aicbt_init、rwnx_plat_patch_load 等）

**作用**：

- **WiFi**：按芯片分支（8801/8800DC/8800D80/8800D80x2）执行：系统配置表写寄存器（syscfg_tbl、rf_tbl_masked）、patch 表上传、固件块写入（RAM_FMAC_FW_ADDR 等）、patch_config（如 aicwifi_patch_config）、最后 **DBG_START_APP_REQ** 启动固件。
- **蓝牙**：aicbt_patch_table 解析、adid/patch/ext_patch 上传、aicbt_patch_table_load 写寄存器并可选拉复位。
- 支持 8800DC 的 DPD/LOFT 校准、M2D OTA 等可选逻辑。

**实现要点**：

- 强依赖 **cmd_mgr** 与 **DBG_*** 消息；地址与文件名来自 BSP 固件表与芯片 rev。

**移植到 StarryOS**：

- 在 BSP 侧实现“给定固件 buffer + 目标地址”的块写入（等价于 rwnx_plat_bin_fw_upload_android 的逻辑），以及 START_APP；patch/calib 表可按需从配置文件或常量表读取。保持与现有 firmware 表、platform 抽象一致。

### 3.6 预留内存（Reserved Memory）

**位置**：`aic_bsp_driver.c`（CONFIG_RESV_MEM_SUPPORT、aicbsp_resv_mem_alloc_skb、resv_skb）

**作用**：

- 在模块初始化时预分配若干 skb（如 1536*64 字节），用于 TX 数据路径，避免在中断或关键路径上分配失败。

**移植到 StarryOS**：

- 若 StarryOS 有类似“预分配缓冲区池”的机制，可在 BSP 或 FDRV 的 TX 路径使用；否则可用静态缓冲区或 alloc 池替代，保证在发送路径上不阻塞即可。

---

## 4. FDRV 层与内核无线栈集成

### 4.1 cfg80211 与 wiphy

**位置**：`rwnx_main.c`（wiphy_new、rwnx_cfg80211_ops、bands、channels、cipher_suites、iface_combination）

**作用**：

- 注册一个 **wiphy**，作为该 WiFi 设备在内核中的代表；所有 802.11 相关操作都通过 **cfg80211_ops** 回调进入驱动。
- 声明支持的频段（2.4G/5G）、信道、速率、HT/VHT/HE 能力、接口类型（STA/AP/P2P_CLIENT/P2P_GO/P2P_DEVICE/MESH）、接口组合与 DFS 能力（radar_detect_widths）。
- 提供加密套件（WEP/TKIP/CCMP/GCMP 等）及管理帧类型（tx/rx）。

**实现要点**：

- `rwnx_cfg80211_init` 里 `wiphy_new(&rwnx_cfg80211_ops, sizeof(struct rwnx_hw))`，然后设置 bands、regulatory、features 等；最后 `wiphy_register`。每个 cfg80211 操作（如 add_virtual_intf、scan、connect）内部会转成对 LMAC 的 **IPC 命令**（通过 rwnx_send_msg 或等效接口）。

**移植到 StarryOS**：

- StarryOS 无 cfg80211，需要自建“无线控制平面”：
  - **选项 A**：在 `wireless` crate 内定义一套与 cfg80211 语义接近的 trait（如 WiphyOps：add_interface、scan、connect、start_ap 等），由 fdrv 实现，供上层（如 wpa_supplicant 的 Rust 绑定或简单 CLI）调用。
  - **选项 B**：通过现有 socket/ioctl 或新的 syscall，把“扫描/连接/AP”等映射为有限的几条命令，由 wireless 内部再转为对固件的 IPC。
- 频段/信道/速率等能力以配置或常量形式保存在 `wireless` 中，不依赖 Linux 的 ieee80211_channel 等结构体，但字段语义应对齐以便与用户空间或脚本一致。

### 4.2 虚拟接口与 net_device

**位置**：`rwnx_main.c`（add_virtual_intf、del_virtual_intf、ndo_open/stop/xmit、register_netdevice）

**作用**：

- 每个“虚拟接口”（STA/AP/P2P 等）对应一个 **net_device**（如 wlan0）；ndo_xmit 接收来自 IP 栈的 skb，交给驱动 TX 路径；驱动 RX 路径把收到的 802.11 帧解封装后通过 netif_rx 或 NAPI 送入协议栈。
- 与 cfg80211 的 add_virtual_intf 绑定：创建 net_device、分配 rwnx_vif、注册到 rwnx_hw->vifs。

**实现要点**：

- 数据面：net_device 的 dev_queue_xmit → ndo_start_xmit → rwnx_tx（或 aicwf 的 tx 入口）→ 802.11 封装 → aicwf_bus_txdata；反向为 SDIO 收包 → 解析 → rwnx_rx 或 aicwf_rx → netif_rx。

**移植到 StarryOS**：

- StarryOS 若已有“网络设备”抽象（如 axnet/arceos 的 NetDriver），则在 wireless 中实现该抽象：向上提供“发送 raw 以太网帧 / 接收 raw 以太网帧”的接口，内部完成 802.11 封装/解封装与 LMAC 通信。
- 若尚无统一网络栈，可先实现“一个无线接口”的发送/接收接口，供上层或测试程序直接使用，后续再与 StarryOS 的 socket/网络层对接。

### 4.3 扫描、连接、AP、密钥与站管理

**位置**：`rwnx_main.c`（rwnx_cfg80211_scan、connect、disconnect、start_ap、stop_ap、add_key、add_station 等）

**作用**：

- **scan**：下发扫描请求到 LMAC，结果通过 E2A 事件上报，驱动再通过 cfg80211_scan_done 通知上层。
- **connect/disconnect**：连接参数下发给固件，状态与结果通过事件回调。
- **start_ap**：设置 beacon、信道、基本速率等；**change_beacon/stop_ap** 更新或关闭 AP。
- **add_key/del_key**：PTK/GTK 等写入固件或本地缓存；**add_station/del_station** 用于 AP 模式下管理关联站。
- **get_station**：查询 RSSI、速率等，从固件或本地缓存取。

**实现要点**：

- 上述操作全部通过 **rwnx_msg_tx** 系列或 rwnx_cmds 下发到 LMAC，确认或结果通过 **rwnx_msg_rx** / E2A 回调解析后，再调用 cfg80211 的 API（如 cfg80211_scan_done、cfg80211_connect_result 等）通知上层。

**移植到 StarryOS**：

- 在“无线控制平面”trait 中定义 scan/connect/start_ap/add_key 等接口；fdrv 内部仍用现有 IPC/msg 层与固件通信，只是把“结果”通过回调或 channel 交给 StarryOS 的上层，而不是调用 cfg80211。

### 4.4 数据路径（TX/RX）

**位置**：`rwnx_tx.c`、`rwnx_rx.c`、`aicwf_sdio.c`、`aicwf_txrxif.c`、`sdio_host.c`

**作用**：

- **TX**：net_device 或内部队列的 skb → 802.11 封装（可能加密、填序列号等）→ 放入 aicwf_tx_priv 的队列或直接调用 aicwf_bus_txdata → SDIO 块写（可能聚合多包）。
- **RX**：SDIO 中断或轮询 → 从总线读入 raw 数据 → 解析 IPC/RX 描述符 → 拆成 802.11 帧 → 可能重排序（CONFIG_AICWF_RX_REORDER）→ 提交给上层（netif_rx 或等效）。

**实现要点**：

- TX/RX 与 **ipc_shared.h** 的描述符布局、流控（如 fw_avail_bufcnt）一致；与 BSP 的 cmd_buf、txmsg 路径分离：cmd 走 cmd_buf+txmsg，数据走 txdata/rx 线程或中断下半部。

**移植到 StarryOS**：

- 在 `wireless/driver/fdrv` 的 `sdio_host` 或单独 tx/rx 模块中：
  - TX：实现“提交一个 802.11 或 以太网 帧”的接口，内部做封装与总线发送；
  - RX：在 BSP 或 fdrv 的“从 SDIO 读”的循环/回调里解析 IPC RX 结构，再回调或推入队列给上层。
- 若 StarryOS 无 skb，用 `Vec<u8>` 或自定义 buffer 类型即可，保持“线性缓冲区 + 长度”的语义。

### 4.5 IPC 主机与消息层

**位置**：`ipc_host.c`、`ipc_shared.h`、`rwnx_msg_tx.c`、`rwnx_msg_rx.c`、`lmac_msg.h`、`lmac_types.h`

**作用**：

- **IPC**：定义 Host 与 LMAC 之间的消息格式（A2E/E2A）、描述符数量与布局、流控（如 nx_txdesc_cnt）。
- **rwnx_msg_tx**：将上层“操作”（如 scan、connect、start_ap）编码成 lmac_msg（id、dest_id、src_id、param_len、param[]），通过 BSP 的 rwnx_send_msg 或等效发送。
- **rwnx_msg_rx**：从 E2A 流中解析出消息 id 与 param，分发到对应 handler（如 cmd_mgr.msgind 用于 cfm，其它用于事件如 scan_done、connect_result）。

**实现要点**：

- 所有与固件的“控制”交互都走 IPC 消息；数据走单独的数据描述符与 SDIO 数据通道。msg_rx 通常在 SDIO 接收路径上被调用（根据包头区分 cmd cfm 与 data）。

**移植到 StarryOS**：

- `wireless/driver/fdrv/ipc.rs` 已存在；应完整实现：
  - 与 `ipc_shared.h`/`lmac_msg.h` 一致的二进制格式（id、param_len、param）；
  - 发送侧：组包后调用 BSP 的“发送命令”接口；
  - 接收侧：在总线 RX 路径中识别 E2A 消息，调用 IpcHostCb 或按 id 分发到 cmd 完成或事件处理。

### 4.6 私有命令与 Vendor 扩展

**位置**：`aic_priv_cmd.c`、`aic_vendor.c`

**作用**：

- **aic_priv_cmd**：处理 Android 风格 WiFi 扩展 ioctl（如 SIOCGIWPRIV），字符串解析（如 "set_cmd xxx"），实现 RFTEST、设置 MAC、厂商参数等；部分命令直接调 rwnx_send_* 与固件交互。
- **aic_vendor**：nl80211 vendor 子命令（OUI + subcmd），用于 wpa_supplicant 等与驱动扩展能力交互。

**实现要点**：

- 二者都是“用户空间 → 内核”的扩展入口，最终多数会落到 LMAC 命令或寄存器访问。

**移植到 StarryOS**：

- 若 StarryOS 有 ioctl 或类似“设备控制”的 syscall，可把常用私有命令映射为若干 ioctl 或新 syscall；否则在 wireless 内提供“扩展命令”API（如 set_rf_test、set_mac、get_rssi），供上层或脚本调用。
- vendor 扩展可在“无线控制平面”里用枚举或字符串命令代替 nl80211 vendor 消息。

---

## 5. 功能之间的互动机制

### 5.1 初始化顺序

1. **BSP 先加载**：platform_driver 或等效 → aicbsp_init → 预留内存、平台设备、sysfs。
2. **SDIO 探测**：由 Linux 的 mmc/sdio 子系统触发，或在 aicbsp_set_subsys 中主动调用 aicbsp_sdio_init；BSP 注册 sdio_driver，匹配到卡后 probe。
3. **Probe 内**：chipmatch → aicbsp_platform_init（cmd_mgr_init）→ aicbsp_driver_fw_init（固件下载、BT 初始化、WiFi 初始化、START_APP）→ 可选 aicbsp_sdio_release。
4. **FDRV 注册**：创建 wiphy、注册 cfg80211、注册 net_device（或延迟到 add_virtual_intf）；打开 SDIO 中断、启动 TX/RX 线程或 NAPI。
5. **用户空间**：ifconfig/iw/wpa_supplicant 通过 nl80211/socket 与 cfg80211 交互，驱动通过 IPC 与固件交互。

### 5.2 数据流与控制流分离

- **控制流**：用户/上层 → cfg80211_ops / ioctl / vendor → rwnx_msg_tx / rwnx_cmds → rwnx_send_msg → BSP cmd_buf → aicwf_bus_txmsg → SDIO；回复与事件：SDIO RX 解析 → E2A → cmd_mgr.msgind 或事件回调 → cfg80211_* 或内部状态机。
- **数据流**：IP 栈 → net_device ndo_start_xmit → rwnx_tx / aicwf 队列 → aicwf_bus_txdata → SDIO 数据写；SDIO 数据读 → 解析 RX 描述符 → rwnx_rx / aicwf_rx → netif_rx 或等效。

### 5.3 依赖关系小结

- BSP 不依赖 FDRV；FDRV 依赖 BSP 的：sdio 读写、cmd_mgr（或等效发送/完成）、固件已启动。
- cfg80211 与 net_device 依赖 FDRV 的 IPC 与 TX/RX 实现；私有命令与 vendor 依赖同一 IPC/命令层。
- 所有对固件的“请求-确认”式调用都经过 BSP 的 cmd_mgr（或 StarryOS 下的等价物），保证顺序与超时。

---

## 6. 在 StarryOS 下复刻实现机制的方案

### 6.1 分层对应表

| Linux 内核侧 | StarryOS/wireless 复刻方式 |
|--------------|----------------------------|
| BSP 固件表 + request_firmware | bsp/firmware：固件表 + 从存储或内置 slice 加载 |
| BSP platform + sysfs | bsp/platform：电源/子系统开关 + 配置或调试接口 |
| BSP SDIO 读写 + 中断 | bsp/sdio：SdioOps 实现 + 平台 HAL 的 SDIO 与中断 |
| BSP cmd_mgr + rwnx_set_cmd_tx | bsp/cmd：命令队列 + 超时 + 通过 SdioOps 发 lmac_msg |
| BSP 固件下载/START_APP | bsp：在 platform 或单独 fw_init 中调用 cmd 发 DBG_* |
| cfg80211_ops + wiphy | fdrv：自建 WiphyOps trait + 能力与接口组合配置 |
| net_device + ndo_* | fdrv：实现 StarryOS 的 NetDriver 或最小“发送/接收帧”接口 |
| rwnx_msg_tx/rx + ipc_host | fdrv/ipc：二进制协议一致 + 发送用 BSP cmd、接收在 RX 路径分发 |
| aicwf_sdio TX/RX 线程 | fdrv/sdio_host：用任务或轮询从 SdioOps 收发包，解析 IPC |
| aic_priv_cmd / aic_vendor | fdrv/priv_cmd、vendor：扩展命令 API 或通过 syscall/ioctl 暴露 |

### 6.2 建议实现顺序

1. **BSP 稳定**：sdio 读写与 probe 流程、cmd 队列与 E2A 的 cfm 完成、固件下载与 START_APP，能在无 FDRV 的情况下把固件跑起来（可通过简单读写寄存器验证）。
2. **FDRV IPC**：在 RX 路径中正确解析 E2A，并区分“命令确认”与“数据/事件”；发送侧对所有 LMAC 命令通过 BSP cmd 接口发送。
3. **最小数据路径**：实现“发一包 802.3/802.11”和“收一包并回调”，不依赖完整 net_device，便于联调固件与主机。
4. **控制平面**：实现 scan/connect/start_ap 等与 cfg80211 语义对应的接口，内部走 IPC；再对接 StarryOS 的上层（如 socket、简单 CLI 或 wpa_supplicant 移植）。
5. **私有命令与优化**：把常用 ioctl/vendor 映射到 StarryOS 的 API 或 syscall；再根据需要做重排序、预分配缓冲等优化。

### 6.3 与现有 wireless 代码的衔接

- **driver/bsp**：已有 cmd、export、firmware、platform、sdio 的模块骨架；本分析中的“BSP 功能”应逐一落到这些模块或新增子模块（如 fw_load.rs），并保持与 aic8800_bsp 的接口语义一致。
- **driver/fdrv**：已有 ipc、manager、priv_cmd、sdio_host、vendor；本分析中的“FDRV 功能”应在这些模块中实现具体逻辑，并依赖 bsp 的 cmd 与 sdio，而不是直接依赖 Linux 内核。
- **api**：若 StarryOS 通过 syscall 暴露 WiFi 控制（如 scan、connect），可在 api 层调用 wireless 的 WiphyOps 或 manager，形成“用户程序 → api → wireless → 固件”的完整链。

---

## 7. 附录

### 7.1 rwnx_cfg80211_init 参数与入口说明

**函数**：`int rwnx_cfg80211_init(struct rwnx_plat *rwnx_plat, void **platform_data)`（rwnx_main.c 约 5666 行）

**两个参数含义**：

1. **rwnx_plat (struct rwnx_plat \*)**  
   - 定义在 `rwnx_platform.h`（约 85–105 行），是**平台抽象**，按总线类型不同包含：
     - SDIO：`rwnx_plat->sdiodev`（`struct aic_sdio_dev *`），即已探测、已加载固件并完成 START_APP 的 SDIO 设备；
     - USB：`rwnx_plat->usbdev`；
     - PCI：`rwnx_plat->pci_dev`。
   - 还有 `enabled`、以及一组函数指针：`enable`/`disable`/`deinit`/`get_address`/`ack_irq`/`get_config_reg`，和 `priv[]` 私有数据。
   - 在 SDIO 路径下由 `aicwf_rwnx_sdio_platform_init(sdiodev)`（sdio_host.c）里分配并填写：`rwnx_plat->sdiodev = sdiodev`，再传给 `rwnx_platform_init(rwnx_plat, &drvdata)`。

2. **platform_data (void \*\*)**  
   - 是**输出参数**：函数内部创建并初始化 `struct rwnx_hw *rwnx_hw` 后，在函数末尾执行 `*platform_data = rwnx_hw`（rwnx_main.c 约 6013 行），把“主驱动数据指针”回传给调用方。
   - 调用方（rwnx_platform_init → sdio_host.c / usb_host.c）会把该指针存为对应总线设备的 driver data，供后续 probe/remove 使用。

**是否是 WiFi 模块的“最初”初始化入口？**

- **不是**。WiFi 模块的**最初**入口是：
  1. **BSP**：`aicbsp_init`（aic_bsp_main.c）— 模块加载时注册 platform_driver、创建 platform_device、sysfs 等；
  2. **FDRV**：`rwnx_mod_init`（rwnx_main.c）— 模块加载时注册 SDIO/USB 驱动。

**aicbsp_init 何时、何处被调用？**

- **何时**：在 **BSP 内核模块被加载时**。即用户或启动脚本执行 `insmod aic_bsp.ko`（或 `modprobe` 加载对应模块）、或系统启动时自动加载该模块的那一刻。
- **何处**：**不由其他 C 代码显式调用**，而是由 **Linux 内核的模块加载器** 在加载 aic8800_bsp 模块时，调用通过 `module_init(aicbsp_init)`（aic_bsp_main.c 约 450 行）注册的初始化函数。
- **调用链**：内核 `do_init_module()` → 模块的 `.init` 指针（即 `aicbsp_init`）→ 执行 aicbsp_init()，内部做：预留内存、注册 platform_driver、创建 platform_device、创建 sysfs 组、初始化 aicbsp_power_lock，在 Rockchip 平台上还会调用 `aicbsp_set_subsys(AIC_BLUETOOTH, AIC_PWR_ON)` 上电。**不**在此处做 SDIO 探测或固件加载；SDIO 探测由 FDRV 模块注册的 sdio_driver 在设备插入或上电后触发。

**aicbsp_init 逐行说明**（aic_bsp_main.c 391–427）：

| 行号 | 代码 | 作用 |
|------|------|------|
| 391 | `static int __init aicbsp_init(void)` | 模块初始化函数；`__init` 表示仅在内核加载时使用，之后可丢弃以省内存。 |
| 393 | `int ret;` | 存放各步返回值，用于错误处理。 |
| 394–395 | `printk("%s\n", __func__);` 与 `printk("RELEASE_DATE:...");` | 向内核日志打印当前函数名和固件发布日期，便于确认模块已加载。 |
| 397 | `aicbsp_info.cpmode = testmode;` | 用模块参数 `testmode`（见 387 行 `module_param(testmode, ...)`）设置当前固件模式（0=正常/1=射频测试等）。 |
| 399 | `aicbsp_resv_mem_init();` | 预留内存初始化：预分配若干 skb 等，供后续 TX 路径使用，避免在关键路径上动态分配失败。 |
| 401 | `sema_init(&aicbsp_probe_semaphore, 0);` | 将信号量初始化为 0，用于 BSP 与 FDRV 之间同步“SDIO 已 probe”等事件（FDRV probe 里可能 up 此信号量）。 |
| 403–407 | `platform_driver_register(&aicbsp_driver);` | 向内核注册名为 "aic_bsp" 的 platform_driver；当前 aicbsp_driver 未绑定 probe/remove，仅占位，便于后续与 platform_device 匹配。 |
| 409–414 | `aicbsp_pdev = platform_device_alloc("aic-bsp", -1);` 与 `platform_device_add(aicbsp_pdev);` | 分配并添加名为 "aic-bsp" 的 platform_device，使 sysfs 下出现对应设备节点，并为后续 sysfs 属性提供挂载点。 |
| 416–420 | `sysfs_create_group(&(aicbsp_pdev->dev.kobj), &aicbsp_attribute_group);` | 在 "aic-bsp" 设备下创建 sysfs 组 `aicbsp_info`，暴露 cpmode、hwinfo、fwdebug 等属性（见 364–378 行），用户可通过 echo/cat 读写。 |
| 422 | `mutex_init(&aicbsp_power_lock);` | 初始化电源/子系统互斥锁，保证 `aicbsp_set_subsys` 等上下电调用串行化。 |
| 423–425 | `#if defined CONFIG_PLATFORM_ROCKCHIP ... aicbsp_set_subsys(AIC_BLUETOOTH, AIC_PWR_ON); #endif` | 仅在 Rockchip 平台：对蓝牙子系统上电（AIC_PWR_ON），会触发平台电源与 SDIO 初始化，进而可能触发 FDRV 的 SDIO probe。 |
| 426 | `return 0;` | 初始化成功，返回 0；失败时前面某步已 return 负错误码。 |
- **rwnx_cfg80211_init** 是在**设备探测之后**才被调用的：SDIO 设备 probe → BSP 里 `aicbsp_sdio_init`、`aicbsp_driver_fw_init`（固件下载、START_APP）→ FDRV 里 `aicwf_rwnx_sdio_platform_init(sdiodev)` → `rwnx_platform_init(rwnx_plat, &drvdata)` → **rwnx_cfg80211_init(rwnx_plat, platform_data)**。
- 因此它是 **FDRV/FullMAC 的“驱动上下文”初始化入口**：在总线已探测、固件已加载并启动的前提下，创建 wiphy、挂上 cmd_mgr、注册 cfg80211 与 net_device 等；不是整个 WiFi 内核模块的“最初”入口。

### 7.2 主要文件与符号索引

| 功能 | 主要文件 | 关键符号/逻辑 |
|------|----------|----------------|
| BSP 入口与固件表 | aic_bsp_main.c | aicbsp_init, aicbsp_firmware_list, cpmode_show/store |
| 命令与固件下载 | aic_bsp_driver.c | rwnx_cmd_mgr_*, rwnx_send_msg, rwnx_load_firmware, aicwifi_init, aicbt_init |
| BSP 导出与电源 | aic_bsp_export.h, aicsdio.c | aicbsp_set_subsys, aicbsp_get_feature |
| SDIO 探测与读写 | aicsdio.c | aicbsp_sdio_init, aicwf_sdio_readb/writeb |
| FDRV 主控与 cfg80211 | rwnx_main.c | rwnx_cfg80211_ops, wiphy_new, rwnx_cfg80211_init |
| 消息发送 | rwnx_msg_tx.c | 各 rwnx_send_* 与 LMAC 消息 id |
| 消息接收与命令完成 | rwnx_msg_rx.c, aic_bsp_driver.c | rwnx_rx_handle_msg, cmd_mgr.msgind |
| 数据 TX/RX | rwnx_tx.c, rwnx_rx.c, aicwf_sdio.c | aicwf_bus_txdata, 收包解析与 netif_rx |
| IPC 描述符 | ipc_host.c, ipc_shared.h | nx_txdesc_cnt, 描述符布局 |
| 私有命令 | aic_priv_cmd.c | 字符串解析与 ioctl 分支 |
| Vendor | aic_vendor.c | OUI, vendor 子命令处理 |

以上内容覆盖了 LicheeRV-Nano-Build 中 AIC8800 WiFi 模块在内核侧的主要功能、互动关系与实现要点，以及移植到 StarryOS/wireless 时的复刻思路与顺序。实际移植时建议以“BSP 可独立启动固件”和“FDRV 可收发一包”为两个里程碑，再逐步补齐扫描、连接、AP 与上层 API。

### 7.3 为何 Rust 中三行“未完全实现”

`aicbsp_init` 里与 Linux 对应的三行在 StarryOS/wireless 中的处理如下：

| 原 C 代码 | 作用 | Rust 侧处理 |
|-----------|------|-------------|
| `aicbsp_resv_mem_init();` | 在 `CONFIG_RESV_MEM_SUPPORT` 下预分配 skb 池（如 `resv_skb`，1536×64 字节）供 TX 路径使用；无该配置时为空实现。 | **占位**：当前为无操作并返回 `Ok(())`。预留内存依赖内核的 `dev_alloc_skb` / skb 池与后续 `aicbsp_resv_mem_alloc_skb`；StarryOS 侧尚未提供等价的内存分配器与 skb 抽象，故仅保留接口，待平台具备分配能力后再实现真实预分配与释放。 |
| `sema_init(&aicbsp_probe_semaphore, 0);` | 将“SDIO probe 完成”信号量置为 0；probe 成功时 `up()`，`aicbsp_sdio_init` 里 `down_timeout(2000)` 等待。 | **已实现**：在 BSP 中提供 `ProbeSync`（基于原子与超时轮询），`aicbsp_init` 时重置为“未完成”；后续 SDIO probe 路径调用 `bsp::probe_signal()`，`aicbsp_sdio_init` 等价逻辑可调用 `bsp::probe_wait_timeout_ms(2000)`。 |
| `mutex_init(&aicbsp_power_lock);` | 初始化电源/子系统互斥锁，保护 `aicbsp_set_subsys` 中电源状态与上下电序列。 | **已实现**：使用 `spin::Mutex` 作为全局 `power_lock`；`aicbsp_init` 无需显式“初始化”（静态构造即可）。后续实现 `aicbsp_set_subsys` 时通过 `bsp::power_lock()` / `bsp::power_unlock()` 包裹电源变更路径即可。 |

### 7.4 aicbsp_init 完整调用链（上一步 / 下一步）

#### Linux 内核侧：谁调用 aicbsp_init、init 里做了什么、之后发生什么

**上一步（谁调用）：**

- 内核加载 BSP 模块时，由 **模块加载器** 调用：`do_init_module()` → 模块的 `.init`（即 `module_init(aicbsp_init)` 注册的 `aicbsp_init`）。
- 不由其他驱动或用户显式调用；BSP 模块通常先于 FDRV 模块加载（若分开编译为两个 ko）。

**aicbsp_init 内部顺序：**

| 顺序 | 代码 | 作用 |
|------|------|------|
| 1 | `aicbsp_info.cpmode = testmode` | 保存固件模式（0=正常 / 1=射频测试等）。 |
| 2 | `aicbsp_resv_mem_init()` | 预留内存：在 CONFIG_RESV_MEM_SUPPORT 下预分配 skb 池，否则空实现。 |
| 3 | `sema_init(&aicbsp_probe_semaphore, 0)` | “SDIO probe 完成”信号量置 0，供后面 `aicbsp_sdio_init` 用 `down_timeout` 等待。 |
| 4 | `platform_driver_register(&aicbsp_driver)` | 注册名为 "aic_bsp" 的 platform_driver（当前无 probe/remove，占位）。 |
| 5 | `platform_device_alloc` / `platform_device_add` | 创建 "aic-bsp" platform_device，供 sysfs 挂载。 |
| 6 | `sysfs_create_group(...)` | 在设备下创建 sysfs 组，暴露 cpmode、hwinfo、fwdebug 等。 |
| 7 | `mutex_init(&aicbsp_power_lock)` | 初始化电源/子系统互斥锁。 |
| 8 | `#if CONFIG_PLATFORM_ROCKCHIP` 下 `aicbsp_set_subsys(AIC_BLUETOOTH, AIC_PWR_ON)` | 仅 Rockchip：对蓝牙子系统上电，触发下面整条“上电 → SDIO → 固件”链。 |

**第 8 步展开（aicbsp_set_subsys 内部）：**

- `mutex_lock(&aicbsp_power_lock)`
- 若从“全关”变为“有子系统开”：  
  `aicbsp_platform_power_on()` → **aicbsp_sdio_init()** → **aicbsp_driver_fw_init(aicbsp_sdiodev)** → （可选）`aicbsp_sdio_release()`
- `mutex_unlock(&aicbsp_power_lock)`

其中：

- **aicbsp_sdio_init()**：`sdio_register_driver(&aicbsp_sdio_driver)`，然后 `down_timeout(&aicbsp_probe_semaphore, 2000)` 等待。  
  内核发现 SDIO 设备后调用 **aicbsp_sdio_probe** → 分配 `aic_sdio_dev`、chipmatch、func init、bus init、`aicbsp_platform_init(sdiodev)`（cmd_mgr 初始化）、**up(&aicbsp_probe_semaphore)** → `aicbsp_sdio_init` 返回。
- **aicbsp_driver_fw_init(sdiodev)**：读 chip rev、选固件表、可选 `aicbt_init(sdiodev)`、**aicwifi_init(sdiodev)**（固件下载、patch、start_from_bootrom 等），**不**调用 `rwnx_cfg80211_init`。

**下一步（aicbsp_init 返回之后）：**

- BSP 模块初始化完成；若为 Rockchip，此时已：平台上电、SDIO 已 probe、固件已加载并启动，但 **尚未** 创建 wiphy / 注册 cfg80211。
- 随后 **FDRV 模块** 加载：`module_init(rwnx_mod_init)`。
- **rwnx_mod_init**：  
  `aicbsp_set_subsys(AIC_WIFI, AIC_PWR_ON)`（若电源已由 BT 上电则可能只更新 power_map）、  
  `init_completion(&hostif_register_done)`、  
  **aicsmac_driver_register()** → `aicwf_sdio_register()`（注册 FDRV 的 sdio_driver 或直接调 `aicwf_sdio_probe_`）、  
  然后 `wait_for_completion_timeout(&hostif_register_done, ...)`。
- FDRV 的 SDIO probe（**aicwf_sdio_probe**）里：func init、bus init、**aicwf_rwnx_sdio_platform_init(sdiodev)** → **rwnx_platform_init** → **rwnx_cfg80211_init**（创建 wiphy、挂 cmd_mgr、注册 cfg80211 与 net_device），最后 **aicwf_hostif_ready()** → `complete(&hostif_register_done)`，使 `rwnx_mod_init` 的等待返回。

**小结（Linux）：**

- **上一步**：内核加载 BSP 模块，调用 `aicbsp_init`。
- **aicbsp_init 做了**：cpmode、预留内存、probe 信号量、platform 设备与 sysfs、power_lock、可选 Rockchip 上电并走完“平台上电 → aicbsp_sdio_init（等 probe）→ aicbsp_driver_fw_init（固件加载）”。
- **下一步**：FDRV 模块加载，`rwnx_mod_init` 里上电（或复用已上电）、注册 FDRV SDIO、在 FDRV probe 中完成 **rwnx_cfg80211_init**（wiphy/cfg80211），并通过 completion 通知 `rwnx_mod_init` 返回。

---

#### StarryOS / wireless 侧：当前入口与对应关系

**上一步（谁调用）：**

- **main.rs** 在 `main()` 里调用 `wireless::wireless_driver_init_stub()`；当前 **未** 调用 `bsp::aicbsp_init()`，即 BSP 的“模块初始化”尚未接入启动流程。

**当前 wireless 入口做了什么：**

- `wireless_driver_init_stub()`：只创建 **WirelessDriver&lt;WiphyOpsStub&gt;**（cmd_mgr + 占位 wiphy），对应 LicheeRV 里 **rwnx_cfg80211_init** 创建 rwnx_hw/wiphy 的那一步；**不**做 SDIO 探测、不做固件加载、也不做 BSP 的 platform/sysfs/power_lock 等。

**若接入 aicbsp_init，建议顺序：**

1. **在 StarryOS 启动链中调用 `bsp::aicbsp_init()`**（在调用 `wireless_driver_init_stub()` 或真实 FDRV 初始化之前）：  
   传入 `&mut AicBspInfo`、`testmode`；内部会：设置 cpmode、预留内存占位、`probe_reset()`、power_lock 已就绪。不抽象平台，按 Linux 流程；上电、SDIO 注册/探测、固件加载由上层在 Linux 上按顺序调用。
2. **下一步（BSP 之后）**：  
   - 若有平台 SDIO 与上电：实现并调用 **aicbsp_set_subsys** 等价逻辑（用 `bsp::power_lock()` 包裹），内部：平台上电 → **aicbsp_sdio_init** 等价（注册/发现 SDIO，并 `bsp::probe_wait_timeout_ms(2000)`）→ **aicbsp_driver_fw_init** 等价（chip rev、固件下载、START_APP）。  
   - 再下一步：在“SDIO 已 probe、固件已启动”的前提下，做 FDRV 侧初始化：创建 **WirelessDriver&lt;RealWiphyImpl&gt;** 或当前 stub，即 **rwnx_cfg80211_init** 的 Rust 对应（wiphy + cmd_mgr 注册）。

**表格：StarryOS 当前 vs 建议下一步**

| 阶段 | Linux 对应 | StarryOS 当前 | 建议下一步 |
|------|------------|---------------|-------------|
| 谁调 BSP init | 内核加载 BSP 模块 → aicbsp_init | 未调用 | 在 main/启动链中先调 `bsp::aicbsp_init()` |
| BSP init 内 | cpmode、resv_mem、sema_init、platform、sysfs、mutex、可选 set_subsys(BT) | aicbsp_init 只做 cpmode、probe_reset、power_lock | 上电、SDIO、固件加载由上层在 Linux 上按顺序调用；无 PlatformOps 抽象 |
| 上电 + SDIO | aicbsp_set_subsys → sdio_init → 等 probe → driver_fw_init | 无 | 实现 aicbsp_set_subsys 等价：power_lock、平台上电、SDIO 注册/发现、probe_wait_timeout_ms、固件下载与 START_APP |
| FDRV / wiphy | rwnx_mod_init → aicwf_sdio_register → probe → rwnx_cfg80211_init | wireless_driver_init_stub() 只建 WirelessDriver | 在“SDIO probe + 固件就绪”之后，再调当前 stub 或 RealWiphyImpl，完成“rwnx_cfg80211_init”等价 |

### 7.5 同步原语、platform 设备与“蓝牙上电”的说明

#### 1. sema_init 与 mutex_init：wireless 是否只用一个 spin::Mutex？是否对齐 LicheeRV？

**结论：wireless 中实现了两个独立的同步原语，语义与 LicheeRV 对齐；并非“只用一个 spin::Mutex”。**

| LicheeRV 原语 | 作用 | wireless 对应实现 |
|---------------|------|------------------|
| **mutex aicbsp_power_lock** | 互斥锁，保护 `aicbsp_set_subsys` 中电源状态与上下电序列（同一时刻只允许一个上下电流程）。 | **spin::Mutex**：`bsp::sync::power_lock()` 返回 `MutexGuard`，析构即释放；对应 `mutex_lock` / `mutex_unlock`。 |
| **semaphore aicbsp_probe_semaphore** | 信号量（初值 0）：probe 成功时 `up()`，`aicbsp_sdio_init` 里 `down_timeout(2000)` 等待“SDIO probe 完成”。 | **AtomicBool + 轮询**：`probe_reset()`（sema_init(0)）、`probe_signal()`（up）、`probe_wait_timeout_ms(ms)`（down_timeout）。no_std 下无内核 semaphore，用原子“完成标志 + 超时轮询”实现同一语义。 |

因此：

- **mutex**：一对一用 spin::Mutex 实现，与 LicheeRV 一致。
- **semaphore**：语义一致（“probe 完成”事件 + 超时等待），实现上用 AtomicBool + 忙等替代内核 semaphore，在无调度器的 no_std 环境下是常见做法。

若希望“名字上更对称”，可在文档或注释中明确：**power_lock = Mutex，probe_semaphore = 基于原子的“probe 完成”信号**，二者在 wireless 中均存在且与 LicheeRV 行为对齐。

---

#### 2. platform_driver_register、platform_device_alloc/add、sysfs 的功能与 wireless 为何未实现、是否有必要

**Linux 侧功能与作用：**

| API | 功能与作用 |
|-----|------------|
| **platform_driver_register(&aicbsp_driver)** | 向内核 **platform 总线** 注册一个 **platform_driver**。当前 `aicbsp_driver` 仅包含 `.name = "aic_bsp"`，**未绑定 probe/remove**（注释掉），因此只是“占位”：让内核知道存在名为 "aic_bsp" 的驱动，便于与同名 platform_device 匹配；实际探测与移除由 BSP 的 **SDIO 驱动**（aicbsp_sdio_driver）完成，不依赖该 platform_driver 的 probe。 |
| **platform_device_alloc("aic-bsp", -1)** | 在内存中**分配**一个 **platform_device** 结构，设备名为 `"aic-bsp"`，ID 为 -1（不指定具体 ID）。 |
| **platform_device_add(aicbsp_pdev)** | 将上述 platform_device **加入** platform 总线，使设备出现在 sysfs 中（例如 `/sys/devices/platform/aic-bsp/`），并触发与同名 platform_driver 的匹配（此处无 probe 回调，匹配后无实际操作）。 |
| **sysfs_create_group(&(aicbsp_pdev->dev.kobj), &aicbsp_attribute_group)** | 在该设备的 kobject 下**创建属性组** `aicbsp_attribute_group`（名 `"aicbsp_info"`），组内属性包括 **cpmode**、**hwinfo**、**fwdebug**（见 aic_bsp_main.c 364–378 行）。用户空间可通过 `cat/echo` 读写，例如：`/sys/devices/platform/aic-bsp/aicbsp_info/cpmode`、`hwinfo`、`fwdebug`，用于查看/修改固件模式、硬件信息、调试开关。 |

**总结 Linux 侧目的：**

- platform_driver + platform_device：为 BSP 提供一个**逻辑上的“设备节点”**，主要用途是**给 sysfs 一个挂载点**。
- sysfs 属性组：**暴露 BSP 状态与配置给用户空间**（cpmode、hwinfo、fwdebug），便于调试与配置，不参与驱动核心逻辑（SDIO 探测、固件加载仍由 aicbsp_sdio_* 完成）。

**wireless 中为何没有对应实现：**

- StarryOS/wireless 没有 Linux 的 **platform 总线、设备模型和 sysfs**，也没有 `struct device`/kobject 等概念，因此**无法**也不必要原样实现 `platform_driver_register`、`platform_device_alloc`、`platform_device_add`、`sysfs_create_group`。
- BSP 核心逻辑（cpmode、预留内存、probe 信号量、power_lock、后续 SDIO/固件流程）已在 wireless 中以 `aicbsp_init`、`sync`、平台抽象等实现；**缺少的只是“把 cpmode/hwinfo/fwdebug 暴露给用户空间”的那一层**。

**是否有必要在 wireless 中实现“等价物”：**

- **仅做驱动内部初始化、不对外暴露配置/调试接口**：不必实现 platform 设备或 sysfs；当前做法（AicBspInfo 等结构体内保存 cpmode、hwinfo、fwdebug，仅在驱动内使用）即可。
- **若需要“类似 sysfs 的配置/调试接口”**（例如：用户态改 cpmode、读 hwinfo、开关 fwdebug），则需要在 StarryOS 上用**本系统已有机制**实现，例如：  
  - 在现有 syscall/API 中增加“无线 BSP 属性”的读写接口；或  
  - 在虚拟文件系统（如 axfs）下提供类似 `/sys/aicbsp_info/cpmode` 的节点并由驱动提供 read/write。  
  这属于**平台/OS 的“设备/配置暴露”设计**，与“是否实现 platform_driver/device”无直接对应关系；在 wireless 里**不必**复刻 platform 子系统的名字与结构，只要能在需要时提供等价的数据与接口即可。

**不实现 platform_driver/device 时，芯片与固件如何管理？**

在 LicheeRV 里，**芯片与固件的管理并不依赖 platform_driver 或 platform_device**，二者只负责给 sysfs 提供挂载点；真正的管理路径如下。

- **芯片管理**（与 platform 无关）：  
  1. **发现设备**：由 **SDIO 驱动**完成。`aicbsp_sdio_init()` 里 `sdio_register_driver(&aicbsp_sdio_driver)`，内核发现 AIC SDIO 设备后调用 **aicbsp_sdio_probe**，在其中分配 `aic_sdio_dev`、做 chipmatch(vid/did) 得到 **chipid**（如 PRODUCT_ID_AIC8800DC），并赋值给全局 `aicbsp_sdiodev`。  
  2. **芯片版本与状态**：在 **aicbsp_driver_fw_init(aicbsp_sdiodev)** 里通过 `rwnx_send_dbg_mem_read_req` 读芯片寄存器得到 **chip_rev** 等，写入 **aicbsp_info**（hwinfo、chip_rev 等）。  
  因此：芯片的“是谁、什么版本”由 **SDIO probe + aicbsp_driver_fw_init** 管理，数据放在 **aicbsp_sdiodev** 与 **aicbsp_info** 中；platform_device 不参与。

- **固件管理**（与 platform 无关）：  
  1. **固件表选择**：在 **aicbsp_driver_fw_init** 里根据 **chipid + chip_rev** 选择 **aicbsp_firmware_list**（如 fw_u02、fw_8800dc_u02）。  
  2. **模式选择**：**aicbsp_info.cpmode**（在 aicbsp_init 里由 testmode 设置）决定用固件表中的哪一项（0=正常，1=射频测试等）。  
  3. **加载与启动**：**aicwifi_init(sdiodev)**（被 aicbsp_driver_fw_init 调用）按当前 firmware_list[cpmode] 做固件上传（rwnx_plat_bin_fw_upload_android 等）、patch、**start_from_bootrom**。  
  因此：固件的“用哪张表、哪种模式、如何加载”由 **aicbsp_info + aicbsp_firmware_list + aicwifi_init** 管理；platform_device 仅通过 sysfs 暴露 cpmode/hwinfo/fwdebug 给用户空间，**不参与**固件选择与下载逻辑。

在 **wireless 中的等价管理方式**（无需 platform）：

| 管理内容 | LicheeRV 依赖 | wireless 等价 |
|----------|----------------|---------------|
| **芯片：发现与 ID** | SDIO probe → aic_sdio_dev、chipmatch → aicbsp_sdiodev | 在“SDIO 初始化”等价路径中：发现/创建设备对象（如 SdioState），chipmatch 得到 ProductId，保存到平台自己的“sdiodev”或 BSP 可见结构。 |
| **芯片：版本与状态** | aicbsp_driver_fw_init 里读 chip_rev 等 → aicbsp_info | 在 **aicbsp_driver_fw_init** 等价函数里：通过 BSP 命令/寄存器读 chip_rev，写入 **AicBspInfo**（chip_rev、hwinfo 等）。 |
| **固件：表与模式** | aicbsp_firmware_list + aicbsp_info.cpmode | 使用 **bsp::firmware** 中已有固件表（FW_U02、FW_8800DC_U02 等），用 **AicBspInfo.cpmode** 选模式；与 LicheeRV 一致。 |
| **固件：加载与启动** | aicwifi_init → 固件上传、patch、start_from_bootrom | 使用 **bsp::fw_load**（fw_upload_blocks、fw_start_app）和后续的 patch/start 等价逻辑，入参由 AicBspInfo + 固件表 + 芯片 ID 决定。 |

结论：**不实现 platform_driver_register、platform_device_alloc/add 时，芯片与固件仍然可以完整管理**——芯片由“SDIO 发现 + aicbsp_driver_fw_init 等价”和 **AicBspInfo**（及平台侧的“sdiodev”等价）管理，固件由 **AicBspInfo.cpmode**、**bsp::firmware** 的固件表、以及 **bsp::fw_load** 的上传/启动接口管理。platform 只负责“把 cpmode/hwinfo/fwdebug 挂到 sysfs”；这一层在 wireless 中可用其他方式替代或省略，**不影响**芯片与固件的管理逻辑。

---

#### 3. aicbsp_set_subsys(AIC_BLUETOOTH, AIC_PWR_ON) 与“直接做 SDIO 初始化”的等价关系

**LicheeRV 中该调用的实际效果：**

- 在 Rockchip 平台上，`aicbsp_set_subsys(AIC_BLUETOOTH, AIC_PWR_ON)` 被用来**触发第一次上电**；其内部逻辑与“哪个子系统”（BT/WiFi）无关，只关心 **power_map 从 0 变为非 0**，从而执行：
  1. `aicbsp_platform_power_on()`
  2. `aicbsp_sdio_init()`（注册 BSP SDIO 驱动，等待 probe）
  3. `aicbsp_driver_fw_init(aicbsp_sdiodev)`（chip rev、固件下载、START_APP 等）
  4. （可选）`aicbsp_sdio_release()`

也就是说，**“蓝牙子系统上电”在这里等价于“把 SDIO + 固件这一整条链路拉起来”**；BT 只是触发入口，真正执行的是平台上电 → SDIO 探测 → 固件加载。

**在 wireless 中的替换方式：**

- 若目标平台**没有**独立的 BT/WiFi 电源域区分，或**只做 WiFi**，则**不需要**保留“蓝牙子系统”这一抽象；可以把 LicheeRV 里 `aicbsp_set_subsys(AIC_BLUETOOTH, AIC_PWR_ON)` 的**程序逻辑**直接替换为**同一套 SDIO 初始化序列**：
  1. 平台上电（Linux 上直接调用 aicbsp_platform_power_on 或等价）
  2. SDIO 初始化：注册/发现 SDIO 设备，并 `bsp::probe_wait_timeout_ms(2000)` 等待 probe 完成
  3. `aicbsp_driver_fw_init` 等价：chip rev、固件下载、START_APP
  4. （可选）释放/释放 host 等

因此：**“蓝牙系统上电”这段程序逻辑，在 wireless 中可以直接替换为 SDIO 相关初始化程序**；已去掉 PlatformOps 抽象，上电、SDIO 注册/探测、固件加载由上层在 Linux 上按顺序调用（aicbsp_platform_power_on → aicbsp_sdio_init → aicbsp_driver_fw_init）。

---

### 7.6 POWER_LOCK 与 PROBE_SIGNAL 用法说明

#### 1. POWER_LOCK（电源/子系统互斥锁）

**对应 LicheeRV**：`struct mutex aicbsp_power_lock`（aic_bsp_main.c 237 行），`mutex_init` 在 aicbsp_init 内（422 行），`mutex_lock`/`mutex_unlock` 在 aicbsp_set_subsys 内（aicsdio.c 164、211、224 行）。

**作用**：保证**同一时刻只有一个“上下电/电源状态变更”流程**在执行。`aicbsp_set_subsys` 会修改 power_map、调用平台上电/下电、sdio_init/driver_fw_init，若不加锁，多线程或多次调用可能并发执行，导致状态错乱或重复初始化。

**wireless 中的实现与用法**：

- **定义**：`wireless/driver/bsp/src/sync.rs` 第 9 行，`static POWER_LOCK: spin::Mutex<()> = spin::Mutex::new(())`。
- **获取锁**：调用 `bsp::power_lock()`，返回 `spin::MutexGuard<'static, ()>`；**析构时自动释放**（对应 `mutex_unlock`），无需显式 unlock。
- **典型用法**：在“SDIO 流程”等价逻辑（即 aicbsp_set_subsys 等价）入口处获取锁，在整段“上电 → sdio_init → driver_fw_init”或“下电”完成后由 guard 析构释放，例如：
  ```rust
  let _guard = bsp::power_lock();
  // 平台上电 → sdio_init（内层 probe_wait_timeout_ms）→ driver_fw_init
  // 或：下电、sdio_exit、platform_power_off
  ```
  不要在同一线程内重复 `power_lock()` 而不释放（spin::Mutex 非递归，会死锁）。

**谁调用**：实现“aicbsp_set_subsys”等价的上层（例如 Linux 上按顺序调用平台上电、aicbsp_sdio_init、aicbsp_driver_fw_init 的那段代码）在进入该序列前调用 `power_lock()`，序列结束由 guard 析构释放。

---

#### 2. PROBE_SIGNAL（SDIO probe 完成信号）

**对应 LicheeRV**：`struct semaphore aicbsp_probe_semaphore`（aic_bsp_main.c 20 行），`sema_init(..., 0)` 在 aicbsp_init 内（401 行）；`up(&aicbsp_probe_semaphore)` 在 **aicbsp_sdio_probe** 成功路径末尾（aicsdio.c 353 行）；`down_timeout(..., 2000)` 在 **aicbsp_sdio_init** 内（aicsdio.c 596 行）。

**作用**：**同步“SDIO 驱动注册”与“SDIO 设备 probe 完成”**。流程是：  
1）aicbsp_sdio_init 里先 `sdio_register_driver`，然后 `down_timeout(probe_semaphore, 2000)` 阻塞等待；  
2）内核发现 AIC SDIO 设备后调用 aicbsp_sdio_probe，probe 里做完 chipmatch、func/bus init、cmd_mgr 初始化后 `up(probe_semaphore)`；  
3）aicbsp_sdio_init 的 down_timeout 返回，继续执行，此时 aicbsp_sdiodev 已有效，可调用 aicbsp_driver_fw_init(aicbsp_sdiodev)。  
即“等待 probe 完成，最多 2 秒”，避免在 probe 未完成时就去做固件加载。

**wireless 中的实现与用法**：

- **定义**：`wireless/driver/bsp/src/sync.rs` 第 13 行，`static PROBE_SIGNAL: AtomicBool = AtomicBool::new(false)`。
- **重置（sema_init(0)）**：`bsp::probe_reset()`，在 **aicbsp_init** 内调用（lib.rs 76 行），保证每次 BSP init 后“probe 完成”状态清零，后续 sdio_init 等价逻辑会再次等待。
- **通知“probe 已完成”（up）**：`bsp::probe_signal()`。**调用方**：在“SDIO probe”等价路径成功完成时（即已创建设备、chipmatch、func/bus init、cmd_mgr 初始化后）调用一次。
- **等待“probe 已完成”（down_timeout(2000)）**：`bsp::probe_wait_timeout_ms(2000)`，返回 `Ok(())` 表示在超时内收到信号，`Err(())` 表示超时。**调用方**：在“aicbsp_sdio_init”等价逻辑里，在“注册/启动 SDIO 发现”之后调用，等待 probe 完成再继续做 aicbsp_driver_fw_init。

**时序小结**：

1. aicbsp_init → `probe_reset()`，PROBE_SIGNAL = false。  
2. 上层执行“sdio_init”等价：启动 SDIO 注册/发现，然后 `probe_wait_timeout_ms(2000)` 等待。  
3. “SDIO probe”等价路径（由 OS/总线在发现设备时调用）成功完成后调用 `probe_signal()`，PROBE_SIGNAL = true。  
4. `probe_wait_timeout_ms` 返回 Ok(())，上层继续执行 driver_fw_init 等价。

---

### 7.7 SDIO 流程相关函数位置

#### LicheeRV-Nano-Build（C 代码）

| 步骤 | 函数 / 符号 | 文件路径 | 行号（约） | 说明 |
|------|--------------|----------|------------|------|
| 电源锁 | `aicbsp_power_lock` 定义 | aic8800_bsp/aic_bsp_main.c | 237 | mutex 变量 |
| 电源锁 | `mutex_init(&aicbsp_power_lock)` | aic8800_bsp/aic_bsp_main.c | 422 | aicbsp_init 内 |
| 电源锁 | `mutex_lock` / `mutex_unlock` | aic8800_bsp/aicsdio.c | 164, 211, 224 | aicbsp_set_subsys 内 |
| probe 信号 | `aicbsp_probe_semaphore` 声明 | aic_bsp_main.c | 20 | semaphore 变量 |
| probe 信号 | `sema_init(..., 0)` | aic_bsp_main.c | 401 | aicbsp_init 内 |
| probe 信号 | `up(&aicbsp_probe_semaphore)` | aic8800_bsp/aicsdio.c | 353 | aicbsp_sdio_probe 成功路径末尾 |
| probe 信号 | `down_timeout(..., 2000)` | aic8800_bsp/aicsdio.c | 596 | aicbsp_sdio_init 内 |
| 1. 平台上电 | `aicbsp_platform_power_on` | aic8800_bsp/aicsdio.c | 487 | 静态函数，平台相关上电 |
| 1. 上电调用 | 在 aicbsp_set_subsys 内 | aicsdio.c | 180 | cur_power_state 从 0→1 时 |
| 2. SDIO 初始化 | `aicbsp_sdio_init` | aic8800_bsp/aicsdio.c | 588–603 | 注册 sdio_driver，down_timeout 等 probe |
| 2. SDIO probe | `aicbsp_sdio_probe` | aic8800_bsp/aicsdio.c | 267–362 | 分配 sdiodev、chipmatch、func/bus init、up(probe_semaphore) |
| 3. 固件/芯片初始化 | `aicbsp_driver_fw_init` | aic8800_bsp/aic_bsp_driver.c | 2002–2118 | chip rev、固件表、aicwifi_init |
| 3. 调用 | 在 aicbsp_set_subsys 内 | aicsdio.c | 184 | aicbsp_sdio_init 返回成功后 |
| 可选 release | `aicbsp_sdio_release` | aic8800_bsp/aicsdio.c | 610–... | 释放 host 等 |
| 下电/退出 | `aicbsp_sdio_exit` | aic8800_bsp/aicsdio.c | 605–608 | sdio_unregister_driver |

**头文件 / 导出**：  
- `aicbsp_sdio_init` 声明：aic8800_bsp/aicsdio.h 约 135 行。  
- `aicbsp_driver_fw_init` 声明：aic8800_bsp/aic_bsp_driver.h 约 335 行。  
- `aicbsp_power_lock` 导出：aic_bsp_driver.h 约 586 行。

#### StarryOS wireless（Rust）

| 步骤 | 函数 / 符号 | 文件路径 | 行号（约） | 说明 |
|------|--------------|----------|------------|------|
| 电源锁 | `POWER_LOCK`、`power_lock()` | wireless/driver/bsp/src/sync.rs | 9, 45–47 | 获取锁，guard 析构即释放 |
| probe 信号 | `PROBE_SIGNAL`、`probe_reset` | wireless/driver/bsp/src/sync.rs | 13, 17–19 | aicbsp_init 时重置 |
| probe 信号 | `probe_signal` | wireless/driver/bsp/src/sync.rs | 23–25 | “SDIO probe”成功路径末尾调用 |
| probe 信号 | `probe_wait_timeout_ms` | wireless/driver/bsp/src/sync.rs | 28–40 | “aicbsp_sdio_init”等价里等待 2000ms |
| 导出 | power_lock, probe_reset, probe_signal, probe_wait_timeout_ms | wireless/driver/bsp/src/lib.rs | 35 | pub use sync::... |
| BSP init | `aicbsp_init`（内调 probe_reset） | wireless/driver/bsp/src/lib.rs | 71–77 | 无 platform 参数，只做 cpmode、resv_mem、probe_reset |
| 固件/芯片初始化 | `aicbsp_driver_fw_init` | wireless/driver/bsp/src/sdio.rs | 见该文件 | chip rev、固件表、fw_upload/START_APP 占位 |
| 六函数导出 | aicbsp_platform_power_on 等 | wireless/driver/bsp/src/lib.rs | pub use sdio::... | 见 7.8 节 SDIO 后续流程 |

**说明**：  
- “平台上电”“SDIO 注册/发现”“SDIO probe 回调”在 wireless 中**未实现**，由上层在 Linux 上按顺序调用等价接口；probe 成功时由上层调用 `bsp::probe_signal()` 或 `bsp::aicbsp_sdio_probe(product_id)`，sdio_init 等价里调用 `bsp::aicbsp_sdio_init()`（内层 `probe_wait_timeout_ms(2000)`）。  
- 固件上传、START_APP 的 BSP 侧接口在 `wireless/driver/bsp/src/fw_load.rs`（如 `fw_upload_blocks`、`fw_start_app`），供 aicbsp_driver_fw_init 等价逻辑调用。

---

### 7.8 SDIO 后续流程（BSP SDIO 完成之后做什么）

BSP 侧 SDIO 流程（平台上电 → sdio_init → probe 等待 → driver_fw_init → 可选 sdio_release）完成后，**尚未**创建 wiphy、注册 cfg80211 与 net_device；这些在 **FDRV** 侧完成。整体顺序如下。

#### Linux（LicheeRV）侧

| 顺序 | 阶段 | 调用链 / 动作 | 位置（约） |
|------|------|----------------|------------|
| 1 | BSP SDIO 流程结束 | aicbsp_set_subsys 内：driver_fw_init 成功、可选 aicbsp_sdio_release → mutex_unlock | aicsdio.c 184–187, 211 |
| 2 | FDRV 模块加载 | 内核加载 aic8800_fdrv.ko → `module_init(rwnx_mod_init)` | rwnx_main.c 6278 |
| 3 | rwnx_mod_init 入口 | `aicbsp_set_subsys(AIC_WIFI, AIC_PWR_ON)`（若已上电则只更新 power_map）、`init_completion(&hostif_register_done)`、**aicsmac_driver_register()** | rwnx_main.c 6211–6227 |
| 4 | 注册 FDRV SDIO | **aicwf_sdio_register()**：`sdio_register_driver(&aicwf_sdio_driver)` 或直接 `aicwf_sdio_probe_(get_sdio_func(), NULL)` | rwnx_main.c 6180–6181, aicwf_sdio.c 1229–1235 |
| 5 | FDRV SDIO probe | 内核发现同一 AIC SDIO 设备后调用 **aicwf_sdio_probe**：func init、bus init、**aicwf_rwnx_sdio_platform_init(sdiodev)** | aicwf_sdio.c 762 起, 841 |
| 6 | 平台与 cfg80211 初始化 | **rwnx_platform_init(rwnx_plat, &drvdata)** → **rwnx_cfg80211_init(rwnx_plat, platform_data)**：创建 rwnx_hw、wiphy、挂 cmd_mgr、设置 bands、注册 wiphy 与 net_device | sdio_host.c 133, rwnx_platform.c 3393, rwnx_main.c 5666 |
| 7 | hostif 就绪 | **aicwf_hostif_ready()** → `complete(&hostif_register_done)`，rwnx_mod_init 内 `wait_for_completion_timeout` 返回 | rwnx_main.c 6199–6203, 6229 |
| 8 | 用户空间可用 | wlan0 等接口出现，iw/ wpa_supplicant 等可扫描、连接 | — |

**小结**：SDIO 后续流程 = **FDRV 模块加载** → **注册 FDRV SDIO 驱动** → **FDRV probe**（同一 SDIO 设备）→ **rwnx_platform_init → rwnx_cfg80211_init**（wiphy + cfg80211 + net_device）→ **hostif_ready**，之后用户空间可使用 WiFi 接口。

#### StarryOS / wireless 侧

| 顺序 | 阶段 | 建议动作 | wireless 中对应 |
|------|------|----------|-----------------|
| 1 | BSP SDIO 流程结束 | aicbsp_driver_fw_init 成功、可选 aicbsp_sdio_release、power_lock guard 析构 | bsp::aicbsp_driver_fw_init、bsp::aicbsp_sdio_release |
| 2 | FDRV 侧“驱动上下文”创建 | 在“SDIO 已 probe、固件已启动”的前提下，创建 **WirelessDriver**（cmd_mgr + wiphy），即 **rwnx_cfg80211_init** 的等价 | **wireless::wireless_driver_init_stub()** 或 **WirelessDriver::new(RealWiphyImpl)** |
| 3 | 注册 wiphy / net_device | 若 OS 有 cfg80211/net_device 抽象，则注册 wiphy、创建设备节点（如 wlan0） | 待平台实现 |
| 4 | 用户空间可用 | 通过 syscall 或 socket 提供 scan、connect、start_ap 等 | 待 api/syscall 与上层对接 |

**小结**：SDIO 后续流程在 wireless 中 = **在 BSP SDIO 流程完成后**，调用 **wireless_driver_init_stub()** 或创建 **WirelessDriver&lt;RealWiphyImpl&gt;**，完成“rwnx_cfg80211_init”等价（cmd_mgr + wiphy 注册）；再结合平台注册网口与 syscall，即可对外提供 WiFi 能力。

---

### 7.9 aicbsp_power_on 流程与 LicheeRV 对齐说明

#### LicheeRV 中 aicbsp_platform_power_on 的完整逻辑（aicsdio.c 487–556）

1. **平台相关 GPIO/电源**（按 CONFIG 分支）  
   - **CONFIG_PLATFORM_ALLWINNER**：先取 `aicbsp_bus_index`；之后（在 reg_sdio_notify 之后）执行：`sunxi_wlan_set_power(0)` → `mdelay(50)` → `sunxi_wlan_set_power(1)` → `mdelay(50)` → `sunxi_mmc_rescan_card(aicbsp_bus_index)`。  
   - **CONFIG_PLATFORM_ROCKCHIP2**：`rockchip_wifi_power(0)` → `mdelay(50)` → `rockchip_wifi_power(1)` → `mdelay(50)` → `rockchip_wifi_set_carddetect(1)`。  
   - **CONFIG_PLATFORM_AMLOGIC**：`extern_wifi_set_enable(0)` → `mdelay(200)` → `extern_wifi_set_enable(1)` → `mdelay(200)` → `sdio_reinit()` 等。

2. **卡检测与等待**  
   - `sema_init(&aic_chipup_sem, 0)`，`aicbsp_reg_sdio_notify(&aic_chipup_sem)`：注册 **dummy** SDIO 驱动，probe 时 `up(&aic_chipup_sem)`。  
   - 部分平台在此时再做一次电源序列 + MMC rescan / set_carddetect。  
   - `down_timeout(&aic_chipup_sem, msecs_to_jiffies(2000))`：等待最多 2 秒“卡被检测到”。  
   - 若 2 秒内收到信号：`aicbsp_unreg_sdio_notify()`，若 `aicbsp_load_fw_in_fdrv` 则 return -1，否则 return 0。  
   - 若超时：`aicbsp_unreg_sdio_notify()`，平台下电，return -1。

因此 LicheeRV 的 **aicbsp_platform_power_on** 同时完成：**平台电源/GPIO 序列** + **注册 dummy 驱动并等待“卡就绪”**；真正的 BSP SDIO 驱动注册与 **aicbsp_sdio_probe** 的等待在 **aicbsp_sdio_init** 里完成。

#### StarryOS wireless 侧对齐方式

| 项目 | LicheeRV | StarryOS（wireless/driver/bsp） |
|------|----------|----------------------------------|
| 电源序列 | power(0) → mdelay(50) → power(1) → mdelay(50) | **gpio.rs** `power_on()`：power 拉低 → `sync::delay_spin_ms(50)` → power 拉高 → `delay_spin_ms(50)` |
| 复位序列 | 平台各异，aicsdio.c 内无统一 reset 时序 | **gpio.rs** `reset()`：reset 拉低 → `delay_spin_ms(10)` → reset 拉高 → `delay_spin_ms(50)` |
| 延时原语 | `mdelay(ms)` | **sync.rs** `delay_spin_ms(ms)` + 常量 `LOOPS_PER_MS`（无时钟时为启发式近似） |
| 卡检测/MMC rescan | dummy 驱动 + down_timeout(2000) | 无 MMC 栈；**sdio.rs** `aicbsp_power_on()` 在 GPIO 序列后增加 **delay_spin_ms(50)**，模拟“上电/总线稳定”再返回，调用方随后执行 aicbsp_sdio_init → probe_wait |
| 整段上电入口 | aicbsp_set_subsys 内：platform_power_on → sdio_init → driver_fw_init | **lib.rs** `aicbsp_init()` 内持 **power_lock**，依次：**aicbsp_power_on()** → **aicbsp_sdio_init()** → **aicbsp_driver_fw_init(info)** |

**结论**：`aicbsp_power_on` 整段流程已与 LicheeRV 对齐为：**GPIO 电源/复位序列 + 固定延时**；LicheeRV 中“卡检测/ down_timeout”在 StarryOS 中用“上电后 50ms 固定延时 + 后续 aicbsp_sdio_init 内 probe_wait_timeout_ms(2000)”等效处理。

---

### 7.10 如何验证芯片上电成功与 GPIO 设置正确

#### 1. 软件读回（推荐首选）

- **自动验证**：`aicbsp_power_on()` 在上电+复位+50ms 延时后会调用 `WifiGpioControl::verify_after_power_on()`，从 GPIO 控制器**读回**当前 POWER_EN 与 RESET 引脚电平并打日志。
- **日志含义**：  
  - `WiFi GPIO 验证: POWER_EN=true RESET=true => OK`：软件侧 GPIO 输出与读回一致，**引脚配置与驱动设置正确**。  
  - `POWER_EN=false` 或 `RESET=false => FAIL`：读回与预期不符，可能原因：**引脚号/控制器错误**（见 gpio.rs `wifi_pins`）、**该 GPIO 被其他外设占用**、**硬件未接或反接**。
- **手动读回**：若需在其它时机检查，可创建 `WifiGpioControl` 后调用 `readback_state()` 得到 `(power_high, reset_high)`；上电成功后应为 `(true, true)`。

**代码位置**：`wireless/driver/bsp/src/gpio.rs` — `readback_state()`、`verify_after_power_on()`；`sdio.rs` — `aicbsp_power_on()` 末尾调用 `verify_after_power_on()`。

#### 2. 硬件测量（确认实际电平）

- **万用表**：上电流程跑完后，测 **WIFI_POWER_EN**、**WIFI_RESET** 对应物理引脚（见板级原理图或「05_SG2002_QFN_38 Board GPIO List」）对地电压；应为高电平（通常 1.8V/3.3V，视 IO 域而定）。
- **示波器**：可观察上电顺序与延时是否与代码一致（power 低→50ms→高→50ms，reset 低→10ms→高→50ms）。
- **引脚对应**：当前代码中 `wifi_pins` 为 **GPIO1_4**（POWER_EN）、**GPIO1_5**（RESET）（CV1835 参考）；若板子为 SG2002 Nano W，需按实际原理图核对并修改 `gpio.rs` 中 `WIFI_POWER_EN` / `WIFI_RESET`。

#### 3. SDIO 总线是否就绪（间接验证芯片上电）

- 若后续 **SDIO 主机**已接好且驱动能访问总线，可在 **aicbsp_sdio_init** 或 FDRV probe 中尝试 **SDIO 读（如 CCCR 等）**；能正常读到预期值说明电源与复位已生效、芯片已响应总线。
- 当前 BSP 侧尚未接真实 SDIO 主机，此步需在平台 SDIO 驱动就绪后使用。

#### 4. 小结

| 方法 | 作用 | 说明 |
|------|------|------|
| 软件读回 | 确认 GPIO 方向与输出值被正确写入并读回 | 看日志 `WiFi GPIO 验证: ... => OK/FAIL`；FAIL 时检查引脚定义与硬件 |
| 硬件测量 | 确认物理引脚电平与时序 | 万用表/示波器测 POWER_EN、RESET 对应引脚 |
| SDIO 读 | 确认芯片已上电并响应总线 | 需 SDIO 主机与驱动就绪后做 |

#### 5. 在 StarryOS main 中执行 GPIO 测试

- **入口**：`StarryOS/src/main.rs` 在 `starry_api::init()` 之后调用 **`wireless::bsp::aicbsp_init_gpio_test()`**，仅执行上电 + 读回验证（不执行 aicbsp_sdio_init / aicbsp_driver_fw_init，避免 2 秒 probe 超时）。
- **流程**：`aicbsp_init_gpio_test()` 内部：`probe_reset()` → `power_lock()` → `aicbsp_power_on()`（含 GPIO 序列与 `verify_after_power_on()` 打日志）。
- **日志**：启动后查看串口/控制台，应出现 `Run WiFi BSP GPIO test...`、`WiFi GPIO 验证: POWER_EN=... RESET=... => OK/FAIL`、`WiFi BSP GPIO test done`。若为 `=> OK` 表示 GPIO 设置正确。
- **配置**：GPIO 与 BSP 使用 `log::info!` / `log::debug!`，需保证 axlog 或当前 log 级别至少为 **info**（默认通常已包含），否则可能看不到 `WiFi GPIO 验证` 行。若需更多调试，可临时改为 `log::info!` 或提高 log 级别。
- **完整 BSP 初始化**：当 SDIO 主机与 probe 就绪后，可在 main 中改为调用 `wireless::bsp::aicbsp_init(&mut bsp_info, wireless::bsp::AicBspCpMode::Work, Some(wireless::bsp::ProductId::Aic8800D80))`（或 `None` 由另一执行流调用 aicbsp_sdio_probe），并取消注释相关代码（见 main.rs 中注释块）；随后用 **aicbsp_current_product_id()** 与 **fdrv::aicwf_sdio_probe_equiv(chipid)** 初始化 FDRV SDIO。

---

### 7.11 FDRV SDIO 管理与 LicheeRV 对照

#### LicheeRV 中 FDRV 如何管理 SDIO

| 层次 | 文件 / 结构 | 作用 |
|------|-------------|------|
| **设备** | aicwf_sdio.h **struct aic_sdio_dev** | 持有 func/func2、dev、**bus_if**、**cmd_mgr**、rx_priv、tx_priv、state、chipid、**struct aic_sdio_reg sdio_reg** |
| **寄存器** | **struct aic_sdio_reg** | 按芯片 V1/V2 或 V3 的寄存器偏移（bytemode_len、intr_config、sleep、wakeup、flow_ctrl、rd/wr_fifo_addr 等） |
| **总线** | aicwf_txrxif.h **struct aicwf_bus** | bus_priv.sdio、dev、**struct aicwf_bus_ops *ops**、state、cmd_buf、bustx/busrx 线程、completion |
| **总线操作** | **struct aicwf_bus_ops** | start、stop、txdata(dev, skb)、txmsg(dev, msg, len) |
| **TX/RX 私有** | **struct aicwf_tx_priv** / **aicwf_rx_priv** | sdiodev、cmd_buf、txq/rxq、aggr_buf 等 |
| **Host 环境** | sdio_host.h **struct sdio_host_env_tag** | txdesc_free_idx、txdesc_used_idx、**tx_host_id[queue][cnt]**、pthis |
| **常量** | aicwf_sdio.h / ipc_shared.h | SDIOWIFI_FUNC_BLOCKSIZE、BUFFER_SIZE、TAIL_LEN、NX_TXQ_CNT、NX_TXDESC_CNT0..4、SDIO_SLEEP_ST/ACTIVE_ST |

**程序逻辑（aicwf_sdio_probe）**：chipmatch → func_init / sdiov3_func_init → **aicwf_sdio_bus_init**（aicwf_bus_init：起 bustx/busrx 线程、填 bus_ops）→ aicwf_rwnx_sdio_platform_init（rwnx_cfg80211_init）→ aicwf_hostif_ready()。

#### StarryOS wireless FDRV 侧对齐实现

| LicheeRV | StarryOS（wireless/driver/fdrv） |
|----------|----------------------------------|
| struct aic_sdio_reg | **sdio_bus::SdioReg**（v1_v2_default / v3_default / for_product） |
| struct aic_sdio_dev | **sdio_bus::SdioDev**（chipid、sdio_reg、state）；bus/rwnx_hw 由平台挂接 |
| enum aicwf_bus_state | **sdio_bus::BusState**（Down / Up） |
| struct aicwf_bus_ops | **sdio_bus::BusOps** trait（start、stop、txdata、txmsg） |
| struct sdio_host_env_tag | **sdio_bus::SdioHostEnv**（txdesc_free_idx、txdesc_used_idx、tx_host_id、pthis）；txdesc_push、tx_cfm_advance |
| aicwf_sdio_register / probe / exit | **aicwf_sdio_register_equiv**、**aicwf_sdio_probe_equiv(chipid)**、**aicwf_sdio_exit_equiv** |
| 常量 | **sdio_bus**：SDIOWIFI_FUNC_BLOCKSIZE、SDIO_BUFFER_SIZE、SDIO_TAIL_LEN、NX_TXQ_CNT、NX_TXDESC_CNT_MAX、SDIO_SLEEP_ST/ACTIVE_ST、CMD_BUF_MAX、TXPKT_BLOCKSIZE、MAX_AGGR_TXPKT_LEN 等 |
| 收发抽象 | **sdio_host::SdioHostOps**（send_pkt、recv_pkt），平台实现时可调用 BSP **SdioOps** |

**说明**：StarryOS 无内核 sdio_func/kthread，故无 aicwf_bus 的线程与 completion；**BusOps** 由平台实现，内部通过 BSP SdioOps 或裸机 SDIO 主机完成收发。**SdioHostEnv** 用于 TX 描述符/host_id 追踪，E2A TXCFM 时由上层按 queue_idx 调用 **tx_cfm_advance** 取回 host_id。

---

### 7.12 在 aicbsp_sdio_init 阶段如何探测芯片并初始化 FDRV SDIO

#### LicheeRV 中的两段“探测”

1. **BSP 段（aicbsp_sdio_init 内）**：`sdio_register_driver(&aicbsp_sdio_driver)` 后，内核扫描 SDIO 总线；发现 AIC 卡后调用 **aicbsp_sdio_probe**（chipmatch(vid/did)→chipid、func_init、bus_init、cmd_mgr、`up(probe_semaphore)`），`aicbsp_sdio_init` 的 `down_timeout` 返回，接着执行 `aicbsp_driver_fw_init`。
2. **FDRV 段（aicbsp_init 之后）**：FDRV 模块加载，`aicwf_sdio_register()`；内核再次匹配同一 SDIO 设备时调用 **aicwf_sdio_probe**（chipmatch、func_init、bus_init、rwnx_platform_init → rwnx_cfg80211_init、aicwf_hostif_ready）。

即：**“探测芯片”在 BSP 段由内核发现卡后触发 BSP probe；“初始化 FDRV SDIO”在 BSP 固件加载完成后由 FDRV 的 sdio_driver probe 完成。**

#### StarryOS 无内核 SDIO 总线时的两种用法

**方式一：单线程 + 已知芯片型号（推荐用于板级固定 AIC 芯片）**

1. 在调用 **aicbsp_init** 时传入 **probe_immediate: Some(ProductId)**（如 `Some(ProductId::Aic8800D80)`）。
2. **aicbsp_sdio_init(Some(pid))** 内部会直接调用 **aicbsp_sdio_probe(pid)** 并返回 `Ok(())`，不等待 2 秒；随后 **aicbsp_driver_fw_init** 使用 `CURRENT_PRODUCT_ID` 继续执行。
3. **aicbsp_init** 返回后，用 **bsp::aicbsp_current_product_id()** 得到当前 chipid，再初始化 FDRV SDIO：
   - **fdrv::aicwf_sdio_probe_equiv(chipid)** 得到 **SdioDev**；
   - 按需挂接 BusOps、SdioHostEnv、rwnx_hw 等价（如 **wireless::wireless_driver_init_stub()** 或创建 **WirelessDriver\<RealWiphyImpl\>**）。

示例（main 或平台初始化）：

```text
let mut bsp_info = bsp::AicBspInfo::default();
// 板级固定为 AIC8800D80 时，直接传 Some，无需 SDIO 总线扫描
bsp::aicbsp_init(&mut bsp_info, bsp::AicBspCpMode::Work, Some(bsp::ProductId::Aic8800D80))?;
let chipid = bsp::aicbsp_current_product_id().unwrap();
let _sdiodev = fdrv::aicwf_sdio_probe_equiv(chipid);
let _driver = wireless::wireless_driver_init_stub(); // 或真实 wiphy 注册
```

**方式二：双执行流（平台有 SDIO 主机驱动时）**

1. 主线程调用 **aicbsp_init(info, testmode, None)**，内部 **aicbsp_sdio_init(None)** 会 **probe_wait_timeout_ms(2000)** 阻塞。
2. 另一执行流（另一任务/线程或平台 SDIO 主机“卡检测”回调）：通过 SDIO 读 CCCR/vendor 得到 vid/did，做 chipmatch 得到 **ProductId**，然后调用 **bsp::aicbsp_sdio_probe(product_id)**，触发 **probe_signal()**。
3. 主线程的 **probe_wait_timeout_ms** 返回，**aicbsp_driver_fw_init** 执行；**aicbsp_init** 返回后，同样用 **aicbsp_current_product_id()** 与 **aicwf_sdio_probe_equiv(chipid)** 初始化 FDRV SDIO。

#### 小结

| 阶段 | 作用 | StarryOS 做法 |
|------|------|----------------|
| aicbsp_sdio_init 内 | “探测芯片”并让 BSP 侧 probe 完成 | **Some(pid)**：直接 **aicbsp_sdio_probe(pid)**；**None**：等待其他执行流调用 **aicbsp_sdio_probe** 后 **probe_signal()** |
| aicbsp_init 返回后 | 初始化 FDRV SDIO 驱动 | **aicbsp_current_product_id()** → **fdrv::aicwf_sdio_probe_equiv(chipid)** 得到 SdioDev，再挂 BusOps / wiphy |

---

### 7.13 检测芯片的过程（chipmatch）

#### LicheeRV 中如何得到 chipid

1. **谁提供 vid/did**：内核在枚举 SDIO 总线时，由 MMC/SDIO 核心从卡上读出 **Vendor ID** 和 **Device ID**（SDIO 规范中来自 FBR：Function Basic Registers，Function 0 的 FBR 在 0x100–0x1FF，Vendor 在 0x08–0x09，Device 在 0x0A–0x0B），并填入 `struct sdio_func` 的 `.vendor`、`.device`。
2. **谁做 chipmatch**：BSP 的 **aicbsp_sdio_probe** 或 FDRV 的 **aicwf_sdio_probe** 被内核调用时，传入 `func->vendor`、`func->device`，内部调用 **aicwf_sdio_chipmatch(sdiodev, vid, did)**，按表设置 `sdiodev->chipid`（PRODUCT_ID_AIC8801 / AIC8800DC / AIC8800D80 / AIC8800D80X2）。

**LicheeRV vid/did → ProductId 表（aicsdio.c / aicwf_sdio.c）**：

| Vendor (vid) | Device (did) | ProductId |
|--------------|--------------|-----------|
| 0x5449       | 0x0145       | Aic8801   |
| 0xc8a1       | 0xc08d       | Aic8800Dc |
| 0xc8a1       | 0x0082       | Aic8800D80 |
| 0xc8a1       | 0x2082       | Aic8800D80X2 |

#### StarryOS 中如何做“检测芯片”

1. **板级固定芯片**：已知型号时直接用 **ProductId**，无需读 SDIO：调用 **aicbsp_sdio_init(Some(ProductId::Aic8800D80))** 等即可。
2. **平台有 SDIO 主机**：由平台在“卡就绪”后读 SDIO FBR：
   - Function 0 FBR 基址通常为 0x100；Vendor ID 在 FBR 0x08–0x09（2 字节），Device ID 在 0x0A–0x0B（2 字节），按 SDIO 规范为小端。
   - 用 BSP 的 **SdioOps::readb**（或平台等价）按字节/字读出 vid、did，再调用 **bsp::chipmatch(vid, did)** 得到 `Option<ProductId>`；若为 `Some(pid)`，再调用 **aicbsp_sdio_probe(pid)** 完成 BSP 侧“探测”。
3. **chipmatch 接口**：**bsp::chipmatch(vid: u16, did: u16) -> Option<ProductId>**，与 LicheeRV 上表一致；常量在 **bsp::sdio_ids**（VENDOR_AIC8801、DEVICE_AIC8801 等）。
