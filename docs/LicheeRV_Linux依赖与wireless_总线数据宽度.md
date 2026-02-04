# LicheeRV Linux 内核依赖 与 wireless 总线/数据宽度

本文档回答两类问题：  
1）LicheeRV 中 aic8800 涉及的 **Linux 内核功能** 是什么、在哪、是否已移植到 wireless；  
2）wireless 的 **总线**、**数据宽度**、与固件是否匹配、是否可能导致 bootrom 未响应，以及如何与 LicheeRV 总线逻辑对齐。

---

## 1. LicheeRV 中 aic8800 涉及的 Linux 内核功能

### 1.1 是什么功能？

aic8800 BSP（`aicsdio.c` / `aic_bsp_driver.c`）依赖的是 **Linux MMC 子系统**，不是自己实现 SDIO 协议，而是通过 MMC 提供的 SDIO 接口访问 WiFi 芯片。具体包括：

| 功能 | 说明 | LicheeRV 中的位置 |
|------|------|--------------------|
| **SDIO 驱动框架** | `sdio_register_driver` / `sdio_unregister_driver`，probe/remove 与卡枚举绑定 | `aicsdio.c`：注册 `aicbsp_dummy_sdmmc_driver` / aic8800 FDRV 的 sdio_driver |
| **Host 占用与互斥** | `sdio_claim_host` / `sdio_release_host`，保证同一 host 上 CMD52/CMD53 串行化 | `aicsdio.c`：每次 `aicwf_sdio_readb/writeb`、`sdio_writesb/readsb` 前后 claim/release |
| **单字节 I/O（CMD52）** | `sdio_readb` / `sdio_writeb` | `aicsdio.c`：F1 寄存器读写的底层实现 |
| **块/流 I/O（CMD53）** | `sdio_readsb` / `sdio_writesb`，用于 WR_FIFO 写、RD_FIFO 读 | `aicsdio.c`：`aicwf_sdio_send_pkt`、`aicwf_sdio_recv_pkt`、`aicwf_sdio_readframes` |
| **Function 与块大小** | `sdio_set_block_size(func, 512)`、`sdio_enable_func` | `aicsdio.c`：`aicwf_sdio_func_init` 等，`SDIOWIFI_FUNC_BLOCKSIZE=512` |
| **SDIO 中断** | `sdio_claim_irq` / `sdio_release_irq`，卡拉中断后调用 handler | `aicsdio.c`：`aicwf_sdio_bus_start` 里注册 `aicwf_sdio_hal_irqhandler` |
| **Host 时钟/总线** | `host->ops->set_ios(host, &host->ios)`，设置 clock、bus_width 等 | 由 **MMC host 驱动** 实现；aic8800 仅使用 `host->ios.clock` 可选配置，**不直接设 bus_width** |

**总线宽度（bus width）** 由 **MMC 核心 + host 驱动** 在卡初始化时设置（如 `mmc_set_bus_width` → host 的 `set_ios`），aic8800 驱动本身不调用“设 4-bit”的 API，只是使用 host 已经配置好的 1/4-bit。在常见 Linux 配置下，SDIO 卡枚举完成后会设为 **4-bit**。

### 1.2 这部分功能在哪？

- **BSP 侧（aic8800 对 MMC 的调用）**：  
  `LicheeRV-Nano-Build/osdrv/extdrv/wireless/aic8800/aic8800_bsp/aicsdio.c`、`aicsdio_txrxif.c`、`aic_bsp_driver.c`。  
  这里只调用 `sdio_*`、`sdio_claim_host` 等，不包含 MMC 核心或 host 的实现。

- **Linux 内核中的实现**：  
  - MMC 核心与 SDIO 协议：`linux_5.10/drivers/mmc/core/`（如 `sdio_ops.c`、`sdio.c` 等）。  
  - 具体 SoC 的 **MMC host 驱动**（例如 Cvitek/SG2002 的 SDMMC 控制器驱动）在 `drivers/mmc/host/` 下，实现 `set_ios`（其中会写 host 控制器的 **DATA_WIDTH/总线宽度**）、CMD52/CMD53 的底层发送等。

### 1.3 是否正确移植到 wireless？

- **逻辑已移植**：  
  wireless 在 **无 Linux** 的 StarryOS 上，用自有 SD1 主机实现（`StarryOS/wireless/driver/bsp/src/sdio/backend.rs`）完成了与 LicheeRV **等效的操作**：  
  - CMD0/CMD5/CMD3/CMD7 枚举 SDIO 卡；  
  - CMD52 单字节读写（F1 寄存器）；  
  - CMD53 字节模式读写 WR_FIFO/RD_FIFO；  
  - F1 block size = 512（`set_block_size(1, 512)`），与 LicheeRV `SDIOWIFI_FUNC_BLOCKSIZE` 一致；  
  - F1 0x0B(REGISTER_BLOCK)=1、0x11(BYTEMODE_ENABLE)=1、0x04(INTR_CONFIG)=0x07 等与 aicsdio 一致。

- **未完全对齐的一点：总线数据宽度**  
  - Linux 下由 host 的 `set_ios` 在 SDIO 初始化后通常设为 **4-bit**。  
  - wireless 当前在 `sd1_host_init` 里只设置了 HOST_CTRL1 的 **CARD_DET_TEST + CARD_DET_SEL**，**没有设置 DAT_XFER_WIDTH（4-bit）**，因此控制器默认保持 **1-bit**。  
  - 块大小、F1 配置、CMD52/CMD53 语义均已对齐；**数据宽度** 是当前与 LicheeRV 总线逻辑的主要差异，见下节。

---

## 2. wireless 的总线、数据宽度与对齐

### 2.1 wireless 里有没有“总线”？

有。wireless 使用 **SG2002 的 SD1 控制器** 作为 SDIO 主机，通过 CMD/CLK/DAT0～DAT3 与 AIC8800 连接，这就是“总线”；逻辑上对应 LicheeRV 中 **同一颗 SD1 控制器** 在 Linux MMC 子系统中暴露的 host。

### 2.2 数据传输宽度是多少？

- **当前实现**：  
  `backend.rs` 中只对 HOST_CTRL1 写了 **CARD_DET_TEST | CARD_DET_SEL**，没有写 **DAT_XFER_WIDTH**（SG2002 TRM：HOST_CTL1 0x028 bit1，1=4-bit，0=1-bit，复位 0）。  
  因此 **当前为 1-bit**（仅 DAT0 传数据）。

- **LicheeRV 典型情况**：  
  Linux MMC 在 SDIO 卡初始化完成后会调用 host 的 `set_ios`，将 **bus_width** 设为 **4-bit**（MMC_BUS_WIDTH_4），即 DAT0～DAT3 都用于数据传输。

### 2.3 是否与固件/芯片数据宽度匹配？

- **块大小**：已匹配。wireless 与 LicheeRV 均使用 **512 字节** 作为 F1 块大小，与 AIC8800 固件/协议（IPC 按块、BLOCK_CNT 等）一致。
- **数据线宽度**：  
  - SDIO 规范允许 1-bit 与 4-bit；很多 SDIO WiFi 在 1-bit 下也能工作（仅 DAT0），只是带宽较低。  
  - 若芯片/固件或参考设计默认按 4-bit 调试，而 host 一直用 1-bit，理论上可能在某些平台或时序下表现异常，但 **1-bit 本身一般不会直接导致“完全无响应”**；无响应更常见与电源、复位、时钟、F1 配置或 bootrom 未跑起来有关。  
  - 为与 LicheeRV 行为一致、排除宽度相关差异，**建议在 wireless 中也启用 4-bit**。

### 2.4 会不会因为数据宽度导致 bootrom 未启动的“假象”？

- **直接因果**：仅因“host 用 1-bit 而固件期望 4-bit”就导致 bootrom **完全不应答** 的可能性较低；SDIO 识别与 CMD5 等在 1-bit 下即可完成。  
- **间接/边界情况**：若硬件/PCB 或芯片内部默认按 4-bit 设计，且 1-bit 下时钟/负载与 4-bit 不同，理论上可能影响稳定性；或个别平台对 4-bit 有依赖。  
- **结论**：数据宽度更可能是 **对齐与一致性** 问题，而不是 bootrom 不响应的首要原因；但为与 LicheeRV 完全对齐，应在 wireless 中 **在卡枚举完成后启用 4-bit**。

### 2.5 如何对齐 LicheeRV 的总线逻辑？

1. **块大小**：已对齐，F1 block size = 512。  
2. **数据宽度**：在 **SDIO 卡枚举成功**（CMD7 完成，卡进入 Transfer 状态）**之后**，对 SD1 主机做一次 **4-bit 使能**：  
   - 读 `HOST_CTRL1`（0x028）；  
   - 将 **bit1（DAT_XFER_WIDTH）置 1**（4-bit）；  
   - 写回 `HOST_CTRL1`。  
   这样与 Linux MMC 在 SDIO 初始化后通过 host `set_ios` 设 4-bit 的语义一致。  
3. **时机**：在 `sdio_card_init()` 成功返回后、或 `new_sd1_with_card_init()` 中卡 init 完成后的路径中执行一次即可；无需在每次 CMD52/CMD53 前设置。

实现上可在 `Aic8800SdioHost::sdio_card_init()` 成功返回前（或调用该函数的初始化链中紧接其后）增加一步“设置 HOST_CTRL1 bit1 = 1（4-bit）”，并打日志，便于确认与 LicheeRV 总线逻辑一致。

---

## 3. 小结

| 项目 | LicheeRV | wireless 现状 | 建议 |
|------|----------|----------------|------|
| Linux MMC 依赖 | sdio_*、claim_host、set_block_size、claim_irq、host set_ios | 无 Linux；用自有 SD1 实现等效 CMD52/CMD53 与 F1 配置 | 已移植逻辑；中断用软中断替代 |
| 总线 | SD1 host，MMC 驱动 | 同一 SD1 host，backend 直接操作寄存器 | 一致 |
| 块大小 | 512 | 512 | 已对齐 |
| 数据宽度 | 4-bit（host set_ios） | 1-bit（未设 DAT_XFER_WIDTH） | 在卡 init 后设 HOST_CTRL1 bit1=1，与 LicheeRV 对齐 |
| bootrom 不响应 | — | 数据宽度可能为次要因素；优先排查电源/复位/时钟/F1 配置 | 加上 4-bit 可排除宽度差异 |
