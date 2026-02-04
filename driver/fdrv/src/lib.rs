//! AIC8800 WiFi 全功能驱动 (FDRV)
//!
//! 对应 LicheeRV-Nano-Build/osdrv/extdrv/wireless/aic8800/aic8800_fdrv/
//!
//! 功能包括:
//! - 私有命令 (aic_priv_cmd) - 用户空间 ioctl 接口
//! - IPC 主机 (ipc_host) - 与固件通信
//! - WiFi 管理器 (aicwf_manager)
//! - SDIO Host (sdio_host) - 数据收发
//! - Vendor 命令 (aic_vendor) - nl80211 扩展

#![no_std]

extern crate alloc;

mod ipc;
mod manager;
mod priv_cmd;
mod sdio_bus;
mod sdio_host;
mod vendor;
mod wiphy;

pub use ipc::{ipc_handle_e2a_msg, ipc_send_cmd_sync, CMD_TX_BUF_SIZE, IpcHostCb};
pub use manager::{WifiManager, WifiState};
pub use priv_cmd::{AndroidWifiPrivCmd, PRIV_CMD_BUF_MAX};
pub use sdio_bus::{
    aicwf_sdio_exit_equiv, aicwf_sdio_probe_equiv, aicwf_sdio_register_equiv, BusOps, BusState,
    NX_TXQ_CNT, NX_TXDESC_CNT_MAX, SdioDev, SdioHostEnv, SdioReg, SDIO_ACTIVE_ST, SDIO_BUFFER_SIZE,
    SDIO_SLEEP_ST, SDIO_TAIL_LEN, SDIOWIFI_FUNC_BLOCKSIZE,
};
pub use sdio_host::SdioHost;
pub use vendor::{AIC_OUI, VendorSubcmd};
pub use wiphy::{
    IfaceType, InterfaceId, ScanResult, StationInfo, WiphyOps, WiphyOpsStub,
};
