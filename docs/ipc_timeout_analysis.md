# IPC 超时分析：wait_done 为何 2000ms 超时

结合 LicheeRV-Nano-Build 与 wifi-driver 的 IPC 消息机制，说明 `cmd_mgr wait_done token=0 timeout 2000ms` 的原因。

---

## 1. 日志含义

```
cmd_mgr push reqid=0x0401 token=0          // 登记等待 DBG_MEM_READ_CFM(0x0401)
cmd53_write: addr=0x107 count=20 (F1 reg=0x07)  // 发送 DBG_MEM_READ_REQ 到 F1 wr_fifo，20 字节
cmd53_write: CMD_CMPL ok ...               // 主机侧写成功
dbg_mem_read: request sent, waiting CFM (timeout 2000ms)
dbg_mem_read: after 100ms F1 BLOCK_CNT(0x12)=0x00 FLOW_CTRL(0x0A)=0x11  // 100ms 后仍无数据
cmd_mgr wait_done token=0 timeout 2000ms   // 轮询 2000ms 后超时
```

- **reqid=0x0401**：等待的确认消息为 DBG_MEM_READ_CFM。
- **addr=0x107, count=20**：向 F1 写 FIFO(0x07) 写了 20 字节（serialize_8801：8 字节帧头 + 8 字节 lmac 头 + 4 字节 param）。
- **BLOCK_CNT(0x12)=0x00**：F1 的 BLOCK_CNT 一直为 0，表示设备没有往 rd_fifo 里放过数据（或没更新 BLOCK_CNT）。

---

## 2. LicheeRV 的 IPC 流程（对比用）

### 2.1 发送（与 wireless 一致）

- **rwnx_send_msg** → 组 lmac_msg → **aicwf_set_cmd_tx(dev, msg, sizeof(lmac_msg)+param_len)**。
- **aicwf_set_cmd_tx**（rwnx_cmds.c）：  
  - 帧头：`buffer[0..2]=(len+4) LE2`, `buffer[2]=0x11`, `buffer[3]=0`（8801）；  
  - 4 字节 dummy；  
  - 接着 id/dest_id/src_id/param_len + param。  
  即 **[len+4, 0x11, 0, dummy4][lmac 头 8 + param]**，与 wireless 的 serialize_8801 一致。
- 再 **aicwf_sdio_tx_msg** → flow_ctrl → **aicwf_sdio_send_pkt(sdiodev, payload, len)** → sdio_writesb(F1, wr_fifo_addr, buf, count)。

wireless 侧也是：serialize_8801 组帧 → send_msg（F1 流控 + write_block F1 wr_fifo）。**发送格式和路径与 LicheeRV 对齐。**

### 2.2 接收（关键差异）

- **LicheeRV**：  
  - 设备把 CFM 写入 rd_fifo 后，会拉 **SDIO 中断**。  
  - 主机在 **中断里** 读 BLOCK_CNT（或 bytemode_len）得到长度，再 **sdio_readsb(func, buf, rd_fifo_addr, size)** 收包。  
  - 收到后解析 E2A 消息，调 **on_cfm**，对应 token 的 **is_done** 被置位，**wait_done** 返回。

- **wireless（本实现）**：  
  - **无 SDIO 中断**，只能轮询。  
  - **wait_done** 里每 1ms 调用 **poll_fn** = **poll_rx_one**。  
  - poll_rx_one 调 **recv_pkt**：  
    - 先读 F1 **BLOCK_CNT(0x12)**；  
    - **若 BLOCK_CNT > 0**：按块从 rd_fifo 读，得到一包，解析后 **on_cfm** → **is_done(token)=true**；  
    - **若 BLOCK_CNT == 0**：认为“无数据”，**直接返回 0**，不读 rd_fifo，也不调用 on_cfm。

因此：**只有设备往 rd_fifo 里写了数据并让 BLOCK_CNT>0（或等效地更新了“有数据”状态），本实现的轮询才能收到 CFM。**

---

## 3. 超时的直接原因

- 请求已发出：**cmd53_write F1 0x107 成功**，说明 DBG_MEM_READ_REQ 已写入 F1 wr_fifo。
- 整个 2000ms 内 **BLOCK_CNT 一直为 0**（至少 after 100ms 时仍为 0，且之后 recv_pkt 始终拿不到数据）。
- 因此：
  - **recv_pkt** 每次都因 BLOCK_CNT=0 返回 0；
  - **poll_rx_one** 从未解析到 DBG_MEM_READ_CFM；
  - **on_cfm** 从未被调用；
  - **is_done(token)** 一直为 false；
  - 到 **timeout_ms=2000** 后 **wait_done** 返回 **Err(-62)**。

结论：**超时不是因为 wait_done 或 poll 逻辑写错，而是因为设备侧从未把 DBG_MEM_READ_CFM 放入 rd_fifo（或从未把 BLOCK_CNT 置为非 0）。**

---

## 4. 设备为何不回 CFM（可能原因）

1. **ROM/早期固件未处理 DBG_MEM_READ**  
   上电后第一次读 0x40500000 做 chip_rev 时，芯片可能还在 ROM 或最小 bootloader。若该阶段不处理 DBG_MEM_READ_REQ，就不会回 CFM。

2. **消息/地址/路径与设备预期不一致**  
   若 ROM 只认某种格式或只认 F2 等，当前 F1 wr_fifo + 当前帧格式可能不被处理（目前与 LicheeRV 一致，概率较低但仍可排查）。

3. **电源/时钟/复位**  
   SDIO 枚举虽成功，但芯片内核或 IPC 相关时钟/电源未就绪，也会导致不处理 IPC。

4. **BLOCK_CNT 未更新**  
   少数情况设备写了 rd_fifo 但未更新 BLOCK_CNT；当前实现依赖 BLOCK_CNT，若设备不更新则永远收不到（已去掉“BLOCK_CNT=0 仍读 rd_fifo”的 fallback，避免无数据时 CMD53 读卡死）。

---

## 5. 建议排查方向

1. **确认 LicheeRV 同板同固件** 是否能在相同时机（driver_fw_init 第一次 dbg_mem_read）成功收到 DBG_MEM_READ_CFM；若 LicheeRV 也收不到，则更可能是 ROM/固件或硬件。
2. **加大首包前等待**：在发 DBG_MEM_READ_REQ 后、进入 wait_done 前，适当延长 delay（例如 200–500ms），再开始轮询，排除“设备处理慢”的情况。
3. **核对 ROM 文档/参考**：确认 AIC8801 ROM 是否支持 DBG_MEM_READ_REQ、是否必须在某一步之后才响应。
4. **硬件/电气**：确认 SDIO 电源、时钟、信号质量；必要时用逻辑分析仪看 CMD/DAT 线上是否有设备响应。

---

## 6. 小结

| 项目           | 说明 |
|----------------|------|
| **超时含义**   | wait_done 在 timeout_ms 内未看到 `is_done(token)==true`。 |
| **为何 is_done 不置位** | poll_fn 从未解析到 DBG_MEM_READ_CFM，因为 recv_pkt 一直因 BLOCK_CNT=0 返回 0，on_cfm 从未被调用。 |
| **为何 recv_pkt 总是 0** | 设备未往 rd_fifo 写 CFM（或未更新 BLOCK_CNT）。 |
| **发送侧**     | 与 LicheeRV 一致（帧格式、F1 wr_fifo、流控），请求已成功发出。 |
| **根本点**     | 超时反映的是**设备未回 DBG_MEM_READ_CFM**，需从设备侧（ROM/固件/电源/时钟/硬件）继续排查。 |

---

## 7. 中断机制（与 LicheeRV 对齐）

为与 LicheeRV 的中断驱动方式一致，已在本 BSP 中增加 SDIO 中断支持：

- **平台**：SG2002 / LicheeRV 使用 PLIC 外设 IRQ **0x24**（cv-sd@4310000 `interrupts = <0x24 0x04>`）。
- **实现**：
  - 模块 `sdio_irq`：在 riscv64 上注册 SDMMC IRQ 处理函数；处理函数仅调用 `WaitQueue::notify_one` 唤醒等待任务。
  - `wait_done`：首次调用时 `ensure_sdio_irq_registered()`；每次轮询后若 `use_sdio_irq()` 为真则 `wait_sdio_or_timeout(1ms)` 阻塞，直到 SDMMC 产生中断（如 BUF_RRDY）或 1ms 超时，再继续 poll。
- **效果**：有数据到达时 PLIC 触发 IRQ，任务被唤醒并立即再次 poll，减少无效轮询、更快响应 CFM；若设备仍不写 rd_fifo，行为与纯轮询一致（超时仍会发生）。
- **配置**：`axconfig.toml` 中已增加 `sdmmc-irq = 0x24`（sg2002）及 `sdmmc-irq = 0`（dummy），供其他模块引用；BSP 内按 `target_arch = "riscv64"` 使用 0x24，不依赖 axconfig 宏生成。

---

## 8. LicheeRV 异步中断机制与 wireless 对齐

### 8.1 LicheeRV 中用到的异步/中断机制

| 位置 | 机制 | 作用 |
|------|------|------|
| **aicsdio.c** | `sdio_claim_irq(func, aicwf_sdio_hal_irqhandler)` | 注册 SDIO 功能中断；中断到来时调用 `aicwf_sdio_hal_irqhandler`。 |
| **aicwf_sdio_hal_irqhandler** | 在 IRQ 上下文读 BLOCK_CNT、读 rd_fifo（aicwf_sdio_readframes）、入队；然后 `complete(&bus_if->busrx_trgg)` | 有数据时在中断里收包并入队，再唤醒 busrx 线程。 |
| **aicwf_txrxif.c** | `busrx_trgg`（struct completion）、`busrx_thread`（kthread） | 总线 RX 完成事件；专用 RX 线程阻塞在 `wait_for_completion_interruptible(&busrx_trgg)`。 |
| **sdio_busrx_thread** | `wait_for_completion_interruptible(&bus_if->busrx_trgg)` → `aicwf_process_rxframes(rx_priv)` | 被 IRQ 的 complete 唤醒后，从队列取包、解析 E2A、调 on_cfm 等。 |
| **aic_bsp_driver.c / rwnx_cmds.c** | 每条 cmd 有 `struct completion complete`；RX 路径 on_cfm → `cmd_complete` → `complete(&cmd->complete)` | 单条命令的“确认到达”事件。 |
| **wait_done（发送方）** | `wait_for_completion_killable_timeout(&cmd->complete, tout)` | 发送方阻塞在该 cmd 的 completion 上，直到 RX 路径 complete 或超时。 |

整体链路：**SDIO 中断** → IRQ 里收包并入队 → **complete(busrx_trgg)** → **busrx_thread** 唤醒 → process_rxframes → 解析 → **on_cfm** → **cmd_complete** → **complete(cmd->complete)** → **wait_done** 返回。

### 8.2 wireless 中的对齐实现

| LicheeRV | wireless |
|----------|----------|
| SDIO IRQ → `aicwf_sdio_hal_irqhandler` | SDMMC PLIC IRQ(0x24) → `sdio_irq_handler`（仅 `WaitQueue::notify_one`） |
| `complete(&busrx_trgg)` 唤醒 busrx_thread | `SDIO_WAIT_QUEUE.wait_timeout(1ms)` 的调用方被 notify 唤醒（同一线程内循环 poll） |
| busrx_thread 里 `aicwf_process_rxframes`（出队、解析、on_cfm） | `wait_done` 循环里 `poll_fn(self)` = poll_rx_one → recv_pkt + 解析 + on_cfm |
| `complete(&cmd->complete)` 使 wait_done 返回 | `on_cfm` 内置位 slot.done 并调用 `sdio_irq::notify_wait_done()`，唤醒正在 `wait_sdio_or_timeout` 的线程，下一轮循环发现 `is_done(token)` 即返回 |
| `wait_for_completion_timeout(&cmd->complete)` | `while !is_done { poll_fn; wait_sdio_or_timeout(1ms) }` + 超时 |

**多线程对齐（已实现）**：

- **busrx 线程**：`flow::ensure_busrx_thread_started()` 在 `aicbsp_driver_fw_init` 开头调用一次，启动专用线程（对齐 LicheeRV `kthread_run(sdio_busrx_thread)`）。该线程循环：`wait_sdio_or_timeout(1ms)`（等价 `wait_for_completion_interruptible(&busrx_trgg)`）→ `run_poll_rx_one()`（等价 `aicwf_process_rxframes`：recv_pkt、解析、on_cfm）。
- **首包 chip_rev 读**：使用 `send_dbg_mem_read_busrx`，不长时间持锁；本线程只做 push、send、`wait_done_until(|| is_done(token))`、take_cfm；CFM 由 busrx 线程收包并 `on_cfm` → `notify_wait_done()` 唤醒本线程。
- **锁顺序**：全局统一 CMD_MGR → SDIO_DEVICE，避免与 busrx 线程死锁。
- **exit**：`aicbsp_sdio_exit` 中置 `BUSRX_RUNNING=false` 并 `notify_wait_done()`，使 busrx 线程退出。
