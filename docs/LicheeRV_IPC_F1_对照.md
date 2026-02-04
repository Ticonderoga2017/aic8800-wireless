# LicheeRV 与 StarryOS：probe 后首次 IPC 的 F1 读法及初始化对照

本文档对照 **LicheeRV-Nano-Build** 中 AIC8800 BSP/FDRV 在 **probe 后、第一次 DBG_MEM_READ/WRITE 前** 的 F1 用法与初始化，说明 StarryOS 已对齐的修改与依据。

## 1. F1 收包长度与 BLOCK/BYTEMODE（8801）

LicheeRV 对 8801 设 **BYTEMODE_ENABLE(0x11)=1** 即 **no byte mode**（块模式）。此时 RX 数据长度由 **BLOCK_CNT(0x12)** 表示，不是 BYTEMODE_LEN(0x02)。

**aicsdio.c 1449-1472（8801/DC/DW）**：
- 先读 **block_cnt_reg(0x12)**，`while(intstatus)` 循环；
- `data_len = intstatus * SDIOWIFI_FUNC_BLOCKSIZE`（512）；
- 若 **intstatus < 64**：直接用 `data_len = block_cnt*512` 从 rd_fifo 读；
- 若 **intstatus >= 64**：再读 **bytemode_len_reg(0x02)**，`data_len = (*byte_len)*4`，再读 rd_fifo。

| 项目 | LicheeRV | 修改后 StarryOS |
|------|----------|-----------------|
| 有数据判断 | 读 **BLOCK_CNT(0x12)**，非 0 表示有数据 | 同：先读 BLOCK_CNT(0x12) |
| 长度计算 | block_cnt&lt;64：data_len=block_cnt*512；block_cnt≥64：读 0x02，data_len=byte_len*4 | 同逻辑（BYTEMODE_THRESH=64） |
| 读数据 | sdio_readsb(rd_fifo_addr, data_len) | read_block(rd_fifo, buf[..read_len]) |

**结论**：8801 在 no byte mode(0x11=1) 下，**主用 BLOCK_CNT(0x12)** 判断与长度；仅当 block_cnt≥64 时辅用 BYTEMODE_LEN(0x02)。StarryOS 已按此在 **ops.rs recv_pkt** 中实现。

## 2. F1 在发 IPC 前的初始化（8801）

LicheeRV 在 **aicwf_sdio_func_init**（probe 内、早于 driver_fw_init / 任何 IPC）对 F1 做：

- `aicwf_sdio_writeb(sdiodev, REGISTER_BLOCK, 1)`  // F1 0x0B = 1  
- `aicwf_sdio_writeb(sdiodev, BYTEMODE_ENABLE_REG, 1)`  // F1 0x11 = 1（1 表示 no byte mode）

出处：aicsdio.c 1783–1794（8801/8800DC/DW 公共路径）。

| 项目 | LicheeRV | 原 StarryOS | 修改后 StarryOS |
|------|----------|-------------|-----------------|
| F1 block size             | sdio_set_block_size(func, 512)（aicsdio.c 1728） | 未设置 F1 | **aicbsp_sdio_init 内对 8801 先 set_block_size(1, 512)** |
| F1 0x0B (REGISTER_BLOCK) | = 1 | 未写 | **aicbsp_sdio_init 内对 8801 写 1** |
| F1 0x11 (BYTEMODE_ENABLE) | = 1 | 未写 | **aicbsp_sdio_init 内对 8801 写 1** |

**结论**：在第一次 DBG_MEM_READ/WRITE 前必须对 8801 做：**F1 block size=512**（与 LicheeRV sdio_set_block_size(sdiodev->func) 一致，否则 CMD53 写 F1 WR_FIFO 可能不被芯片处理）、F1 0x0B=1、0x11=1。已在 **aicbsp_sdio_init** 中按上述顺序实现。

## 3. 其他 F1 寄存器（已一致）

- **FLOW_CTRL**：0x0A，发送前检查 `(val & 0x7F) > 2`（LicheeRV DATA_FLOW_CTRL_THRESH），StarryOS 已按此做流控。  
- **RD_FIFO_ADDR**：0x08；**WR_FIFO_ADDR**：0x07，与 LicheeRV 一致。  
- **BLOCK_CNT**：0x12，8801 有数据时以 **0x12 为主**（非 0 表示有数据，&lt;64 时 data_len=block_cnt×512）；仅当 block_cnt≥64 时辅用 BYTEMODE_LEN(0x02) 得 data_len=byte_len×4。与 aicsdio.c 1449-1472、LicheeRV_BYTEMODE与BLOCK_CNT_8801.md 一致。

## 4. 参考文件（LicheeRV）

- `aic8800_fdrv/aicwf_sdio.h`：SDIOWIFI_* 常量（0x02, 0x0A, 0x0B, 0x11, 0x12, 0x07, 0x08）。  
- `aic8800_fdrv/aicwf_sdio.c`：`aicwf_sdio_intr_get_len_bytemode`（读 bytemode_len_reg）、`aicwf_sdio_recv_pkt`（读 rd_fifo）、`aicwf_sdio_func_init`（F2 block size、enable func、F1 0x0B/0x11）。  
- `aic8800_bsp/aicsdio.c`：`aicwf_sdio_reg_init`、`aicwf_sdio_func_init`（F1 0x0B/0x11）、`aicwf_sdio_intr_get_len_bytemode`、`aicwf_sdio_readframes`。  
- `aic8800_bsp/aic_bsp_driver.c`：`aicbsp_driver_fw_init`（先 dbg_mem_read，再 aicbsp_system_config，再 aicwifi_init）。

## 5. 修改摘要（StarryOS）

1. **sdio/ops.rs**  
   - Aic8801 的 `recv_pkt`：改为读 **F1 BYTEMODE_LEN(0x02)**，`read_len = byte_len * 4`，再从 rd_fifo 读 `read_len` 字节；不再用 BLOCK_CNT(0x12) 与 512 字节块。

2. **sdio/flow.rs**  
   - 在 **aicbsp_sdio_init** 中，F2 block size 设置之后、创建 Aic8800Sdio 之前：若 `product_id == Aic8801`，则写 **F1 REGISTER_BLOCK(0x0B)=1**、**F1 BYTEMODE_ENABLE(0x11)=1**。  
   - **busrx 启动时机**：LicheeRV 在 **probe 内 aicwf_sdio_bus_init → aicwf_bus_init** 里启动 busrx，早于 **aicbsp_driver_fw_init**。StarryOS 在 **aicbsp_sdio_init 末尾**（设置完 SDIO_DEVICE、CMD_MGR 后）调用 `ensure_busrx_thread_started()`，这样在发首包 DBG_MEM_READ 前 busrx 已在轮询 rd_fifo，与 LicheeRV 顺序一致。  
   - 周期性/100ms 的 F1 调试日志中增加 **BYTEMODE_LEN(0x02)**，便于观察 8801 是否有回包长度。

按上述对照后，StarryOS 在“probe 后、第一次 DBG_MEM_READ/WRITE”阶段的 F1 读法、F1 初始化与 **busrx 启动顺序** 已与 LicheeRV 对齐。

---

## 6. IPC 格式 / 地址 / 时序 与 LicheeRV 逐项对照

### 6.1 发送格式（A2E，主机→芯片）

| 项目 | LicheeRV（aicsdio.c / aic_bsp_driver.c） | StarryOS（修改后） |
|------|------------------------------------------|--------------------|
| **包头** | buffer[0..2]=len+4（12bit LE），buffer[2]=0x11，buffer[3]=0（8801 无 CRC），buffer[4..8]=dummy | serialize_8801 一致 |
| **消息体** | put_u16 id/dest_id/src_id/param_len，memcpy param | 一致（LE） |
| **len 含义** | `len = sizeof(lmac_msg)+param_len` = 8+param_len；发送总长 `len+8` = 16+param_len | 一致 |
| **DRV_TASK_ID** | 100（aic_bsp_driver.h） | 100（fw_load.rs） |
| **8801 发送长度** | 未满 512 时：加 4 字节 tail，再 `len = (payload/512+1)*512`，**一次写 512 字节** 到 WR_FIFO（aicsdio.c 972-976, 993） | **ipc_send_len_8801**：同逻辑，发送 512 字节 |

**差异与修复**：原先我们只发 20 字节，LicheeRV 对 8801 会填充到 512 字节再写 WR_FIFO；芯片/bootrom 可能按块取数。已在 flow.rs 对 8801 使用 `ipc_send_len_8801(len)`，发送 512 字节。

### 6.2 地址

| 项目 | LicheeRV | StarryOS |
|------|----------|----------|
| **WR_FIFO（发 IPC）** | F1 偏移 0x07（SDIOWIFI_WR_FIFO_ADDR），aicwf_sdio_send_pkt(sdiodev->func, wr_fifo_addr, buf, count) | F1 基址 0x100 + 0x07 = **0x107**，write_block(addr, buf) |
| **RD_FIFO（收 CFM）** | F1 偏移 0x08（SDIOWIFI_RD_FIFO_ADDR），sdio_readsb(rd_fifo_addr, size) | F1 基址 0x100 + 0x08 = **0x108**，read_block(base, buf) |
| **流控** | F1 0x0A（FLOW_CTRL），(val&0x7F) 非 0 才发 | 一致 |

**说明**：SDIO 规范 F1 基址为 0x100（F2 为 0x200），故偏移 0x07/0x08 对应完整地址 0x107/0x108，与 LicheeRV 的 wr_fifo_addr/rd_fifo_addr 用法一致，无地址差异。

### 6.3 时序

| 项目 | LicheeRV | StarryOS |
|------|----------|----------|
| **发 IPC 前** | flow_ctrl 轮询至 (fc&0x7F)!=0，最多重试 + udelay/mdelay | FLOW_CTRL 轮询 >2，最多 10 次，每次 sleep(1ms) |
| **发 IPC 后等 CFM** | wait_for_completion_timeout | wait_done_until(CMD_DONE_WAIT_QUEUE) + busrx 收包后 notify |
| **首包前延时** | 无显式 msleep；busrx 已起 | 发 DBG_MEM_READ 后 sleep(100ms) 再轮询 |

若仍无 CFM，可再核对：上电/复位时序、GPIO 极性、或硬件 SDIO 线序。StarryOS 已与 LicheeRV Amlogic 对齐使用 **POST_POWER_STABLE_MS=200**。
