# mmc crate 与 Linux MMC 子系统对照

本文档说明 **wireless/mmc** 与 Linux 内核 **MMC 子系统** 的模块对应关系及 aic8800 所依赖功能的实现位置。

---

## 1. Linux MMC 子系统结构（aic8800 涉及部分）

- **include/linux/mmc/host.h**：`struct mmc_host`、`struct mmc_ios`、`host->ops->set_ios`
- **include/linux/mmc/card.h**：`struct mmc_card`、RCA
- **include/linux/mmc/sdio_func.h**：`struct sdio_func`、`sdio_driver`、`sdio_claim_host`、`sdio_release_host`、`sdio_readb`、`sdio_writeb`、`sdio_readsb`、`sdio_writesb`、`sdio_set_block_size`、`sdio_enable_func`、`sdio_disable_func`、`sdio_claim_irq`、`sdio_release_irq`
- **include/linux/mmc/sdio_ids.h**：`SDIO_DEVICE`、`SDIO_ANY_ID`、各类 `SDIO_CLASS_*`
- **drivers/mmc/core/sdio_ops.c**：`mmc_io_rw_direct`（CMD52）、`mmc_io_rw_extended`（CMD53）等底层实现

aic8800 BSP/FDRV 仅使用上述 API，不直接依赖块设备、请求队列等其它 MMC 子模块。

---

## 2. mmc crate 模块与 Linux 对应

| mmc 模块 | Linux 位置 | 内容 |
|----------|------------|------|
| **types** | card.h, host.h, sdio_ids.h | `SdioDeviceId`、`MmcIos`、`MmcBusWidth`、`sdio_class`、`SDIO_ANY_ID`、`SDIO_FUNC_BLOCKSIZE_DEFAULT` |
| **host** | host.h，core 中 host 占用 | `MmcHost` trait：`claim_host() -> Guard`、`set_ios(&MmcIos)`；`with_host_claimed(host, closure)` |
| **card** | card.h | `MmcCard`（`rca: Rca`）、`Rca` 类型 |
| **cccr** | sdio_ops.c 中 F0 CCCR 访问 | `CccrAccess`、`DelayMs`、`sdio_enable_function`、`sdio_disable_function`、`SDIO_CCCR_IO_ENABLE`、`SDIO_CCCR_IO_READY`；`sdio_f0_reg`（F0 0x04/0x13/0xF0 等 D80/V3 用常量） |
| **sdio_func** | sdio_func.h，sdio_ops.c | `SdioFunc` trait：`readb`、`writeb`、`readsb`、`writesb`、`set_block_size`、`enable_func`、`disable_func`；可选 `read_f0`/`write_f0`（对应 `sdio_f0_readb`/`sdio_f0_writeb`） |
| **driver** | sdio_driver | `SdioDriver` trait：`id_table`、`probe`、`remove`、`matches`；`sdio_register_driver`、`sdio_unregister_driver`、`sdio_try_probe`、`sdio_driver_remove` |

---

## 3. 实现归属

- **mmc**：trait、类型与 **CCCR 规范逻辑**（`cccr.rs` 中使能/禁用 function 的序列），`#![no_std]`，无平台依赖。
- **bsp**：在 `sdio/mmc_impl.rs` 中实现：
  - **MmcHost**：`BspSdioHost`，`claim_host()` 返回 `SDIO_DEVICE.lock()` 的 guard，`set_ios()` 调用 `Aic8800SdioHost::set_ios_raw`。
  - **SdioFunc**：`BspSdioFuncRef<'a>`，委托 host 的 read/write/block、`set_block_size`，`enable_func`/`disable_func` 委托 `mmc::sdio_enable_function`/`sdio_disable_function`（通过 `CccrViaSdioHost` + `BspDelay`）。
- **fdrv**：可依赖 `mmc`，通过 `&dyn SdioFunc` / `&dyn MmcHost` 与 BSP 解耦。

---

## 4. 参考文档

- aic8800 对 MMC 的调用逐项对照：`aic8800_MMC调用与wireless_逐项对照.md`
- 总线与数据宽度：`LicheeRV_Linux依赖与wireless_总线数据宽度.md`
