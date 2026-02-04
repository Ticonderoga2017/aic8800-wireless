# platform 与 mmc 职责划分

## 1. platform 相关逻辑可否由 mmc 代替？

**结论：不可以。** platform 与 mmc 属于不同层次，职责不同。

### LicheeRV 中的 “platform” 指什么

在 aic_bsp_main.c 中，“platform” 指 **Linux 平台/设备模型**，与 MMC/SDIO 总线无关：

| 内容 | 作用 | 对应 Linux 子系统 |
|------|------|--------------------|
| platform_driver_register | 注册“BSP 模块”为平台驱动 | 平台总线 (platform_bus_type) |
| platform_device_alloc/add | 创建“aic-bsp”虚拟设备 | 平台设备 |
| sysfs_create_group | 暴露 cpmode、hwinfo 等属性 | sysfs |
| mutex_init(aicbsp_power_lock) | 电源/上电序列互斥 | 同步原语 |
| sema_init(aicbsp_probe_semaphore) | 等待 SDIO probe 完成 | 同步原语 |

上述逻辑属于 **“BSP 模块生命周期”** 和 **“电源/探测同步”**，不是 SDIO 总线或卡枚举。

### mmc 负责什么

mmc crate 对应 Linux 的 **MMC 子系统**（drivers/mmc/core、include/linux/mmc）：

- **MmcHost**：SDIO 主机、claim_host、set_ios  
- **SdioFunc**：function 的 readb/writeb/readsb/writesb、set_block_size、enable_func/disable_func  
- **SdioDriver**：id_table、probe、remove  
- **卡枚举**：由**主机驱动**（如 bsp backend）做 CMD0/5/3/7，mmc 只提供类型与驱动注册/匹配

即：mmc 管的是 **SDIO 总线与驱动模型**，不管“模块加载、平台设备、sysfs、电源锁”。

### wireless 中的对应关系

| LicheeRV | wireless | 归属 |
|----------|----------|------|
| platform_driver/device、sysfs | 无（无设备模型） | 不实现或由 bsp 以“逻辑等价”代替 |
| mutex_init(power_lock) | sync::power_lock()（静态 Mutex） | **bsp/sync** |
| sema_init(probe_semaphore) | sync::probe_reset() / probe_signal() | **bsp/sync** |
| aicbsp_init 整体 | aicbsp_init（resv_mem、probe_reset、power_lock、power_on、sdio_init、driver_fw_init） | **bsp** |

因此：**platform 相关逻辑由 bsp 承担（sync + aicbsp_init/exit），不能、也不应由 mmc 代替。** mmc 只负责 SDIO 驱动注册/反注册与 probe 分发（见下）。

---

## 2. SDIO 驱动缺失逻辑可否实现在 mmc 上？

**可以，且应放在 mmc。**  

LicheeRV 中与“SDIO 驱动”直接对应的是：

- `sdio_register_driver(&aicbsp_sdio_driver)`  
- 枚举到卡后内核根据 **id_table** 匹配并调用 **probe**  
- `sdio_unregister_driver`，remove 时调用 **remove**

这些是 **MMC 子系统的“驱动注册与匹配”** 语义，放在 mmc 最合适：

- 在 **mmc** 中实现：
  - **sdio_register_driver(driver)**：登记当前 SDIO 驱动（存 `&'static dyn SdioDriver`）
  - **sdio_unregister_driver()**：清除登记
  - **sdio_try_probe(device_id, func)**：若已登记驱动且 id_table 匹配，则调用 `driver.probe(func)`，否则返回 Err
  - **sdio_driver_remove(func)**：若已登记驱动，则调用 `driver.remove(func)`

- **bsp** 只做：
  - 提供实现 `SdioDriver` 的 AIC BSP 驱动（id_table、probe 里做 ensure_bustx/busrx、probe_signal 等）
  - 在 aicbsp_init 时调用 **mmc::sdio_register_driver**
  - 在 aicbsp_sdio_init 枚举并得到 device_id 后，调用 **mmc::sdio_try_probe(func)**
  - 在 aicbsp_sdio_exit 时先 **mmc::sdio_driver_remove(func)**，再 **mmc::sdio_unregister_driver**，再停线程、disable_func、清设备

这样，“SDIO 驱动”的注册/反注册与 probe/remove 语义在 mmc 上完整实现，bsp 只负责枚举与具体驱动实现，与 LicheeRV 的层次一致。

---

## 3. 当前实现状态（已落地）

- **mmc**：已实现 `sdio_register_driver`、`sdio_unregister_driver`、`sdio_try_probe`、`sdio_driver_remove`（见 `mmc/src/driver.rs`）。
- **bsp**：在 `aicbsp_init` 中调用 `sdio::register_aicbsp_sdio_driver()`；在 `aicbsp_sdio_init` 创建设备后调用 `mmc::sdio_try_probe(&BspSdioFuncRef(sdio, 1))`；在 `aicbsp_sdio_exit` 中先 `mmc::sdio_driver_remove` 再 `mmc_impl::unregister_aicbsp_sdio_driver()`。静态 `AicBspSdioDriver` 实现 `mmc::SdioDriver`（id_table 为 AIC 各型号，probe/remove 当前为空，实际初始化仍在 init 内联完成）。
