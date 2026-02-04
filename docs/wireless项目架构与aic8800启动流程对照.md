# wireless 项目架构与 aic8800 启动流程对照

本文档说明 **wireless** 的 crate 划分、与 LicheeRV aic8800 启动流程的逐项对照及当前缺失项。

---

## 1. wireless 项目架构（当前）

```
wireless/
├── Cargo.toml          # 依赖 bsp、fdrv、ieee80211、mmc
├── mmc/                # SDIO/MMC 抽象（与 driver 同级）
│   ├── src/
│   │   ├── lib.rs、types.rs、host.rs、card.rs、cccr.rs、sdio_func.rs、driver.rs
│   └── README.md
├── ieee80211/          # cfg80211/mac80211 抽象（与 driver 同级）
│   ├── src/
│   │   ├── ieee80211.rs、cfg80211.rs、mac80211.rs
│   └── README.md
├── driver/
│   ├── bsp/            # 板级支持（SG2002 + AIC8800）
│   │   └── src/
│   │       ├── sdio/   # backend、mmc_impl、flow、ops、cis、types
│   │       ├── fw_load.rs、gpio.rs、cmd.rs、...
│   └── fdrv/           # 全功能驱动框架（依赖 ieee80211）
│       └── src/
│           ├── sdio_bus.rs、sdio_host.rs、wiphy.rs、...
└── docs/
```

- **mmc**：与 Linux MMC 子系统对应的 trait 与类型；**CCCR 使能/禁用** 的规范逻辑已放入 mmc（`cccr.rs`），bsp 通过 `CccrAccess`/`DelayMs` 复用。
- **bsp**：SDIO 主机实现（SG2002 SD1）、卡枚举、F1/F2 使能、CIS 探测、固件加载、bustx/busrx 线程、命令管理；**属于 MMC 的“规范行为”** 已尽量委托给 mmc，仅保留平台相关的 CMD52/53 与寄存器访问在 backend。

---

## 2. LicheeRV aic8800 启动流程（简要）

| 步骤 | LicheeRV | wireless 对应 |
|------|----------|----------------|
| 1 | **aicbsp_init**：resv_mem_init、probe_semaphore、platform_driver、platform_device、sysfs、power_lock、aicbsp_set_subsys(BT, PWR_ON) | **aicbsp_init**：probe_reset、power_lock、后续在锁内执行 power_on → sdio_init → driver_fw_init |
| 2 | **aicbsp_set_subsys(PWR_ON)**：mutex_lock(power_lock) → **aicbsp_platform_power_on** → **aicbsp_sdio_init** → **aicbsp_driver_fw_init(sdiodev)** → **aicbsp_sdio_release(sdiodev)** | 同序：**aicbsp_power_on**（GPIO/pinmux/稳定延时）→ **aicbsp_sdio_init** → **aicbsp_driver_fw_init**；sdio_release 在 fdrv 或上层按需调用 |
| 3 | **aicbsp_sdio_init**：平台上电 → **sdio_register_driver**（dummy）→ 等待 probe（卡检测）→ probe 内 chipmatch、func_init、**bus_init**（bustx/busrx 线程）、cmd_mgr、**aicbsp_platform_init**、**aicbsp_driver_fw_init** 由 set_subsys 在 sdio_init 返回后调用 | **aicbsp_sdio_init**：上电在 power_on 中完成；无内核故无 sdio_register；直接 **new_sd1_with_card_init**（CMD0/5/3/7、4-bit）→ F1/F2 使能（用 mmc cccr）→ **probe_from_sdio_cis** → 建 **Aic8800Sdio**、**RwnxCmdMgr**、**ensure_bustx/busrx_thread_started**；**aicbsp_driver_fw_init** 在主流程中随后调用 |
| 4 | **aicbsp_driver_fw_init**：读 chip_rev(0x40500000) → 选 firmware_list → system_config(DBG_MEM_WRITE 表) → 固件上传(adid/patch/table/wl_fw) → start_app → hwinfo 等 | **aicbsp_driver_fw_init**：**send_dbg_mem_read_busrx**(CHIP_REV) → 按 product_id/chip_rev 选固件 → **fw_upload_blocks**、**fw_start_app**；**aicbsp_system_config** 的 DBG_MEM_WRITE 表在 8800d80 等 compat 中有对应，可按需补全 |
| 5 | **aicbsp_sdio_release**：claim_host → release_irq → release_host，供 fdrv 独立使用 SDIO | **aicbsp_sdio_release**：停 bustx/busrx、释放 SDIO_DEVICE（take），与 LicheeRV 语义一致 |

---

## 3. 已对齐部分

- **上电与 SDIO 枚举**：GPIO/pinmux、稳定延时、CMD0→CMD5→CMD3→CMD7、4-bit、F1/F2 使能（含 mmc CCCR 逻辑）。
- **CIS 与 chipmatch**：probe_from_sdio_cis、product_id、Aic8800Sdio 与寄存器布局（V1/V2/V3）。
- **bustx/busrx**：两线程、submit_cmd_tx_and_wait_tx_done、run_poll_rx_one、on_cfm、notify_cmd_done。
- **固件加载**：chip_rev、固件列表、fw_upload_blocks、fw_start_app；IPC 长度与 512 块尾对齐。
- **mmc 抽象**：MmcHost（claim/set_ios）、SdioFunc（readb/writeb/readsb/writesb、**read_f0/write_f0**、set_block_size、enable_func/disable_func 用 mmc cccr）；**F0 常量**（cccr::sdio_f0_reg）供 D80/V3 路径使用。

---

## 4. 抽象接口与 Linux / LicheeRV 对照（复刻清单）

wireless 中与 aic8800/Linux MMC 一一对应的抽象如下，均已实现并用于 BSP。

| 抽象 | Linux / LicheeRV | wireless 实现 |
|------|-------------------|----------------|
| **MmcHost** | `struct mmc_host`、`sdio_claim_host`/`release_host`、`host->ops->set_ios` | `mmc::host::MmcHost`（claim_host → Guard、set_ios）；BSP：`BspSdioHost` |
| **SdioFunc** | `struct sdio_func`、`sdio_readb`/`writeb`、`sdio_readsb`/`writesb`、`sdio_set_block_size`、`sdio_enable_func`/`disable_func`、`sdio_f0_readb`/`sdio_f0_writeb` | `mmc::sdio_func::SdioFunc`（含可选 read_f0/write_f0）；BSP：`BspSdioFuncRef` |
| **CCCR** | F0 0x02/0x03 使能/轮询、sdio_enable_function 规范 | `mmc::cccr::CccrAccess`、`sdio_enable_function`/`sdio_disable_function`、`sdio_f0_reg` |
| **SdioDriver** | `sdio_driver`、id_table、probe/remove | `mmc::driver::SdioDriver`（无内核时由 BSP 枚举后按 id 调用 probe） |
| **类型** | `sdio_device_id`、`mmc_ios`、SDIO_CLASS_*、SDIO_ANY_ID | `mmc::types::SdioDeviceId`、`MmcIos`、`MmcBusWidth`、`sdio_class` |

**TODO（可选/按需）：**

- 在 D80/V3 启动路径中按 LicheeRV 顺序调用 `write_f0(SDIO_F0_04, 0x07)`、`write_f0(SDIO_F0_13, feature.sdio_phase)` 等。
- aicbsp_system_config 中按芯片型号补全 DBG_MEM_WRITE 表。
- 若需与 LicheeRV 完全一致，可在 aicbsp_sdio_exit 中调用 `disable_func` 关闭 F1/F2。

---

## 5. 缺失或待补全部分

| 项目 | LicheeRV | wireless 现状 | 建议 |
|------|----------|---------------|------|
| **aicbsp_platform_init** | LicheeRV 仅做 cmd_mgr_init、sdiodev->cmd_mgr.sdiodev 赋值 | wireless 在 probe 路径已建 RwnxCmdMgr 并挂到流程，等价 | 无需再实现 platform_init，除非增加其它平台初始化 |
| **aicbsp_system_config** | DBG_MEM_WRITE 表（8800/8800dc/8800d80 等） | 部分在 compat/fw_load 中 | 按芯片型号补全表中项 |
| **预留内存 resv_mem** | alloc_skb 池等 | 占位空实现 | 若 fdrv 需要 skb 池再实现 |
| **aicbsp_info**（cpmode、chip_rev、hwinfo、fwlog） | 全局信息与 sysfs | 有 CURRENT_PRODUCT_ID、chip_rev 在流程中使用；无 sysfs | 可选：导出 aicbsp_info 结构供上层或调试 |
| **F0 0x04 等 D80 路径** | 部分 D80/D80X2 需写 F0 0x04/0x13/0xF0 等 | **mmc** 已提供 **SdioFunc::write_f0/read_f0** 与 **cccr::sdio_f0_reg**；BSP 已实现，D80/V3 流程中按需调用即可 | 见 aic8800_MMC调用与wireless_逐项对照.md |
| **RFKill / 平台电源策略** | 部分平台 rfkill、set_subsys 细粒度 | 未实现 | 按需加 GPIO 或电源策略 |

---

## 6. 小结

- **bus → mmc** 重命名已完成；**aic8800 依赖的 MMC 功能** 已在 **mmc** crate 中完整复刻：claim_host/set_ios、readb/writeb/readsb/writesb、set_block_size、enable_func/disable_func、**read_f0/write_f0**（对应 Linux sdio_f0_*）、CCCR 规范逻辑与 **sdio_f0_reg** 常量；bsp 通过 `CccrAccess`/`DelayMs` 与 `BspSdioFuncRef` 实现。
- **启动顺序** 与 LicheeRV 一致：power_on → sdio_init（枚举+F1/F2+probe+bustx/busrx）→ driver_fw_init（chip_rev、固件、start_app）→ 可选 sdio_release。
- **缺失** 主要为平台/芯片可选项（platform_init、system_config 表、resv_mem、aicbsp_info 导出、D80/V3 流程中实际调用 write_f0、RFKill），可按需逐步补全。
