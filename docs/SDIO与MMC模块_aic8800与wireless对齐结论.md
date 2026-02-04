# SDIO/MMC 模块：aic8800 与 wireless 对齐结论

本文档在现有逐项对照基础上，给出 **SDIO 与 mmc 模块** 的**对齐结论**：已完全对齐项、设计差异项、可选/未实现项。

---

## 一、结论摘要

| 类别 | 说明 |
|------|------|
| **已完全对齐** | aic8800 所用 MMC/SDIO API 在 wireless 的 mmc crate 与 bsp sdio 中均有对应实现，语义一致。 |
| **设计差异** | 无内核/无 platform 导致的流程差异（如“主动枚举 + 内联 probe”代替“register → 内核枚举 → probe”），已用等价方式实现。 |
| **可选/未实现** | 不影响当前 WiFi 功能的项（如 dummy 驱动、host caps、release_irq 抽象、init 后提高 SDIO 时钟）。 |

**结论：SDIO 与 mmc 模块已与 aic8800 所需能力对齐，无功能缺失；差异仅为无 Linux 设备模型下的设计取舍与可选优化。**

---

## 二、mmc crate 与 Linux MMC 子系统对应

| Linux（aic8800 使用） | wireless mmc | 状态 |
|----------------------|--------------|------|
| **类型** | | |
| sdio_device_id、SDIO_ANY_ID、sdio_class | types：SdioDeviceId、SDIO_ANY_ID、sdio_class | ✅ 已对齐 |
| mmc_ios、mmc_bus_width | MmcIos、MmcBusWidth | ✅ 已对齐 |
| mmc_card、RCA | card：MmcCard、Rca | ✅ 已对齐 |
| **Host** | | |
| sdio_claim_host / sdio_release_host | MmcHost::claim_host() -> Guard | ✅ 已对齐 |
| host->ops->set_ios(clock, bus_width) | MmcHost::set_ios(&MmcIos) | ✅ 已对齐 |
| **SdioFunc** | | |
| sdio_readb / sdio_writeb | SdioFunc::readb / writeb | ✅ 已对齐 |
| sdio_readsb / sdio_writesb | SdioFunc::readsb / writesb | ✅ 已对齐 |
| sdio_set_block_size | SdioFunc::set_block_size | ✅ 已对齐 |
| sdio_enable_func / sdio_disable_func | SdioFunc::enable_func / disable_func；cccr 规范实现 | ✅ 已对齐 |
| sdio_f0_readb / sdio_f0_writeb | SdioFunc::read_f0 / write_f0 | ✅ 已对齐 |
| sdio_claim_irq / sdio_release_irq | SdioFunc::claim_irq / release_irq（默认空实现） | ✅ 已对齐（软中断替代） |
| **驱动模型** | | |
| sdio_register_driver | mmc::sdio_register_driver | ✅ 已对齐 |
| sdio_unregister_driver | mmc::sdio_unregister_driver | ✅ 已对齐 |
| id_table 匹配后 probe | mmc::sdio_try_probe(func) | ✅ 已对齐 |
| remove 回调 | mmc::sdio_driver_remove(func) | ✅ 已对齐 |
| **CCCR/F0** | | |
| F0 寄存器常量（0x04/0x13/0xF0 等） | cccr::sdio_f0_reg | ✅ 已对齐 |

---

## 三、bsp sdio 与 aic8800 BSP（aicsdio.c）对应

| aic8800 BSP 行为 | wireless bsp sdio | 状态 |
|------------------|-------------------|------|
| **驱动注册与流程** | | |
| sdio_register_driver(&aicbsp_sdio_driver) | aicbsp_init 中 register_aicbsp_sdio_driver() | ✅ 已对齐 |
| 枚举到卡后 id_table 匹配 → probe | aicbsp_sdio_init 创建设备后 mmc::sdio_try_probe(&BspSdioFuncRef) | ✅ 已对齐 |
| sdio_unregister_driver | aicbsp_sdio_exit 中 unregister_aicbsp_sdio_driver() | ✅ 已对齐 |
| remove → sdio_disable_func | aicbsp_sdio_exit 中 sdio_driver_remove 后写 CCCR 关闭 F1/F2 | ✅ 已对齐 |
| **I/O** | | |
| sdio_claim_host 后 readb/writeb/readsb/writesb | SDIO_DEVICE.lock() / with_sdio；Aic8800SdioHost read_byte/write_byte/read_block/write_block | ✅ 已对齐 |
| F1/F2 基址 0x100/0x200、块大小 512 | flow + backend；FUNC1_BASE/FUNC2_BASE、set_block_size(1/2,512) | ✅ 已对齐 |
| F1 0x0B=1、0x11=1、0x04=0x07（8801） | flow.rs 4.2 节 | ✅ 已对齐 |
| **F0** | BspSdioFuncRef::read_f0/write_f0 委托 host | ✅ 已对齐 |
| **中断** | 不注册 PLIC；sdio_tick + busrx 轮询 | ✅ 设计一致（软中断替代） |
| **退出** | 停 bustx/busrx → remove → unregister → CCCR 关 F1/F2 → 清 SDIO_DEVICE/CMD_MGR | ✅ 已对齐 |
| aicbsp_exit 调用 aicbsp_sdio_exit | lib.rs aicbsp_exit() 内 aicbsp_sdio_exit() | ✅ 已对齐 |

---

## 四、设计差异（非缺失）

| 项目 | LicheeRV | wireless | 说明 |
|------|----------|----------|------|
| 正式驱动注册时机 | aicbsp_sdio_init 内 sdio_register_driver，然后 down_timeout(probe_semaphore) 等内核 probe | aicbsp_init 内先 register；aicbsp_sdio_init 内主动枚举 + probe_from_sdio_cis + 创建设备后 sdio_try_probe | 无内核时无法“注册后等内核回调”，用“先注册、枚举后主动 try_probe”等价 |
| dummy 驱动（等卡出现） | platform_power_on 内 sdio_register_driver(dummy) → rescan → down_timeout(aic_chipup_sem) → unregister(dummy) | 无；上电后固定延时再主动 CMD0/5/3/7 枚举 | 用“延时 + 主动枚举”代替“等卡检测”，语义等价 |
| probe 完成同步 | aicbsp_sdio_init 里 down_timeout(probe_semaphore)；probe 里 up | 同一 aicbsp_sdio_init 内同步完成 CIS + chipmatch + probe_signal，无等待 | 单线程顺序执行，无需 semaphore |

---

## 五、可选 / 未实现（不影响当前功能）

| 项目 | 说明 |
|------|------|
| host->caps \|= MMC_CAP_NONREMOVABLE | 无 host caps 抽象；可忽略 |
| aicbsp_sdio_release | 当前为 stub（release_irq/release_host 抽象）；FDRV 未要求独立 release 时可不实现 |
| init 后提高 SDIO 时钟 | LicheeRV 可设 feature.sdio_clock；wireless 未在 init 后改 CLK_CTL，可按需补充 |
| dummy sdio 驱动注册/反注册 | 仅“等卡出现”用；wireless 用固定延时 + 枚举，可不实现 |

---

## 六、参考文档

- **逐项 API 对照**：`aic8800_MMC调用与wireless_逐项对照.md`
- **mmc 模块结构**：`mmc_crate与Linux_MMC子系统对照.md`
- **启动流程与 exit**：`wireless启动流程与aic8800逐行对照.md`
- **platform 与 mmc 职责**：`platform与mmc职责划分.md`
