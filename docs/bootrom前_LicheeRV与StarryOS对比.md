# bootrom 未启动：bootrom 前程序、配置、参数与 LicheeRV 对比

本文档对比 **LicheeRV** 与 **StarryOS wireless** 在「首次 SDIO 访问 / bootrom 响应」之前的程序顺序、配置与参数，以及**中断机制**（LicheeRV 硬件 IRQ vs wireless 软中断）对齐说明。bootrom 不启动时，可能不单是 bootrom 本身问题，需逐项核对前置条件。

---

## 1. LicheeRV：bootrom 前发生了什么

### 1.1 U-Boot 阶段（cvi_board_init.c）

路径：`LicheeRV-Nano-Build/build/boards/sg200x/sg2002_licheervnano_sd/u-boot/cvi_board_init.c`

| 顺序 | 动作 | 寄存器/值 | 说明 |
|------|------|-----------|------|
| 1 | GPIOA_26 pinmux | 0x0300104C = 0x3 | WiFi 电源/复位引脚设为 GPIO |
| 2 | GPIOA DIR | 0x03020004 \|= (1<<26) | 输出 |
| 3 | 拉低 | 0x03020000 &= ~(1<<26) | 下电/复位 |
| 4 | 延时 | suck_loop(50) | 约 50×1.26ms ≈ 63ms |
| 5 | 拉高 | 0x03020000 \|= (1<<26) | 上电/释放复位 |
| 6 | SDIO pinmux | 0x030010D0..E4 = 0 | D3/D2/D1/D0/CMD/CLK = SD1，**紧接拉高后，无额外延时** |
| … | 其他板级 | LED、UART、LCD、I2C 等 | |
| 7 | 结尾 | suck_loop(50) | “wait hardware bootup” |

**要点**：U-Boot **不**做 SD1 主机初始化（不碰 RSTGEN/CLKGEN），不发起 CMD5。首次 SDIO 总线活动在 **Linux 启动后** 由 MMC 子系统完成。

### 1.2 Linux 阶段（到首次 CMD5 / bootrom 通信）

- **上电到首次 CMD5 的间隔** = U-Boot 拉高 → Linux 启动 → MMC host 探测 → 枚举，约 **数秒～数十秒**。
- aic8800 BSP：`aicbsp_platform_power_on()` 在无 Allwinner/Amlogic/Rockchip 时**不再拉 GPIO**，假定 U-Boot 已上电；首次 CMD5 由内核 MMC 在 host 初始化时完成。

---

## 2. StarryOS：bootrom 前发生了什么

### 2.1 main 到首次 SDIO 的调用链

当前入口（如 `main.rs` 中 `aicbsp_minimal_ipc_verify(500)`）：

1. **starry_api::init()**：VFS mount、`register_timer_callback`（如 time::inc_irq_cnt）、alarm 等。**不**做 WiFi/GPIO/SDIO。
2. **aicbsp_power_on_with_stable_ms(500)**：
   - `set_wifi_power_pinmux_to_gpio()`（0x0300104C=0x3）
   - WifiGpioControl::new/init，`power_on_and_reset()`：低 50ms → 高 50ms
   - `verify_after_power_on()`（读回 GPIO）
   - `delay_spin_ms(50)` → `set_sd1_sdio_pinmux_after_power()`（D3–CLK=0）→ `axtask::sleep(500ms)`
3. **aicbsp_sdio_init()**：
   - `sd1_host_init()`：RSTGEN 释放 SD1、CLKGEN 使能 SD1、再次配置 SDIO pinmux
   - 创建 Host、`enable_sd_interface_clock()`（INT_CLK → INT_CLK_STABLE → SD_CLK，FREQ_SEL=0x80）
   - 卡枚举 CMD0→CMD5→CMD3→CMD7，F1 0x0B/0x11/0x04 等

因此 **bootrom 前** 在 StarryOS 侧包含：pinmux、电源序列、稳定延时、SD1 主机复位/时钟、pinmux 再次设置、控制器时钟、卡枚举。与 LicheeRV 的差异主要在：**上电到首次 CMD5 的间隔**（LicheeRV 数秒，StarryOS 50+500ms 可调）以及 **RSTGEN/CLKGEN 在 BSP 内完成**（LicheeRV 在 Linux MMC 内完成）。

### 2.2 与 LicheeRV 的逐项对照

| 项目 | LicheeRV | StarryOS | 一致？ |
|------|----------|----------|--------|
| GPIOA_26 pinmux | 0x0300104C=0x3，在拉 GPIO 前 | aicbsp_power_on 首行 set_wifi_power_pinmux_to_gpio() | ✅ |
| 上电序列 | 低 → suck_loop(50) → 高 | 低 50ms → 高 50ms（gpio.rs） | ✅ |
| 拉高后 SDIO pinmux | 拉高后**立即** D3–CLK=0 | 拉高 → verify → 50ms → set_sd1_sdio_pinmux_after_power() | ⚠️ 多 50ms，再 500ms 才 host init |
| RSTGEN/CLKGEN | Linux MMC 在 probe 时 | sd1_host_init() 内 | ✅ 顺序一致（先 RSTGEN 再 CLKGEN） |
| 上电到首次 CMD5 | 数秒（Linux 启动） | 50+500ms（可调 POST_POWER_STABLE_MS） | ⚠️ 若 bootrom 需更长可改为 1000 |

---

## 3. 中断机制：LicheeRV 硬件 IRQ vs wireless 软中断

### 3.1 LicheeRV（aicsdio.c）

- **claim**：`sdio_claim_host(sdiodev->func)` → `sdio_claim_irq(sdiodev->func, aicwf_sdio_hal_irqhandler)`。
- **IRQ 处理**：`aicwf_sdio_hal_irqhandler` 内读 F1 block_cnt/misc_int_status，读/写帧，最后 `complete(&bus_if->busrx_trgg)`。
- **busrx 线程**：`wait_for_completion_interruptible(&bus_if->busrx_trgg)` 阻塞，被唤醒后 `aicwf_process_rxframes(rx_priv)`。
- **命令完成**：process_rxframes 中解析到 CFM 后 `complete(&cmd->complete)`，发送侧 `wait_for_completion_killable_timeout(&cmd->complete, ...)` 返回。

即：**硬件 IRQ → complete(busrx_trgg) → busrx 唤醒 → process_rxframes**；**CFM 到达 → complete(cmd->complete) → 发送方唤醒**。

### 3.2 wireless（sdio_irq.rs + flow.rs）

- **wireless 不启用 PLIC**：`sdmmc_irq()` 恒为 0，不调用 `axhal::irq::register`，不注册 SDIO 硬件中断。
- **软中断对齐**：用定时器 tick 模拟「有数据可处理」的触发：
  - 应用在首包前：`set_use_soft_irq_wake(true)` 且 `axtask::register_timer_callback(|_| wireless::bsp::sdio_tick)`。
  - 每次 tick：`sdio_tick()` → `SDIO_WAIT_QUEUE.notify_one(false)`。
  - busrx：`wait_sdio_or_timeout(dur)` 阻塞，被 notify 后返回，再 `run_poll_rx_one()`（等价 process_rxframes）。
  - CFM：`run_poll_rx_one` 内 on_cfm → `notify_wait_done()` / `notify_cmd_done()`，主线程自 `wait_done_until` / `wait_cmd_done_timeout` 唤醒。

逻辑对齐：**LicheeRV** 为「IRQ → complete(busrx_trgg) → process_rxframes」；**wireless** 为「timer → sdio_tick → notify_one → wait_sdio_or_timeout 返回 → run_poll_rx_one」。若未启用软中断，则 `use_sdio_irq()` 为 false，busrx 以轮询 + `sleep(RX_POLL_MS)` 方式运行，语义仍为「周期执行 run_poll_rx_one」。

---

## 4. 检查清单（bootrom 不启动时）

1. **bootrom 前程序**  
   - 确认 pinmux、上电序列、稳定延时与上文一致；必要时增大 `POST_POWER_STABLE_MS`（如 1000）再测。
2. **配置/参数**  
   - RSTGEN/CLKGEN/CLK_CTL 读回值见《上电复位时钟时序_完整检查清单.md》；GPIO 验证见日志 `WiFi GPIO 验证: ... => OK`。
3. **中断/软中断**  
   - wireless 不启用 PLIC；若需与 LicheeRV 一致「等待再处理」，则在 main 中 `set_use_soft_irq_wake(true)` 并 `register_timer_callback(sdio_tick)`。
4. **同板对比**  
   - 同一块板在 LicheeRV 下能读 chip_rev 则芯片与 bootrom 正常，差异多为上电到首包间隔或主机侧时序。
