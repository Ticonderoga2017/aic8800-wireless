//! AIC8800 SDIO 接口与流程
//! 对照 LicheeRV-Nano-Build aic8800_bsp/aicsdio.c、aic8800_fdrv/aicwf_sdio.c
//!
//! 按 AIC8800 芯片设计实现，无额外抽象：
//! - `types` — 类型、常量（aicsdio.h）、chipmatch
//! - `ops` — SdioOps、Aic8800Sdio（按 chipid 选 V1/V2 或 V3 寄存器）
//! - `cis` — FBR/CIS 读与解析、probe_from_sdio_cis
//! - `backend` — Aic8800SdioHost（基于 SG2002 SD1 的 CMD52/CMD53）
//! - `flow` — SDIO 流程六函数

mod backend;
mod cis;
mod flow;
pub mod irq;
mod mmc_impl;
mod ops;
mod types;

// 类型与常量
pub use types::{
    chipmatch, reg, reg_v3, sdio_ids, ProductId, SdioState, SdioType, BUFFER_SIZE, SDIO_DEVICE_ID_AIC,
    SDIO_FUNC_BLOCKSIZE, SDIO_VENDOR_ID_AIC, TAIL_LEN,
};

// AIC8800 设备接口（对照 aicsdio.c）
pub use ops::{Aic8800Sdio, SdioOps};

// FBR/CIS
pub use cis::{
    parse_cis_for_manfid, probe_from_sdio_cis, product_id_to_vid_did, read_fbr_cis_ptr,
    read_vendor_device, sdio_fbr_base, CISTPL_MANFID, SDIO_FBR_CIS,
};

// AIC8800 SDIO 主机：基于 SG2002 SD1 的 CMD52/CMD53 实现
pub use backend::Aic8800SdioHost;

// 流程六函数与 IPC 导出（供 FDRV 发送 LMAC 命令与注册 E2A 回调）
pub use flow::{
    aicbsp_current_product_id, aicbsp_driver_fw_init, aicbsp_minimal_ipc_verify, aicbsp_power_on,
    aicbsp_sdio_exit, aicbsp_sdio_init, aicbsp_sdio_probe, aicbsp_sdio_release,
    submit_cmd_tx_and_wait_tx_done, with_cmd_mgr, set_e2a_indication_cb, set_rx_data_indication_cb,
    sdio_poll_rx_once, E2aIndicationCb, RxDataIndicationCb,
};

// mmc crate 实现（MmcHost / SdioFunc）及 SDIO 驱动注册
pub use mmc_impl::{register_aicbsp_sdio_driver, BspSdioFuncRef, BspSdioHost};
