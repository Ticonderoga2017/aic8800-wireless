//! 数据面接口：802.3 帧经 SDIO 与固件收发，对照 aic8800 aicwf_txrxif.c / aicwf_sdio_bus_txdata / aicwf_process_rxframes
//! 与 ipc_shared.h hostdesc、aicwf_sdio.h SDIO 类型常量对齐。

use core::result::Result;

/// 与 aicwf_sdio.h 一致：RX 时若 (buf[2] & SDIO_TYPE_CFG) != SDIO_TYPE_CFG 则为数据帧
pub const SDIO_TYPE_DATA: u8 = 0x00;
pub const SDIO_TYPE_CFG: u8 = 0x10;
pub const SDIO_TYPE_CFG_CMD_RSP: u8 = 0x11;
pub const SDIO_TYPE_CFG_DATA_CFM: u8 = 0x12;
pub const SDIO_TYPE_CFG_PRINT: u8 = 0x13;

/// 与 ipc_shared.h 对齐：hostdesc 等描述符与缓冲计数
pub const IPC_RXBUF_CNT: usize = 128;
pub const IPC_RXDESC_CNT: usize = 128;
/// RX 向量/硬件头长度（DMA_HDR_PHYVECT_LEN 等）
pub const RX_HWHRD_LEN: usize = 36;
/// 命令/CFM 缓冲最大长度（与 BSP RWNX_CMD_E2AMSG_LEN_MAX 一致）
pub const CMD_BUF_MAX: usize = 256;
/// 数据 RX 队列最大长度（占位，与 aicwf_sdio 队列对齐时再定）
pub const MAX_RXQLEN: usize = 64;

/// 数据发送：将一条 802.3 帧提交给固件（经 SDIO），与 aicwf_frame_tx -> aicwf_bus_txdata 对齐
/// 与 ipc_shared.h hostdesc（packet_len, eth_dest_addr, eth_src_addr, ethertype, vif_idx, staid 等）对齐
pub trait TxDataIf {
    /// 提交一个数据包发送，成功返回 Ok(())，失败返回负错误码
    fn tx_data(&self, buf: &[u8]) -> Result<(), i32>;
}

/// 数据接收回调：从固件收到一条 802.3 帧时调用（在 busrx 或收包线程上下文），与 rwnx_rx、netif_rx 语义对齐
pub type RxDataCb = Option<unsafe fn(buf: *const u8, len: usize)>;

static mut RX_DATA_CB: RxDataCb = None;

/// 注册数据接收回调（FDRV 在初始化时调用）。BSP 在 poll_rx_one 中当 (buf[2]&0x7f) 非 SDIO_TYPE_CFG* 时应对数据帧调用此回调
pub fn set_rx_data_cb(cb: RxDataCb) {
    unsafe { RX_DATA_CB = cb };
}

/// 由 BSP 在检测到数据帧时调用（BSP 需通过 fdrv 导出或链接此符号）
#[inline]
pub fn fdrv_rx_data_invoke(buf: *const u8, len: usize) {
    if let Some(cb) = unsafe { RX_DATA_CB } {
        unsafe { cb(buf, len) };
    }
}

/// 提交数据包发送（全局入口，对应 aicwf_frame_tx）。需由 BSP 或平台实现：入队到 txdata 队列并触发 bustx
pub fn tx_data(_buf: &[u8]) -> Result<(), i32> {
    Err(-38) // -ENOSYS 占位
}
