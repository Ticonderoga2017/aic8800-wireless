# BLOCK_CNT 恒为 0：LicheeRV 逐函数分析与可能原因

## 现象

- 主机：CMD53 写 512 字节到 F1 0x07 (WR_FIFO) 成功（CMD_CMPL ok）。
- 等待 CFM 期间 F1 BLOCK_CNT(0x12)=0x00、BYTEMODE_LEN(0x02)=0x00、FLOW_CTRL(0x0A)=0x11。
- 3000ms 超时，未收到 CFM。

即：**芯片侧没有向 RD_FIFO 写入任何数据**（BLOCK_CNT 未更新），bootrom/固件未对 DBG_MEM_READ 做出响应。

---

## 一、LicheeRV 从“上电”到“首包 IPC”的完整路径

### 1. U-Boot（cvi_board_init.c）

- `mmio_write_32(0x0300104C, 0x3)`：GPIOA_26 选为 GPIO。
- `val = mmio_read_32(0x03020004); val |= (1<<26); mmio_write_32(0x03020004, val)`：方向输出。
- `val &= ~(1<<26); mmio_write_32(0x03020000, val)`：拉低。
- `suck_loop(50)`：**50ms**。
- `val |= (1<<26); mmio_write_32(0x03020000, val)`：拉高。
- 随后配置 SD1 pinmux（D3/D2/D1/D0/CMD/CLK = 0）。

### 2. 内核 MMC 枚举

- 主机发 CMD5、CMD3、CMD7 等，卡响应并上报 OCR、RCA。
- 读 CCCR、FBR、CIS，得到 vendor/device。

### 3. aicbsp_sdio_probe（aicsdio.c）

- chipmatch(vid, did) → 得到 chipid（如 PRODUCT_ID_AIC8801）。
- **aicwf_sdio_func_init(sdiodev)**：
  - aicwf_sdio_reg_init：wr_fifo_addr=0x07, rd_fifo_addr=0x08, block_cnt_reg=0x12 等。
  - sdio_claim_host(func)。
  - **sdio_set_block_size(func, 512)**。
  - **sdio_enable_func(func)**。
  - **udelay(100)** ← F1 使能后固定 **100μs**。
  - 可选：设置 SDIO 时钟。
  - sdio_release_host(func)。
  - 对 8801：**aicwf_sdio_writeb(sdiodev, 0x0B, 1)**，**aicwf_sdio_writeb(sdiodev, 0x11, 1)**。
- **aicwf_sdio_bus_init**：创建 tx/rx 线程、信号量等。
- aicbsp_platform_init：cmd_mgr 等。
- up(probe_semaphore)。

### 4. aicwf_sdio_bus_start（aicsdio.c 1297）

- 8801：sdio_claim_irq(func, aicwf_sdio_hal_irqhandler)。
- **aicwf_sdio_writeb(sdiodev, intr_config_reg, 0x07)**（F1 0x04=0x07）。
- bus_if->state = BUS_UP_ST。

说明：**首包 IPC 前**，LicheeRV 已做完 F1 block_size=512、F1 enable、**100μs 延时**、F1 0x0B=1、0x11=1，以及 **F1 0x04=0x07**（在 bus_start 里）。

### 5. 首次 IPC（rwnx_send_dbg_mem_read_req）

- rwnx_msg_zalloc(DBG_MEM_READ_REQ, TASK_DBG, DRV_TASK_ID, 4) → 填 memaddr=0x40500000。
- rwnx_send_msg(..., DBG_MEM_READ_CFM, cfm)。
- 内部：消息序列化到 cmd_buf（rwnx_set_cmd_tx 逻辑）：
  - buffer[0..2] = (len+4) 低 12bit LE，buffer[2]=0x11，buffer[3]=0（8801）。
  - buffer[4..8] = dummy。
  - buffer[8..16] = id/dest_id/src_id/param_len（LE），param 4 字节。
  - len = 8+param_len，总长 16+param_len。
- aicwf_bus_txmsg(bus, buffer, len+8) → 触发 tx 线程。
- aicwf_sdio_tx_process → **aicwf_sdio_tx_msg**：
  - 若 len 不是 512 的倍数：补 TAIL_LEN(4)，再 roundup 到 512。
  - **aicwf_sdio_flow_ctrl(sdiodev)**：读 F1 0x0A，**(val&0x7F) > 2** 才继续。
  - **aicwf_sdio_send_pkt(sdiodev, payload, len)**：sdio_claim_host(func)；**sdio_writesb(func, wr_fifo_addr=0x07, buf, count)**；sdio_release_host。
- Linux sdio_writesb：CMD53 写，func=1，addr=0x07，count=512。

### 6. 芯片侧（预期）

- 卡收到 CMD53 写 F1 0x07、512 字节。
- bootrom 从 WR_FIFO 取数，解析 IPC（len+4, 0x11, 0, dummy, id=0x0400, dest=1, src=100, param_len=4, memaddr=0x40500000）。
- 处理 DBG_MEM_READ：读 0x40500000，组 DBG_MEM_READ_CFM，写入 RD_FIFO，**更新 BLOCK_CNT(0x12)**（及可能 BYTEMODE_LEN）。
- 若有中断：拉 SDIO 中断；主机在 aicwf_sdio_hal_irqhandler 里读 BLOCK_CNT，再 sdio_readsb(rd_fifo)。

---

## 二、StarryOS 与 LicheeRV 的差异与已对齐项

| 项目 | LicheeRV | StarryOS | 说明 |
|------|----------|----------|------|
| 上电 50ms 低/高 | 有 | 有 | gpio 一致。 |
| F1 使能后延时 | **udelay(100)** 在 enable_func 后立即 | 无 100μs，仅有 IO_READY 轮询 | **建议补 100μs**。 |
| F1 block size 512 | 在 enable_func **之前** | 在 F2 使能、F2 block 之后，发 IPC **之前** | 发首包时已为 512，逻辑可接受。 |
| F1 0x0B=1, 0x11=1 | func_init 末尾 | 有，且 0x04=0x07 已加 | 一致。 |
| F1 0x04=0x07 | bus_start 里 | aicbsp_sdio_init 里 8801 分支 | 已对齐。 |
| 发 512 字节到 0x07 | sdio_writesb(func, 0x07, buf, 512) | cmd53_write addr=0x107, count=512 | 等价。 |
| 流控 (0x0A)&0x7F>2 | 有 | 有 | 一致；你方日志 0x0A=0x11 满足。 |
| 序列化格式 | len+4 LE, 0x11, 0, dummy4, id/dest/src/param_len, param | serialize_8801 同 | 一致。 |
| CHIP_REV 地址 0x40500000 | 有 | 有 | 一致。 |
| DRV_TASK_ID=100, TASK_DBG=1 | 有 | 有 | 一致。 |

---

## 三、BLOCK_CNT 恒为 0 的可能原因（按可能性）

1. **芯片/bootrom 未跑起来或未监听 WR_FIFO**  
   - 电源/复位/时钟未满足；或上电到 SDIO 访问间隔与芯片要求不符。  
   - **建议**：在 F1 enable 后增加 **100μs 延时**（与 LicheeRV udelay(100) 一致），再继续 F1 0x0B/0x11 及后续；必要时略增上电稳定时间再进 sdio_init。

2. **F1 使能后立即写 0x0B/0x11，芯片未就绪**  
   - LicheeRV 在 enable_func 与写 0x0B/0x11 之间插了 udelay(100)。  
   - **建议**：在 set_block_size(1,512) 之后、写 F1 0x0B 之前加 100μs 延时（或至少在“F1 使能”等效点后 100μs 再写 F1 寄存器）。

3. **SDIO 时钟/电压/线序**  
   - 与 LicheeRV 运行环境（时钟、IO 电压、接线）不一致，可能导致卡不响应或只部分响应。  
   - 需对照硬件与同一块板在 LicheeRV 下的表现。

4. **IPC 格式或 dest/src 被芯片拒绝**  
   - 已与 rwnx_set_cmd_tx 对照，格式和 ID 一致；若仍怀疑，可用 LicheeRV 抓首包二进制对比。

5. **芯片仅在有 SDIO 中断使能时才回写 CFM**  
   - 我们已写 F1 0x04=0x07；若原厂行为是“必须主机使能中断才更新 BLOCK_CNT”，则需确认 0x04 含义及是否需额外步骤。

6. **bootrom 版本/芯片版本差异**  
   - 不同 U 版本 bootrom 对 IPC 或 FIFO 的时序要求可能不同，需原厂或同板 LicheeRV 对比。

---

## 四、建议的代码修改（最小且与 LicheeRV 一致）

1. **在 aicbsp_sdio_init 中，对 8801 在“F1 使能”等效步骤之后、写 F1 0x0B/0x11 之前，增加 100μs 延时。**  
   - 更稳妥的位置：在 **set_block_size(1, 512)** 之后、**write_byte(F1 0x0B, 1)** 之前，插入 100μs。  
   - 这样与 LicheeRV “enable_func → udelay(100) → release_host → 再写 F1 0x0B/0x11” 的时序一致（我们 F1 block size 在此时已设，等价于 LicheeRV 的 set_block_size 在 enable 前已做）。

2. **可选**：在 aicbsp_power_on 中上电稳定时间由 200ms 略增（例如 300ms）做一次对比测试，排除“上电稍慢”导致 bootrom 未就绪。

3. **保留**：F1 0x04=0x07、0x0B=1、0x11=1，以及发 512 字节到 0x107、流控、序列化格式，这些已与 LicheeRV 对齐，无需再改。

---

## 五、小结

- 主机侧：**发 512 字节到 F1 0x07 成功、流控 0x0A=0x11、格式与 LicheeRV 一致**，说明“主机没发对”的可能性低。
- 问题更可能是：**芯片/bootrom 未在预期时序下就绪，或未开始响应 WR_FIFO**。
- 最先应做的代码改动：**在 F1 初始化路径中增加与 LicheeRV 一致的 100μs 延时**（F1 enable/block 之后、写 0x0B/0x11 之前）；再结合硬件与 LicheeRV 同板对比、必要时略增上电稳定时间排查。
