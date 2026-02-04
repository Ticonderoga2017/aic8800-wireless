# aic8800 所有 Linux MMC 调用与 wireless 逐项对照

本文档列出 LicheeRV 中 aic8800 BSP/FDRV **所有** 调用 Linux MMC/SDIO 子系统的位置，并给出 wireless 中的对应实现及是否一致。

---

## 一、BSP 层（aicsdio.c）

### 1. 驱动注册

| LicheeRV | 位置 | wireless 对应 | 一致 |
|----------|------|----------------|------|
| `sdio_register_driver(&aicbsp_sdio_driver)` | aicbsp_sdio_init 内 | **mmc::sdio_register_driver**；aicbsp_init 中 **register_aicbsp_sdio_driver()**，创建设备后 **mmc::sdio_try_probe** | 是 |
| `sdio_unregister_driver(&aicbsp_sdio_driver)` | aicbsp_sdio_exit | **mmc::sdio_unregister_driver**；aicbsp_sdio_exit 中先 **sdio_driver_remove** 再 **unregister_aicbsp_sdio_driver()** | 是 |
| `sdio_register_driver(&aicbsp_dummy_sdmmc_driver)` | aicsdio.c 124（等卡） | 无；上电后固定延时 + 主动枚举 | 设计差异（语义等价） |
| `sdio_unregister_driver(&aicbsp_dummy_sdmmc_driver)` | aicsdio.c 128 | 无 | 一致 |

### 2. Host 占用（串行化）

| LicheeRV | 位置 | wireless 对应 | 一致 |
|----------|------|----------------|------|
| `sdio_claim_host(sdiodev->func)` | 每次 readb/writeb/readsb/writesb 前 | `SDIO_DEVICE.lock()` / `with_sdio(f)` 持锁 | 是：同一 host 上串行访问 |
| `sdio_release_host(sdiodev->func)` | 每次上述调用后 | `with_sdio` 闭包返回后放锁 | 是 |

### 3. 单字节 I/O（CMD52）

| LicheeRV | 位置 | wireless 对应 | 一致 |
|----------|------|----------------|------|
| `sdio_readb(func, regaddr, &ret)` | aicwf_sdio_readb 627 | `Aic8800Sdio::read_byte(addr)`，addr = 0x100+reg（F1） | 是 |
| `sdio_writeb(func, val, regaddr, &ret)` | aicwf_sdio_writeb 645 | `Aic8800Sdio::write_byte(addr, val)` | 是 |
| `sdio_readb(func_msg, regaddr, &ret)` | aicwf_sdio_readb_func2 636 | `readb_func2` / `read_byte(0x200+reg)`（8800DC/DW） | 是 |
| `sdio_writeb(func_msg, val, regaddr, &ret)` | aicwf_sdio_writeb_func2 654 | `writeb_func2` / `write_byte(0x200+reg)` | 是 |

### 4. 块/流 I/O（CMD53）

| LicheeRV | 位置 | wireless 对应 | 一致 |
|----------|------|----------------|------|
| `sdio_writesb(func, wr_fifo_addr, buf, count)` | aicwf_sdio_send_pkt 713 | `send_pkt` / `write_block(FUNC1_BASE+WR_FIFO_ADDR, buf)`，backend cmd53_write_chunk | 是 |
| `sdio_readsb(func, buf, rd_fifo_addr, size)` | aicwf_sdio_recv_pkt 730 | `recv_pkt` / `read_block(FUNC1_BASE+RD_FIFO_ADDR, buf)`，backend cmd53_read_chunk | 是 |
| `sdio_writesb(func_msg, 7, buf, count)` | aicwf_sdio_send_msg 702（8800DC/DW） | `send_msg` 写 F2 偏移 7（FUNC2_MSG_ADDR_OFFSET） | 是（8801 不用 F2） |
| `sdio_readsb(func_msg, buf, rd_fifo_addr, size)` | aicwf_sdio_recv_pkt 734（msg!=0） | `recv_pkt(..., msg=1)` 读 F2 RD_FIFO | 是 |

### 5. Function 使能与块大小

| LicheeRV | 位置 | wireless 对应 | 一致 |
|----------|------|----------------|------|
| `sdio_set_block_size(func, SDIOWIFI_FUNC_BLOCKSIZE)` | aicwf_sdio_func_init 1728，1755（F2） | `host.set_block_size(1, 512)` / `set_block_size(2, 512)`（flow.rs） | 是（512） |
| `sdio_enable_func(func)` | aicwf_sdio_func_init 1733，1763（F2） | CCCR 0x02 写 IO_ENABLE（F1 bit1 / F2 bit2），等 IO_READY 0x03（flow.rs） | 是 |
| `udelay(100)` 在 enable_func 后 | aicsdio.c 1740 | `sync::delay_spin_us(100)`（flow.rs 4.2 节前） | 是 |

### 6. F1 寄存器配置（8801）

| LicheeRV | 位置 | wireless 对应 | 一致 |
|----------|------|----------------|------|
| F1 REGISTER_BLOCK(0x0B)=1 | aicsdio.c 1784 | flow.rs host.write_byte(f1_base+REGISTER_BLOCK, 1) | 是 |
| F1 BYTEMODE_ENABLE(0x11)=1 | aicsdio.c 1790 | flow.rs host.write_byte(f1_base+BYTEMODE_ENABLE, 1) | 是 |
| F1 INTR_CONFIG(0x04)=0x07 | aicwf_sdio_bus_start 1307，flow 在 init 中写 | flow.rs host.write_byte(f1_base+INTR_CONFIG, 0x07) | 是 |

### 7. 中断

| LicheeRV | 位置 | wireless 对应 | 一致 |
|----------|------|----------------|------|
| `sdio_claim_irq(func, aicwf_sdio_hal_irqhandler)` | aicwf_sdio_bus_start 1305 | 不注册 PLIC；软中断 `sdio_tick` + busrx 轮询 | 设计一致（软中断替代） |
| `sdio_release_irq(func)` | aicwf_sdio_release 1653 等 | 无 | 一致 |

### 8. Host 时钟（可选）

| LicheeRV | 位置 | wireless 对应 | 一致 |
|----------|------|----------------|------|
| `host->ios.clock = feature.sdio_clock; host->ops->set_ios(host, &host->ios)` | aicwf_sdio_func_init 1742 | backend 用 CLK_CTL FREQ_SEL 设时钟；init 时 ~400kHz，未在 init 后提高 | 可补充：与 feature 一致时可设同样频率 |

### 9. 总线宽度

| LicheeRV | 位置 | wireless 对应 | 一致 |
|----------|------|----------------|------|
| Host 由 MMC 核心 set_ios 设 4-bit | 内核 MMC 在 SDIO 枚举后 | backend 在 sdio_card_init 成功后设 HOST_CTRL1 bit1=1（4-bit） | 是 |

### 10. Function 0（F0）读写

| LicheeRV | 位置 | wireless 对应 | 一致 |
|----------|------|----------------|------|
| `sdio_f0_writeb(func, val, reg, &ret)` | aicsdio.c 1337；aicwf_sdio.c 2375/3046/3179 等 | **mmc::SdioFunc::write_f0(reg, val)**；BspSdioFuncRef 委托 host.write_byte(reg) | 是 |
| `sdio_f0_readb`（若用） | 同上 | **SdioFunc::read_f0(reg)** | 是 |
| F0 寄存器常量 | 0x04、0x13、0xF0/0xF1/0xF2/0xF8 | **mmc::cccr::sdio_f0_reg**（SDIO_F0_04、SDIO_F0_13、SDIO_F0_F0 等） | 是 |

### 11. 释放/下电

| LicheeRV | 位置 | wireless 对应 | 一致 |
|----------|------|----------------|------|
| `sdio_disable_func(func)` | aicwf_sdio_func_deinit 1902，1908 | **aicbsp_sdio_exit** 中在清 SDIO_DEVICE 前写 CCCR 0x02 关闭 F1(bit1)、F2(bit2) | 是 |

---

## 二、FDRV 层（aicwf_sdio.c）

FDRV 通过 BSP 的 `aicwf_sdio_readb/writeb`、`aicwf_sdio_send_pkt`、`aicwf_sdio_recv_pkt` 等访问硬件，这些在 BSP 内已映射到 sdio_*。wireless 的 `SdioOps`（read_byte、write_byte、send_msg、recv_pkt 等）与 BSP 接口一一对应，逻辑一致。

---

## 三、常量与寄存器

| 常量/寄存器 | LicheeRV（aicsdio.h） | wireless（types/reg） | 一致 |
|-------------|------------------------|------------------------|------|
| SDIOWIFI_FUNC_BLOCKSIZE | 512 | 512 / SDIO_FUNC_BLOCKSIZE | 是 |
| TAIL_LEN | 4 | 4 | 是 |
| TX_ALIGNMENT | 4 | ipc_send_len 需先 4 对齐（见下） | 已对齐 |
| BUFFER_SIZE | 1536 | 1536 | 是 |
| BYTEMODE_LEN_REG | 0x02 | reg::BYTEMODE_LEN 0x02 | 是 |
| INTR_CONFIG_REG | 0x04 | reg::INTR_CONFIG 0x04 | 是 |
| BLOCK_CNT_REG | 0x12 | reg::BLOCK_CNT 0x12 | 是 |
| FLOW_CTRL_REG | 0x0A | reg::FLOW_CTRL 0x0A | 是 |
| FLOWCTRL_MASK | 0x7F | reg::FLOWCTRL_MASK 0x7F | 是 |
| WR_FIFO_ADDR | 0x07 | reg::WR_FIFO_ADDR 0x07 | 是 |
| RD_FIFO_ADDR | 0x08 | reg::RD_FIFO_ADDR 0x08 | 是 |
| REGISTER_BLOCK | 0x0B | reg::REGISTER_BLOCK 0x0B | 是 |
| BYTEMODE_ENABLE | 0x11 | reg::BYTEMODE_ENABLE 0x11 | 是 |

---

## 四、IPC 发送长度与对齐（aicsdio.c aicwf_sdio_tx_msg）

LicheeRV 顺序（972-978）：

1. `len = payload_len`；若 `len % TX_ALIGNMENT != 0`，则 `adjust_len = roundup(len, 4)`，用 0 填充至 `adjust_len`，`len = payload_len`（更新后）。
2. 若 `len % SDIOWIFI_FUNC_BLOCKSIZE != 0`：在末尾加 `TAIL_LEN`(4) 字节 0，`len = (payload_len/512 + 1)*512`；否则 `len = payload_len`。

即：**先 4 字节对齐，再按 512 取整（不足则 +4 再取整）**。wireless 的 `ipc_send_len_8801` 需与此完全一致，且发送前应把 `buf[serialized_len..send_len]` 填 0。

---

## 五、已修正项（与 LicheeRV 完全一致）

1. **ipc_send_len_8801**（flow.rs）：先按 TX_ALIGNMENT(4) 对齐，再按 BLOCK(512) 取整（不足则 +TAIL_LEN(4) 后取整），与 aicwf_sdio_tx_msg 964-978 行一致；三处调用点在提交前对 `buf[len..send_len].fill(0)`，与 LicheeRV 的 adjust_str/TAIL 填 0 一致。
2. **4-bit 总线**（backend.rs）：卡枚举成功后设 HOST_CTRL1 bit1=1（DAT_XFER_WIDTH）。

---

## 六、可选/后续

1. **Host 时钟**：若 LicheeRV feature.sdio_clock > 0，可在 init 后对 CLK_CTL FREQ_SEL 设相同频率。
2. **D80/D80X2**：若支持，需补 F0 0x04 写及 V3 寄存器路径。
3. **sdio_disable_func**：若需与 LicheeRV 完全一致，可在 aicbsp_sdio_exit 中写 CCCR 关闭 F1/F2。
