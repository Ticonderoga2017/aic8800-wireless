# LicheeRV 与 wireless：中断与多线程对比

本文档对照 **LicheeRV**（Linux aic8800 BSP + FDRV）与 **StarryOS wireless** 的**中断机制**和**多线程设计**，说明差异与对齐方式。

---

## 1. LicheeRV：中断与多线程

### 1.1 线程

LicheeRV 在 `aicwf_bus_init` 里起 **两个内核线程**（aicsdio_txrxif.c）：

| 线程 | 入口 | 阻塞等待 | 被唤醒后执行 |
|------|------|----------|--------------|
| **bustx** | `aicwf_sdio_bustx_thread` | `wait_for_completion_interruptible(&bus->bustx_trgg)` | `aicwf_sdio_tx_process(sdiodev)`：从 TX 队列取数据，CMD53 写 WR_FIFO |
| **busrx** | `aicwf_sdio_busrx_thread` | `wait_for_completion_interruptible(&bus_if->busrx_trgg)` | `aicwf_process_rxframes(rx_priv)`：读 F1/BLOCK_CNT，CMD53 读 RD_FIFO，解析 IPC，入队或交给 cmd_mgr |

**谁唤醒 bustx**：上层发命令或数据时，在 `rwnx_set_cmd_tx` / `aicwf_sdio_bus_txmsg` / 数据路径里把报文入队后调用 `complete(&bus_if->bustx_trgg)`，bustx 线程被唤醒后真正执行 CMD53 写。

**谁唤醒 busrx**：见下「中断」。

### 1.2 中断（硬件 IRQ）

- **注册**：`aicwf_sdio_bus_start` 里对 SDIO func 调用 `sdio_claim_irq(sdiodev->func, aicwf_sdio_hal_irqhandler)`，由 Linux MMC 子系统把 SDIO 卡的中断接到该 handler（底层为 SoC/PLIC 的 SDMMC IRQ）。
- **触发**：WiFi 芯片有数据可读时拉 SDIO 中断线，内核调用 `aicwf_sdio_hal_irqhandler`（在软中断或线程化 IRQ 上下文）。
- **handler 行为**（aicsdio.c 1428–1528）：
  1. 读 F1 的 block_cnt_reg / misc_int_status_reg 得到有数据长度；
  2. 用 `aicwf_sdio_readframes` 从 RD_FIFO 读一帧；
  3. `aicwf_sdio_enq_rxpkt` 入队到 rx_priv；
  4. **`complete(&bus_if->busrx_trgg)`**，唤醒 **busrx 线程**。
- **busrx 被唤醒后**：`aicwf_process_rxframes` 从队列取包、解析 IPC；若是 CFM，则调用 `cmd_mgr->msgind`（即 `cmd_mgr_msgind`）→ 匹配 reqid → **`cmd_complete(cmd_mgr, cmd)`** → **`complete(&cmd->complete)`**，唤醒正在等这条命令的**调用线程**。

### 1.3 发送命令并等 CFM 的流程（主路径）

1. **调用线程**（如 ioctl/DBG_MEM_READ）：`cmd_mgr_queue` → 入队 cmd，`rwnx_set_cmd_tx` 把消息交给 sdiodev，并 **`complete(&bustx_trgg)`**。
2. **bustx 线程** 被唤醒，`aicwf_sdio_tx_process` 里对当前消息做 **CMD53 写 WR_FIFO**。
3. 调用线程里 **`wait_for_completion_killable_timeout(&cmd->complete, tout)`** 阻塞。
4. **SDIO 硬件 IRQ** 发生 → `aicwf_sdio_hal_irqhandler` → 读 F1、读帧、入队 → **`complete(&busrx_trgg)`**。
5. **busrx 线程** 被唤醒，`aicwf_process_rxframes` 取包、解析到 CFM → `cmd_mgr_msgind` → **`complete(&cmd->complete)`**。
6. 调用线程从 `wait_for_completion_*` 返回，得到 CFM。

**小结 LicheeRV**：**1 个硬件 IRQ**（SDIO 卡中断 → handler 读 F1/读帧 → complete(busrx_trgg)）+ **2 个 kthread**（bustx：写 WR_FIFO；busrx：等 busrx_trgg → process_rxframes → 必要时 complete(cmd->complete)）。

---

## 2. wireless（StarryOS）：中断与多线程

### 2.1 线程

wireless 里**只起 1 个后台线程**（flow.rs `ensure_busrx_thread_started`）：

| 线程 | 入口 | 阻塞等待 | 被唤醒后执行 |
|------|------|----------|--------------|
| **busrx** | `busrx_thread_fn` | `wait_sdio_or_timeout(1ms)`（有软中断时阻塞；无则直接返回） | `run_poll_rx_one()`：读 F1/BLOCK_CNT 等，CMD53 读 RD_FIFO，解析 IPC，on_cfm 时 `notify_cmd_done()` |

**没有 bustx 线程**：发送 IPC 的线程（如 main 或调用 `send_dbg_mem_read_busrx` 的线程）**自己**在 `send_msg` 里做 CMD53 写 WR_FIFO，不通过单独线程。

### 2.2 “中断”（仅软中断，无 PLIC）

- **不启用 PLIC**：`sdmmc_irq()` 恒为 0，不调用 `axhal::irq::register`，不注册 SDIO 硬件中断。
- **软中断**：用**定时器 tick** 模拟「有数据可处理」：
  - 应用在首包前：`set_use_soft_irq_wake(true)` 且 `axtask::register_timer_callback(|_| wireless::bsp::sdio_tick)`；
  - 每次 tick 调用 **`sdio_tick()`** → **`SDIO_WAIT_QUEUE.notify_one(false)`**；
  - busrx 若在 **`wait_sdio_or_timeout(dur)`** 上阻塞，会被唤醒，然后执行 **`run_poll_rx_one()`**。
- 若未启用软中断（`use_sdio_irq() == false`），busrx 不阻塞，每轮 `run_poll_rx_one()` 后 **`axtask::sleep(RX_POLL_MS)`** 让出 CPU。

### 2.3 发送命令并等 CFM 的流程

1. **主线程**：`send_dbg_mem_read_busrx` → `send_msg`（**本线程** CMD53 写 WR_FIFO）→ 进入 `wait_done_until`，内部循环：若 `condition()` 已为 true 则返回，否则 **`wait_cmd_done_timeout(1ms)`**（在 CMD_DONE_WAIT_QUEUE 上阻塞）。
2. **busrx 线程**：被 **定时器 tick → sdio_tick → notify_one** 唤醒（或轮询+ sleep），**`run_poll_rx_one`** → 读到 CFM → **`notify_cmd_done()`** = `CMD_DONE_WAIT_QUEUE.notify_one(true)`。
3. 主线程从 **`wait_cmd_done_timeout`** 返回，下一轮 **`condition()`** 为 true，**`wait_done_until`** 返回。

**小结 wireless**：**0 个硬件 IRQ**，**1 个软中断源**（定时器 → sdio_tick → notify_one）+ **1 个 busrx 线程**；发送在同一线程内完成，无 bustx 线程。

---

## 3. 对照表

| 项目 | LicheeRV | wireless |
|------|----------|----------|
| **SDIO 硬件中断** | 使用：`sdio_claim_irq(..., aicwf_sdio_hal_irqhandler)`，卡有数据时触发 | 不使用：不注册 PLIC，`sdmmc_irq()==0` |
| **“有数据”的触发** | 硬件 IRQ → handler 读 F1/读帧 → `complete(&busrx_trgg)` | 定时器 tick → `sdio_tick()` → `SDIO_WAIT_QUEUE.notify_one()` |
| **busrx 线程** | 有；`wait_for_completion(&busrx_trgg)` → `aicwf_process_rxframes` | 有；`wait_sdio_or_timeout` → `run_poll_rx_one` |
| **bustx 线程** | 有；`wait_for_completion(&bustx_trgg)` → `aicwf_sdio_tx_process`（CMD53 写） | 无；发送线程自己 `send_msg`（CMD53 写） |
| **等 CFM 的同步** | `wait_for_completion_killable_timeout(&cmd->complete)`，由 busrx 里 `cmd_complete` → `complete(&cmd->complete)` 唤醒 | `wait_done_until` 内 `wait_cmd_done_timeout(1ms)`，由 busrx 里 `notify_cmd_done()` 唤醒 |
| **参与 SDIO 的线程数** | 3：调用线程 + bustx + busrx | 2：主/调用线程 + busrx |

---

## 4. 差异总结

1. **中断**：LicheeRV 用 **SDIO 硬件 IRQ** 驱动「有数据 → 唤醒 busrx」；wireless 用 **定时器软中断（sdio_tick）** 驱动，逻辑等价（都是「事件 → 唤醒 busrx → process_rxframes/run_poll_rx_one」），wireless 不启用 PLIC。
2. **bustx**：LicheeRV 有专门 **bustx 线程** 负责 CMD53 写；wireless **没有** bustx，谁发谁写，减少一个线程，BSP 当前只有「发一条等一条」的同步命令，无需独立 TX 队列与线程。
3. **线程数**：LicheeRV 与 SDIO 相关的为 **2 个 kthread（bustx + busrx）** + 任意调用线程；wireless 为 **1 个 busrx 线程** + 主/调用线程。
4. **同步语义**：两边一致——等 CFM 的线程在「完成对象/队列」上阻塞，busrx 在收到 CFM 后 **complete / notify_cmd_done** 唤醒该线程。
