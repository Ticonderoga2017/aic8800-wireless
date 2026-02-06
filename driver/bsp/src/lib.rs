//! AIC8800 WiFi BSP (Board Support Package)
//!
//! 对应 LicheeRV-Nano-Build/osdrv/extdrv/wireless/aic8800/aic8800_bsp/
//!
//! 功能包括:
//! - 固件管理 (fmacfw, fw_adid, fw_patch, fw_patch_table)
//! - SDIO 接口抽象
//! - 命令管理 (rwnx_cmd_mgr)
//! - 预留内存管理
//!
//! 不抽象平台，按 Linux 流程实现；上电、SDIO 注册/探测、固件加载由上层在 Linux 上直接调用对应接口。

#![no_std]

use axerrno::AxResult;

mod cmd;
mod export;
mod firmware;
mod firmware_data;
mod fw_load;
mod gpio;
mod sdio;
mod sdio_irq;
mod sync;

pub use sdio_irq::{sdio_tick, set_use_soft_irq_wake, SDIO_TIMER_POLL_INTERVAL_MS};

pub use cmd::{
    cmd_flags, IpcE2AMsg, LmacMsg, LmacMsgHeader, RwnxCmdMgr, TaskId, IPC_E2A_MSG_PARAM_SIZE,
    LMAC_MSG_MAX_LEN, RWNX_80211_CMD_TIMEOUT_MS, RWNX_CMD_E2AMSG_LEN_MAX,
    SCANU_START_REQ, SCANU_START_CFM, SCANU_RESULT_IND,
    SM_CONNECT_REQ, SM_CONNECT_CFM, SM_CONNECT_IND,
    SM_DISCONNECT_REQ, SM_DISCONNECT_CFM, SM_DISCONNECT_IND,
    MM_ADD_IF_REQ, MM_ADD_IF_CFM, MM_REMOVE_IF_REQ, MM_REMOVE_IF_CFM,
    MM_STA_ADD_REQ, MM_STA_ADD_CFM, MM_STA_DEL_REQ, MM_STA_DEL_CFM,
    MM_KEY_ADD_REQ, MM_KEY_ADD_CFM, MM_KEY_DEL_REQ, MM_KEY_DEL_CFM,
    MM_SET_POWER_REQ, MM_SET_POWER_CFM,
    MM_PS_CHANGE_IND, MM_RSSI_STATUS_IND,
    MM_GET_STA_INFO_REQ, MM_GET_STA_INFO_CFM,
    APM_START_REQ, APM_START_CFM, APM_STOP_REQ, APM_STOP_CFM,
    DRV_TASK_ID,
};
pub use export::{AicBspFeature, AicBspInfo, AicBspPwrState, AicBspSubsys, SkBuffId};
pub use firmware::{
    AicBspCpMode, AicBspFirmware, ChipRev, FW_8800DC_U02, FW_8800D80_U02, FW_U02,
    get_firmware_list,
};
pub use fw_load::{
    build_dbg_mem_block_write_req, build_dbg_start_app_req, fw_start_app, fw_upload_blocks,
    get_firmware_by_name, get_wifi_firmware, send_dbg_mem_read, set_wifi_firmware,
    CHIP_REV_MEM_ADDR, DBG_MEM_BLOCK_WRITE_CFM, DBG_START_APP_CFM, HOST_START_APP_AUTO,
    RAM_FMAC_FW_ADDR, RAM_FMAC_FW_PATCH_ADDR,
};
pub use sdio::{
    aicbsp_current_product_id, aicbsp_driver_fw_init, aicbsp_minimal_ipc_verify, aicbsp_power_on,
    aicbsp_sdio_exit, aicbsp_sdio_init, aicbsp_sdio_probe, aicbsp_sdio_release, chipmatch,
    parse_cis_for_manfid, probe_from_sdio_cis, read_fbr_cis_ptr, read_vendor_device, sdio_fbr_base,
    submit_cmd_tx_and_wait_tx_done, with_cmd_mgr, set_e2a_indication_cb, set_rx_data_indication_cb,
    sdio_poll_rx_once, E2aIndicationCb, RxDataIndicationCb,
    Aic8800Sdio, Aic8800SdioHost, BspSdioFuncRef, BspSdioHost, ProductId, SdioOps, SdioState, SdioType,
    CISTPL_MANFID, SDIO_FBR_CIS, reg as sdio_reg, reg_v3 as sdio_reg_v3, sdio_ids,
};
pub use sync::{delay_spin_ms, delay_spin_us, power_lock, probe_reset, probe_signal, probe_wait_timeout_ms, LOOPS_PER_MS};

/// 固件路径最大长度 (对应 FW_PATH_MAX)
pub const FW_PATH_MAX: usize = 200;
/// 默认固件路径 (对应 aic_fw_path)
pub const DEFAULT_FW_PATH: &str = "/lib/firmware/aic";

/// BSP 全局信息（对应 aic_bsp_main.c 中 aicbsp_info 全局变量）
/// aicbsp_init 时写入，aicbsp_set_subsys 内 aicbsp_driver_fw_init 读取/更新
static BSP_INFO: spin::Mutex<AicBspInfo> = spin::Mutex::new(AicBspInfo::default_const());

/// 预留内存初始化（对应 aic_bsp_driver.c aicbsp_resv_mem_init）
/// 预分配 skb 等供 TX 路径使用；无平台实现时为空操作
fn aicbsp_resv_mem_init() -> AxResult<()> {
    // CONFIG_RESV_MEM_SUPPORT 时预分配 skb；此处占位
    Ok(())
}

/// 预留内存反初始化（对应 aicbsp_resv_mem_deinit）
#[allow(dead_code)]
fn aicbsp_resv_mem_deinit() -> AxResult<()> {
    Ok(())
}

/// BSP 模块初始化（对应 aic_bsp_main.c aicbsp_init，391–427 行）
///
/// 与 LicheeRV 一致：仅做“模块加载”时的工作，**不**执行上电/SDIO/固件加载。
/// - 设置 cpmode、预留内存初始化、probe 信号量重置（sema_init）。
/// - 不在此处注册 SDIO 驱动（LicheeRV 在 aicbsp_sdio_init 内 sdio_register_driver）。
/// - 不在此处持 power_lock 或调用 aicbsp_power_on / aicbsp_sdio_init / aicbsp_driver_fw_init；
///   该序列由 **aicbsp_set_subsys(AIC_WIFI, AIC_PWR_ON)** 执行（对应 FDRV rwnx_main 调用 set_subsys）。
///
/// # 参数
/// - `info`: BSP 全局信息，用于保存 cpmode、hwinfo 等；会写入静态供 set_subsys 使用
/// - `testmode`: 固件模式（0=正常，1=射频测试等），对应模块参数 testmode
pub fn aicbsp_init(info: &mut AicBspInfo, testmode: AicBspCpMode) -> AxResult<()> {
    log::info!(target: "wireless::bsp", "aicbsp_init (module init only, align LicheeRV)");

    info.cpmode = testmode as u8;
    *BSP_INFO.lock() = info.clone();
    aicbsp_resv_mem_init()?;
    sync::probe_reset();

    Ok(())
}

/// 仅执行 GPIO 上电与验证（用于在 main 中测试 GPIO 设置是否正确，不执行 sdio_init/driver_fw_init）
///
/// 流程：probe_reset → power_lock → aicbsp_power_on（含 verify_after_power_on 打日志）。
/// 通过日志 "WiFi GPIO 验证: POWER_EN=... RESET=... => OK/FAIL" 判断 GPIO 是否正确。
pub fn aicbsp_init_gpio_test() -> AxResult<()> {
    log::info!(target: "wireless::bsp", "aicbsp_init_gpio_test: GPIO power-on and verify only");
    sync::probe_reset();
    let _guard = sync::power_lock();
    aicbsp_power_on()?;
    Ok(())
}

/// BSP 模块反初始化（对应 aic_bsp_main.c aicbsp_exit）
///
/// 与 LicheeRV 一致：若曾执行过 sdio_init，则先 aicbsp_sdio_exit（停线程、清设备、disable_func），再 resv_mem_deinit。
pub fn aicbsp_exit(info: &mut AicBspInfo) -> AxResult<()> {
    log::info!(target: "wireless::bsp", "aicbsp_exit");
    // 与 aic_bsp_main.c 435-436 一致：if(aicbsp_sdiodev) aicbsp_sdio_exit()
    aicbsp_sdio_exit();
    aicbsp_resv_mem_deinit()?;
    *info = AicBspInfo::default();
    Ok(())
}

/// BSP 平台初始化（由 FDRV 或上层在 probe 前调用，对应 aicbsp_platform_init 的语义）
pub fn aicbsp_platform_init() -> AxResult<()> {
    Ok(())
}

/// BSP 平台反初始化
pub fn aicbsp_platform_deinit() -> AxResult<()> {
    Ok(())
}

/// 设置子系统电源（对应 aicsdio.c aicbsp_set_subsys，157–225 行）
///
/// 与 LicheeRV 一致：整段“上电 → sdio_init → driver_fw_init”在 **power_lock** 内执行。
/// - AIC_WIFI AIC_PWR_ON：aicbsp_power_on → aicbsp_sdio_init（内建 sdio_register_driver + probe）→ aicbsp_driver_fw_init。
/// - AIC_WIFI AIC_PWR_OFF：aicbsp_sdio_exit → aicbsp_platform_power_off（当前为空，可后续接 GPIO 下电）。
pub fn aicbsp_set_subsys(subsys: AicBspSubsys, state: AicBspPwrState) -> AxResult<()> {
    if subsys != AicBspSubsys::Wifi {
        return Ok(());
    }
    if state == AicBspPwrState::On {
        let _guard = sync::power_lock();
        log::info!(target: "wireless::bsp", "aicbsp_set_subsys: AIC_WIFI AIC_PWR_ON (power_on → sdio_init → driver_fw_init)");
        log::info!(target: "wireless::bsp", "步骤1: GPIO复位和电源控制");
        sdio::aicbsp_power_on()?;
        log::info!(target: "wireless::bsp", "步骤2: SDIO 接口初始化（sdio_register_driver 在 aicbsp_sdio_init 内）");
        sdio::aicbsp_sdio_init()?;
        log::info!(target: "wireless::bsp", "步骤3: 驱动固件初始化");
        sdio::aicbsp_driver_fw_init(&mut *BSP_INFO.lock())?;
    } else {
        log::info!(target: "wireless::bsp", "aicbsp_set_subsys: AIC_WIFI AIC_PWR_OFF");
        sdio::aicbsp_sdio_exit();
        // aicbsp_platform_power_off()：LicheeRV 下拉电源等，当前无 GPIO 下电接口则留空
    }
    Ok(())
}
