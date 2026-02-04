//! 固件下载与启动
//! 对应 aic_bsp_driver.c 中 rwnx_plat_bin_fw_upload_android、rwnx_send_dbg_start_app_req 等
//! 通过 CmdMgr + 平台提供的 tx_fn 发送 DBG_* 消息

use crate::cmd::{LmacMsg, RwnxCmdMgr};
use crate::LMAC_MSG_MAX_LEN;

/// DBG 任务 ID (TASK_DBG)
const TASK_DBG: u16 = 1;
/// 驱动侧任务 ID (TASK_API)，与 LicheeRV aic_bsp_driver.h #define DRV_TASK_ID 100 一致
const DRV_TASK_ID: u16 = 100;

/// 消息 ID：LicheeRV 使用 LMAC_FIRST_MSG(TASK_DBG)=1024 起
pub const DBG_MEM_READ_REQ: u16 = 1024;
pub const DBG_MEM_READ_CFM: u16 = 1025;
pub const DBG_MEM_WRITE_REQ: u16 = 1026;
pub const DBG_MEM_WRITE_CFM: u16 = 1027;
pub const DBG_MEM_BLOCK_WRITE_REQ: u16 = 1034; // 1024+10
pub const DBG_MEM_BLOCK_WRITE_CFM: u16 = 1035;
pub const DBG_START_APP_REQ: u16 = 1036;
pub const DBG_START_APP_CFM: u16 = 1037;
pub const DBG_MEM_MASK_WRITE_REQ: u16 = 1038;
pub const DBG_MEM_MASK_WRITE_CFM: u16 = 1039;

/// RAM FMAC 固件基址 (与 LicheeRV aic_bsp_driver.h RAM_FMAC_FW_ADDR 0x00120000 一致)
pub const RAM_FMAC_FW_ADDR: u32 = 0x0012_0000;
/// RAM FMAC 补丁固件地址 (LicheeRV RAM_FMAC_FW_PATCH_ADDR)
pub const RAM_FMAC_FW_PATCH_ADDR: u32 = 0x0019_0000;
/// 启动类型：自动
pub const HOST_START_APP_AUTO: u32 = 1;
/// 启动类型：Dummy（供上层选用）
#[allow(dead_code)]
pub const HOST_START_APP_DUMMY: u32 = 0;

/// 芯片版本寄存器地址（读得 chip_rev，LicheeRV aic_bsp_driver.c）
pub const CHIP_REV_MEM_ADDR: u32 = 0x4050_0000;

/// 固件存储：按名称在本地（embed）或注册表中查找，对应 LicheeRV 从路径/数组按名加载
use spin::Mutex;
const MAX_FIRMWARE_SLOTS: usize = 12;
static WIFI_FIRMWARE_STORE: Mutex<[Option<(&'static str, &'static [u8])>; MAX_FIRMWARE_SLOTS]> =
    Mutex::new([None; MAX_FIRMWARE_SLOTS]);

/// 注册 WiFi 固件数据（按文件名，如 "fmacfw.bin"、"fmacfw_patch.bin"），可多次调用注册多份
pub fn set_wifi_firmware(name: &'static str, data: &'static [u8]) {
    let mut guard = WIFI_FIRMWARE_STORE.lock();
    if let Some(slot) = guard.iter_mut().find(|s| s.is_none()) {
        *slot = Some((name, data));
    }
}

/// 按文件名取已注册的 WiFi 固件
pub fn get_wifi_firmware(name: &str) -> Option<&'static [u8]> {
    let guard = WIFI_FIRMWARE_STORE.lock();
    guard
        .iter()
        .find_map(|s| s.as_ref().filter(|(n, _)| *n == name).map(|(_, d)| *d))
}

/// 按名称取固件：先查本地（firmware_data，如 embed 或平台预置），再查注册表。对应 LicheeRV 按名从路径读
pub fn get_firmware_by_name(name: &str) -> Option<&'static [u8]> {
    crate::firmware_data::get_firmware_by_name(name).or_else(|| get_wifi_firmware(name))
}

/// 构建 DBG_MEM_READ_REQ 消息（param: memaddr 4 字节）
pub fn build_dbg_mem_read_req(mem_addr: u32) -> LmacMsg {
    let mut msg = LmacMsg::new(DBG_MEM_READ_REQ, TASK_DBG, DRV_TASK_ID, 4);
    msg.param[0..4].copy_from_slice(&mem_addr.to_le_bytes());
    msg
}

/// 从 DBG_MEM_READ_CFM 的 param 解析 memdata（param 前 8 字节：memaddr, memdata）
pub fn parse_dbg_mem_read_cfm(param: &[u8]) -> Option<u32> {
    if param.len() < 8 {
        return None;
    }
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&param[4..8]);
    Some(u32::from_le_bytes(buf))
}

/// 构建 DBG_MEM_WRITE_REQ 消息（param: memaddr 4 + memdata 4）
pub fn build_dbg_mem_write_req(mem_addr: u32, mem_data: u32) -> LmacMsg {
    let mut msg = LmacMsg::new(DBG_MEM_WRITE_REQ, TASK_DBG, DRV_TASK_ID, 8);
    msg.param[0..4].copy_from_slice(&mem_addr.to_le_bytes());
    msg.param[4..8].copy_from_slice(&mem_data.to_le_bytes());
    msg
}

/// 构建 DBG_MEM_MASK_WRITE_REQ 消息（param: memaddr, memmask, memdata 各 4 字节）
pub fn build_dbg_mem_mask_write_req(mem_addr: u32, mem_mask: u32, mem_data: u32) -> LmacMsg {
    let mut msg = LmacMsg::new(DBG_MEM_MASK_WRITE_REQ, TASK_DBG, DRV_TASK_ID, 12);
    msg.param[0..4].copy_from_slice(&mem_addr.to_le_bytes());
    msg.param[4..8].copy_from_slice(&mem_mask.to_le_bytes());
    msg.param[8..12].copy_from_slice(&mem_data.to_le_bytes());
    msg
}

/// 发送 DBG_MEM_READ_REQ 并等待 CFM，返回 memdata
/// 若提供 `after_delay`，在 100ms 延时后、开始轮询前调用一次（可用来读 F1 状态寄存器做调试）
pub fn send_dbg_mem_read<F, E>(
    cmd_mgr: &mut RwnxCmdMgr,
    mut tx_fn: F,
    mem_addr: u32,
    timeout_ms: u32,
    poll_fn: &mut dyn FnMut(&mut RwnxCmdMgr),
    after_delay: Option<&mut dyn FnMut()>,
) -> Result<u32, i32>
where
    F: FnMut(&LmacMsg) -> Result<(), E>,
{
    let msg = build_dbg_mem_read_req(mem_addr);
    let token = cmd_mgr.push(DBG_MEM_READ_CFM).ok_or(-12)?;
    tx_fn(&msg).map_err(|_| -5)?;
    log::info!(target: "wireless::bsp", "dbg_mem_read: request sent, waiting CFM (timeout {}ms)", timeout_ms);
    // 给卡足够时间处理请求并把 CFM 放入 rd_fifo（首包/ROM 可能较慢，100ms 后再轮询）
    crate::delay_spin_ms(100);
    if let Some(f) = after_delay {
        f();
    }
    cmd_mgr.wait_done(token, timeout_ms, poll_fn, None)?;
    let mut cfm_buf = [0u8; 16];
    let len = cmd_mgr.take_cfm(token, &mut cfm_buf).ok_or(-5)?;
    parse_dbg_mem_read_cfm(&cfm_buf[..len]).ok_or(-5)
}

/// 发送 DBG_MEM_WRITE_REQ（不等待 CFM 内容，仅等待完成）
/// tick: 每 500ms 调用一次 tick(waited_ms)，可用于打 F1 BLOCK_CNT/FLOW_CTRL 等调试日志。
pub fn send_dbg_mem_write<F, E>(
    cmd_mgr: &mut RwnxCmdMgr,
    mut tx_fn: F,
    mem_addr: u32,
    mem_data: u32,
    timeout_ms: u32,
    poll_fn: &mut dyn FnMut(&mut RwnxCmdMgr),
    tick: Option<&mut dyn FnMut(u32)>,
) -> Result<(), i32>
where
    F: FnMut(&LmacMsg) -> Result<(), E>,
{
    let msg = build_dbg_mem_write_req(mem_addr, mem_data);
    log::warn!(
        target: "wireless::bsp",
        "send_dbg_mem_write: id={} dest={} src={} param_len={} addr=0x{:08x} data=0x{:08x}",
        msg.header.id, msg.header.dest_id, msg.header.src_id, msg.header.param_len,
        mem_addr, mem_data
    );
    let token = cmd_mgr.push(DBG_MEM_WRITE_CFM).ok_or(-12)?;
    tx_fn(&msg).map_err(|_| -5)?;
    cmd_mgr.wait_done(token, timeout_ms, poll_fn, tick)
}

/// 发送 DBG_MEM_MASK_WRITE_REQ
pub fn send_dbg_mem_mask_write<F, E>(
    cmd_mgr: &mut RwnxCmdMgr,
    mut tx_fn: F,
    mem_addr: u32,
    mem_mask: u32,
    mem_data: u32,
    timeout_ms: u32,
    poll_fn: &mut dyn FnMut(&mut RwnxCmdMgr),
) -> Result<(), i32>
where
    F: FnMut(&LmacMsg) -> Result<(), E>,
{
    let msg = build_dbg_mem_mask_write_req(mem_addr, mem_mask, mem_data);
    let token = cmd_mgr.push(DBG_MEM_MASK_WRITE_CFM).ok_or(-12)?;
    tx_fn(&msg).map_err(|_| -5)?;
    cmd_mgr.wait_done(token, timeout_ms, poll_fn, None)
}

/// 构建 DBG_MEM_BLOCK_WRITE_REQ 消息
/// param: memaddr(4) + memsize(4) + memdata(memsize bytes)
pub fn build_dbg_mem_block_write_req(
    mem_addr: u32,
    mem_size: u32,
    mem_data: &[u8],
) -> Option<LmacMsg> {
    let param_len = 8 + mem_data.len();
    if param_len > LMAC_MSG_MAX_LEN {
        return None;
    }
    let mut msg = LmacMsg::new(DBG_MEM_BLOCK_WRITE_REQ, TASK_DBG, DRV_TASK_ID, param_len as u16);
    msg.param[0..4].copy_from_slice(&mem_addr.to_le_bytes());
    msg.param[4..8].copy_from_slice(&mem_size.to_le_bytes());
    msg.param[8..8 + mem_data.len()].copy_from_slice(mem_data);
    Some(msg)
}

/// 构建 DBG_START_APP_REQ 消息
/// param: bootaddr(4) + boottype(4)
pub fn build_dbg_start_app_req(boot_addr: u32, boot_type: u32) -> LmacMsg {
    let mut msg = LmacMsg::new(DBG_START_APP_REQ, TASK_DBG, DRV_TASK_ID, 8);
    msg.param[0..4].copy_from_slice(&boot_addr.to_le_bytes());
    msg.param[4..8].copy_from_slice(&boot_type.to_le_bytes());
    msg
}

/// 固件块上传：将 data 以 1KB 为单位写入设备 mem_addr，最后一块可不足 1KB
/// tx_fn: 将序列化后的 LmacMsg 发送到总线；内部会通过 CmdMgr 等待 CFM
pub fn fw_upload_blocks<F, E>(
    cmd_mgr: &mut RwnxCmdMgr,
    mut tx_fn: F,
    mem_addr: u32,
    data: &[u8],
    timeout_ms: u32,
    poll_fn: &mut dyn FnMut(&mut RwnxCmdMgr),
) -> Result<(), i32>
where
    F: FnMut(&LmacMsg) -> Result<(), E>,
{
    const BLOCK: usize = 1024;
    let total_blocks = (data.len() + BLOCK - 1) / BLOCK;
    log::info!(target: "wireless::bsp", "fw_upload_blocks: start addr=0x{:08x} len={} ({} blocks)", mem_addr, data.len(), total_blocks);
    let mut addr = mem_addr;
    let mut off = 0;
    let mut block_index: usize = 0;
    while off < data.len() {
        let len = (data.len() - off).min(BLOCK);
        let block = &data[off..off + len];
        let msg = build_dbg_mem_block_write_req(addr, len as u32, block).ok_or(-12)?; // -ENOMEM
        let token = cmd_mgr.push(DBG_MEM_BLOCK_WRITE_CFM).ok_or(-12)?;
        tx_fn(&msg).map_err(|_| -5)?; // -EIO
        cmd_mgr.wait_done(token, timeout_ms, &mut *poll_fn, None)?;
        addr += len as u32;
        off += len;
        block_index += 1;
        // 进度：每 32 块或 25%/50%/75%/100% 打一次，便于确认“第一个固件是否加载完”
        let pct = (off * 100) / data.len();
        if block_index % 32 == 0 || pct == 25 || pct == 50 || pct == 75 || off == data.len() {
            log::info!(target: "wireless::bsp", "fw_upload_blocks: progress block {}/{} ({}% done)", block_index, total_blocks, pct);
        }
    }
    log::info!(target: "wireless::bsp", "fw_upload_blocks: done, {} blocks written", block_index);
    Ok(())
}

/// 启动固件：发送 DBG_START_APP_REQ，等待 DBG_START_APP_CFM
pub fn fw_start_app<F, E>(
    cmd_mgr: &mut RwnxCmdMgr,
    mut tx_fn: F,
    boot_addr: u32,
    boot_type: u32,
    timeout_ms: u32,
    poll_fn: &mut dyn FnMut(&mut RwnxCmdMgr),
) -> Result<(), i32>
where
    F: FnMut(&LmacMsg) -> Result<(), E>,
{
    log::info!(target: "wireless::bsp", "fw_start_app addr=0x{:08x} type={}", boot_addr, boot_type);
    let msg = build_dbg_start_app_req(boot_addr, boot_type);
    let token = cmd_mgr.push(DBG_START_APP_CFM).ok_or(-12)?;
    tx_fn(&msg).map_err(|_| -5)?;
    cmd_mgr.wait_done(token, timeout_ms, poll_fn, None)
}
