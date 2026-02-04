# LicheeRV 逐字节/逐项对照：IPC、电源、时序

用于排查 BLOCK_CNT 恒为 0 时逐字符、逐数字对比 LicheeRV 与 StarryOS。

---

## 1. IPC 首包（DBG_MEM_READ 0x40500000）字节级对照

### 1.1 LicheeRV 来源

- **rwnx_cmds.c** `aicwf_set_cmd_tx(dev, msg, len)`：`len` = 8 + param_len（lmac 头 8 字节 + param）
- DBG_MEM_READ param_len=4，故 len=12；写入 buffer 后调用 `aicwf_bus_txmsg(bus, buffer, len + 8)`，即 **20 字节** 有效载荷。

| 偏移 | LicheeRV 赋值 | 含义 | StarryOS serialize_8801 | 日志 24B |
|------|----------------|------|--------------------------|----------|
| 0 | (len+4)&0xff = 0x10 | 长度低字节 (12+4=16) | buf[0]=0x10 | 10 |
| 1 | ((len+4)>>8)&0x0f = 0 | 长度高 4bit | buf[1]=0x00 | 00 |
| 2 | 0x11 | 固定 | buf[2]=0x11 | 11 |
| 3 | 0 (8801 无 CRC) | buffer[3]=0 | buf[3]=0x00 | 00 |
| 4..7 | dummy word 0 | index+=4 | buf[4..8].fill(0) | 00 00 00 00 |
| 8..9 | put_u16(msg->id) LE | id=0x0400 | id.to_le_bytes() | 00 04 |
| 10..11 | put_u16(msg->dest_id) | dest=1 | dest_id.to_le_bytes() | 01 00 |
| 12..13 | put_u16(msg->src_id) | src=100 | src_id.to_le_bytes() | 64 00 |
| 14..15 | put_u16(msg->param_len) | param_len=4 | param_len.to_le_bytes() | 04 00 |
| 16..19 | memcpy(param) | 0x40500000 LE | param 4 字节 | 00 00 50 40 |

**结论**：日志 `10 00 11 00 00 00 00 00 00 04 01 00 64 00 04 00 00 00 50 40 00 00 00 00` 与 LicheeRV **逐字节一致**。

### 1.2 发送长度与填充

- LicheeRV **aicwf_sdio_tx_msg**：payload_len=20 → (20%512)!=0 → 补 TAIL_LEN(4) → len=(24/512+1)*512=**512**，一次 `sdio_writesb(func, wr_fifo_addr=0x07, payload, 512)`。
- StarryOS **ipc_send_len_8801**：serialized=20 → +4 再 roundup 到 512 → **send_len=512**，cmd53_write addr=0x107 count=512。

**结论**：发送 512 字节到 F1 0x07 与 LicheeRV 一致。

---

## 2. 电源与时序逐项对照

### 2.1 U-Boot cvi_board_init.c（LicheeRV Nano）

| 步骤 | 代码/数值 | StarryOS |
|------|-----------|----------|
| GPIOA_26 pinmux | 0x0300104C = 0x3 | set_wifi_power_pinmux_to_gpio() 首行 ✅ |
| DIR 输出 | 0x03020004 \|= (1<<26) | WifiGpioControl init ✅ |
| 拉低 | 0x03020000 &= ~(1<<26) | power_on 中 set_pin(false) ✅ |
| 延时 | suck_loop(50) ≈ 63ms | delay_spin_ms(50) ✅ |
| 拉高 | 0x03020000 \|= (1<<26) | set_pin(true) ✅ |
| **SDIO pinmux** | **紧接着** 0x030010D0..E4=0 | **原为 sd1_host_init 内（稳定延时之后）** → 已改为拉高+50ms 后 **set_sd1_sdio_pinmux_after_power()** ✅ |

### 2.2 顺序差异（已修复）

- **LicheeRV U-Boot**：拉高 → **立刻** 写 SDIO pinmux（D3/D2/D1/D0/CMD/CLK=0），**无**“拉高后再等 50/200ms 再 pinmux”。
- **StarryOS 原逻辑**：拉高 → 50ms → 500ms sleep → **sd1_host_init**（RSTGEN、CLKGEN、**再** pinmux）。即 SDIO pinmux 在稳定延时**之后**，与 U-Boot 相反。
- **本次修改**：在 `aicbsp_power_on_with_stable_ms` 中，在 `delay_spin_ms(50)` 之后、`sleep(stable_ms)` 之前调用 **set_sd1_sdio_pinmux_after_power()**，使“拉高 → SDIO pinmux → 稳定延时”与 U-Boot 一致。

### 2.3 F1 初始化（aicwf_sdio_func_init / aicbsp_sdio_init）

| 项目 | LicheeRV | StarryOS |
|------|----------|----------|
| set_block_size(F1, 512) | enable_func **前** | 8801 分支内 set_block_size(1,512) ✅ |
| sdio_enable_func(F1) | 有 | CCCR 0x02 \|= 0x02 + 等 IO_READY ✅ |
| udelay(100) | enable_func **后** 立即 | delay_spin_us(100) 在写 0x0B 前 ✅ |
| F1 0x0B=1, 0x11=1 | 有 | write_byte(0x10B,1), write_byte(0x111,1) ✅ |
| F1 0x04=0x07 | bus_start 里 | aicbsp_sdio_init 内 8801 写 0x104=0x07 ✅ |

---

## 3. 数字与常量

| 常量 | LicheeRV | StarryOS |
|------|----------|----------|
| len+4 (DBG_MEM_READ 首包) | 16 (0x10) | 16 ✅ |
| buffer[1] 高 4bit | &0x0f | &0x0f ✅ |
| DRV_TASK_ID / src_id | 100 | 100 ✅ |
| TASK_DBG / dest_id | 1 | 1 ✅ |
| DBG_MEM_READ_REQ id | 0x0400 | 0x0400 ✅ |
| CHIP_REV 地址 | 0x40500000 | 0x4050_0000 ✅ |
| WR_FIFO 地址 | 0x07 (F1) | 0x107 (func1 reg 0x07) ✅ |
| TAIL_LEN | 4 | 4 ✅ |
| SDIOWIFI_FUNC_BLOCKSIZE | 512 | 512 ✅ |

---

## 4. 小结

- **IPC 内容**：首 24 字节与 LicheeRV 一致；发送 512 字节到 F1 0x07 一致。
- **电源**：pinmux GPIO、低 50ms、高 50ms 一致；**SDIO pinmux 时机**已改为“拉高 + 50ms 后立即设置”，与 U-Boot 一致。
- **F1**：block 512、100μs、0x0B/0x11/0x04 已对齐。

若 BLOCK_CNT 仍为 0，可再查：同板 LicheeRV 是否正常、电压/晶振、或尝试进一步加大 stable_ms（如 1000）。
