# LicheeRV bootrom 启动时序完整对照（BLOCK_CNT 恒为 0 排查）

本文档对照 **LicheeRV 中与 bootrom 启动相关的所有代码、数据、数字与程序逻辑**，重点：电源/复位序列、SDIO 主机初始化顺序、首次 SDIO 访问时机，以及可能影响 bootrom 行为的寄存器与时序。

---

## 1. LicheeRV Nano (SG2002) 实际运行时的时序

### 1.1 U-Boot（cvi_board_init.c）— 唯一做 WiFi 上电的地方

路径：`LicheeRV-Nano-Build/build/boards/sg200x/sg2002_licheervnano_sd/u-boot/cvi_board_init.c`

| 步骤 | 代码/数据 | 说明 |
|------|-----------|------|
| GPIOA_26 pinmux | `mmio_write_32(0x0300104C, 0x3)` | 设为 GPIO，与 StarryOS 一致 |
| DIR 输出 | `0x03020004 \|= (1<<26)` | 与 StarryOS 一致 |
| 拉低 | `0x03020000 &= ~(1<<26)` | 下电/复位 |
| **延时** | **`suck_loop(50)`** | 约 50×1.26ms ≈ **63ms**（注释 1.26ms/loop） |
| 拉高 | `0x03020000 \|= (1<<26)` | 上电/释放复位 |
| SDIO pinmux | `0x030010D0..E4 = 0` (D3/D2/D1/D0/CMD/CLK) | **紧接在拉高之后，无额外延时** |

结论：U-Boot 中**拉高后没有“再等 50ms/200ms”**；SDIO 引脚立即配为 SD1。但 U-Boot **不**做 SDIO 主机初始化、不发 CMD5，所以**第一次 SDIO 总线活动发生在 Linux 启动之后**。

### 1.2 Linux 侧：首次 SDIO 访问何时发生？

- aic8800 BSP Makefile 默认：`CONFIG_PLATFORM_UBUNTU = y`，**不**设 Allwinner/Amlogic/Rockchip。
- `aicbsp_platform_power_on()` 在无上述平台时：**不拉 GPIO、不 rescan**，只做 `aicbsp_reg_sdio_notify` 和 `down_timeout(aic_chipup_sem)`，即**假定卡已由 U-Boot 上电并保持上电**。
- 首次 CMD5/枚举由内核 MMC 子系统在**某次 host 初始化/rescan** 时完成，时间点通常在 Linux 启动后数秒。

因此：**从“电源拉高”到“首次 CMD5”的时间 = 整个 Linux 启动时间（约数秒～数十秒）**，bootrom 有充足时间就绪。

### 1.3 与 StarryOS 的差异（bootrom 可能“没启动”的原因）

| 项目 | LicheeRV (SG2002) | StarryOS |
|------|-------------------|----------|
| 谁上电 | U-Boot 上电，Linux 不再上电 | BSP 自己上电 |
| 拉高到首次 CMD5 | **数秒～数十秒**（Linux 启动时间） | **50+200=250ms**（可配置） |
| 上电后是否 rescan | 平台相关；UBUNTU 平台无 rescan，卡早就在 | 无“卡检测”概念，上电后直接 host init + 枚举 |

若 bootrom 需要**比 250ms 更长的就绪时间**（例如冷启动、电压爬升慢、晶振稳定慢），则可能出现：主机已发 IPC、但芯片尚未运行 bootrom → BLOCK_CNT 恒为 0。

---

## 2. LicheeRV 各平台上电与首包前时序（代码级）

### 2.1 aicbsp_platform_power_on（aicsdio.c 487–556）

| 平台 | 上电序列 | 上电后动作 | 首包前“稳定时间” |
|------|----------|------------|------------------|
| CONFIG_PLATFORM_AMLOGIC | extern_wifi 0 → **mdelay(200)** → extern_wifi 1 → **mdelay(200)** → sdio_reinit() | 200ms 高后再 reinit SDIO | 约 200ms 高后才有 SDIO 访问 |
| CONFIG_PLATFORM_ALLWINNER | sunxi_wlan 0 → **mdelay(50)** → sunxi_wlan 1 → **mdelay(50)** → **sunxi_mmc_rescan_card** | 50ms 高后 rescan | 50ms 高 + rescan 耗时 |
| CONFIG_PLATFORM_ROCKCHIP2 | rockchip_wifi 0 → **mdelay(50)** → rockchip_wifi 1 → **mdelay(50)** → **rockchip_wifi_set_carddetect(1)** | 50ms 高后卡检测 | 50ms 高 + 枚举耗时 |
| 无（如 UBUNTU） | **无**（假定 U-Boot 已上电） | 仅等 dummy_probe | 从 U-Boot 拉高到 Linux 首包 = 整段启动时间 |

### 2.2 aicwf_sdio_func_init（aicsdio.c 1704–1796）— 8801

- `sdio_set_block_size(func, 512)`（在 enable 前）
- `sdio_enable_func(func)`
- **udelay(100)** ← 使能 F1 后固定 **100μs**
- （可选）设置 SDIO 时钟
- `sdio_release_host`
- **aicwf_sdio_writeb(sdiodev, 0x0B, 1)**、**aicwf_sdio_writeb(sdiodev, 0x11, 1)**

StarryOS 已对齐：F1 block 512、100μs 延时、0x0B=1、0x11=1、0x04=0x07。

### 2.3 aicwf_sdio_bus_start（aicsdio.c 1297–1349）— 8801

- `sdio_claim_irq(..., aicwf_sdio_hal_irqhandler)`
- **aicwf_sdio_writeb(sdiodev, intr_config_reg, 0x07)**（F1 0x04=0x07）

StarryOS 在 aicbsp_sdio_init 内 8801 分支已写 F1 0x04=0x07，无需在“bus_start”再写。

---

## 3. 数字与寄存器汇总（与 LicheeRV/TRM 一致）

| 项目 | 值 | 来源 |
|------|-----|------|
| GPIOA_26 pinmux | 0x0300104C = 0x3 | U-Boot cvi_board_init.c |
| 上电：低保持 | 50ms (U-Boot suck_loop(50) ≈63ms) | 同上 |
| 上电：高保持（平台） | Amlogic 200ms；Allwinner/RK2 50ms | aicsdio.c |
| F1 使能后延时 | **udelay(100)** | aicwf_sdio_func_init |
| F1 0x0B, 0x11, 0x04 | 1, 1, 0x07 | aicsdio.c |
| SDIO pinmux | 0x030010D0/D4/D8/DC/E0/E4 = 0 | U-Boot |
| CHIP_REV 地址 | 0x40500000 | aic_bsp_driver.c |
| DRV_TASK_ID / TASK_DBG | 100 / 1 | aic_bsp_driver.h |

---

## 4. 建议的代码与测试（bootrom 未启动时）

1. **加大“上电拉高 → 首次 SDIO 访问”的间隔**  
   - 将 `POST_POWER_STABLE_MS` 从 200 改为 **500 或 1000** 做对比测试。  
   - 或调用 `aicbsp_minimal_ipc_verify(500)` / `aicbsp_minimal_ipc_verify(1000)`，用更长稳定时间验证是否能让 BLOCK_CNT 更新。

2. **确认 pinmux 与上电顺序**  
   - 已在 `aicbsp_power_on()` 首行调用 `set_wifi_power_pinmux_to_gpio()`，保证先 GPIO 模式再拉 GPIO。  
   - SDIO D0–D3/CMD/CLK 在 `sd1_host_init()` 中设置，位于“上电稳定延时”之后，顺序正确。

3. **硬件与同板对比**  
   - 同一块板在 LicheeRV 下若能正常读 chip_rev，说明 bootrom 与芯片本身正常；差异主要在**上电到首包的间隔**或主机侧时序。  
   - 若可能，用逻辑分析仪/示波器确认：CMD53 写 0x107 是否到达卡端、卡端是否有 SDIO 回读/时钟。

4. **不改变的部分**  
   - IPC 格式、reqid/dest/src、512 字节写 WR_FIFO、F1 0x0B/0x11/0x04、100μs 延时已与 LicheeRV 对齐，无需再改。

---

## 5. 小结

- **LicheeRV Nano (SG2002)**：WiFi 在 **U-Boot 里上电**，首次 SDIO 访问在 **Linux 启动后**，bootrom 有**数秒级**就绪时间。  
- **StarryOS**：BSP 上电后仅 **250ms** 就做 host init 和首包 IPC；若 bootrom 需要更长时间，会出现 BLOCK_CNT 恒为 0。  
- **建议**：将上电稳定时间增至 **500ms 或 1000ms**（或通过 `aicbsp_minimal_ipc_verify(stable_ms)` 传入）做验证；若由此能收到 CFM，则说明问题为“上电到首包”间隔不足，可再在正式流程中固定为合适值。
