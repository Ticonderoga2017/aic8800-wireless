//! SDIO Host 接口
//! 对应 LicheeRV sdio_host.c, sdio_host.h
//!
//! FDRV 层对“SDIO 收发”的抽象：平台实现 SdioHostOps（send_pkt/recv_pkt），
//! 内部可调用 BSP SdioOps。TX 描述符/host_id 管理在 sdio_bus::SdioHostEnv，
//! aicwf_sdio_host_txdesc_push / aicwf_sdio_host_tx_cfm_handler 语义见 sdio_bus。

use core::marker::{Send, Sync};
use core::result::Result;

use bsp::{ProductId, SdioType};

/// SDIO Host 操作 trait - 平台实现
pub trait SdioHostOps: Send + Sync {
    fn send_pkt(&self, buf: &[u8], count: usize) -> Result<usize, i32>;
    fn recv_pkt(&self, buf: &mut [u8], size: u32, msg: u8) -> Result<usize, i32>;
}

/// SDIO Host 设备
pub struct SdioHost {
    chipid: ProductId,
}

impl SdioHost {
    pub fn new(chipid: ProductId) -> Self {
        Self { chipid }
    }

    pub fn chipid(&self) -> ProductId {
        self.chipid
    }

    /// 发送配置命令 - 通过平台 ops 实现
    pub fn send_cfg_cmd(ops: &dyn SdioHostOps, buf: &[u8]) -> Result<usize, i32> {
        ops.send_pkt(buf, buf.len())
    }

    /// 接收配置响应
    pub fn recv_cfg_rsp(ops: &dyn SdioHostOps, buf: &mut [u8], msg_type: SdioType) -> Result<usize, i32> {
        ops.recv_pkt(buf, buf.len() as u32, msg_type as u8)
    }

    /// 发送数据
    pub fn send_data(ops: &dyn SdioHostOps, buf: &[u8]) -> Result<usize, i32> {
        ops.send_pkt(buf, buf.len())
    }

    /// 接收数据
    pub fn recv_data(ops: &dyn SdioHostOps, buf: &mut [u8], len: u32) -> Result<usize, i32> {
        ops.recv_pkt(buf, len, SdioType::Data as u8)
    }
}
