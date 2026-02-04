# BLOCK_CNT 一直为 0：从完整日志看程序流程与可能原因

本文档根据你提供的完整日志和当前代码，梳理「发 IPC → 等 CFM → BLOCK_CNT 恒为 0」的程序流程，并归纳**为什么 BLOCK_CNT 会一直为 0** 以及**可能根因**。

---

## 1. 日志中的程序流程（简要）

| 阶段 | 日志/现象 |
|------|-----------|
| 上电 | GPIO 拉低→50ms→拉高，sleep(200ms)，`power+reset done, waited 50+200ms before sdio_init` |
| SDIO 枚举 | CMD0/CMD5/CMD3/CMD7 成功，RCA=0xfc9c，F1/F2 使能，CIS 读出 vid=0x5449 did=0x0145 → Aic8801 |
| F1 初始化 | F1 block size=512，F1 0x0B=1、0x11=1，busrx_thread started |
| 首包 DBG_MEM_READ | send_msg → **cmd53_write addr=0x107 count=512**，CMD_CMPL ok，request sent，waiting CFM 2000ms |
| 100ms 时采样 | **F1 BLOCK_CNT(0x12)=0x00 BYTEMODE_LEN(0x02)=0x00 FLOW_CTRL(0x0A)=0x11** |
| 等待 CFM | 500/1000/1500ms still waiting，2000ms timeout，no CFM received |
| 后续 | 用默认 chip_rev U02 继续，aicbsp_system_config_8801 发 DBG_MEM_WRITE，再次 cmd53_write 0x107 512 字节，wait_done 2000ms 再次超时 -62 |

结论：**主机侧**写 F1 WR_FIFO(0x107) 已成功（CMD53 完成），且在整个 2000ms 内 F1 0x12 始终为 0，即**芯片从未把“有数据”反映到 BLOCK_CNT**。

---

## 2. 为什么 BLOCK_CNT 会一直为 0？

在当前实现里，BLOCK_CNT 只来自**芯片**：

- 我们**只读** F1 0x12，不写。
- 按 LicheeRV/8801 设计：设备把 CFM 写入 F1 RD_FIFO 后，会更新 **BLOCK_CNT(0x12)**（以及必要时 BYTEMODE_LEN），主机再根据 0x12 从 RD_FIFO 读。

因此：

- **BLOCK_CNT 一直为 0** ⇒ 在主机视角等价于：**芯片从未在 F1 RX 路径上“放数据”**（没写 RD_FIFO，或没更新 0x12）。
- 不是“我们把它设成 0”，而是“芯片侧从未给出非 0”。

可能只有两类情况：

1. **芯片确实没有往 F1 RD_FIFO 写任何 CFM**（没响应、没跑起来、没认到请求等）。
2. **芯片写了 RD_FIFO 但没按 8801 约定更新 BLOCK_CNT(0x12)**（协议/实现与 LicheeRV 假设不一致，需芯片/原厂确认）。

当前日志与 LicheeRV 行为都支持“以 BLOCK_CNT 为有数据依据”，所以更符合的是第 1 类：**芯片没有对本次 IPC 做出“写回 CFM”的动作**。

---

## 3. 主机侧已确认无误的部分（与 LicheeRV 一致）

从日志和代码可确认：

| 项目 | 状态 |
|------|------|
| 写地址 | addr=0x107 = F1 + 0x07 (WR_FIFO)，与 LicheeRV WR_FIFO_ADDR 一致 |
| 写长度 | count=512，与 LicheeRV 8801 填充到 512 再写一致（ipc_send_len_8801） |
| CMD53 | CMD_CMPL ok，PRESENT_STS 正常，说明主机 SDIO 写完成 |
| F1 初始化 | F1 block size=512，0x0B=1、0x11=1，与 aicwf_sdio_func_init 一致 |
| 流控 | FLOW_CTRL(0x0A)=0x11，(0x11&0x7F)>2，发送前已通过 |
| IPC 格式 | 首 32 字节：len+4=0x0010, 0x11 0x00, id=0x0400(DBG_MEM_READ_REQ), dest=1, src=100, param_len=4，与 serialize_8801 一致 |
| 收包逻辑 | 先读 BLOCK_CNT(0x12)，非 0 再算长度、读 RD_FIFO，与 LicheeRV 8801 一致 |
| busrx 轮询 | 每 1ms run_poll_rx_one，持续 2000ms，不会“漏掉”短暂的 BLOCK_CNT 非 0 |

因此：**从软件流程和协议上看，主机没有把 BLOCK_CNT 置 0，也没有少做“该做的事”；BLOCK_CNT 恒为 0 反映的是芯片没有在 F1 上给出 RX 数据。**

---

## 4. 可能根因归纳（按优先级）

在“芯片未对本次 IPC 回 CFM”的前提下，可以按下面几类排查。

### 4.1 芯片/bootrom 未就绪或未处理 IPC（最可能）

- **现象**：bootrom 未跑起来、或未解析到 WR_FIFO 里的请求，因此从不写 CFM 到 RD_FIFO，也不更新 BLOCK_CNT。
- **可能原因**：
  - 上电/复位后 **200ms 仍不足**（可试 300/500ms 或更长）。
  - **时钟/晶振**未稳定或未给到芯片。
  - **复位极性/时序**与芯片要求不符（如需要“先高再低再高”等），导致芯片未正确出 reset。
  - 芯片处于某种 **low-power/standby**，未监听 F1 WR_FIFO。

**建议**：  
- 适当加大 `POST_POWER_STABLE_MS`（如 300/500ms）或在上电与首次 IPC 之间加固定延时再试。  
- 对照原理图/原厂说明确认 GPIO 上电、复位顺序与极性。  
- 若有 LicheeRV 同板成功日志，对比“上电到首包”的时间差。

### 4.2 主机写入了 WR_FIFO，但芯片“没看到”

- **可能原因**：
  - **块边界/对齐**：个别平台或芯片对 F1 的 CMD53 块写有要求（例如必须 512 字节整块、首地址对齐），我们已是 512 字节写 0x107，一般没问题，但若有原厂 TRM 可再对一下。
  - **F1 0x0B/0x11 与写顺序**：当前顺序为 F1 block size → 0x0B=1 → 0x11=1 → 再发 IPC，与 LicheeRV 一致；若原厂有“写 0x0B/0x11 后需延时再写 WR_FIFO”的说明，可加短延时试。

### 4.3 硬件/电气

- SDIO 数据线、CLK、电源不稳定或接线错误，导致：
  - 主机以为写成功（CMD53 完成），但**芯片端数据错误/未收到**；
  - 或芯片回了 CFM，但**主机侧读不到**（此时 BLOCK_CNT 仍可能为 0，因为主机读的是芯片侧的 0x12）。
- **建议**：用逻辑分析仪/示波器抓 SDIO 线，确认 CMD53 写阶段总线上确有 512 字节写向 F1，以及是否有任何来自芯片的读周期（F1 0x12/0x08 等）。

### 4.4 协议/格式的极端情况

- 当前 IPC 头与 LicheeRV serialize_8801 一致，id=0x0400(DBG_MEM_READ_REQ)，dest/src/param 均合理。
- 若原厂 bootrom 对**某字段或填充**有未公开要求（例如某字节必须为 0、长度必须整块等），也可能导致“静默不响应”。  
- 这类只能通过**对比 LicheeRV 同板成功时的首包二进制**或原厂确认来排除。

---

## 5. 小结

| 问题 | 结论 |
|------|------|
| **BLOCK_CNT 为什么一直是 0？** | 主机只读 F1 0x12，从未写；0 表示**芯片从未在 F1 RX 路径上给出数据**（未写 RD_FIFO 或未更新 0x12）。 |
| **小块/字节模式有没有把 block_cnt 弄成 0？** | 没有。我们按 LicheeRV：先看 BLOCK_CNT，为 0 直接当无数据；小块时 block_cnt=1、读 512 字节。没有“强制 block_cnt=0”的逻辑。 |
| **程序流程是否有明显错误？** | 从日志看：上电→SDIO 枚举→F1 初始化→发 512 字节到 0x107→轮询 0x12，与 LicheeRV 对齐；未见主机侧逻辑错误。 |
| **最值得先查的方向** | 芯片/bootrom 是否真的就绪并在处理 IPC（时序、复位、时钟、电源）；其次用硬件抓 SDIO 确认“写确实到卡、读是否发生”。 |

若要继续缩范围，建议做一次**最小复现**：只做「上电 → 延时（可试 300/500ms）→ SDIO init → F1 0x0B/0x11 → 启动 busrx → 发一次 DBG_MEM_READ」，长时间观察 0x12 是否在任意时刻变为非 0；若仍从不非 0，则更偏向芯片侧/硬件/时序，而非“小块或 BYTEMODE 导致 block_cnt 被置 0”的软件问题。
