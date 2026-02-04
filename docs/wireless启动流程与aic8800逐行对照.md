# wireless 启动流程与 aic8800 逐行对照

本文档按 **LicheeRV aic8800 BSP 的完整执行顺序** 逐行对照，标出 wireless 中**已对齐**与**仍缺失**的部分，并列出 **SDIO 注册/反注册** 等必须显式说明的差异。

---

## 一、LicheeRV 完整执行流程（按调用顺序）

### 1. 模块加载：aic_bsp_main.c `aicbsp_init`（391–427 行）

| 行号 | LicheeRV | wireless 对应 | 状态 |
|------|----------|----------------|------|
| 396 | `aicbsp_info.cpmode = testmode` | `info.cpmode = testmode as u8`（aicbsp_init 参数） | ✅ 已对齐 |
| 398 | `aicbsp_resv_mem_init()` | `aicbsp_resv_mem_init()`（空实现） | ✅ 已对齐 |
| 401 | `sema_init(&aicbsp_probe_semaphore, 0)` | `sync::probe_reset()`（PROBE_SIGNAL=false） | ✅ 已对齐 |
| 403–407 | `platform_driver_register(&aicbsp_driver)` | **无**：无 Linux 设备模型 | ⚠️ 缺失（设计上无平台驱动） |
| 409–415 | `platform_device_alloc` / `platform_device_add` | **无** | ⚠️ 缺失（设计上无 platform_device） |
| 417–421 | `sysfs_create_group(&aicbsp_pdev->dev.kobj, ...)` | **无** | ⚠️ 缺失（无 sysfs） |
| 423 | `mutex_init(&aicbsp_power_lock)` | `sync::power_lock()` 使用静态 `POWER_LOCK`（首次取锁即“就绪”） | ✅ 已对齐 |
| 424–426 | `aicbsp_set_subsys(AIC_BLUETOOTH, AIC_PWR_ON)`（部分平台） | `aicbsp_set_subsys` 存在但为空实现；上电由主流程直接 `aicbsp_power_on` | ✅ 语义等价 |

### 2. 上电路径：aicbsp_set_subsys(..., PWR_ON)（aicsdio.c 157–227 行）

| 行号 | LicheeRV | wireless 对应 | 状态 |
|------|----------|----------------|------|
| 165 | `mutex_lock(&aicbsp_power_lock)` | `sync::power_lock()` 取 guard | ✅ 已对齐 |
| 182 | `aicbsp_platform_power_on()` | `aicbsp_power_on()` | ✅ 已对齐（见下） |
| 184 | `aicbsp_sdio_init()` | `aicbsp_sdio_init()` | ✅ 已对齐（见下） |
| 186 | `aicbsp_driver_fw_init(aicbsp_sdiodev)` | `aicbsp_driver_fw_init(info)` | ✅ 已对齐 |
| 187–189 | `aicbsp_sdio_release(aicbsp_sdiodev)`（CONFIG_FDRV_NO_REG_SDIO 时不做） | `aicbsp_sdio_release()`（当前为空 stub） | ⚠️ 见“缺失” |
| 211 | `pre_power_map = cur_power_map`；`mutex_unlock` | guard drop 释放锁 | ✅ 已对齐 |

### 3. 平台上电：aicsdio.c `aicbsp_platform_power_on`（487–558 行）

| 行号 | LicheeRV | wireless 对应 | 状态 |
|------|----------|----------------|------|
| 494–499 | Allwinner：`aicbsp_bus_index`、`sunxi_wlan_set_power(0)`、50ms、`sunxi_wlan_set_power(1)`、50ms | GPIO 单引脚 `power_on_and_reset()`：低 50ms→高 50ms + `delay_spin_ms(50)` | ✅ 已对齐（SG2002 单引脚） |
| 520–521 | `sema_init(&aic_chipup_sem, 0)`；`aicbsp_reg_sdio_notify(&aic_chipup_sem)` | **无** | ❌ 见“SDIO 注册” |
| 521 | `aicbsp_reg_sdio_notify` → **sdio_register_driver(&aicbsp_dummy_sdmmc_driver)** | **无**：无“dummy 驱动注册” | ❌ 缺失 |
| 527–534 | Allwinner：`sunxi_wlan_set_power(0)`→50ms→`(1)`→50ms→`sunxi_mmc_rescan_card` | 我们无 MMC 子系统，不做 rescan | ⚠️ 设计差异 |
| 535 | **down_timeout(&aic_chipup_sem, 2000)** 等卡检测（dummy probe 里 up） | **无**：我们上电后固定延时（POST_POWER_STABLE_MS 500ms）再枚举 | ⚠️ 设计差异（我们主动枚举代替“等卡出现”） |
| 536 | `aicbsp_unreg_sdio_notify()` → **sdio_unregister_driver(&aicbsp_dummy_sdmmc_driver)** | **无** | ❌ 缺失（与上成对） |
| 541–557 | 超时路径：unreg_sdio_notify、power_off、return -1 | 我们枚举失败直接返回 Err | ✅ 语义等价 |

结论（platform_power_on）：  
- **SDIO 注册/反注册（dummy）**：LicheeRV 用 **sdio_register_driver(dummy)** → rescan → **down_timeout(aic_chipup_sem)** 等“卡出现”，再 **sdio_unregister_driver(dummy)**。  
- wireless **没有** 这两步，用“上电 + 固定延时 + 主动 CMD0/5/3/7 枚举”代替“等内核检测到卡”。若需**语义级**对齐，可增加“可选：注册 dummy 等价（仅占位，不实际等卡）”的注释或空 API；当前为**设计取舍**，非功能缺失。

### 4. SDIO 初始化：aicsdio.c `aicbsp_sdio_init`（588–605 行）

| 行号 | LicheeRV | wireless 对应 | 状态 |
|------|----------|----------------|------|
| 591 | **sdio_register_driver(&aicbsp_sdio_driver)** | **无** | ❌ 见“SDIO 注册” |
| 597 | **down_timeout(&aicbsp_probe_semaphore, 2000)** 等 probe | **无**：我们在同一 `aicbsp_sdio_init` 内同步完成枚举 + CIS + chipmatch + 等价 probe | ⚠️ 设计差异 |
| 602 | return 0 | Ok(()) | ✅ |

说明：  
- LicheeRV：**注册正式 SDIO 驱动** → 内核枚举到卡后调用 **aicbsp_sdio_probe** → probe 里 **up(probe_semaphore)** → `aicbsp_sdio_init` 里 down_timeout 返回。  
- wireless：无内核，**不调用** sdio_register_driver；在 **aicbsp_sdio_init** 内顺序执行：`new_sd1_with_card_init()`（CMD0/5/3/7、4-bit）→ 使能 F1 → **probe_from_sdio_cis**（读 FBR/CIS、chipmatch、**aicbsp_sdio_probe(pid)**）→ 使能 F2（非 8801）→ F1 0x0B/0x11/0x04 → 建 Aic8800Sdio、RwnxCmdMgr、启动 bustx/busrx。  
- 因此：**“SDIO 注册”在 wireless 中以“主动枚举 + 内联 probe”替代**；**“反注册”在 aicbsp_sdio_exit 中以“清设备 + 停线程”体现**，但**没有**显式的 `sdio_unregister_driver` 等价调用名（见下文 aicbsp_sdio_exit）。

### 5. Probe：aicsdio.c `aicbsp_sdio_probe`（267–362 行）

| 行号 | LicheeRV | wireless 对应 | 状态 |
|------|----------|----------------|------|
| 270–277 | func 空 / vid/did 不匹配则 return -ENODEV | cis 中 chipmatch 失败则 probe_from_sdio_cis 返回 Err | ✅ 已对齐 |
| 298–299 | **host->caps \|= MMC_CAP_NONREMOVABLE** | **无**（无 host 抽象 caps） | ⚠️ 可选 |
| 302 | `func = func->card->sdio_func[0]`（用 F1） | 我们始终用 F1 基址 0x100 | ✅ 已对齐 |
| 305–317 | kzalloc bus_if、sdiodev；aicbsp_sdiodev = sdiodev | SDIO_DEVICE、CMD_MGR 静态；CURRENT_PRODUCT_ID | ✅ 已对齐 |
| 321 | `aicwf_sdio_chipmatch(sdiodev, func->vendor, func->device)` | `probe_from_sdio_cis` 内 chipmatch(vid, did) → ProductId | ✅ 已对齐 |
| 323–325 | func_msg（8800DC/DW） | 非 8801 时使能 F2、set_block_size(2,512) | ✅ 已对齐 |
| 337 | **aicwf_sdio_func_init(sdiodev)** | 在 aicbsp_sdio_init 内：set_block_size(1,512)、F1 使能（CCCR）、0x0B=1、0x11=1、0x04=0x07、udelay(100) 等价 | ✅ 已对齐 |
| 346 | **aicwf_sdio_bus_init(sdiodev)** | ensure_bustx_thread_started、ensure_busrx_thread_started | ✅ 已对齐 |
| 350 | **aicbsp_platform_init(sdiodev)** | aicbsp_platform_init() 空实现；RwnxCmdMgr 已在 init 内创建 | ✅ 已对齐 |
| 353 | **up(&aicbsp_probe_semaphore)** | **sync::probe_signal()**（在 aicbsp_sdio_probe 内，由 cis 路径调用） | ✅ 已对齐 |

### 6. 固件初始化：aic_bsp_driver.c `aicbsp_driver_fw_init` 与 wireless flow.rs

| LicheeRV 步骤 | wireless 对应 | 状态 |
|---------------|----------------|------|
| dbg_mem_read chip_rev | send_dbg_mem_read_busrx(CHIP_REV_MEM_ADDR) | ✅ 已对齐 |
| 按 product_id/chip_rev 选固件表 | get_firmware_list、get_firmware_by_name | ✅ 已对齐 |
| aicbsp_system_config（DBG_MEM_WRITE 表） | aicbsp_system_config_8801 | ✅ 已对齐（8801） |
| 固件上传 adid/patch/table、wl_fw | fw_upload_blocks(wl_fw)、patch、aicwifi_patch_config、aicwifi_sys_config | ✅ 已对齐 |
| start_app | fw_start_app(HOST_START_APP_AUTO) | ✅ 已对齐 |

### 7. 释放与退出

| LicheeRV | wireless 对应 | 状态 |
|----------|----------------|------|
| **aicbsp_sdio_release**（614–623）：bus_if->state=BUS_DOWN_ST；claim_host→release_irq→release_host（及 func_msg） | **aicbsp_sdio_release()**：空 stub，仅 log | ⚠️ 未实现（无 release_irq/release_host 抽象） |
| **aicbsp_sdio_exit**（607–609）：**sdio_unregister_driver(&aicbsp_sdio_driver)** | **aicbsp_sdio_exit()**：停 BUSTX_RUNNING/BUSRX_RUNNING、清 SDIO_DEVICE/CMD_MGR、CURRENT_PRODUCT_ID；**无** unregister_driver 调用 | ⚠️ 见下 |
| **aicbsp_sdio_remove**（365–408）：release、**aicwf_sdio_func_deinit**（内 **sdio_disable_func**）、dev_set_drvdata NULL、kfree | aicbsp_sdio_exit **未**调用 **sdio_disable_func**（F1/F2 CCCR 关闭） | ❌ 缺失 |
| **aicbsp_exit**（433–449）：**aicbsp_sdio_exit()**、sysfs_remove、platform_device_del、platform_driver_unregister、mutex_destroy、resv_mem_deinit | **aicbsp_exit** 仅 resv_mem_deinit、info 清零，**未调用 aicbsp_sdio_exit()** | ❌ 缺失 |

---

## 二、完整执行流程并列表（wireless 侧）

### wireless 当前实际执行顺序（如 main 调用 aicbsp_init）

1. **aicbsp_init**
   - aicbsp_resv_mem_init()
   - sync::probe_reset()
   - sync::power_lock() 取 guard
   - **aicbsp_power_on()**（GPIO pinmux、power_on_and_reset、50+POST_POWER_STABLE_MS）
   - **aicbsp_sdio_init()**
     - new_sd1_with_card_init()（CMD0/5/3/7、4-bit）
     - F1 使能（CCCR 0x02/0x03）
     - probe_from_sdio_cis(1) → chipmatch → **aicbsp_sdio_probe(pid)**
     - F2 使能（非 8801）、set_block_size(2,512)
     - F1 set_block_size(1,512)、0x0B=1、0x11=1、0x04=0x07
     - Aic8800Sdio、CMD_MGR 写入静态；ensure_bustx_thread_started；ensure_busrx_thread_started
   - **aicbsp_driver_fw_init(info)**（chip_rev、固件表、system_config、upload、start_app）
   - （未调用 aicbsp_sdio_release）
   - guard drop

2. **aicbsp_exit**（若被调用）
   - aicbsp_resv_mem_deinit()
   - **未调用 aicbsp_sdio_exit()** → 设备与线程未清理

### LicheeRV 侧对应顺序（简化）

1. aicbsp_init：resv_mem_init、sema_init(probe_semaphore)、platform_driver/device、sysfs、mutex_init；可选 set_subsys(BT, ON)。
2. aicbsp_set_subsys(ON)：power_lock → **aicbsp_platform_power_on**（dummy **sdio_register** → rescan → **down_timeout(aic_chipup_sem)** → **sdio_unregister** dummy）→ **aicbsp_sdio_init**（**sdio_register_driver** 正式 → **down_timeout(probe_semaphore)**）→ aicbsp_driver_fw_init → aicbsp_sdio_release → power_unlock。
3. 内核枚举到卡 → **aicbsp_sdio_probe**（host caps、chipmatch、func_init、**bus_init**、platform_init、**up(probe_semaphore)**）。
4. aicbsp_exit：**aicbsp_sdio_exit**（**sdio_unregister_driver**）→ sysfs/device/driver 清理、mutex_destroy、resv_mem_deinit。

---

## 三、已对齐部分汇总

- 上电与电源锁：power_lock、power_on（GPIO 时序、延时）。
- SDIO 枚举：CMD0→CMD5→CMD3→CMD7、4-bit、F1/F2 使能、set_block_size、F1 0x0B/0x11/0x04。
- CIS 与 chipmatch：probe_from_sdio_cis、ProductId、aicbsp_sdio_probe(pid)、probe_signal。
- bustx/busrx 线程：ensure_bustx_thread_started、ensure_busrx_thread_started、submit_cmd_tx_and_wait_tx_done、poll_rx_one、on_cfm。
- 固件：chip_rev、固件表、aicbsp_system_config_8801、fw_upload_blocks、fw_start_app。
- IPC 长度与 512 块尾：ipc_send_len_8801 与 LicheeRV 一致。
- aicbsp_sdio_exit 内：停 bustx/busrx、清 SDIO_DEVICE/CMD_MGR/CURRENT_PRODUCT_ID（与 remove 后“无设备”语义一致）。

---

## 四、缺失或需补全部分（逐项）

| 序号 | 项目 | LicheeRV 位置 | wireless 现状 | 建议 |
|------|------|----------------|----------------|------|
| 1 | **SDIO 注册（正式驱动）** | aicsdio.c 591：sdio_register_driver(&aicbsp_sdio_driver) | 无：用“主动枚举 + 内联 probe”替代 | 文档标明“以主动枚举替代，无 register_driver 调用”；若需 API 对称可增加空函数 `sdio_register_driver_equiv()` 占位。 |
| 2 | **SDIO 反注册（正式驱动）** | aicsdio.c 608：sdio_unregister_driver(&aicbsp_sdio_driver) | aicbsp_sdio_exit 清设备/线程，但无显式“unregister”名 | 文档标明“exit 内清设备即等价”；可选：在 aicbsp_sdio_exit 注释或增加 `sdio_unregister_driver_equiv()` 空调用以对称。 |
| 3 | **Dummy SDIO 注册/反注册（等卡）** | aicsdio.c 521/536：aicbsp_reg_sdio_notify / aicbsp_unreg_sdio_notify（dummy driver） | 无 | 设计上以“固定延时 + 主动枚举”代替“等卡检测”；无需实现 dummy，仅文档说明。 |
| 4 | **aicbsp_sdio_release** | aicsdio.c 613–623：release_irq、release_host | 空 stub | 无 PLIC 时 release_irq 无操作；release_host 我们为 guard 模式，可保持 stub 或文档说明“FDRV 按需 claim_host 即可”。 |
| 5 | **aicbsp_sdio_exit 中 sdio_disable_func** | aicsdio.c remove → aicwf_sdio_func_deinit → sdio_disable_func | 未调用 | 在 **aicbsp_sdio_exit** 中，在清 SDIO_DEVICE 前，若存在 MmcHost/SdioFunc，对 F1/F2 调用 **disable_func**（或通过 mmc 的 sdio_disable_function）。 |
| 6 | **aicbsp_exit 调用 aicbsp_sdio_exit** | aic_bsp_main.c 435–436：if(sdiodev) aicbsp_sdio_exit() | aicbsp_exit 未调用 aicbsp_sdio_exit | **在 aicbsp_exit 中增加 aicbsp_sdio_exit()**（与 LicheeRV __exit 一致）。 |
| 7 | platform_driver / platform_device / sysfs | aic_bsp_main.c 403–421 | 无 | 无设备模型时不做；文档标明“不实现”。 |
| 8 | host->caps MMC_CAP_NONREMOVABLE | aicsdio.c 300、382 | 无 | 可选；我们无 host 抽象 caps，可忽略或后续在 MmcHost 扩展。 |
| 9 | probe 的 down_timeout(probe_semaphore) | aicsdio.c 597 | 我们同步 probe，不等待 | 设计差异；我们 init 内同步完成，无需改。 |

---

## 五、建议代码修改（最小必做）

1. **aicbsp_exit 中调用 aicbsp_sdio_exit**  
   - 在 `driver/bsp/src/lib.rs` 的 `aicbsp_exit` 中，在 `aicbsp_resv_mem_deinit()` 之前或之后（与 LicheeRV 顺序一致）调用 **aicbsp_sdio_exit()**，确保卸载时设备与线程被清理。

2. **aicbsp_sdio_exit 中调用 sdio_disable_func（F1/F2）**  
   - 在 `flow.rs` 的 **aicbsp_sdio_exit** 中，在 `SDIO_DEVICE.lock().take()` 之前，若当前存在 SDIO 设备（可通过持锁取 as_ref），对该设备的 F1、F2 调用 mmc 的 **sdio_disable_function**（或 SdioFunc::disable_func），再清空 SDIO_DEVICE/CMD_MGR。  
   - 注意：take() 会拿走所有权，因此需要在 take 之前用 as_ref 拿到 host/func 做 disable，或先 clone/拿到 BspSdioFuncRef 再 take。

3. **（可选）文档/注释**  
   - 在 `aicbsp_sdio_init` / `aicbsp_sdio_exit` 注释中明确写出：“无 sdio_register_driver/sdio_unregister_driver 调用，以主动枚举与 exit 内清设备/线程等价。”

完成上述 1、2 后，wireless 的启动与退出流程与 aic8800 的对应关系即可在“无内核、无设备模型”前提下做到逐项可对照、且关键反注册与 disable_func 不缺失。
