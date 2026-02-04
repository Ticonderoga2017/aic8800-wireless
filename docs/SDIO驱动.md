结论先说：**BSP 在固件加载完成后，自己不再需要“占着”SDIO，会通过 aicbsp_sdio_release 把 host 放掉；但整机仍然需要 SDIO，因为 FDRV 和芯片以后所有通信都走 SDIO。**

---

## 1. BSP 在固件加载后还占不占 SDIO？

**不占。** 流程就是这样设计的：

- BSP 在 **aicbsp_driver_fw_init** 里做完：读 chip rev、选固件表、固件下载、patch、**START_APP**。
- 之后会调 **aicbsp_sdio_release**（LicheeRV 里在 aicbsp_set_subsys 里，`#ifndef CONFIG_FDRV_NO_REG_SDIO` 时会调）：  
  对 func 做 `sdio_claim_host` → `sdio_release_irq` → `sdio_release_host`，把 BSP 对 SDIO host 的占用释放掉。

所以：**“加载固件”这件事做完之后，BSP 就不再需要持有 SDIO host，会主动 release。**

---

## 2. 那整机还需要 SDIO 吗？

**需要。** SDIO 是 AIC 芯片和主机之间的**唯一数据/命令通道**，不只在下载固件时用：

- **BSP 阶段**：用 SDIO 做 DBG_MEM_*（写固件、读 chip rev）、START_APP 等。
- **BSP 之后（FDRV 阶段）**：  
  - 所有 LMAC 命令、IPC、数据包（TX/RX）都还是通过 **同一块 SDIO** 和芯片通信。  
  - FDRV 会自己 `sdio_claim_host`、读写、注册 sdio_irq 等，继续用这条 SDIO 总线。

所以：  
**“BSP 不需要再占 SDIO” ≠ “系统不需要 SDIO”。**  
BSP 只是把“谁在用 SDIO”从自己交给 FDRV，总线本身一直要用。

---

## 3. 小结

| 对象            | 固件加载完成后是否还需要 SDIO？                     |
| --------------- | --------------------------------------------------- |
| **BSP**         | 不需要再占；会 **aicbsp_sdio_release** 释放 host。  |
| **系统 / FDRV** | **需要**；FDRV 用同一 SDIO 做运行时命令和数据收发。 |

一句话：**BSP 在完成加载固件后不再需要持有 SDIO，会 release；但 SDIO 总线本身仍然需要，因为 FDRV 和芯片的后续通信都依赖它。**



可以，不注册 BSP 的 SDIO 驱动是可行的；做法就是**只让 FDRV 注册并管理 SDIO**。

---

## 1. 不注册 BSP 的 SDIO 驱动是否可行？

**可行。**

LicheeRV 里之所以有「BSP 先注册一个 SDIO 驱动」，是因为采用了**两段式**：  
BSP 先 probe → 固件加载 →（可选）release → FDRV 再 probe 同一设备。

也可以改成**一段式**：**只注册一个 SDIO 驱动（FDRV 的）**，在 FDRV 的 probe 里**按顺序**做完 BSP 该做的事和 FDRV 该做的事：

- 发现设备后只触发 **FDRV 的 probe**
- 在 FDRV probe 里依次做：
  - chipmatch、func/bus 初始化、cmd_mgr 初始化（相当于 BSP 的 aicbsp_sdio_probe 内容）
  - 固件加载与启动（相当于 aicbsp_driver_fw_init）
  - wiphy/cfg80211、net_device 等 FDRV 初始化

这样就不需要 BSP 再单独 `sdio_register_driver`，BSP 只提供**函数**（如固件加载、命令管理），不占一个「驱动槽位」。

---

## 2. 不注册时，FDRV 如何管理 SDIO？

**由 FDRV 自己注册唯一的 SDIO 驱动，并在自己的 probe 里统一管理 SDIO。**

| 项目             | BSP 单独注册（LicheeRV 当前）                                | 不注册 BSP 驱动（只 FDRV 注册）                              |
| ---------------- | ------------------------------------------------------------ | ------------------------------------------------------------ |
| 谁注册 SDIO 驱动 | BSP 一个 + FDRV 一个（或 BSP 先、FDRV 后）                   | **只有 FDRV** 注册一个                                       |
| 谁先 probe       | BSP probe → 固件加载 → 再 FDRV probe                         | **FDRV 一次 probe**                                          |
| probe 里做什么   | BSP：chipmatch、func/bus、cmd_mgr、up(sem)<br>FDRV：wiphy、cfg80211 等 | **FDRV probe 里顺序做**：chipmatch、func/bus、cmd_mgr → 调 BSP 的固件加载 → 再 wiphy、cfg80211 等 |
| SDIO 谁在用      | BSP probe 时 BSP 用；release 后 FDRV 用                      | **始终由 FDRV**：claim_host、读写、中断、数据收发都在 FDRV 的 SDIO 层 |

也就是说：

- **设备发现**：内核发现 AIC SDIO 卡 → 只匹配到 **FDRV 的 sdio_driver** → 只调用 **FDRV 的 probe**。
- **SDIO 管理**：在 FDRV probe 里完成 SDIO 的 claim、func/bus 初始化；之后的读写、中断、TX/RX 都由 FDRV 的 SDIO 模块（如 aicwf_sdio）持有并管理，BSP 不再单独「拥有」一个 SDIO 驱动。

---

## 3. 在 StarryOS 里的对应关系

StarryOS 当前没有 Linux 的 `sdio_register_driver`，等价做法是：

- **不注册 BSP 的 SDIO 驱动** → 不需要「BSP 等 probe」这一步，即可以**不做** `aicbsp_sdio_init()` 里的 `probe_wait_timeout_ms(2000)`（或只在有平台 SDIO 主机时由平台触发「probe 等价」）。
- **FDRV 管理 SDIO**：由平台/上层在「发现 SDIO 设备」或「决定用哪块 SDIO」时，直接调用 FDRV 的初始化路径：  
  先做 chipmatch、func/bus、cmd_mgr、再调 BSP 的 `aicbsp_driver_fw_init`、最后做 wiphy 等；SDIO 的读写、中断、数据路径都归 FDRV（或平台提供的 SdioOps 实现）管理。

**结论**：不注册 BSP 的 SDIO 驱动是可行的；由 FDRV 单独注册（或在无内核 SDIO 时由上层统一入口）并在一次初始化流程里顺序完成 BSP 逻辑和 FDRV 逻辑，SDIO 由 FDRV 统一管理即可。



简单说：**注册 BSP 的 SDIO 驱动，是为了在“内核发现 AIC SDIO 卡”时，让 BSP 先拿到这个设备、做完芯片识别和固件加载，再交给 FDRV 做 WiFi。**

---

## 1. 为什么要“BSP 自己注册一个 SDIO 驱动”？

在 LicheeRV 里，AIC8800 的流程是**分两段**的：

1. **BSP 段**：发现 SDIO 设备 → 识别芯片（chipmatch）→ 初始化 func/bus、cmd_mgr → **下载固件并启动**（`aicbsp_driver_fw_init`）。
2. **FDRV 段**：再用同一个 SDIO 设备做 WiFi（cfg80211、net_device、数据收发等）。

内核的 SDIO 子系统是“**谁注册了驱动，谁就在设备匹配时被 probe**”。所以：

- **注册 BSP 的 SDIO 驱动**（`sdio_register_driver(&aicbsp_sdio_driver)`）  
  → 内核在总线上发现 AIC 的 SDIO 卡（vid/did 匹配）时，会调用 **aicbsp_sdio_probe**。  
  → BSP 在 probe 里完成：分配 `aic_sdio_dev`、chipmatch、func/bus/cmd_mgr 初始化，最后 `up(&aicbsp_probe_semaphore)`，让正在 `aicbsp_sdio_init()` 里等着的 `down_timeout` 返回。  
  → 这样 **aicbsp_sdio_init** 返回后，`aicbsp_sdiodev` 已经有效，**紧接着才能调 aicbsp_driver_fw_init** 去下载、启动固件。

也就是说：**“注册 BSP 的 SDIO 驱动” = 让内核在发现 AIC SDIO 卡时，把设备先交给 BSP，而不是直接交给 FDRV。** 这样 BSP 才能按顺序完成“发现设备 → probe → 固件加载”。

---

## 2. 和 FDRV 的 SDIO 驱动是什么关系？

- **BSP 的 sdio_driver**：只关心“发现 AIC 卡 + probe 建立 sdiodev + 等固件加载”。  
  `aicbsp_sdio_init()` 里注册的就是这个，用来拿**第一次**的 probe 机会。
- **FDRV 的 sdio_driver**（如 aicwf_sdio_driver）：关心“同一块 AIC 卡，但用来做 WiFi 数据、cfg80211 等”。  
  通常在 BSP 固件加载完成、甚至 `aicbsp_sdio_release` 之后，再由 FDRV 模块去注册或通过别的路径绑定到同一 SDIO 设备。

所以：  
**注册 BSP 的 SDIO 驱动，是为了在“卡被枚举出来”时，先由 BSP 做 probe 和固件加载；否则设备可能被别的驱动先占住，或者 FDRV 先 probe 但此时固件还没加载，无法正常工作。**

---

## 3. 一句话

**注册 BSP 的 SDIO 驱动，是为了让内核在发现 AIC SDIO 设备时，把第一次 probe 交给 BSP，从而完成芯片识别和固件下载；之后 FDRV 再使用同一设备做 WiFi。**

正在 LicheeRV-Nano-Build 中查找 BSP 层通过 SDIO 读 FBR 得到 vid/did、调用 chipmatch 并调用 aicbsp_sdio_probe 的完整流程位置。

---

## 1. “用 SDIO 读 FBR 得到 vid/did”在完整流程中的位置

**不是 AIC BSP 自己读 FBR**，而是 **Linux 内核 MMC/SDIO 子系统**在枚举 SDIO 卡时完成的：

| 阶段                  | 位置                                     | 说明                                                         |
| --------------------- | ---------------------------------------- | ------------------------------------------------------------ |
| 读 FBR / CIS          | `linux_5.10/drivers/mmc/core/sdio.c`     | 每个 function 会调 `sdio_read_fbr(func)`，再 `sdio_read_func_cis(func)`。 |
| 解析 CIS 得到 vid/did | `linux_5.10/drivers/mmc/core/sdio_cis.c` | `sdio_read_cis()` 会读 FBR 区的 CIS（`SDIO_FBR_BASE(fn) + SDIO_FBR_CIS`），解析到 **CISTPL_MANFID (0x20)** 时调用 **`cistpl_manfid()`**（约 81–100 行），从 4 字节里取出 vendor、device 并写入 **`func->vendor`**、**`func->device`**。 |

也就是说：**“用 SDIO 读 FBR 得到 vid/did”发生在内核卡枚举阶段**：  
`sdio.c` → `sdio_read_fbr` / `sdio_read_func_cis` → `sdio_cis.c` 的 **`cistpl_manfid()`** 里完成对 vid/did 的解析和赋值。

---

## 2. chipmatch 与 aicbsp_sdio_probe 在完整流程中的位置

BSP 层“动态识别芯片”的逻辑（用 vid/did → chipmatch → 再继续 probe）**全部在 BSP 的 probe 回调里**，且 **vid/did 来自内核已填好的 `func->vendor` / `func->device`**。

### 2.1 整体调用链

```
aicbsp_set_subsys(..., 1) 或等价上电路径
  → aicbsp_platform_power_on()     [aicsdio.c 约 487 行]
       → 平台 GPIO/电源、rescan
       → aicbsp_reg_sdio_notify() 注册 dummy 驱动，等 2 秒（仅确认卡可见，不跑 chipmatch）
  → aicbsp_sdio_init()            [aicsdio.c 587 行]
       → sdio_register_driver(&aicbsp_sdio_driver)
       → down_timeout(&aicbsp_probe_semaphore, 2000)   // 等 probe 完成
  ← 内核匹配到 WLAN class 的 SDIO 设备后调用：
       aicbsp_sdio_probe(struct sdio_func *func, ...)  [aicsdio.c 267 行]
```

### 2.2 在 aicbsp_sdio_probe 内部的具体位置

| 步骤                  | 文件                    | 行号    | 代码/含义                                                    |
| --------------------- | ----------------------- | ------- | ------------------------------------------------------------ |
| 1. 使用的 vid/did     | `aic8800_bsp/aicsdio.c` | 280     | `func->vendor`, `func->device`（内核已在 CIS 解析时从 FBR 区填好） |
| 2. 再次校验是否为 AIC | 同上                    | 282–293 | 若 vid/did 不在 AIC 表内则设 `aicbsp_load_fw_in_fdrv` 并 return |
| 3. chipmatch          | 同上                    | **321** | **`err = aicwf_sdio_chipmatch(sdiodev, func->vendor, func->device);`** → 得到 chipid 写入 `sdiodev->chipid` |
| 4. 后续初始化         | 同上                    | 323–351 | func_init、bus_init、`aicbsp_platform_init(sdiodev)`         |
| 5. 通知“probe 完成”   | 同上                    | **353** | **`up(&aicbsp_probe_semaphore);`** → `aicbsp_sdio_init()` 里的 `down_timeout` 返回 |

chipmatch 函数本身在 **235–253 行**：根据 vid/did 查表，设置 `sdio_dev->chipid`（如 `PRODUCT_ID_AIC8801`、`PRODUCT_ID_AIC8800DC` 等）。

---

## 3. 简要结论

- **读 FBR 得到 vid/did**：在 **内核** 的 SDIO 枚举里完成，具体是  
  `drivers/mmc/core/sdio.c` → `sdio_read_fbr` / `sdio_read_func_cis` →  
  `drivers/mmc/core/sdio_cis.c` 的 **`cistpl_manfid()`**（约 81–100 行），结果在 **`func->vendor`**、**`func->device`**。
- **chipmatch**：在 **`aic8800_bsp/aicsdio.c`** 的 **`aicbsp_sdio_probe()`** 里、第 **321 行** 被调用：  
  `aicwf_sdio_chipmatch(sdiodev, func->vendor, func->device)`。
- **aicbsp_sdio_probe**：由内核在匹配到 `aicbsp_sdmmc_ids`（SDIO_CLASS_WLAN）后调用，位置 **267–362 行**；  
  若 chipmatch 成功，会继续做 func/bus/platform 初始化，最后在第 **353 行** `up(&aicbsp_probe_semaphore)` 通知 `aicbsp_sdio_init()` 完成。

所以在 LicheeRV 里，BSP 层“动态识别芯片”的程序逻辑就是：**在 aicbsp_sdio_probe 里用内核已填好的 func->vendor / func->device 调 aicwf_sdio_chipmatch 得到 chipid，再继续做 BSP 的 func/bus/platform 初始化**；**没有任何一段是 BSP 自己用 SDIO 读 FBR**，读 FBR 和填 vid/did 都在内核 MMC 的 CIS 解析里完成。

---

## 1. 新增常量与辅助

| 项                            | 说明                                                        |
| ----------------------------- | ----------------------------------------------------------- |
| **`sdio_fbr_base(func_num)`** | FBR 基址：`func_num * 0x100`，对应 Linux `SDIO_FBR_BASE(f)` |
| **`SDIO_FBR_CIS`**            | FBR 内 CIS 指针偏移 `0x09`（3 字节小端）                    |
| **`CISTPL_MANFID`**           | CIS tuple 制造商/设备 ID，code `0x20`，4 字节               |

---

## 2. 读 FBR 得到 CIS 指针

- **`read_fbr_cis_ptr<O: SdioOps>(ops: &O, func_num: u8) -> Result<u32, i32>`**  
  从 `SDIO_FBR_BASE(func_num) + SDIO_FBR_CIS` 读 3 字节（小端），得到 24 位 CIS 起始地址。  
  对应 Linux `sdio_read_cis` 里从 `SDIO_FBR_BASE(fn) + SDIO_FBR_CIS` 读 3 字节。

---

## 3. 解析 CIS 得到 vendor/device

- **`parse_cis_for_manfid<O: SdioOps>(ops: &O, cis_ptr: u32) -> Result<Option<(u16, u16)>, i32>`**  
  从 `cis_ptr` 起遍历 CIS tuple 链：每项 1 字节 code、1 字节 link、link 字节数据；遇到 code `0x20` 且 link ≥ 4 时，按 Linux `cistpl_manfid` 解析 4 字节为 `(vendor, device)` 并返回；未找到或结束返回 `Ok(None)`；最多 256 个 tuple 防死循环。

---

## 4. 高层接口

- **`read_vendor_device<O: SdioOps>(ops: &O, func_num: u8) -> Result<Option<(u16, u16)>, i32>`**  
  先 `read_fbr_cis_ptr(ops, func_num)`，再 `parse_cis_for_manfid(ops, cis_ptr)`，返回 `Some((vid, did))` 或 `Ok(None)`。

- **`probe_from_sdio_cis<O: SdioOps>(ops: &O, func_num: u8) -> Result<(), i32>`**  
  调用 `read_vendor_device`，若得到 `Some((vid, did))` 则 `chipmatch(vid, did)`；若为 `Some(pid)` 则调用 `aicbsp_sdio_probe(pid)` 并返回 `Ok(())`，否则返回 `Err(-1)`。

---

## 5. 使用方式

平台在拥有实现了 `SdioOps` 的 SDIO 主机后，可：

- 仅要 vid/did：`bsp::read_vendor_device(&sdio_ops, 1)`（1 为 WiFi function）。
- 要“读 FBR/CIS → chipmatch → probe”：`bsp::probe_from_sdio_cis(&sdio_ops, 1)`。

上述函数和常量已在 `bsp::lib.rs` 中导出（如 `read_fbr_cis_ptr`、`parse_cis_for_manfid`、`read_vendor_device`、`probe_from_sdio_cis`、`sdio_fbr_base`、`SDIO_FBR_CIS`、`CISTPL_MANFID`）。




---

## 1. LicheeRV 里 bus/platform 为 wireless 做了什么

| 机制         | 作用                                                         | 对 wireless 的“注册 / 传递”                                  |
| ------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| **SDIO bus** | `sdio_register_driver(&aicbsp_sdio_driver)` / FDRV 的 `aicwf_sdio_register()` | **注册**：驱动挂到 SDIO 总线；**传递**：内核发现卡后调用 `aicbsp_sdio_probe(func)` / `aicwf_sdio_probe(func)`，传入 `struct sdio_func *func`（含 vendor/device），驱动里分配 `aic_sdio_dev`、chipmatch、func/bus init、`aicbsp_platform_init(sdiodev)`，并把 `sdiodev` 作为后续固件/FDRV 的上下文。 |
| **Platform** | `platform_driver_register` + `platform_device_add`           | 只提供 sysfs 挂载点（如 `/sys/devices/platform/aic-bsp/`），**不**参与 SDIO 探测，也**不**向 wireless 传递 sdiodev。 |

也就是说：**真正为 wireless 做“注册 + 传递”的是 SDIO bus**（注册驱动 → 总线发现设备 → probe 回调并传入 func/sdiodev）；platform 只是给 BSP 一个逻辑设备节点，不负责把设备“交给” wireless。

---

## 2. StarryOS 里现有的 bus/platform 能做什么

| 组件                                | 能力                                                         | 和 wireless 的关系                                           |
| ----------------------------------- | ------------------------------------------------------------ | ------------------------------------------------------------ |
| **axdriver bus**                    | `bus/mmio.rs`、`bus/pci.rs`：按 MMIO 区间或 PCI 枚举，调 `Driver::probe_mmio` / `probe_pci`，得到 `AxDeviceEnum`（Net / Block / Display 等），放进 `AllDevices`，`init_drivers()` 把 `AllDevices` 交给上层。 | 只有 **MMIO/PCI**，没有 **SDIO**；设备类型只有 Net/Block/Display/Input/Vsock，**没有** “无线/SDIO” 这一类。 |
| **Platform**（axhal / dyn_drivers） | `axhal::PLATFORM` 表示板级名；`rdrive::PlatformDevice` 等是 FDT 解析出的设备，用于 virtio 等。 | 是“当前平台是谁”，以及块/网卡等通用设备的探测，**没有** wireless 或 SDIO 的“注册 + probe 回调”语义。 |

因此：**当前 StarryOS 的 bus/platform 不能像 LicheeRV 的 SDIO 那样，为 wireless 提供“注册驱动 → 总线发现设备 → probe 时传入设备句柄”这一套服务**，原因是：

- 没有 SDIO 总线类型；
- 没有“无线设备”或“SDIO 设备”这类统一设备类型；
- platform 不负责“发现 SDIO 卡并调用 wireless probe”。

---

## 3. 能否做到“和 LicheeRV 一样”为 wireless 提供注册与传递？

**不能直接复用现有 bus/platform**，但**通过扩展可以做到语义等价的“注册 + 传递”**，有两种常见做法：

### 方案 A：在系统里加一层“SDIO/无线设备”的注册与传递（更接近 LicheeRV）

思路：在 StarryOS 里引入“SDIO 设备发现”或“无线设备”的抽象，由它来扮演“注册 + 传递”的角色：

1. **注册**：  
   - 要么在 axdriver 里增加“SDIO 总线”或“无线设备”类型，在 `probe_bus_devices()`（或类似入口）里，当存在平台提供的 SDIO 主机（实现 `SdioOps`）时，做一次“SDIO 探测”；  
   - 要么在启动链里单独有一个“无线/SDIO 初始化”步骤，内部调用 `bsp::probe_from_sdio_cis(ops, 1)` 等，相当于“总线发现设备”。

2. **传递**：  
   - 探测到 AIC 卡后：chipmatch 得到 ProductId，调用 `aicbsp_sdio_probe(pid)`、`aicbsp_driver_fw_init`、`fdrv::aicwf_sdio_probe_equiv(chipid)`，得到 `SdioDev` / `WirelessDriver`；  
   - 把这些结果**交给上层**：例如放入 `AllDevices` 的新成员（如 `wireless: Option<WirelessDriver>`），或通过全局/注册表让 net 栈或其它模块能拿到。这样上层拿到的就是“bus 发现并初始化好的 wireless 设备”，等价于 LicheeRV 里“SDIO probe 里建好 sdiodev 并传给后续逻辑”。

这样，**StarryOS 的“bus”就能像 LicheeRV 的 SDIO bus 一样，为 wireless 提供：注册（驱动/初始化入口）、发现设备（SDIO 探测）、传递（把 SdioDev/WirelessDriver 交给上层）**。Platform 仍可只表示板级或提供 SDIO host，不必实现成 Linux 的 platform_driver/device。

### 方案 B：保持当前 wireless 设计（不用统一 bus，但语义等价）

当前做法已经能完成“注册 + 传递”，只是不经过统一 bus：

- **注册**：由 main/启动链**按顺序**调用  
  `aicbsp_power_on` → `aicbsp_sdio_init(Some(pid))` 或 `probe_from_sdio_cis(ops, 1)` → `aicbsp_driver_fw_init` → `wireless_driver_init_stub()` 或真实 FDRV 初始化。
- **传递**：  
  - BSP 侧：`CURRENT_PRODUCT_ID`、可选 `AicBspInfo`；  
  - FDRV 侧：`aicbsp_current_product_id()`、`aicwf_sdio_probe_equiv(chipid)` 得到 `SdioDev`，再创建 `WirelessDriver`；  
  - 谁需要 wireless，谁在初始化链之后通过 `wireless::xxx()` 或你定义的全局/句柄拿到驱动实例。

这里“注册”= 调用顺序和入口，“传递”= 全局状态 + 函数参数 + 返回值，**功能上已经等价于 LicheeRV 的“SDIO 发现 + probe 里建 sdiodev 并往下传”**，只是没有“总线 + probe 回调”的形式。

---

## 4. 小结

| 问题                                                         | 结论                                                         |
| ------------------------------------------------------------ | ------------------------------------------------------------ |
| StarryOS 里现有的 bus/platform 能否**直接**像 LicheeRV 一样为 wireless 做注册、传递？ | **不能**。现有 bus 只有 MMIO/PCI，没有 SDIO；platform 也不是“SDIO 设备注册 + probe 传递”的层。 |
| 能否**通过扩展**做到和 LicheeRV 一样的“注册、传递”效果？     | **可以**。                                                   |
- **方案 A**：加一层 SDIO/无线设备发现，把探测结果（SdioDev/WirelessDriver）放进 AllDevices 或类似结构，由“bus/初始化层”统一交给上层，形式最接近 LicheeRV。  
- **方案 B**：保持当前显式调用链 + `CURRENT_PRODUCT_ID` + 参数/返回值传递，不做统一 bus，但“注册 + 传递”的语义已经和 LicheeRV 等价。 |

若你希望“wireless 从系统 bus/platform 里被注册并拿到设备句柄”，需要按**方案 A** 在 StarryOS 里为 SDIO/无线增加一层发现与传递；若只关心“能正确初始化和把驱动实例交给上层”，当前**方案 B** 已经足够。



## 拆分结果

原来的单文件 `wireless/driver/bsp/src/sdio.rs` 改为目录 **`wireless/driver/bsp/src/sdio/`**，并按功能分成这些模块：

| 文件                    | 功能                                                         |
| ----------------------- | ------------------------------------------------------------ |
| **`mod.rs`**            | 子模块声明与对外重导出，保持 `bsp::sdio::*` 的 API 不变      |
| **`types.rs`**          | 类型与常量：`ProductId`、`SdioState`、`SdioType`、`sdio_ids`、`reg`、`reg_v3`、通用常量、`chipmatch` |
| **`ops.rs`**            | SDIO 抽象：`SdioOps`、`SdioHostBackend`、`SdioOpsImpl`（FIFO 基址与封装） |
| **`cis.rs`**            | FBR/CIS：`sdio_fbr_base`、`SDIO_FBR_CIS`、`CISTPL_MANFID`、`read_fbr_cis_ptr`、`parse_cis_for_manfid`、`read_vendor_device`、`probe_from_sdio_cis` |
| **`backend_sg2002.rs`** | SG2002 SD1 硬件后端：`Sg2002SdioBackend`（CMD52 读写、SDMMC 寄存器） |
| **`backend_dummy.rs`**  | 占位后端：`SdioBackend`（无硬件时返回 -ENODEV）              |
| **`flow.rs`**           | SDIO 流程六函数：`CURRENT_PRODUCT_ID`、`aicbsp_power_on`、`aicbsp_sdio_init`、`aicbsp_sdio_probe`、`aicbsp_driver_fw_init`、`aicbsp_sdio_release`、`aicbsp_sdio_exit`、`aicbsp_current_product_id` |

**依赖关系**：`types` 无内部依赖 → `ops` 依赖 `types` → `cis` 依赖 `ops` 与 `types`，并调用 `flow::aicbsp_sdio_probe` → `flow` 依赖 `types`；两个 backend 仅依赖 `ops`。

**对外接口**：`lib.rs` 的 `pub use sdio::{ ... }` 未改，外部仍通过 `bsp::sdio::*` 和 `bsp::*` 使用，例如 `aicbsp_power_on`、`SdioOpsImpl`、`chipmatch` 等。

`cargo check -p bsp` 已通过，仅剩原有 gpio 相关告警。