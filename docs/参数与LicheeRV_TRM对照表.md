# BSP 参数与 LicheeRV / SG2002 TRM 对照表

本文档逐项对照 **寄存器地址、管脚号、硬件基址、超时与常量** 与 LicheeRV-Nano-Build、SG2002 TRM、LicheeRV U-Boot 的一致性。  
**已修正**：`RAM_FMAC_FW_ADDR` 已由 `0x1e0000` 改为 `0x00120000` 与 LicheeRV aic_bsp_driver.h 一致。

---

## 1. SDIO / F1·F2 寄存器（AIC8801）

| 项目 | LicheeRV (aicsdio.h / aicwf_sdio.h) | StarryOS (types.rs / ops.rs) | 结论 |
|------|-------------------------------------|------------------------------|------|
| BYTEMODE_LEN | 0x02 | reg::BYTEMODE_LEN 0x02 | 一致 |
| INTR_CONFIG | 0x04 | reg::INTR_CONFIG 0x04 | 一致 |
| SLEEP | 0x05 | reg::SLEEP 0x05 | 一致 |
| WAKEUP | 0x09 | reg::WAKEUP 0x09 | 一致 |
| FLOW_CTRL | 0x0A | reg::FLOW_CTRL 0x0A | 一致 |
| REGISTER_BLOCK | 0x0B | reg::REGISTER_BLOCK 0x0B | 一致 |
| BYTEMODE_ENABLE | 0x11 | reg::BYTEMODE_ENABLE 0x11 | 一致 |
| BLOCK_CNT | 0x12 | reg::BLOCK_CNT 0x12 | 一致 |
| FLOWCTRL_MASK | 0x7F | reg::FLOWCTRL_MASK 0x7F | 一致 |
| WR_FIFO_ADDR | 0x07 | reg::WR_FIFO_ADDR 0x07 | 一致 |
| RD_FIFO_ADDR | 0x08 | reg::RD_FIFO_ADDR 0x08 | 一致 |
| F1 基址 | func 1 → 0x100 | FUNC1_BASE 0x100 | 一致 |
| F2 基址 | func 2 → 0x200 | FUNC2_BASE 0x200 | 一致 |
| F2 消息口偏移 | 7 | FUNC2_MSG_ADDR_OFFSET 7 | 一致 |
| DATA_FLOW_CTRL_THRESH | 2 | FLOW_CTRL_THRESH 2 | 一致 |
| SDIOWIFI_FUNC_BLOCKSIZE | 512 | SDIO_FUNC_BLOCKSIZE 512 / BLOCKSIZE 512 | 一致 |
| TAIL_LEN | 4 | TAIL_LEN 4 (types) / ipc_send_len_8801 内 4 | 一致 |
| BYTEMODE_THRESH (block_cnt≥64 用 0x02) | 64 | ops.rs 64 | 一致 |

**F1 完整地址（flow.rs 调试用）**：0x112 = F1+0x12 (BLOCK_CNT)，0x102 = F1+0x02 (BYTEMODE_LEN)，0x10A = F1+0x0A (FLOW_CTRL)，与上述偏移一致。

---

## 2. SDIO Vendor/Device ID（chipmatch）

| 芯片 | LicheeRV (aicsdio.c / aicwf_sdio.h) | StarryOS (types.rs sdio_ids) | 结论 |
|------|--------------------------------------|------------------------------|------|
| AIC8801 vendor | 0x5449 | VENDOR_AIC8801 0x5449 | 一致 |
| AIC8801 device | 0x0145 | DEVICE_AIC8801 0x0145 | 一致 |
| AIC8801 F2 device | 0x0146 | DEVICE_AIC8801_FUNC2 0x0146 | 一致 |
| AIC8800DC vendor | 0xc8a1 | VENDOR_AIC8800DC 0xc8a1 | 一致 |
| AIC8800DC device | 0xc08d | DEVICE_AIC8800DC 0xc08d | 一致 |
| AIC8800D80 vendor | 0xc8a1 | VENDOR_AIC8800D80 0xc8a1 | 一致 |
| AIC8800D80 device | 0x0082 | DEVICE_AIC8800D80 0x0082 | 一致 |
| AIC8800D80 F2 | 0x0182 | DEVICE_AIC8800D80_FUNC2 0x0182 | 一致 |
| AIC8800D80X2 vendor | 0xc8a1 | VENDOR_AIC8800D80X2 0xc8a1 | 一致 |
| AIC8800D80X2 device | 0x2082 | DEVICE_AIC8800D80X2 0x2082 | 一致 |

---

## 3. SG2002 硬件基址与管脚（backend.rs / gpio.rs）

| 项目 | TRM / U-Boot / 文档 | StarryOS | 结论 |
|------|---------------------|----------|------|
| SD1 控制器基址 | memorymap_sg2002: 0x04320000 | SD1_PHYS_BASE 0x0432_0000 | 一致 |
| RSTGEN 基址 | reset_registers: 0x03003000 | RSTGEN_PHYS 0x0300_3000 | 一致 |
| RSTGEN SOFT_RSTN_0 偏移 | 0x000 | RSTGEN_SOFT_RSTN_0 0x000 | 一致 |
| SD1 复位位 | SOFT_RSTN_0 Bit17 (sdmmc.rst) | RSTGEN_SD1_BIT 1<<17 | 一致 |
| CLKGEN 基址 | div_crg: 0x03002000 | CLKGEN_PHYS 0x0300_2000 | 一致 |
| clk_en_0 偏移 | 0x000 | CLKGEN_CLK_EN_0 0x000 | 一致 |
| SD1 时钟位 | bit21/22/23 (clk_axi4_sd1, clk_sd1, clk_100k_sd1) | CLKGEN_SD1_BITS (1<<21)\|(1<<22)\|(1<<23) | 一致 |
| PINMUX 基址 | memorymap: 0x03001000 | PINMUX_BASE 0x0300_1000 | 一致 |
| WiFi 电源 pinmux 偏移 | U-Boot 0x0300104C；PINOUT 0x0300_104C | WIFI_PWR 0x04C → 0x0300104C | 一致 |
| WiFi 电源 pinmux 值 | 0x3 (XGPIOA[26]) | 0x3 | 一致 |
| SD1_D3 偏移 | 0x030010D0 | D3 0x0D0 | 一致 |
| SD1_D2/D1/D0/CMD/CLK | 0x0D4, 0x0D8, 0x0DC, 0x0E0, 0x0E4 | 同 | 一致 |
| GPIO0 基址 | (GPIO 域) | GPIO0_BASE 0x03020000 | 与 U-Boot 0x03020004 同域 |
| WiFi 电源引脚 | GPIOA_26 (controller=0, pin=26) | WIFI_POWER_EN pin 26, controller 0 | 一致 |

---

## 4. 固件与芯片内存地址（fw_load.rs / flow.rs）

| 项目 | LicheeRV (aic_bsp_driver.h / .c) | StarryOS | 结论 |
|------|-----------------------------------|----------|------|
| 芯片版本寄存器 | mem_addr = 0x40500000 | CHIP_REV_MEM_ADDR 0x4050_0000 | 一致 |
| RAM FMAC 固件基址 | RAM_FMAC_FW_ADDR 0x00120000 | RAM_FMAC_FW_ADDR **0x0012_0000**（已修正） | 一致 |
| RAM FMAC 补丁地址 | RAM_FMAC_FW_PATCH_ADDR 0x00190000 | RAM_FMAC_FW_PATCH_ADDR 0x0019_0000 | 一致 |
| rd_patch_addr (8801) | RAM_FMAC_FW_ADDR + 0x0180 | RD_PATCH_ADDR_8801 = RAM_FMAC_FW_ADDR + 0x0180 | 一致 |
| patch start_addr (8801) | 0x1e6000 | PATCH_START_ADDR_8801 0x1e6000 | 一致 |
| patch_addr_reg (8801) | 0x1e5318 | PATCH_ADDR_REG_8801 0x1e5318 | 一致 |
| patch_num_reg (8801) | 0x1e531c | PATCH_NUM_REG_8801 0x1e531c | 一致 |
| HOST_START_APP 自动 | 1 | HOST_START_APP_AUTO 1 | 一致 |

---

## 5. aicbsp_system_config 表（8801）

| 序号 | LicheeRV aicbsp_syscfg_tbl | StarryOS AICBSP_SYSCFG_TBL_8801 | 结论 |
|------|----------------------------|----------------------------------|------|
| 1 | 0x40500014, 0x00000101 | 0x4050_0014, 0x0000_0101 | 一致 |
| 2 | 0x40500018, 0x00000109 | 0x4050_0018, 0x0000_0109 | 一致 |
| 3 | 0x40500004, 0x00000010 | 0x4050_0004, 0x0000_0010 | 一致 |
| 4 | 0x40040000, 0x00001AC8 | 0x4004_0000, 0x0000_1AC8 | 一致 |
| 5 | 0x40040084, 0x00011580 | 0x4004_0084, 0x0001_1580 | 一致 |
| 6 | 0x40040080, 0x00000001 | 0x4004_0080, 0x0000_0001 | 一致 |
| 7 | 0x40100058, 0x00000000 | 0x4010_0058, 0x0000_0000 | 一致 |
| 8 | 0x50000000, 0x03220204 | 0x5000_0000, 0x0322_0204 | 一致 |
| 9 | 0x50019150, 0x00000002 | 0x5001_9150, 0x0000_0002 | 一致 |
| 10 | 0x50017008, 0x00000000 | 0x5001_7008, 0x0000_0000 | 一致 |

---

## 6. syscfg_tbl_masked / rf_tbl_masked（8801）

| 项目 | LicheeRV (aic_bsp_driver.c) | StarryOS | 结论 |
|------|-----------------------------|----------|------|
| syscfg 条目 | {0x40506024, 0x000000FF, 0x000000DF} | SYSCFG_TBL_MASKED_8801 同 | 一致 |
| rf 条目 | {0x40344058, 0x00800000, 0x00000000} | RF_TBL_MASKED_8801 (0x4034_4058, …) | 一致 |

---

## 7. 消息 ID 与任务 ID（fw_load.rs / cmd.rs）

| 项目 | LicheeRV (lmac_msg.h / aic_bsp_driver.h) | StarryOS | 结论 |
|------|------------------------------------------|----------|------|
| DRV_TASK_ID | 100 | DRV_TASK_ID 100 | 一致 |
| TASK_DBG | 1 | TASK_DBG 1 (TaskId::Dbg) | 一致 |
| LMAC_FIRST_MSG(TASK_DBG) | 1<<10 = 1024 | DBG_MEM_READ_REQ 1024 | 一致 |
| DBG_MEM_READ_CFM | 1025 | 1025 | 一致 |
| DBG_MEM_WRITE_REQ/CFM | 1026/1027 | 1026/1027 | 一致 |
| DBG_MEM_BLOCK_WRITE_REQ/CFM | 1034/1035 | 1034/1035 | 一致 |
| DBG_START_APP_REQ/CFM | 1036/1037 | 1036/1037 | 一致 |
| DBG_MEM_MASK_WRITE_REQ/CFM | 1038/1039 | 1038/1039 | 一致 |

---

## 8. 时序与超时

| 项目 | LicheeRV / U-Boot | StarryOS | 结论 |
|------|-------------------|----------|------|
| 上电序列 低/高 延时 | 50ms / 50ms (Allwinner/Rockchip；Nano W) | gpio power_on 50ms + 50ms | 一致 |
| 上电后稳定延时 | Amlogic 200ms | POST_POWER_STABLE_MS 200 | 一致 |
| F1/F2 IO_READY 轮询超时 | 100ms 量级 | IO_READY_F1_MS / IO_READY_F2_MS 100 | 一致 |
| CMD 等待超时 | wait_for_completion_timeout 等 | wait_done_until 2000ms；CMD_TIMEOUT_MS | 对齐 |
| dbg_mem_read 后延时 | 无显式 100ms | 100ms 再轮询 | 可选，无冲突 |
| flow_ctrl 重试 | 50 次 + udelay | 10 次 × 1ms | 阈值一致，次数不同 |

---

## 9. 已修正项

- **RAM_FMAC_FW_ADDR**：原为 `0x1e0000`，已改为 **0x0012_0000**，与 LicheeRV aic_bsp_driver.h 一致。

---

## 10. FBR/CIS（cis.rs）

- FBR base：func * 0x100，与 SDIO 规范一致。
- CCCR CIS 指针：0x09-0x0B（SDIO_FBR_CIS 0x09）。
- CISTPL_MANFID 0x20，与 SDIO CIS 一致。

以上所有列出的寄存器地址、管脚号、硬件基址与关键数字均已与 LicheeRV 或 TRM/U-Boot 核对，除已修正的 RAM_FMAC_FW_ADDR 外，未发现其余错误。
