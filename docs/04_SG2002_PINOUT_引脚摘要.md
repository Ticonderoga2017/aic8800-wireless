# 04_SG2002_PINOUT.xlsx 引脚信息摘要

本文档从 `LicheeRV-docs/04_SG2002_PINOUT.xlsx` 中提取与 WiFi/SD1/GPIOA_26 相关的引脚与寄存器信息，并与当前 BSP 程序对照。

---

## 1. Excel 结构（来自 workbook.xml）

- **1. 管腳信息表(QFN)**：Pin Num、Pin Name、IO Type、IOGroup、PowerDomain、IO_cfg_register、**Function_select register**、fmux_default、Description、Note
- **2. 功能信號表(QFN)**：功能与引脚对应
- **3. 管脚控制寄存器(QFN)**、**4. 管腳默認狀態(QFN)**、**5. 管腳分佈圖(QFN)** 等

以下内容来自 **1. 管腳信息表** 对应的 sharedStrings 与 sheet 数据。

---

## 2. WiFi 电源引脚：EMMC_DAT2 / GPIOA_26

| 项目 | 04_SG2002_PINOUT 内容 | 程序（backend/gpio） | 结论 |
|------|------------------------|----------------------|------|
| 引脚名 | **EMMC_DAT2** | 使用同一物理引脚作 GPIO | 一致 |
| Function_select 寄存器 | **FMUX_GPIO_REG_IOCTRL_EMMC_DAT2 = 0x0300_104C** | `PINMUX_BASE + 0x04C` = 0x0300104C | 一致 |
| 功能选择值 | 0 : EMMC_DAT[2]；1 : SPINOR_HOLD_X (default)；2 : SPINAND_HOLD；**3 : XGPIOA[26]** | 写 **0x3** 选 GPIO 模式 | 一致 |
| fmux_default | 0x1 | 上电前需写 0x3 才能当 GPIO 用 | 一致 |

**结论**：程序中对 **0x0300104C = 0x3** 的配置与官方 PINOUT 表一致，即选择 **XGPIOA[26]**，用于 LicheeRV Nano W 的 WiFi 电源控制。

---

## 3. SD1（SDIO1）引脚与寄存器

| Pin Name | Function_select register (PINOUT) | 程序 backend.rs 偏移 | 结论 |
|----------|------------------------------------|----------------------|------|
| SD1_D3   | **0x0300_10D0**                    | 0x0D0                | 一致 |
| SD1_D2   | **0x0300_10D4**                    | 0x0D4                | 一致 |
| SD1_D1   | **0x0300_10D8**                    | 0x0D8                | 一致 |
| SD1_D0   | **0x0300_10DC**                    | 0x0DC                | 一致 |
| SD1_CMD  | **0x0300_10E0**                    | 0x0E0                | 一致 |
| SD1_CLK  | **0x0300_10E4**                    | 0x0E4                | 一致 |

PINOUT 中说明：上述 SD1 引脚功能选择 0 为 SD1 对应信号（如 PWR_SD1_D3_VO32 等），与 U-Boot 写 0 选择 SD1 功能一致。

---

## 4. 其他相关（EMMC 与 GPIO 对应）

| 信号       | 寄存器地址     | 功能 3 含义   |
|------------|----------------|---------------|
| EMMC_DAT2  | 0x0300_104C    | XGPIOA[26]    |
| EMMC_CLK   | 0x0300_1050    | XGPIOA[22]    |
| EMMC_DAT0  | 0x0300_1054    | XGPIOA[25]    |
| EMMC_DAT3  | 0x0300_1058    | XGPIOA[27]    |
| EMMC_CMD   | 0x0300_105C    | XGPIOA[23]    |
| EMMC_DAT1  | 0x0300_1060    | XGPIOA[24]    |

LicheeRV Nano 核心板将 EMMC_DAT2 引出为 **GPIOA_26/EMMC_D2**（KiCad）；Nano W 上该引脚用作 WiFi 电源，程序写 0x0300104C=0x3 正确。

---

## 5. 小结

- **04_SG2002_PINOUT.xlsx** 与程序中的 **PINMUX 基址 0x03001000**、**WiFi 电源 0x0300104C=0x3（XGPIOA[26]）**、**SD1 各引脚 0x030010D0~0x030010E4** 完全一致，未发现引脚或寄存器错误。
