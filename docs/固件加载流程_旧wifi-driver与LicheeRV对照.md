# 固件加载流程：旧 wifi-driver 与 LicheeRV 对照

## 一、正确流程（你要的顺序）

1. **先得到芯片型号**（AIC8801）：从 SDIO CIS / Vendor ID、Device ID 得到 chipid。
2. **再读芯片版本**（chip_rev，U02/U03 等）：通过 **bootrom** 响应地址 **0x40500000**（DBG_MEM_READ），在**加载任何固件之前**读。
3. **根据 chipid + chip_rev 选固件表**（U02 用一套，U03 用另一套）。
4. **加载固件**：主固件 → patch → ADID → patch table 等。
5. **系统配置**（aicbsp_syscfg_tbl / aicwifi_sys_config）。
6. **START_APP**（从 bootrom 启动固件）。

---

## 二、旧版 wifi-driver (`/home/orleone/old-sg2002-wifi/wifi-driver`) 流程

### 入口：`driver/src/lib.rs` → `WifiDriver::init()`

1. GPIO 复位、电源。
2. `AIC8800Chip::new(sdio_device)`，`chip.init()`。
3. **chip.init() 里**（`aic8800/src/chip.rs`）：
   - `sdio.init()`；
   - **`detect_chip()`**：从 CIS/VID/DID 得到 **chip_id（AIC8801 等）**；
   - F1/F2 使能、REGISTER_BLOCK(0x0B)=1、BYTEMODE_ENABLE(0x11)=1、块大小、中断；
   - **`detect_chip_revision_from_bootrom()`**（约 616–628 行）：**读 0x40500000**（`read_chip_memory` → IPC **DBG_MEM_READ**），得到 **chip_rev**，写入 `self.info.chip_rev`。
4. **`load_firmware_complete()`**（`aic8800/src/firmware.rs` 432–465 行）：
   - 此时 **chip_id 和 chip_rev 都已就绪**；
   - `get_firmware_names()` 按 **chip_rev** 选 U02/U03 固件名；
   - `load_firmware_aic8801()`：FMAC 主固件 → FMAC patch → ADID → Patch bin → patch config → **sys_config** → **start_firmware**。

要点：**芯片版本是在 chip.init() 里、在 load_firmware_complete() 之前、通过 bootrom 地址 0x40500000 读到的**；然后用 chip_rev 选固件再加载。

---

## 三、LicheeRV 里“加载固件”功能在哪（精确位置）

### 1. 调用链（BSP 路径，例如上电/子系统的 power on）

- **`aicbsp_set_subsys()`**  
  `LicheeRV-Nano-Build/osdrv/extdrv/wireless/aic8800/aic8800_bsp/aicsdio.c` **约 177–184 行**  
  - 上电后：`aicbsp_platform_power_on()` → **`aicbsp_sdio_init()`** → **`aicbsp_driver_fw_init(aicbsp_sdiodev)`**。

- **Probe 时**（同文件 **aicsdio.c 266–364 行** `aicbsp_sdio_probe`）：  
  - **chip 型号**：**`aicwf_sdio_chipmatch(sdiodev, func->vendor, func->device)`**（约 321 行）→ 设置 `sdiodev->chipid`（如 PRODUCT_ID_AIC8801）；  
  - 然后 **`aicwf_sdio_func_init(sdiodev)`**（约 337 行，8801/DC/DW）或 `aicwf_sdiov3_func_init`（D80/D80X2）；  
  - 再 **`aicwf_sdio_bus_init(sdiodev)`**（约 346 行）→ 内部会起 **busrx** 等。  
  - **注意**：probe 里**不**调用 `aicbsp_driver_fw_init`；固件加载在 power on 路径里调。

### 2. 读芯片版本 + 选固件表 + 系统配置 + 固件加载入口

**文件：**  
`LicheeRV-Nano-Build/osdrv/extdrv/wireless/aic8800/aic8800_bsp/aic_bsp_driver.c`

| 步骤 | 函数名 | 行号（约） | 作用 |
|------|--------|------------|------|
| 入口 | **`aicbsp_driver_fw_init`** | **1241–1331** | 先读 chip_rev，再选固件表，再 aicbsp_system_config(8801)，再 aicwifi_init。 |
| 读 chip_rev | **`rwnx_send_dbg_mem_read_req(sdiodev, 0x40500000, &rd_mem_addr_cfm)`** | **1254–1255（AIC8801）** | 从 **bootrom 地址 0x40500000** 读；chip_rev = `(rd_mem_addr_cfm.memdata >> 16)`。 |
| 选固件表 | `aicbsp_info.chip_rev` 判断，`aicbsp_firmware_list = fw_u03` 等 | 1264–1265 | 若非 U02 则用 U03 固件表。 |
| 8801 系统配置 | **`aicbsp_system_config(sdiodev)`** | 1268–1269 | 写 **aicbsp_syscfg_tbl**（见 1197–1209 行）。 |
| 固件加载总入口 | **`aicwifi_init(sdiodev)`** | **1325** | 按 chipid 分支：上传固件、patch、patch_config、sys_config、start_from_bootrom。 |

### 3. AIC8801 固件加载具体步骤（在 aicwifi_init 内）

**同文件 `aic_bsp_driver.c`，`aicwifi_init()` 约 1176–1240 行，AIC8801 分支 1178–1212：**

| 顺序 | 行号（约） | 调用 | 作用 |
|------|------------|------|------|
| 1 | 1184 | **`rwnx_plat_bin_fw_upload_android(sdiodev, RAM_FMAC_FW_ADDR, aicbsp_firmware_list[...].wl_fw)`** | 下载 **主固件**（wl_fw 已按 chip_rev 选好） |
| 2 | 1188 | **`rwnx_plat_bin_fw_upload_android(..., RAM_FMAC_FW_PATCH_ADDR, RAM_FMAC_FW_PATCH_NAME)`** | 下载 **wifi fw patch** |
| 3 | 1193 | **`aicwifi_patch_config(sdiodev)`** | patch 配置（约 1121–1174） |
| 4 | 1197 | **`aicwifi_sys_config(sdiodev)`** | 系统配置（约 1110–1119） |
| 5 | 1201 | **`aicwifi_start_from_bootrom(sdiodev)`** | 从 bootrom 启动（约 1029–1043，调 `rwnx_send_dbg_start_app_req`） |

### 4. 其它相关函数（同一文件）

- **`rwnx_send_dbg_mem_read_req`**：约 **378–391** 行，发 DBG_MEM_READ，收 CFM 得到 memdata（读 0x40500000 即用这个）。
- **`rwnx_plat_bin_fw_upload_android`**：约 **786–832** 行，`rwnx_load_firmware` + 多次 **`rwnx_send_dbg_mem_block_write_req`** 写内存。
- **`aicbsp_system_config`**：约 **1212–1224** 行，写 **aicbsp_syscfg_tbl**（表在 1197–1209）。
- **`aicwifi_start_from_bootrom`**：约 **1029–1043** 行，`rwnx_send_dbg_start_app_req(..., RAM_FMAC_FW_ADDR, HOST_START_APP_AUTO, ...)`。

### 5. F1 初始化（probe 里、发 IPC 前，8801）

**文件：**  
`LicheeRV-Nano-Build/osdrv/extdrv/wireless/aic8800/aic8800_bsp/aicsdio.c`  
**`aicwf_sdio_func_init`** 约 **1704–1796** 行（8801/DC/DW）：

- **1782**：`aicwf_sdio_writeb(sdiodev, sdiodev->sdio_reg.register_block, block_bit0)` → F1 **0x0B = 1**；
- **1788**：`aicwf_sdio_writeb(sdiodev, sdiodev->sdio_reg.bytemode_enable_reg, byte_mode_disable)` → F1 **0x11 = 1**。

DC/DW 还会对 **F2** 写 0x0B、0x11（约 1768–1778）。  
**busrx** 在 **`aicwf_sdio_bus_init`**（约 1912 行起）里启动，早于 `aicbsp_driver_fw_init`。

---

## 四、StarryOS 当前问题（流程 vs 实现）

- **顺序**：我们和 LicheeRV 一致——先 product_id（chip 型号）→ 再在 driver_fw_init 里 **DBG_MEM_READ(0x40500000)** 拿 chip_rev → 再 sys_config → 再固件上传、START_APP。
- **实际问题**：**DBG_MEM_READ 收不到 CFM**（BYTEMODE_LEN/BLOCK_CNT 一直为 0），所以 **chip_rev 没有真正从 bootrom 读到**，只能走超时后的默认 U02；你说“芯片版本你没读到”是对的。
- **和“对齐 LicheeRV”的关系**：流程顺序、F1 0x0B/0x11、busrx 时机、用 0x02 做 RX 长度等，是按 LicheeRV 标的；但芯片不回 CFM，所以**读 chip_rev 这一步在运行时不成立**，需要排查的是**为什么 bootrom 不响应 DBG_MEM_READ**（硬件/电源/复位/时序/SDIO 初始化差异等），而不是再改“先读版本再加载固件”的流程。

---

## 五、LicheeRV 固件加载功能位置小结（你要的“标出来”）

| 功能 | 文件 | 函数/位置 |
|------|------|------------|
| 从 bootrom 读芯片版本 | aic8800_bsp/**aic_bsp_driver.c** | **aicbsp_driver_fw_init** 内，AIC8801 分支：**rwnx_send_dbg_mem_read_req(sdiodev, 0x40500000, &rd_mem_addr_cfm)**，**1254–1255**；chip_rev = (memdata>>16) **1257** |
| 根据 chip_rev 选固件表 | 同上 | **1264–1265**（U02 / fw_u03）；DC/D80 等分支类似，选 fw_8800dc_u01/u02、fw_8800d80_u02 等 |
| 8801 系统配置（写 syscfg 表） | 同上 | **aicbsp_system_config** **1268–1269**；表 **aicbsp_syscfg_tbl** **1197–1209**，实现 **1212–1224** |
| 固件加载总入口 | 同上 | **aicwifi_init(sdiodev)** **1325**；AIC8801 分支 **1178–1212** |
| 下载主固件 / patch | 同上 | **rwnx_plat_bin_fw_upload_android** **786–832**；被 aicwifi_init 调，1184、1188 等 |
| 从 bootrom 启动固件 | 同上 | **aicwifi_start_from_bootrom** **1029–1043**；**rwnx_send_dbg_start_app_req** |
| DBG_MEM_READ 实现 | 同上 | **rwnx_send_dbg_mem_read_req** **378–391** |
| Probe：芯片型号 + F1 初始化 + busrx | aic8800_bsp/**aicsdio.c** | **aicbsp_sdio_probe** **266–364**；chipmatch **321**；**aicwf_sdio_func_init** **337**（F1 0x0B/0x11 在 **1782、1788**）；**aicwf_sdio_bus_init** **346** |
| 谁调 driver_fw_init | aic8800_bsp/**aicsdio.c** | **aicbsp_set_subsys** **181–184**：aicbsp_sdio_init() → **aicbsp_driver_fw_init(aicbsp_sdiodev)** |

如果你愿意，下一步可以一起对“为什么我们这边 DBG_MEM_READ 收不到 CFM”列一个最小复现和排查清单（例如：只做 SDIO init + F1 0x0B/0x11 + busrx + 一次 DBG_MEM_READ，看 F1 0x02/0x12 是否有变化、是否有时序/电源要求等）。

---

## 六、driver_fw_init 报 -62 / aicbsp_system_config_8801 failed 的原因

现象：`dbg_mem_read: after 100ms F1 BYTEMODE_LEN(0x02)=0x00 BLOCK_CNT(0x12)=0x00`，随后 `wait_done_until` 超时，再用默认 chip_rev 继续时 `aicbsp_system_config_8801` 也报 **-62 (ETIMEDOUT)**。

**原因**：芯片**没有对任何 IPC 请求回复 CFM**——没有向 F1 RX FIFO 写数据，所以 BYTEMODE_LEN/BLOCK_CNT 一直为 0，busrx 收不到包，`on_cfm` 不会被调用，主线程等不到完成。第一步 DBG_MEM_READ 超时后，第二步 aicbsp_system_config_8801 里的 DBG_MEM_WRITE 同样在等 CFM，因此再次超时 (-62)。

**可能方向**：bootrom 未就绪、电源/复位/时钟、F1 0x0B/0x11 配置或上电/复位时序与芯片要求不一致；或需在发 IPC 前增加延时/额外配置。需结合硬件与 LicheeRV 上电/复位序列逐项对比。

---

## 七、上电与复位序列、延时逐项对照（排查 IPC 无响应）

### 7.1 对照表

| 项目 | LicheeRV (aicsdio.c) | 旧 wifi-driver (gpio + lib) | StarryOS 当前 |
|------|----------------------|-----------------------------|----------------|
| **平台电源** | Allwinner: power(0)→**50ms**→power(1)→**50ms**→rescan；Rockchip2 同 50/50；Amlogic: **200ms**/200ms | 两引脚：power 低→spin→高→spin；再 reset 低→spin→高→spin（spin 为短忙等，非固定 ms） | 单引脚 GPIOA_26：低 **50ms**→高 **50ms**（与 U-Boot/LicheeRV 一致） |
| **上电后到 SDIO** | 50ms 高后即 rescan，再 down_timeout(2000) 等卡检测（实际首访由 MMC 枚举时机决定） | power_on_and_reset 后直接 SDIO 设备 new + chip.init()，中间无显式 ms 级延时 | power_on_and_reset 后 **50ms spin + 100ms sleep**（`POST_POWER_STABLE_MS=100`），再 sdio_init |
| **复位** | 与电源同一控制（Allwinner/Rockchip 仅一个 power 序列） | 独立 reset 引脚：低→短延时→高→短延时 | 与上电共用 GPIOA_26，一次 power_on 即完成 |
| **下电** | Allwinner: power(0)→**100ms**→rescan；Rockchip2: carddetect(0)→**200ms**→power(0)→**200ms** | power_off：reset 低→spin→power 低 | power_off：pin 低→**100ms** |

### 7.2 已复刻的完整上电流程（StarryOS）

1. **gpio.rs**：`power_on()` = 低 50ms → 高 50ms（与 LicheeRV Allwinner/Rockchip2、U-Boot 一致）。
2. **flow.rs `aicbsp_power_on()`**：  
   `power_on_and_reset()`（即一次 power_on）→ `delay_spin_ms(50)` → **`axtask::sleep(POST_POWER_STABLE_MS)`**（100ms）→ 再进入 `aicbsp_sdio_init()`。  
   即：上电高电平后约 **50+100=150ms** 才做 SDIO 枚举与 IPC，给 bootrom 就绪时间。

### 7.3 若仍 IPC 无响应的排查建议

- **延时**：将 `flow.rs` 中 `POST_POWER_STABLE_MS` 改为 **200**（对齐 Amlogic 的 200ms），或适当加大，观察 BYTEMODE_LEN/CFM 是否出现。
- **GPIO 极性**：确认原理图：GPIOA_26 高 = 上电/释放复位、低 = 下电/复位。若反了，需在代码里对 set_pin 取反。
- **pinmux**：确认 0x0300104C 等已设为 GPIO 模式（与 U-Boot/设备树一致）。
- **SDIO 时钟/电压**：确认 SD1 时钟、电压在枚举前已稳定；必要时在 sdio_init 前再加短延时。
- **最小复现**：仅做 power_on → 150ms（或 200ms）→ sdio_init → F1 0x0B/0x11 → busrx → 一次 DBG_MEM_READ，看 F1 0x02 是否出现非 0。
- **IPC 参数**：LicheeRV `aic_bsp_driver.h` 中 **DRV_TASK_ID = 100**（驱动侧 src_id）。若误用 12，bootrom 可能不回 CFM；已改为 100（`fw_load.rs`）。
