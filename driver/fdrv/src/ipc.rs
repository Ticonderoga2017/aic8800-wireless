//! IPC 主机接口
//! 对应 ipc_host.c, ipc_host.h
//!
//! Host 与固件间的通信，E2A (Emb to App) 消息处理；send_cmd_sync 与 CmdMgr 对接

use core::result::Result;

use bsp::{LmacMsg, RwnxCmdMgr, IpcE2AMsg};

/// 命令发送缓冲区大小（A2E 头 8 字节 + param）
pub const CMD_TX_BUF_SIZE: usize = 8 + bsp::LMAC_MSG_MAX_LEN;

/// IPC 回调 - 对应 ipc_host_cb_tag
/// 平台需实现这些回调以完成 Host-FW 数据交换
pub trait IpcHostCb {
    /// 发送数据确认
    fn send_data_cfm(&self, host_id: *mut ()) -> i32;
    /// 接收数据指示
    fn recv_data_ind(&self, host_id: *mut ()) -> u8;
    /// 接收雷达指示
    fn recv_radar_ind(&self, host_id: *mut ()) -> u8;
    /// 接收不支持RX向量指示
    fn recv_unsup_rx_vec_ind(&self, host_id: *mut ()) -> u8;
    /// 接收消息指示
    fn recv_msg_ind(&self, host_id: *mut ()) -> u8;
    /// 接收消息ACK指示
    fn recv_msgack_ind(&self, host_id: *mut ()) -> u8;
    /// 接收调试指示
    fn recv_dbg_ind(&self, host_id: *mut ()) -> u8;
    /// 主信标时间指示
    fn prim_tbtt_ind(&self);
    /// 次信标时间指示
    fn sec_tbtt_ind(&self);
}

/// 处理从固件收到的 E2A 消息：将确认写入 CmdMgr，供 send_cmd_sync 的 wait_done 匹配
pub fn ipc_handle_e2a_msg(cmd_mgr: &mut RwnxCmdMgr, msg: &IpcE2AMsg) -> Result<(), i32> {
    let param_len = msg.param_len as usize;
    let len = param_len.min(bsp::IPC_E2A_MSG_PARAM_SIZE * 4);
    log::debug!(target: "wireless::fdrv", "ipc_handle_e2a_msg id=0x{:04x} param_len={}", msg.id, param_len);
    let param_bytes = unsafe {
        core::slice::from_raw_parts(msg.param.as_ptr() as *const u8, len)
    };
    cmd_mgr.on_cfm(msg.id, param_bytes);
    Ok(())
}

/// 同步发送一条 LMAC 命令并等待对应 CFM
/// tx_fn: 将序列化后的消息发送到总线（如通过 SdioOps）
/// poll_fn: 等待期间轮询，接收 &mut RwnxCmdMgr（如执行一次 RX 读，并对 E2A 调用 ipc_handle_e2a_msg(cmd_mgr, msg)）
pub fn ipc_send_cmd_sync<F, E>(
    cmd_mgr: &mut RwnxCmdMgr,
    msg: &LmacMsg,
    reqid: u16,
    timeout_ms: u32,
    tx_fn: &mut F,
    poll_fn: &mut dyn FnMut(&mut RwnxCmdMgr),
) -> Result<(), i32>
where
    F: FnMut(&LmacMsg) -> Result<(), E>,
{
    log::debug!(target: "wireless::fdrv", "ipc_send_cmd_sync msg_id=0x{:04x} reqid=0x{:04x}", msg.header.id, reqid);
    let token = cmd_mgr.push(reqid).ok_or(-12)?; // -ENOMEM
    tx_fn(msg).map_err(|_| -5)?; // -EIO
    cmd_mgr.wait_done(token, timeout_ms, poll_fn, None)
}
