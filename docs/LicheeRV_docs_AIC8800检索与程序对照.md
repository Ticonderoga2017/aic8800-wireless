# LicheeRV-docs 检索与程序对照（AIC8800/WiFi）

本文档记录对 `LicheeRV-docs` 板卡资料的检索结果，以及与本项目 WiFi BSP 程序的对照结论，用于排查 BLOCK_CNT 恒为 0 等问题时确认**引脚、极性、寄存器**是否有误。

---

## 1. LicheeRV-docs 中与 AIC8800/WiFi 相关的内容

### 1.1 目录结构概览

- **原理图 PDF**：`LicheeRV_Nano-70405_Schematic.pdf`、`70415`、`70418`（未在仓库内做文本提取；需手动打开核对网络名与 GPIO）。
- **KiCad 原理图**：`LicheeRV_Nano_KICAD/DEMO.kicad_sch`（LicheeRV_Nano **核心板**引脚定义）。
- **Nano W 外形**：`LicheeRV_Nano_W_Step/` 中有 `QFN-44_L12.0-W12.0-WiFi-LicheeRV`、`U13` 等，说明 **Nano W** 带 WiFi 模组（QFN-44 封装，与 AIC8800 系列常见封装一致）。
- **iBOM**：`LicheeRV_Nano-70405_iBOM/` 为 70405 的 BOM，未在可读文本中搜到 “AIC8800” 字样；AIC 芯片可能以位号/型号出现在 PDF 或图片中。
- **TRM/数据手册**：`SG2002_Preliminary_Datasheet_V1.0-alpha_CN.pdf`、`sg2002_trm_cn_v1.02.pdf`；同目录下 `sophgo-doc-sg2002-trm-v1.02/SG200X/` 为 RST 版 TRM，可搜索。

**结论**：LicheeRV-docs 中**没有**单独的 “AIC8800 数据手册”或“WiFi 寄存器说明”文本；WiFi 相关依据为：**原理图**、**KiCad 引脚**、**U-Boot 板级代码**、**SG2002 TRM**。

### 1.2 KiCad 原理图（DEMO.kicad_sch）—— 核心板引脚

- **U1** = LicheeRV_Nano 核心模块。
- **Pin 12**：`GPIOA_26/EMMC_D2`（与 U-Boot 中 WiFi 电源引脚一致）。
- **Pin 17–22**：SDIO1_D1、SDIO1_CLK、SDIO1_CMD、SDIO1_D0、SDIO1_D2、SDIO1_D3（与 SD1 数据/命令/时钟对应）。

说明：在 **Nano W** 上，Pin 12 用作 **GPIO（WiFi 电源）**，不再作 EMMC_D2；SDIO1 与 WiFi 模组连接。

### 1.3 SG2002 TRM（sophgo-doc-sg2002-trm-v1.02/SG200X）

- **内存映射**（memorymap_sg2002.table.rst）：
  - PINMUX：`0x03001000`–`0x03001FFF`
  - GPIO0：`0x03020000`–`0x03020FFF`
  - SD1：`0x04320000`–`0x0432FFFF`
- **SDMMC**（sdmmc.rst）：SDIO1 对应 SD1_CLK、SD1_CMD、SD1_D0–D3，用于 SDIO 设备（如 WIFI）。
- **包引脚表**（package_pin_sg2002.table.rst）：SD1_D3/D2/D1/D0/CMD/CLK 为物理引脚 51–56，注明 “Check 0x0502_70E4”（为 RTC 域配置；LicheeRV Nano 使用 **Active 域** `0x03001xxx` 配置 SD1 与 GPIOA_26）。

---

## 2. 程序与板卡/TRM/U-Boot 对照

### 2.1 寄存器基址与复位/时钟

| 项目       | 程序（backend.rs / gpio.rs） | TRM / U-Boot           | 结论     |
|------------|-----------------------------|------------------------|----------|
| PINMUX 基址 | `0x0300_1000`               | TRM 0x03001000         | 一致     |
| GPIO0 基址  | `0x03020000`                | TRM 0x03020000         | 一致     |
| SD1 基址    | `0x0432_0000`               | TRM 0x04320000         | 一致     |
| RSTGEN      | `0x0300_3000`，bit17 SD1    | TRM SOFT_RSTN_0 bit17  | 一致     |
| CLKGEN      | `0x0300_2000`，clk_en_0     | TRM CLKGEN             | 一致     |

### 2.2 SD1 Pinmux 偏移（Active 域 0x03001000 + offset）

| 信号       | 程序偏移 | U-Boot（cvi_board_init.c） | 结论 |
|------------|----------|----------------------------|------|
| WiFi 电源  | 0x04C    | 0x0300104C = 0x3           | 一致 |
| SD1_D3     | 0x0D0    | 0x030010D0 = 0             | 一致 |
| SD1_D2     | 0x0D4    | 0x030010D4 = 0             | 一致 |
| SD1_D1     | 0x0D8    | 0x030010D8 = 0             | 一致 |
| SD1_D0     | 0x0DC    | 0x030010DC = 0             | 一致 |
| SD1_CMD    | 0x0E0    | 0x030010E0 = 0             | 一致 |
| SD1_CLK    | 0x0E4    | 0x030010E4 = 0             | 一致 |

### 2.3 GPIOA_26 与极性

- **引脚**：程序使用 `GpioPin::new(0, 26)`，即 GPIOA_26；KiCad 与 U-Boot 均为 GPIOA_26。
- **极性**：U-Boot 序列为「先低 → 约 50ms → 再高」；程序 `power_on()` 为「拉低 50ms → 拉高 50ms」，与 U-Boot 一致，**高 = 上电/释放复位，低 = 下电/复位**，无需取反。
- **Pinmux 顺序**：程序在 `aicbsp_power_on()` 开头调用 `set_wifi_power_pinmux_to_gpio()`（0x0300104C=0x3），再驱动 GPIO，与 U-Boot「先写 pinmux 再拉 GPIO」一致。

### 2.4 GPIO 控制器寄存器（gpio.rs）

- 数据寄存器 0x00、方向寄存器 0x04，与 U-Boot 使用的 `0x03020000`（level）、`0x03020004`（DIR）一致。

---

## 3. 结论与建议

- **检索结论**：LicheeRV-docs 中未找到 AIC8800 专用寄存器或时序文档；WiFi 相关依据为原理图、KiCad、U-Boot 与 SG2002 TRM。
- **程序对照**：当前 BSP 的 **PINMUX 基址与偏移、GPIO 基址与极性、SD1 基址、RSTGEN/CLKGEN** 均与 TRM 及 LicheeRV U-Boot 一致，**未发现引脚或极性错误**。
- 若 BLOCK_CNT 仍恒为 0，建议：
  1. 用 **万用表/示波器** 确认 GPIOA_26 上电后为高电平、SDIO 线有主机发出的波形；
  2. 若有 **70415/70418 原理图 PDF**（Nano W 版本），打开确认 WiFi 模组电源/复位网络是否与 GPIOA_26 一致及有无反相器；
  3. 延长上电稳定时间或检查硬件焊接/供电。

---

**文档生成**：根据 LicheeRV-docs 与 sophgo-doc-sg2002-trm-v1.02 检索及对 `backend.rs`、`gpio.rs`、`flow.rs` 的对照整理。
