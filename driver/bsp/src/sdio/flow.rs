//! SDIO 流程六函数（对应 LicheeRV aicsdio.c / aic_bsp_driver.c）
//!
//! 本模块提供 SDIO 流程的六个入口函数及说明，与 LicheeRV 中 aicbsp_set_subsys 内
//! “平台上电 → sdio_init → probe 等待 → driver_fw_init → sdio_release”及 sdio_exit 对应。
//! 多线程：与 LicheeRV 100% 对齐 — bustx_thread（wait(bustx_trgg) + tx_process）+ busrx_thread（wait(busrx_trgg) + process_rxframes）。

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use axerrno::{AxError, AxResult};
use spin::{Mutex, Once};

use crate::cmd::{LmacMsg, RwnxCmdMgr};
use crate::export::AicBspInfo;
use crate::fw_load::{
    build_dbg_mem_read_req, build_dbg_mem_write_req, send_dbg_mem_read, send_dbg_mem_write, send_dbg_mem_mask_write,
    fw_upload_blocks, fw_start_app, get_firmware_by_name,
    parse_dbg_mem_read_cfm_with_addr,
    DBG_MEM_BLOCK_WRITE_REQ, DBG_MEM_READ_CFM, DBG_MEM_WRITE_CFM, DBG_MEM_WRITE_REQ, DBG_MEM_BLOCK_WRITE_CFM, CHIP_REV_MEM_ADDR, RAM_FMAC_FW_ADDR, RAM_FMAC_FW_PATCH_ADDR, HOST_START_APP_AUTO,
};
use crate::firmware::get_firmware_list;
use crate::gpio::WifiGpioControl;
use crate::sync;

use skb::SkBuff;

use super::backend::Aic8800SdioHost;
use super::ops::{CisReadOps, SdioOps};
use super::ops::Aic8800Sdio;
use super::types::ProductId;

/// 未 probe 时使用的产品 ID 占位值（用于静态存储）
const PRODUCT_ID_NONE: u32 = 0xFFFF;

/// 当前已 probe 的 SDIO 产品 ID（由 aicbsp_sdio_probe 写入，aicbsp_driver_fw_init 等读取）
pub(super) static CURRENT_PRODUCT_ID: AtomicU32 = AtomicU32::new(PRODUCT_ID_NONE);

/// 已 probe 的 SDIO 设备（用于 aicbsp_driver_fw_init 发送 IPC、固件上传等）
static SDIO_DEVICE: Mutex<Option<Aic8800Sdio>> = Mutex::new(None);
/// 命令管理器（与 SDIO 设备配对，用于 DBG_* 请求-确认）
static CMD_MGR: Mutex<Option<RwnxCmdMgr>> = Mutex::new(None);

/// 是否已启动 busrx 线程（对齐 LicheeRV busrx_thread）
static BUSRX_RUNNING: AtomicBool = AtomicBool::new(false);
/// 是否已启动 bustx 线程（对齐 LicheeRV aicwf_sdio_bustx_thread）
static BUSTX_RUNNING: AtomicBool = AtomicBool::new(false);

/// 待发送的 CMD 消息（LicheeRV tx_priv->cmd_buf/cmd_len/cmd_txstate），bustx 线程取走后执行 send_msg
/// 主线程只写 payload_len 字节（如 24），bustx 内照抄 aicwf_sdio_tx_msg 做 align+TAIL+512
/// 与 LicheeRV CMD_BUF_MAX 对齐：须容纳 DBG_MEM_BLOCK_WRITE_REQ 整包（16+1032=1048，向上取整 1536）
const PENDING_CMD_TX_CAP: usize = 1536;
static PENDING_CMD_TX: Mutex<Option<([u8; PENDING_CMD_TX_CAP], usize)>> = Mutex::new(None);
/// bustx 完成 send_msg 后的结果（LicheeRV cmd_tx_succ），调用方 wait_tx_done 后取
static TX_RESULT: Mutex<Option<i32>> = Mutex::new(None);

/// 与 LicheeRV aicwf_sdio_tx_msg(aicsdio.c 953-978) 完全一致：在同一 buffer 上做 4 字节对齐、TAIL(4)、向上取整到 512，返回应发送长度。
/// payload 为 cmd_buf，payload_len 为 cmd_len；写 pad 到 buf 中并返回 len（512 或 payload_len）。
#[inline]
fn aicwf_sdio_tx_msg_pad(buf: &mut [u8], payload_len: usize) -> usize {
    const TX_ALIGNMENT: usize = 4;
    const SDIOWIFI_FUNC_BLOCKSIZE: usize = 512;
    const TAIL_LEN: usize = 4;
    let mut len = payload_len;
    // if ((len % TX_ALIGNMENT) != 0) { adjust_len = roundup(len, TX_ALIGNMENT); memcpy(payload+payload_len, adjust_str, ...); payload_len += ... }
    if len % TX_ALIGNMENT != 0 {
        let adjust_len = (len + TX_ALIGNMENT - 1) / TX_ALIGNMENT * TX_ALIGNMENT;
        let need = adjust_len - len;
        if buf.len() >= payload_len + need {
            buf[payload_len..payload_len + need].fill(0);
        }
        len = adjust_len;
    }
    // link tail is necessary
    let send_len = if len % SDIOWIFI_FUNC_BLOCKSIZE != 0 {
        if buf.len() >= len + TAIL_LEN {
            buf[len..len + TAIL_LEN].fill(0);
        }
        let payload_len_after_tail = len + TAIL_LEN;
        (payload_len_after_tail / SDIOWIFI_FUNC_BLOCKSIZE + 1) * SDIOWIFI_FUNC_BLOCKSIZE
    } else {
        len
    };
    if send_len > len && buf.len() >= send_len {
        buf[len..send_len].fill(0);
    }
    send_len
}

/// IPC 接收缓冲区大小。LicheeRV 按 block_cnt*512 读满整块以排空 RD_FIFO，否则芯片可能不响应下一包 IPC；故至少 512。
const IPC_RX_BUF_SIZE: usize = 512;

/// LicheeRV 8801 IPC 发送长度：与 aicwf_sdio_tx_msg 完全一致（aicsdio.c 964-978）
/// 1) 先 4 字节对齐（TX_ALIGNMENT=4）；2) 未满 512 时加 TAIL_LEN(4) 再向上取整到 512。
#[inline]
fn ipc_send_len_8801(serialized_len: usize) -> usize {
    const TX_ALIGNMENT: usize = 4;
    const BLOCK: usize = 512;
    const TAIL_LEN: usize = 4;
    let len4 = (serialized_len + TX_ALIGNMENT - 1) / TX_ALIGNMENT * TX_ALIGNMENT;
    if len4 % BLOCK == 0 {
        len4
    } else {
        ((len4 + TAIL_LEN + BLOCK - 1) / BLOCK) * BLOCK
    }
}

/// 从 SDIO 收一包并解析为 E2A 消息，若为 CFM 则调用 cmd_mgr.on_cfm（对应 LicheeRV RX 路径）
/// LicheeRV 接收时用 skb->data+4 作为 ipc_e2a_msg，即前 4 字节为前缀，E2A 头从 offset 4 开始
/// 用于 log 的十六进制前缀（最多 32 字节），避免分配
struct HexPrefix<'a>(&'a [u8], usize);
impl core::fmt::Display for HexPrefix<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let n = self.1.min(32);
        for i in 0..n {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{:02x}", self.0.get(i).copied().unwrap_or(0))?;
        }
        if self.1 > 32 {
            write!(f, " ...")?;
        }
        Ok(())
    }
}

/// E2A 指示回调类型：msg_id + param 指针与长度（由 FDRV 注册，用于 SCANU_RESULT_IND / SM_CONNECT_IND / SM_DISCONNECT_IND 等）
/// 在 busrx 线程内调用，param 在调用期间有效。
pub type E2aIndicationCb = Option<unsafe fn(msg_id: u16, param: *const u8, param_len: usize)>;
static E2A_INDICATION_CB: spin::Mutex<E2aIndicationCb> = spin::Mutex::new(None);

/// 注册 E2A 指示回调（FDRV 在初始化时调用，用于接收 scan_done/connect_result/disconnect 等）
pub fn set_e2a_indication_cb(cb: E2aIndicationCb) {
    *E2A_INDICATION_CB.lock() = cb;
}

/// 与 LicheeRV aicwf_txrxif.h 一致：数据帧 RX 时硬件头长度
const RX_HWHRD_LEN_DATA: usize = 60;
const RX_ALIGNMENT: usize = 4;

/// 数据帧 RX 指示回调：当 (buf[2] & SDIO_TYPE_CFG) != SDIO_TYPE_CFG 时调用，与 aicwf_process_rxframes → rwnx_rxdataind_aicwf 对齐
/// 由上层在初始化时注册为 fdrv::fdrv_rx_data_invoke
pub type RxDataIndicationCb = Option<unsafe fn(ptr: *const u8, len: usize)>;
static RX_DATA_INDICATION_CB: spin::Mutex<RxDataIndicationCb> = spin::Mutex::new(None);

/// 注册数据帧 RX 回调（上层在初始化时调用，传入 fdrv::fdrv_rx_data_invoke）
pub fn set_rx_data_indication_cb(cb: RxDataIndicationCb) {
    *RX_DATA_INDICATION_CB.lock() = cb;
}

/// 与 LicheeRV aicsdio.h 一致：仅 SDIO_TYPE_CFG_CMD_RSP(0x11) 时调用 rwnx_rx_handle_msg → msgind → on_cfm
const SDIO_TYPE_CFG_CMD_RSP: u8 = 0x11;

/// 从 buf[offset..n] 解析一帧并 on_cfm；返回本帧长度（含头），无法解析返回 0
///
/// 与 LicheeRV 一致：4B 前缀后为 `ipc_e2a_msg`，含 id/dest/src/param_len(8B) + **pattern(4B)** + param。
/// param 从 offset+16 开始。仅当 type&0x7f == SDIO_TYPE_CFG_CMD_RSP(0x11) 时调用 on_cfm（aicsdio_txrxif.c 213 行）。
fn parse_one_cfm_at(buf: &[u8], n: usize, offset: usize, cmd_mgr: &mut RwnxCmdMgr) -> usize {
    if offset + 16 > n {
        return 0;
    }
    let type_bits = buf[offset + 2] & 0x7f;
    if type_bits == SDIO_TYPE_CFG_CMD_RSP {
        let param_len = u16::from_le_bytes([buf[offset + 10], buf[offset + 11]]) as usize;
        if param_len <= 256 {
            let total = 16 + param_len; // 8B e2a header + 4B pattern + param（LicheeRV ipc_e2a_msg）
            if offset + total <= n {
                let msg_id = u16::from_le_bytes([buf[offset + 4], buf[offset + 5]]);
                let param_start = offset + 16;
                log::info!(target: "wireless::bsp::sdio", "poll_rx_one: CFM (4B prefix) msg_id=0x{:04x} param_len={}", msg_id, param_len);
                if msg_id == DBG_MEM_BLOCK_WRITE_CFM {
                    log::info!(target: "wireless::bsp::sdio", "poll_rx_one: CFM 已收到 DBG_MEM_BLOCK_WRITE_CFM (0x040b)");
                }
                cmd_mgr.on_cfm(msg_id, &buf[param_start..offset + total]);
                if let Some(cb) = *E2A_INDICATION_CB.lock() {
                    unsafe { cb(msg_id, buf[param_start..].as_ptr(), param_len) };
                }
                return total;
            }
        }
    }
    // 与 LicheeRV 一致：仅接受合法的 IPC CFM；param_len=0 的 8 字节多为缓冲区尾随垃圾，若接受会误触发 on_cfm(0x8800/0x0000 等) 并刷屏
    if offset + 8 <= n {
        let param_len = u16::from_le_bytes([buf[offset + 6], buf[offset + 7]]) as usize;
        if param_len > 0 && param_len <= 256 {
            let total = 8 + param_len;
            if offset + total <= n {
                let msg_id = u16::from_le_bytes([buf[offset], buf[offset + 1]]);
                let param_start = offset + 8;
                log::info!(target: "wireless::bsp::sdio", "poll_rx_one: CFM (no prefix) msg_id=0x{:04x} param_len={}", msg_id, param_len);
                if msg_id == DBG_MEM_BLOCK_WRITE_CFM {
                    log::info!(target: "wireless::bsp::sdio", "poll_rx_one: CFM 已收到 DBG_MEM_BLOCK_WRITE_CFM (0x040b)");
                }
                cmd_mgr.on_cfm(msg_id, &buf[param_start..offset + total]);
                if let Some(cb) = *E2A_INDICATION_CB.lock() {
                    unsafe { cb(msg_id, buf[param_start..].as_ptr(), param_len) };
                }
                return total;
            }
        }
    }
    0
}

/// 与 LicheeRV aicwf_process_rxframes 一致：一次 recv_pkt 可能读到多包，须循环解析直至无完整帧。
/// 数据帧：(buf[2] & SDIO_TYPE_CFG) != SDIO_TYPE_CFG 时调用 set_rx_data_indication_cb 注册的回调；
/// 配置帧：0x10..0x13 走 parse_one_cfm_at（on_cfm + E2A 指示回调）。
fn poll_rx_one(sdio: &dyn SdioOps, cmd_mgr: &mut RwnxCmdMgr) {
    const SDIO_TYPE_CFG: u8 = 0x10;
    let mut skb = SkBuff::alloc(IPC_RX_BUF_SIZE);
    let n = match sdio.recv_pkt(skb.data_mut(), IPC_RX_BUF_SIZE as u32, 1) {
        Ok(s) => s,
        Err(e) => {
            log::warn!(target: "wireless::bsp::sdio", "poll_rx_one: recv_pkt err={} (e.g. BUF_RRDY timeout)", e);
            return;
        }
    };
    if n == 0 {
        return;
    }
    skb.set_len(n);
    let buf = skb.data();
    let mut offset = 0;
    while offset < n {
        if offset + 3 <= n {
            let pkt_len = u16::from_le_bytes([buf[offset], buf[offset + 1]]) as usize;
            let type_byte = buf[offset + 2];
            if (type_byte & SDIO_TYPE_CFG) != SDIO_TYPE_CFG {
                let aggr_len = pkt_len + RX_HWHRD_LEN_DATA;
                let adjust_len = (aggr_len + RX_ALIGNMENT - 1) & !(RX_ALIGNMENT - 1);
                let total = 3 + adjust_len;
                if offset + total <= n {
                    if let Some(cb) = *RX_DATA_INDICATION_CB.lock() {
                        unsafe { cb(buf[offset..].as_ptr(), 3 + aggr_len) };
                    }
                    offset += total;
                    continue;
                }
            }
        }
        let consumed = parse_one_cfm_at(buf, n, offset, cmd_mgr);
        if consumed == 0 {
            if offset == 0 {
                log::warn!(
                    target: "wireless::bsp::sdio",
                    "poll_rx_one: n={} raw={} (no valid data or 4B-prefix or 8B header)",
                    n,
                    HexPrefix(buf, n.min(32))
                );
            }
            break;
        }
        offset += consumed;
    }
}

/// 在持有 SDIO_DEVICE 与 CMD_MGR 锁时执行 f，供 RX 线程与多线程 IPC 路径使用（短暂持锁）
pub fn with_cmd_mgr<R, F>(f: F) -> Option<R>
where
    F: FnOnce(&mut RwnxCmdMgr) -> R,
{
    CMD_MGR.lock().as_mut().map(f)
}

/// 在持有 SDIO_DEVICE 锁时执行 f（短暂持锁）
pub fn with_sdio<R, F>(f: F) -> Option<R>
where
    F: FnOnce(&Aic8800Sdio) -> R,
{
    SDIO_DEVICE.lock().as_ref().map(f)
}

/// 获取 SDIO_DEVICE 锁，供 mmc crate 的 MmcHost 实现使用（claim_host 返回此 guard）
pub(super) fn lock_sdio_device() -> spin::MutexGuard<'static, Option<Aic8800Sdio>> {
    SDIO_DEVICE.lock()
}

/// 读 F1 寄存器并打日志：8801 在 no byte mode(0x11=1) 下收 CFM 看 BLOCK_CNT(0x12)；BYTEMODE_LEN(0x02)/FLOW_CTRL(0x0A) 一并打出便于对照 LicheeRV
pub fn log_f1_block_cnt_flow_ctrl(waited_ms: u32) {
    with_sdio(|sdio| {
        let bc = sdio.read_byte(0x112).unwrap_or(0xff); // F1 0x12 BLOCK_CNT（8801 主用，非 0 表示有数据）
        let bm = sdio.read_byte(0x102).unwrap_or(0xff); // F1 0x02 BYTEMODE_LEN（block_cnt>=64 时用）
        let fc = sdio.read_byte(0x10A).unwrap_or(0xff); // F1 0x0A FLOW_CTRL
        log::warn!(
            target: "wireless::bsp::sdio",
            "F1 @ {}ms: BLOCK_CNT(0x12)=0x{:02x} BYTEMODE_LEN(0x02)=0x{:02x} FLOW_CTRL(0x0A)=0x{:02x}",
            waited_ms, bc, bm, fc
        );
    });
}

/// 从 SDIO 收一包并解析 E2A（on_cfm + E2A 指示回调）。供 FDRV 在 wait_done_until 的 poll_fn 中调用，以在等待 CFM 时收包。
pub fn sdio_poll_rx_once() {
    run_poll_rx_one();
}

/// RX 线程循环体：从 SDIO 收一包并解析、on_cfm（对齐 LicheeRV aicwf_process_rxframes）；锁顺序 CMD_MGR → SDIO_DEVICE 避免死锁
fn run_poll_rx_one() {
    let mut cmd_guard = CMD_MGR.lock();
    let cmd_mgr = match cmd_guard.as_mut() {
        Some(c) => c,
        None => return,
    };
    let sdio_guard = SDIO_DEVICE.lock();
    let sdio = match sdio_guard.as_ref() {
        Some(s) => s,
        None => return,
    };
    poll_rx_one(sdio as &dyn SdioOps, cmd_mgr);
}

/// busrx 线程：wait(busrx_trgg 等价) + run_poll_rx_one，与 LicheeRV aicwf_sdio_busrx_thread 对齐
fn busrx_thread_fn() {
    const RX_POLL_MS: u64 = 1;
    while BUSRX_RUNNING.load(Ordering::Relaxed) {
        crate::sdio_irq::wait_sdio_or_timeout(core::time::Duration::from_millis(RX_POLL_MS));
        run_poll_rx_one();
        // 无 PLIC/软中断时 wait_sdio_or_timeout 直接返回，busrx 会占满 CPU、main 无法调度到 send_msg，故每轮主动 yield
        if !crate::sdio_irq::use_sdio_irq() {
            axtask::sleep(core::time::Duration::from_millis(RX_POLL_MS));
        }
    }
    log::debug!(target: "wireless::bsp::sdio", "busrx_thread exit");
}

/// 确保已启动 busrx 线程（仅启动一次，对齐 LicheeRV aicwf_bus_init 里 kthread_run(sdio_busrx_thread)）
pub fn ensure_busrx_thread_started() {
    static STARTED: Once<()> = Once::new();
    STARTED.call_once(|| {
        BUSRX_RUNNING.store(true, Ordering::Relaxed);
        let _ = axtask::spawn(busrx_thread_fn);
        log::info!(target: "wireless::bsp::sdio", "busrx_thread started (align LicheeRV aicwf_sdio_busrx_thread)");
        // 让出 CPU，确保 busrx 至少被调度一次后再发首包，避免主线程持 SDIO 锁时 busrx 从未运行
        axtask::sleep(core::time::Duration::from_millis(1));
    });
}

/// bustx 线程：wait(bustx_trgg) + tx_process，与 LicheeRV aicwf_sdio_bustx_thread 一致。
/// LicheeRV 使用 wait_for_completion_interruptible(&bustx_trgg)，即无限等待；notify 时线程必须在队列上才能被唤醒。
/// 若用短超时(1ms)+超时后 sleep(1ms)，则超时期间线程不在 BUSTX_WAIT_QUEUE，主线程 notify_bustx() 会丢失，导致 submit_cmd_tx 一直等不到 tx_done。
/// 故此处用较长 wait 超时（如 60s），使线程绝大部分时间在队列上，主线程 notify 能可靠唤醒。
fn bustx_thread_fn() {
    const BUSTX_WAIT_MS: u64 = 60_000;
    while BUSTX_RUNNING.load(Ordering::Relaxed) {
        let woken = !crate::sdio_irq::wait_bustx_or_timeout(core::time::Duration::from_millis(BUSTX_WAIT_MS));
        if woken {
            let slot = PENDING_CMD_TX.lock().take();
            if let Some((mut buf, payload_len)) = slot {
                // 与 LicheeRV aicwf_sdio_tx_msg 一致：bustx 内在同一 buffer 上做 align+TAIL+512，再 flow_ctrl+send_pkt
                let send_len = aicwf_sdio_tx_msg_pad(&mut buf, payload_len);
                let result = match with_sdio(|sdio| sdio.send_msg(&buf[..send_len], send_len)) {
                    Some(Ok(_)) => 0,
                    Some(Err(e)) => e,
                    None => {
                        log::warn!(target: "wireless::bsp::sdio", "bustx: with_sdio returned None (SDIO_DEVICE not ready?), result=-5");
                        -5
                    }
                };
                *TX_RESULT.lock() = Some(result);
                crate::sdio_irq::notify_tx_done();
            }
        }
    }
    log::debug!(target: "wireless::bsp::sdio", "bustx_thread exit");
}

/// 确保已启动 bustx 线程（仅启动一次，对齐 LicheeRV aicwf_bus_init 里 kthread_run(aicwf_sdio_bustx_thread)）
pub fn ensure_bustx_thread_started() {
    static STARTED: Once<()> = Once::new();
    STARTED.call_once(|| {
        BUSTX_RUNNING.store(true, Ordering::Relaxed);
        let _ = axtask::spawn(bustx_thread_fn);
        log::info!(target: "wireless::bsp::sdio", "bustx_thread started (align LicheeRV aicwf_sdio_bustx_thread)");
        axtask::sleep(core::time::Duration::from_millis(1));
    });
}

/// 与 LicheeRV aicwf_sdio_bus_txmsg 对齐：提交 CMD 到 bustx 线程，等待 CMD53 写完成后返回（再等 CFM 由调用方 wait_done_until）。
/// LicheeRV aicsdio_txrxif.h / aicwf_txrxif.h：CMD_TX_TIMEOUT 5000（ms）
const TX_DONE_TIMEOUT_MS: u64 = 5000;

/// 与 LicheeRV aicwf_sdio_bus_txmsg 对齐：提交 CMD 到 bustx，等待 CMD53 写完成后返回。
/// 循环等待以避免 WaitQueue 虚假唤醒导致 TX_RESULT 仍为 None 而误报 -5；仅在实际超时轮次累加 elapsed。
/// FDRV 用于发送 LMAC 命令（scan/connect/disconnect 等）后等待 TX 完成，再配合 with_cmd_mgr + wait_done_until 等 CFM。
pub fn submit_cmd_tx_and_wait_tx_done(buf: &[u8], len: usize) -> Result<(), i32> {
    if len > PENDING_CMD_TX_CAP {
        return Err(-22);
    }
    *TX_RESULT.lock() = None;
    // 与 LicheeRV 一致：rwnx_set_cmd_tx 内 memset(buffer,0,CMD_BUF_MAX)，再填 [0..len]；此处整块零初始化后拷贝前 len 字节
    let mut arr = [0u8; PENDING_CMD_TX_CAP];
    arr[..len].copy_from_slice(buf);
    *PENDING_CMD_TX.lock() = Some((arr, len));
    crate::sdio_irq::notify_bustx();
    let total_ms = TX_DONE_TIMEOUT_MS;
    let chunk_ms: u64 = 50;
    let dur = core::time::Duration::from_millis(chunk_ms);
    let mut elapsed_ms: u64 = 0;
    loop {
        let timed_out = crate::sdio_irq::wait_tx_done_timeout(dur);
        if let Some(result) = TX_RESULT.lock().take() {
            return if result == 0 { Ok(()) } else { Err(result) };
        }
        if timed_out {
            elapsed_ms += chunk_ms;
            if elapsed_ms >= total_ms {
                log::warn!(target: "wireless::bsp::sdio", "submit_cmd_tx_and_wait_tx_done: timeout {}ms (bustx may be blocked or not running)", total_ms);
                return Err(-110);
            }
        }
    }
}

/// 多线程模式下发送 DBG_MEM_READ_REQ 并等待 CFM（与 LicheeRV 一致：经 bustx 线程写 WR_FIFO，busrx 收 CFM）
pub fn send_dbg_mem_read_busrx(mem_addr: u32, timeout_ms: u32, after_delay: Option<&mut dyn FnMut()>) -> Result<u32, i32> {
    use crate::fw_load::{build_dbg_mem_read_req, parse_dbg_mem_read_cfm_full, parse_dbg_mem_read_cfm_with_addr, DBG_MEM_READ_CFM};
    let token = with_cmd_mgr(|c| c.push(DBG_MEM_READ_CFM)).flatten().ok_or(-12)?;
    let product_id = aicbsp_current_product_id().ok_or(-22)?;
    log::warn!(target: "wireless::bsp", "dbg_mem_read_busrx: sending request (mem_addr=0x{:08x})", mem_addr);
    let msg = build_dbg_mem_read_req(mem_addr);
    let mut buf = [0u8; 512];
    let len = if product_id == ProductId::Aic8801 {
        msg.serialize_8801(&mut buf)
    } else {
        msg.serialize(&mut buf)
    };
    // 与 LicheeRV 一致：主线程只传 payload_len，bustx 内 aicwf_sdio_tx_msg_pad 做 align+TAIL+512
    submit_cmd_tx_and_wait_tx_done(&buf[..len], len).map_err(|e| {
        log::warn!(target: "wireless::bsp", "dbg_mem_read_busrx: submit_cmd_tx (bustx) failed, err={}", e);
        e
    })?;
    log::warn!(target: "wireless::bsp", "dbg_mem_read_busrx: request sent, waiting CFM (timeout {}ms)", timeout_ms);
    axtask::sleep(core::time::Duration::from_millis(100));
    if let Some(f) = after_delay {
        f();
    }
    // 使用完整 timeout_ms（如 2000ms），与 aicbsp_system_config 等一致，便于：1) 每 500ms 打 F1 BLOCK_CNT/FLOW_CTRL；2) 给设备足够时间回 CFM
    // 不传 tick：周期性读 F1(0x02/0x12/0x0A) 时 CMD52 可能在某些板上挂死并持锁，导致整机卡住；仅保留“still waiting”日志
    let ok = RwnxCmdMgr::wait_done_until(
        timeout_ms,
        || with_cmd_mgr(|c| c.is_done(token)).unwrap_or(false),
        None,
        None,
        None,
    );
    ok.map_err(|_| -62)?;
    let mut cfm_buf = [0u8; 16];
    let len = with_cmd_mgr(|c| c.take_cfm(token, &mut cfm_buf)).flatten().ok_or(-5)?;
    let memdata = parse_dbg_mem_read_cfm_with_addr(&cfm_buf[..len], mem_addr).ok_or(-5)?;
    if let Some((a, d)) = parse_dbg_mem_read_cfm_full(&cfm_buf[..len]) {
        let order = if d == mem_addr { "swap[memdata][memaddr]" } else { "[memaddr][memdata]" };
        log::info!(target: "wireless::bsp::sdio", "dbg_mem_read CFM: raw param [0]=0x{:08x} [1]=0x{:08x} requested=0x{:08x} {} -> memdata=0x{:08x} (chip_rev_byte=0x{:02x})",
            a, d, mem_addr, order, memdata, (memdata >> 16) as u8);
    }
    Ok(memdata)
}

/// 上电/复位后到首次 SDIO 访问前的稳定延时(ms)。
/// LicheeRV 实际：U-Boot 上电后首次 CMD5 在 Linux 启动后（数秒）；Amlogic 平台 200ms 高后 reinit。
/// 若 BLOCK_CNT 恒为 0（bootrom 未响应），可改为 500 或 1000 再测（见 docs/LicheeRV_bootrom启动时序_完整对照.md）。
/// 上电后稳定延时(ms)。LicheeRV 上 Linux 启动到首包 IPC 有数秒；此处适当加大以给 bootrom 足够就绪时间。
const POST_POWER_STABLE_MS: u64 = 1000;

/// 内部：上电序列 + 可配置的稳定延时(ms)。供 aicbsp_power_on 与 aicbsp_minimal_ipc_verify 复用。
fn aicbsp_power_on_with_stable_ms(stable_ms: u64) -> AxResult<()> {
    super::backend::set_wifi_power_pinmux_to_gpio();
    let mut gpio_ctrl = WifiGpioControl::new()?;
    gpio_ctrl.init()?;
    gpio_ctrl.power_on_and_reset()?;
    // bootrom 未启动时：确认电源/复位引脚读回为高（OK）；若为 FAIL 则检查 pinmux/极性
    let _ = gpio_ctrl.verify_after_power_on();
    sync::delay_spin_ms(50);
    // 与 U-Boot 一致：拉高后立即设 SDIO pinmux（cvi_board_init：high → wifi sdio pinmux），再进入稳定延时
    super::backend::set_sd1_sdio_pinmux_after_power();
    axtask::sleep(core::time::Duration::from_millis(stable_ms));
    log::info!(target: "wireless::bsp::sdio", "aicbsp_power_on: power+reset done, SDIO pinmux set, waited 50+{}ms before sdio_init", stable_ms);
    Ok(())
}

/// **aicbsp_power_on** — 平台上电（与 LicheeRV aicbsp_platform_power_on + 旧 wifi-driver 对齐）
///
/// **LicheeRV**（aicsdio.c）：Allwinner power(0)→50ms→power(1)→50ms→rescan；Rockchip2 同 50/50；Amlogic 200/200，再 down_timeout(2000) 等卡检测。
/// **旧 wifi-driver**：两引脚，先 power_on（power 低→延时→高→延时），再 reset（reset 低→延时→高→延时），然后 SDIO init。
/// **LicheeRV Nano W**：单引脚 GPIOA_26，U-Boot 为低 50ms→高 50ms。
///
/// **本实现**：单引脚 power_on_and_reset()：低 50ms→高 50ms；再 delay_spin_ms(50)+axtask::sleep(POST_POWER_STABLE_MS)，再 sdio_init。
/// 调用方在返回后应继续执行 aicbsp_sdio_init → aicbsp_driver_fw_init。
///
/// **互斥**：由调用方在进入“上电 → sdio_init → driver_fw_init”序列前持 `sync::power_lock()`。
///
/// **顺序**：必须先设 WiFi 电源引脚 pinmux 为 GPIO，再驱动该引脚；否则引脚可能仍为默认功能，芯片无法上电。
#[inline]
pub fn aicbsp_power_on() -> AxResult<()> {
    aicbsp_power_on_with_stable_ms(POST_POWER_STABLE_MS)
}

/// **最小 IPC 验证**：仅 上电(可配置延时) → SDIO init → 发一次 DBG_MEM_READ → 按间隔打 F1 状态，用于排查 BLOCK_CNT 恒为 0。
///
/// 流程：pinmux+GPIO 上电 → 稳定延时 `stable_ms`（默认 500）→ sdio_init（枚举、F1 0x0B/0x11、busrx）→ 发一次 DBG_MEM_READ(0x40500000)，等待 CFM 期间每 500ms 打 F1 BLOCK_CNT/BYTEMODE_LEN/FLOW_CTRL。
/// 返回 Ok(()) 表示收到 CFM；Err 表示超时或发送失败。
pub fn aicbsp_minimal_ipc_verify(stable_ms: u64) -> AxResult<()> {
    use crate::fw_load::{build_dbg_mem_read_req, parse_dbg_mem_read_cfm_with_addr, DBG_MEM_READ_CFM, CHIP_REV_MEM_ADDR};

    log::info!(target: "wireless::bsp::sdio", "aicbsp_minimal_ipc_verify: start (stable_ms={})", stable_ms);
    sync::probe_reset();
    let _guard = sync::power_lock();

    aicbsp_power_on_with_stable_ms(stable_ms)?;
    aicbsp_sdio_init()?;

    // SDIO 枚举完成后，再给 bootrom 一段时间再发首包 IPC（对齐 LicheeRV 上从枚举到首包的时间差）
    const POST_SDIO_INIT_DELAY_MS: u64 = 500;
    log::info!(target: "wireless::bsp::sdio", "aicbsp_minimal_ipc_verify: post sdio_init delay {}ms before first IPC", POST_SDIO_INIT_DELAY_MS);
    axtask::sleep(core::time::Duration::from_millis(POST_SDIO_INIT_DELAY_MS));

    let product_id = aicbsp_current_product_id().ok_or(AxError::BadState)?;
    if product_id != ProductId::Aic8801 {
        log::warn!(target: "wireless::bsp::sdio", "aicbsp_minimal_ipc_verify: only Aic8801 supported, got {:?}", product_id);
        return Err(AxError::BadState);
    }

    // 与 LicheeRV 一致：首条 SDIO 命令由 bustx 发出（send_msg：读 FLOW_CTRL + 写 WR_FIFO），避免 busrx 先轮询 CMD52 读 BLOCK_CNT 导致超时并占满 inhibit
    ensure_bustx_thread_started();
    let token = with_cmd_mgr(|c| c.push(DBG_MEM_READ_CFM)).flatten().ok_or(AxError::BadState)?;
    let msg = build_dbg_mem_read_req(CHIP_REV_MEM_ADDR);
    let mut buf = [0u8; 512];
    let len = msg.serialize_8801(&mut buf);
    // 与 LicheeRV 一致：主线程只传 payload_len，bustx 内 aicwf_sdio_tx_msg_pad 做 TAIL+512
    log::info!(target: "wireless::bsp::sdio", "minimal_ipc_verify: IPC head 24B {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11],
        buf[12], buf[13], buf[14], buf[15], buf[16], buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23]);
    if let Err(e) = submit_cmd_tx_and_wait_tx_done(&buf[..len], len) {
        log::error!(target: "wireless::bsp::sdio", "aicbsp_minimal_ipc_verify: submit_cmd_tx (bustx) failed {}", e);
        return Err(AxError::BadState);
    }
    ensure_busrx_thread_started();
    const MINIMAL_VERIFY_CFM_TIMEOUT_MS: u32 = 1500;
    log::info!(target: "wireless::bsp::sdio", "aicbsp_minimal_ipc_verify: request sent, waiting CFM ({}ms), F1 logged every 500ms", MINIMAL_VERIFY_CFM_TIMEOUT_MS);
    axtask::sleep(core::time::Duration::from_millis(100));
    let mut tick = |waited_ms: u32| {
        let mut bc = 0u8;
        with_sdio(|sdio| {
            bc = sdio.read_byte(0x112).unwrap_or(0xff);
            let bm = sdio.read_byte(0x102).unwrap_or(0xff);
            let fc = sdio.read_byte(0x10A).unwrap_or(0xff);
            log::info!(target: "wireless::bsp::sdio", "minimal_verify @ {}ms: F1 BLOCK_CNT(0x12)=0x{:02x} BYTEMODE_LEN(0x02)=0x{:02x} FLOW_CTRL(0x0A)=0x{:02x}", waited_ms, bc, bm, fc);
        });
        // 与 LicheeRV 一致：有数据时主动收包。先 yield 让 busrx 有机会先取走 FIFO，再主线程收一次，避免锁竞争下只有主线程抢到锁且 recv_pkt 超时
        if bc > 0 {
            axtask::sleep(core::time::Duration::from_millis(1));
            run_poll_rx_one();
        }
    };
    let mut do_poll = || run_poll_rx_one();
    let ok = RwnxCmdMgr::wait_done_until(
        MINIMAL_VERIFY_CFM_TIMEOUT_MS,
        || with_cmd_mgr(|c| c.is_done(token)).unwrap_or(false),
        Some(&mut tick),
        Some(&mut do_poll),
        None,
    );
    match ok {
        Ok(()) => {
            let mut cfm_buf = [0u8; 16];
            let len = with_cmd_mgr(|c| c.take_cfm(token, &mut cfm_buf)).flatten().unwrap_or(0);
            if let Some(memdata) = parse_dbg_mem_read_cfm_with_addr(&cfm_buf[..len], CHIP_REV_MEM_ADDR) {
                log::info!(target: "wireless::bsp::sdio", "aicbsp_minimal_ipc_verify: OK, CFM received, memdata=0x{:08x}", memdata);
                return Ok(());
            }
        }
        Err(_) => {}
    }
    log::error!(target: "wireless::bsp::sdio", "aicbsp_minimal_ipc_verify: FAIL (timeout or no CFM)");
    Err(AxError::BadState)
}

/// **aicbsp_sdio_init** — 用默认 cis_ops 读 FBR/CIS、chipmatch 并完成 probe（对照 LicheeRV aicbsp_sdio_init）
///
/// **LicheeRV 流程（aicsdio.c）**：
/// 1. `sdio_register_driver(&aicbsp_sdio_driver)`；
/// 2. 内核 MMC 枚举 SDIO 卡 → 读各 function 的 FBR/CIS → 填好 `func->vendor`、`func->device`；
/// 3. 用 id_table 匹配 → 调用 `aicbsp_sdio_probe(func)`；
/// 4. `aicbsp_sdio_probe` 内：`aicwf_sdio_chipmatch(sdiodev, func->vendor, func->device)` → func_init → bus_init →
///    `aicbsp_platform_init(sdiodev)` → `up(&aicbsp_probe_semaphore)`；
/// 5. `aicbsp_sdio_init` 里 `down_timeout(&aicbsp_probe_semaphore, 2000)` 返回。
///
/// **本实现（StarryOS）**：无内核 MMC，在 init 内用**默认 cis_ops** 模拟“枚举读 CIS”：
/// 1. 创建 SDIO 主机与 `CisReadOps`（仅持 host，不依赖 ProductId，识别前即可用）；
/// 2. 调用 `probe_from_sdio_cis(&cis_ops, 1)`：读 F1 的 FBR/CIS → vid/did → chipmatch → `aicbsp_sdio_probe(pid)`；
/// 3. 返回成功则 `CURRENT_PRODUCT_ID` 已设置，可继续 `aicbsp_driver_fw_init`；失败则返回 `Err`。
///
/// 必须先读 CIS 才能得到 ProductId，故此处使用不依赖芯片型号的 `CisReadOps`，不传 ProductId。
/// 成功后会将 host 与 cmd_mgr 存入静态，供 aicbsp_driver_fw_init 使用。
///
/// 与 LicheeRV 一致：sdio_register_driver(&aicbsp_sdio_driver) 在 **本函数内** 最先执行（aicsdio.c 591 行），
/// 不在 aicbsp_init 中注册。
#[inline]
pub fn aicbsp_sdio_init() -> AxResult<()> {
    // 0. 与 LicheeRV 一致：在 aicbsp_sdio_init 内注册 BSP SDIO 驱动（非 aicbsp_init）
    if let Err(e) = super::mmc_impl::register_aicbsp_sdio_driver() {
        if e != -16 {
            log::warn!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: sdio_register_driver err={} (ignored)", e);
        }
        // e == -16 (EBUSY) 表示已注册，与 LicheeRV 单次 register 语义一致，忽略
    }
    // 1. 初始化 SD 控制器并执行 SDIO 卡枚举
    let (host, rca) = Aic8800SdioHost::new_sd1_with_card_init().map_err(|e| {
        log::error!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: SDIO card enumeration failed (err={})", e);
        match e {
            -110 => log::error!(target: "wireless::bsp::sdio", "  err=-110 (ETIMEDOUT): CMD5 超时，卡未响应；检查：1) 板上是否有 SDIO 模组 2) GPIO 电源/复位引脚配置是否正确"),
            -19 => log::error!(target: "wireless::bsp::sdio", "  err=-19 (ENODEV): CMD5 返回 OCR=0，未检测到 SDIO 卡"),
            -74 => log::error!(target: "wireless::bsp::sdio", "  err=-74 (EBADMSG): CMD5 响应格式错误；可能原因：1) WiFi 模组未正确上电（检查 gpio.rs 中 WIFI_POWER_EN/WIFI_RESET 引脚配置）2) SD1 pinmux 配置不完整 3) 时钟频率问题"),
            _ => log::error!(target: "wireless::bsp::sdio", "  错误码 {}", e),
        }
        AxError::BadState
    })?;

    let present_sts = host.read_present_sts();
    log::info!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: card enumerated, RCA=0x{:04x}, PRESENT_STS=0x{:08x}", rca, present_sts);

    // 1.5 与 Linux 一致：首次 CMD52（读 CCCR）在 1-bit 下进行（sdio_card_init 已不在此前切 4-bit），无需长延时
    const POST_ENUM_DELAY_MS: u32 = 2;
    sync::delay_spin_ms(POST_ENUM_DELAY_MS);

    // 2. 枚举并启用 Function 1（SDIO 规范：须先 IO_ENABLE 再等 IO_READY，该 function 的 FBR/CIS 才有效）（当前仍 1-bit，与 Linux sdio_read_cccr 一致）
    let io_enable = host.read_byte(0x02).map_err(|e| {
        log::error!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: read CCCR 0x02 failed {}", e);
        AxError::BadState
    })?;
    host.write_byte(0x02, io_enable | 0x02).map_err(|e| {
        log::error!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: enable F1 (CCCR 0x02) failed {}", e);
        AxError::BadState
    })?;
    const IO_READY_F1_MS: u32 = 100;
    const IO_READY_POLL_MS: u32 = 2;
    let mut waited_ms: u32 = 0;
    // F1 ready: 标准 bit1(0x02)；部分 AIC 卡用 bit4(0x10) 表示 F1/IO 就绪（与 F2 一致）
    const IO_READY_F1_BIT: u8 = 0x02;
    const IO_READY_F1_BIT4: u8 = 0x10;
    while waited_ms < IO_READY_F1_MS {
        let io_ready = host.read_byte(0x03).unwrap_or(0);
        if (io_ready & IO_READY_F1_BIT) != 0 || (io_ready & IO_READY_F1_BIT4) != 0 {
            log::info!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: F1 enabled, IO_READY (0x03=0x{:02x}) after {}ms", io_ready, waited_ms);
            break;
        }
        sync::delay_spin_ms(IO_READY_POLL_MS);
        waited_ms += IO_READY_POLL_MS;
    }
    if waited_ms >= IO_READY_F1_MS {
        log::warn!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: F1 IO_READY timeout (0x03=0x{:02x}), continuing CIS read anyway", host.read_byte(0x03).unwrap_or(0));
    }

    // 3. 用 CisReadOps 读 FBR/CIS 并 chipmatch（仍 1-bit，与 Linux sdio_read_common_cis 一致）
    let cis_ops = CisReadOps::new(&host);
    super::cis::probe_from_sdio_cis(&cis_ops, 1).map_err(|e| {
        log::error!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: probe_from_sdio_cis failed (err={})", e);
        match e {
            -110 => log::error!(target: "wireless::bsp::sdio", "  err=-110 (ETIMEDOUT): CMD52 读 FBR/CIS 超时；可能原因：1) 卡未正确枚举 2) GPIO 电源引脚配置错误"),
            -5 => log::error!(target: "wireless::bsp::sdio", "  err=-5 (EIO): CMD52 响应错误；卡可能未正确初始化"),
            -1 => log::error!(target: "wireless::bsp::sdio", "  err=-1: CIS 中无 MANFID 或芯片不匹配"),
            _ => log::error!(target: "wireless::bsp::sdio", "  错误码 {}", e),
        }
        AxError::BadState
    })?;

    // 3.2 在 1-bit 下完成 8801 的 F1 配置，避免切 4-bit 后首条 CMD52 超时（inhibit_cmd=1、INT_STS=0）
    let pid = aicbsp_current_product_id().ok_or(AxError::BadState)?;
    if pid == ProductId::Aic8801 {
        use super::types::reg;
        let f1_base = 0x100u32;
        host.set_block_size(1, 512).map_err(|e| {
            log::error!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: set_block_size(1, 512) failed (1-bit) {}", e);
            AxError::BadState
        })?;
        log::info!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: Aic8801 F1 block size=512 in 1-bit (align LicheeRV sdio_set_block_size(func))");
        sync::delay_spin_us(100);
        host.write_byte(f1_base + u32::from(reg::REGISTER_BLOCK), 1).map_err(|e| {
            log::error!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: F1 REGISTER_BLOCK(0x0B)=1 failed {}", e);
            AxError::BadState
        })?;
        host.write_byte(f1_base + u32::from(reg::BYTEMODE_ENABLE), 1).map_err(|e| {
            log::error!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: F1 BYTEMODE_ENABLE(0x11)=1 failed {}", e);
            AxError::BadState
        })?;
        // F1 INTR_CONFIG(0x04)=0x07 与 LicheeRV 一致在 bus_start 中、在 claim_irq 之后写入，见 driver_fw_init 内 enable_8801_sdio_intr
        log::info!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: Aic8801 F1 0x0B=1 0x11=1 in 1-bit (0x04=0x07 deferred to driver_fw_init after busrx+IRQ)");
    }

    // 3.5 与 LicheeRV 差异：本 SoC 上 4-bit 下 CMD52 均超时（INT_STS=0 inhibit_cmd=1），故 8801 在 init 内不切 4-bit，首包 IPC（FLOW_CTRL+WR_FIFO）在 1-bit 下完成；非 8801 仍按 Linux 顺序切 4-bit
    if pid != ProductId::Aic8801 {
        host.enable_4bit_bus();
        const POST_4BIT_DELAY_MS: u32 = 10;
        sync::delay_spin_ms(POST_4BIT_DELAY_MS);
    } else {
        log::info!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: Aic8801 — keep 1-bit for first IPC (4-bit CMD52 timeout on this SoC), LicheeRV uses 4-bit after MMC set_ios)");
    }

    // 4. 启用 SDIO Function 2 并等待 IO_READY（仅非 8801：LicheeRV 对 8801 仅用 F1，不 enable F2，避免芯片侧行为差异）
    if pid != ProductId::Aic8801 {
        let io_enable = host.read_byte(0x02).map_err(|e| {
            log::error!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: read CCCR 0x02 failed {}", e);
            AxError::BadState
        })?;
        host.write_byte(0x02, io_enable | 0x04).map_err(|e| {
            log::error!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: enable F2 (CCCR 0x02) failed {}", e);
            AxError::BadState
        })?;
        const IO_READY_F2_MS: u32 = 100;
        let mut waited_ms: u32 = 0;
        const IO_READY_F2_BIT2: u8 = 0x04;
        const IO_READY_F2_BIT4: u8 = 0x10;
        while waited_ms < IO_READY_F2_MS {
            let io_ready = host.read_byte(0x03).unwrap_or(0);
            if (io_ready & IO_READY_F2_BIT2) != 0 || (io_ready & IO_READY_F2_BIT4) != 0 {
                log::info!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: enabled SDIO Function 2, IO_READY F2 (0x03=0x{:02x}) after {}ms", io_ready, waited_ms);
                break;
            }
            sync::delay_spin_ms(IO_READY_POLL_MS);
            waited_ms += IO_READY_POLL_MS;
        }
        if waited_ms >= IO_READY_F2_MS {
            log::warn!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: F2 IO_READY timeout (0x03=0x{:02x}), continuing anyway", host.read_byte(0x03).unwrap_or(0));
        }
        host.set_block_size(2, 512).map_err(|e| {
            log::error!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: set_block_size(2, 512) failed {}", e);
            AxError::BadState
        })?;
    } else {
        log::info!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: Aic8801 — skip F2 enable (LicheeRV 8801 only F1)");
    }

    // 5. 按当前 product_id 构造 Aic8800Sdio 与 cmd_mgr，存入静态供 driver_fw_init 使用
    let sdio = Aic8800Sdio::new(host, pid);
    SDIO_DEVICE.lock().replace(sdio);

    // 5.1 与 LicheeRV 一致：枚举到卡后按 id_table 调用 probe。LicheeRV 在 aicbsp_sdio_init 内 sdio_register_driver，故此处先确保已注册（minimal_ipc_verify 不经过 aicbsp_init 时也成立）
    let _ = super::mmc_impl::register_aicbsp_sdio_driver();
    if let Some(ref sdio) = *SDIO_DEVICE.lock() {
        let func_ref = super::mmc_impl::BspSdioFuncRef::new(sdio, 1);
        if let Err(e) = mmc::sdio_try_probe(&func_ref) {
            log::warn!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: sdio_try_probe err={} (id_table 不匹配或 probe 失败)", e);
        }
    }

    CMD_MGR.lock().replace(RwnxCmdMgr::new());

    // 6. 与 LicheeRV 一致：bustx 在 bus_init 里启动，首条 SDIO 命令由 bustx 发出（send_msg），避免 busrx 先轮询 CMD52 导致超时
    //    busrx 由调用方在“需要收包前”启动：minimal_verify 在 submit 后、aicbsp_driver_fw_init 在发首包前
    ensure_bustx_thread_started();

    // 7. 与 LicheeRV 一致：bus_start（claim_irq + F1 INTR_CONFIG=0x07）在 probe 完成后、首包 IPC 前执行。
    //    LicheeRV 在 aicwf_sdio_probe → bus_init → aicwf_bus_start 中完成；若此处不做，设备可能不对 MEM_WRITE 回 CFM（BLOCK_CNT 恒 0）。见 BLOCK_CNT流程与LicheeRV对照.md
    if pid == ProductId::Aic8801 {
        crate::sdio_irq::ensure_sdio_irq_registered();
        let f1_intr = 0x100u32 + u32::from(super::types::reg::INTR_CONFIG);
        if let Some(Err(e)) = with_sdio(|sdio| sdio.write_byte(f1_intr, 0x07)) {
            log::warn!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: Aic8801 F1 INTR_CONFIG(0x04)=0x07 failed {} (bus_start)", e);
        } else {
            log::info!(target: "wireless::bsp::sdio", "aicbsp_sdio_init: Aic8801 bus_start (claim_irq + F1 0x04=0x07) done, align LicheeRV");
        }
    }

    Ok(())
}

/// **aicbsp_sdio_probe** — SDIO 设备探测成功后的收尾（BSP 侧）
///
/// **作用**：在“SDIO 设备已被发现并完成 chipmatch、func/bus 初始化、cmd_mgr 初始化”之后调用，
/// 记录当前产品 ID 并通知“probe 完成”，使正在 `aicbsp_sdio_init` 里等待的 `down_timeout` 返回。
///
/// **执行过程（LicheeRV）**：由内核在匹配到 AIC SDIO 设备后调用 `aicbsp_sdio_probe`，其内：分配
/// `aic_sdio_dev`/`aicwf_bus`、chipmatch(vid/did)→chipid、func_init、bus_init、`aicbsp_platform_init(sdiodev)`
///（cmd_mgr_init）、最后 `up(&aicbsp_probe_semaphore)`。
///
/// **本实现**：上层在“probe 等价”路径（已确定产品 ID、完成平台侧 func/bus/cmd_mgr 初始化）末尾调用
/// 本函数，传入当前 `product_id`；本函数写入 `CURRENT_PRODUCT_ID` 并调用 `sync::probe_signal()`。
#[inline]
pub fn aicbsp_sdio_probe(product_id: ProductId) {
    CURRENT_PRODUCT_ID.store(product_id as u32, Ordering::SeqCst);
    log::debug!(target: "wireless::bsp::sdio", "aicbsp_sdio_probe: product_id={:?}, probe_signal", product_id);
    sync::probe_signal();
}

/// 8801 aicbsp_system_config 表（DBG_MEM_WRITE，在 aicwifi_init 之前执行，对应 aic_bsp_driver.c aicbsp_syscfg_tbl）
/// LicheeRV 顺序：driver_fw_init 内 dbg_mem_read → aicbsp_system_config → aicwifi_init（fw_upload → patch → aicwifi_sys_config → start）
const AICBSP_SYSCFG_TBL_8801: &[(u32, u32)] = &[
    (0x4050_0014, 0x0000_0101), // 1) order must not change
    (0x4050_0018, 0x0000_0109), // 2)
    (0x4050_0004, 0x0000_0010), // 3)
    (0x4004_0000, 0x0000_1AC8), // U02 bootrom: fix panic
    (0x4004_0084, 0x0001_1580),
    (0x4004_0080, 0x0000_0001),
    (0x4010_0058, 0x0000_0000),
    (0x5000_0000, 0x0322_0204), // pmic interface init
    (0x5001_9150, 0x0000_0002), // for 26m xtal, set div1
    (0x5001_7008, 0x0000_0000), // stop wdg
];

/// 8801 aicbsp_system_config：在 aicwifi_init（固件上传）之前执行，与 LicheeRV aicbsp_system_config() 对齐。
/// 使用闭包 send_one(addr, data)，调用方在 send_one 内用 with_cmd_mgr 短暂持锁（push → 释放 → tx → 持锁 wait_done），
/// 避免长期持有 CMD_MGR 导致 busrx 无法处理 CFM 而超时。
fn aicbsp_system_config_8801(send_one: &mut dyn FnMut(u32, u32) -> Result<(), i32>) -> Result<(), i32> {
    let n = AICBSP_SYSCFG_TBL_8801.len();
    log::info!(target: "wireless::bsp::sdio", "aicbsp_system_config_8801: start ({} entries)", n);
    for (i, &(addr, data)) in AICBSP_SYSCFG_TBL_8801.iter().enumerate() {
        send_one(addr, data)?;
        if (i + 1) % 5 == 0 || i + 1 == n {
            log::info!(target: "wireless::bsp::sdio", "aicbsp_system_config_8801: progress {}/{}", i + 1, n);
        }
    }
    log::info!(target: "wireless::bsp::sdio", "aicbsp_system_config_8801: done");
    Ok(())
}

/// 8801 系统配置表（syscfg_tbl_masked，对应 aic_bsp_driver.c aicwifi_sys_config，在 fw_upload 之后）
const SYSCFG_TBL_MASKED_8801: &[(u32, u32, u32)] = &[
    (0x4050_6024, 0x0000_00FF, 0x0000_00DF), // clk gate lp_level
];

/// 8801 RF 配置表（rf_tbl_masked，对应 aic_bsp_driver.c）
const RF_TBL_MASKED_8801: &[(u32, u32, u32)] = &[
    (0x4034_4058, 0x0080_0000, 0x0000_0000), // pll trx
];

/// 8801 aicwifi_patch_config：读 config_base，写 patch 表（对应 aic_bsp_driver.c aicwifi_patch_config）
const RD_PATCH_ADDR_8801: u32 = RAM_FMAC_FW_ADDR + 0x0180;
const PATCH_START_ADDR_8801: u32 = 0x1e6000;
const PATCH_ADDR_REG_8801: u32 = 0x1e5318;
const PATCH_NUM_REG_8801: u32 = 0x1e531c;
/// patch_tbl 与 LicheeRV 一致：!CONFIG_LINK_DET_5G 一项 + CONFIG_MCU_MESSAGE 两项
const PATCH_TBL_8801: &[(u32, u32)] = &[
    (0x0104, 0x0000_0000),   // link_det_5g
    (0x004c, 0x0000_004B),   // pkt_cnt_1724=0x4B
    (0x0050, 0x0011_FC00),   // ipc_base_addr
];

/// 主线程直接发 DBG_MEM_READ_REQ + wait_done_until，不经过 bustx，供 init 路径避免与 busrx 争用（与 LicheeRV 发送时总线独占一致）
fn send_dbg_mem_read_direct(mem_addr: u32, timeout_ms: u32, product_id: ProductId) -> Result<u32, i32> {
    let token = with_cmd_mgr(|c| c.push(DBG_MEM_READ_CFM)).flatten().ok_or(-12)?;
    let msg = build_dbg_mem_read_req(mem_addr);
    let mut buf = [0u8; 512];
    let len = if product_id == ProductId::Aic8801 {
        msg.serialize_8801(&mut buf)
    } else {
        msg.serialize(&mut buf)
    };
    let send_len = if product_id == ProductId::Aic8801 {
        ipc_send_len_8801(len)
    } else {
        len
    };
    if send_len > len {
        buf[len..send_len].fill(0);
    }
    match with_sdio(|sdio| sdio.send_msg(&buf[..send_len], send_len)) {
        Some(Ok(_)) => {}
        Some(Err(e)) => return Err(e),
        None => return Err(-5),
    }
    axtask::sleep(core::time::Duration::from_millis(100));
    RwnxCmdMgr::wait_done_until(
        timeout_ms,
        || with_cmd_mgr(|c| c.is_done(token)).unwrap_or(false),
        None,
        None,
        None,
    )
    .map_err(|_| -62)?;
    let mut cfm_buf = [0u8; 16];
    let len = with_cmd_mgr(|c| c.take_cfm(token, &mut cfm_buf)).flatten().ok_or(-5)?;
    parse_dbg_mem_read_cfm_with_addr(&cfm_buf[..len], mem_addr).ok_or(-5)
}

/// 主线程直接发 DBG_MEM_WRITE + wait_done_until，不持 CMD_MGR，供 init 路径避免与 busrx 争用
fn send_dbg_mem_write_direct(addr: u32, data: u32, timeout_ms: u32, product_id: ProductId) -> Result<(), i32> {
    let msg = build_dbg_mem_write_req(addr, data);
    let token = with_cmd_mgr(|mgr| mgr.push(DBG_MEM_WRITE_CFM)).flatten().ok_or(-12)?;
    let mut buf = [0u8; 512];
    let len = if product_id == ProductId::Aic8801 {
        msg.serialize_8801(&mut buf)
    } else {
        msg.serialize(&mut buf)
    };
    let send_len = if product_id == ProductId::Aic8801 {
        ipc_send_len_8801(len)
    } else {
        len
    };
    if send_len > len {
        buf[len..send_len].fill(0);
    }
    match with_sdio(|sdio| sdio.send_msg(&buf[..send_len], send_len)) {
        Some(Ok(_)) => {}
        Some(Err(e)) => return Err(e),
        None => return Err(-5),
    }
    RwnxCmdMgr::wait_done_until(
        timeout_ms,
        || with_cmd_mgr(|c| c.is_done(token)).unwrap_or(false),
        None,
        None,
        None,
    )
    .map_err(|_| -62)
}

/// init 路径：不持 CMD_MGR，用 send_dbg_mem_read_direct + send_dbg_mem_write_direct（主线程直接发，不经过 bustx），避免与 busrx 争用 SDIO
fn aicwifi_patch_config_8801_init(product_id: ProductId, timeout_ms: u32) -> Result<(), i32> {
    let config_base = send_dbg_mem_read_direct(RD_PATCH_ADDR_8801, timeout_ms, product_id)?;
    send_dbg_mem_write_direct(PATCH_ADDR_REG_8801, PATCH_START_ADDR_8801, timeout_ms, product_id)?;
    let patch_num = (PATCH_TBL_8801.len() * 2) as u32;
    send_dbg_mem_write_direct(PATCH_NUM_REG_8801, patch_num, timeout_ms, product_id)?;
    for (cnt, &(off, val)) in PATCH_TBL_8801.iter().enumerate() {
        let addr = PATCH_START_ADDR_8801 + (cnt as u32) * 8;
        send_dbg_mem_write_direct(addr, off + config_base, timeout_ms, product_id)?;
        send_dbg_mem_write_direct(addr + 4, val, timeout_ms, product_id)?;
    }
    Ok(())
}

fn aicwifi_patch_config_8801(
    cmd_mgr: &mut RwnxCmdMgr,
    tx_fn: &mut dyn FnMut(&LmacMsg) -> Result<(), i32>,
    timeout_ms: u32,
    poll_fn: &mut dyn FnMut(&mut RwnxCmdMgr),
) -> Result<(), i32> {
    let config_base = send_dbg_mem_read(
        cmd_mgr,
        &mut *tx_fn,
        RD_PATCH_ADDR_8801,
        timeout_ms,
        poll_fn,
        None,
    )?;
    send_dbg_mem_write(cmd_mgr, &mut *tx_fn, PATCH_ADDR_REG_8801, PATCH_START_ADDR_8801, timeout_ms, poll_fn, None)?;
    let patch_num = (PATCH_TBL_8801.len() * 2) as u32; // sizeof(patch_tbl)/4 in C
    send_dbg_mem_write(cmd_mgr, &mut *tx_fn, PATCH_NUM_REG_8801, patch_num, timeout_ms, poll_fn, None)?;
    for (cnt, &(off, val)) in PATCH_TBL_8801.iter().enumerate() {
        let addr = PATCH_START_ADDR_8801 + (cnt as u32) * 8;
        send_dbg_mem_write(cmd_mgr, &mut *tx_fn, addr, off + config_base, timeout_ms, poll_fn, None)?;
        send_dbg_mem_write(cmd_mgr, &mut *tx_fn, addr + 4, val, timeout_ms, poll_fn, None)?;
    }
    Ok(())
}

/// 8801 aicwifi_sys_config：写 syscfg_tbl_masked 与 rf_tbl_masked（DBG_MEM_MASK_WRITE）
fn aicwifi_sys_config_8801(
    cmd_mgr: &mut RwnxCmdMgr,
    tx_fn: &mut dyn FnMut(&LmacMsg) -> Result<(), i32>,
    timeout_ms: u32,
    poll_fn: &mut dyn FnMut(&mut RwnxCmdMgr),
) -> Result<(), i32> {
    for &(addr, mask, data) in SYSCFG_TBL_MASKED_8801 {
        send_dbg_mem_mask_write(cmd_mgr, &mut *tx_fn, addr, mask, data, timeout_ms, poll_fn)?;
    }
    for &(addr, mask, data) in RF_TBL_MASKED_8801 {
        send_dbg_mem_mask_write(cmd_mgr, &mut *tx_fn, addr, mask, data, timeout_ms, poll_fn)?;
    }
    Ok(())
}

/// **aicbsp_driver_fw_init** — 固件与芯片初始化（读 chip rev、选固件表、固件上传、patch/sys 配置、START_APP）
///
/// **作用**：在 SDIO probe 完成后、FDRV 注册 wiphy 之前执行：读芯片版本、根据 chipid/chip_rev 选择
/// 固件表、可选蓝牙初始化、然后 WiFi 固件上传与 patch、最后 START_APP 启动固件。
///
/// **执行过程（LicheeRV）**：1）`rwnx_send_dbg_mem_read_req` 读 0x40500000 得 chip_rev；2）按 chipid/chip_rev
/// 设置 `aicbsp_firmware_list`；3）8801 先 `aicbsp_system_config(sdiodev)`（写 aicbsp_syscfg_tbl）；4）可选 `aicbt_init`；
/// 5）`aicwifi_init(sdiodev)`（固件上传、patch、aicwifi_sys_config、start_from_bootrom）。本函数不调用 `rwnx_cfg80211_init`。
///
/// **本实现**：读 chip_rev → 选固件表 → 8801 **aicbsp_system_config** → 按名取固件（本地/注册表）
/// → 3a wl_fw 上传 → 3b patch 上传 → 3c aicwifi_patch_config → 4 aicwifi_sys_config → 5 fw_start_app。
/// 固件由 `get_firmware_by_name` 从本地（embed）或 `set_wifi_firmware` 注册表提供；phy_cfg 在 FDRV 固件就绪后按 ini 应用。
pub fn aicbsp_driver_fw_init(info: &mut AicBspInfo) -> AxResult<()> {
    let product_id = aicbsp_current_product_id().ok_or(AxError::BadState)?;

    // 与 LicheeRV 一致：driver_fw_init 在 aicbsp_sdio_init 之后调用，此时 SDIO_DEVICE 已设置；先检查避免持锁后 bustx 拿不到
    if with_sdio(|_| ()).is_none() {
        log::error!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: SDIO_DEVICE not set (probe not done?)");
        return Err(AxError::BadState);
    }

    // 与 LicheeRV 对齐：先启动 bustx/busrx 线程，再发 IPC
    ensure_bustx_thread_started();
    ensure_busrx_thread_started();
    // 8801：与 LicheeRV aicwf_sdio_bus_start 顺序一致 — 先 claim_irq 再写 F1 INTR_CONFIG(0x04)=0x07，避免中断已使能但 handler 未注册
    if product_id == ProductId::Aic8801 {
        crate::sdio_irq::ensure_sdio_irq_registered();
        let f1_intr = 0x100u32 + u32::from(super::types::reg::INTR_CONFIG);
        if let Some(Err(e)) = with_sdio(|sdio| sdio.write_byte(f1_intr, 0x07)) {
            log::error!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: F1 INTR_CONFIG(0x04)=0x07 failed {}", e);
            return Err(AxError::BadState);
        }
        log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: Aic8801 F1 INTR_CONFIG(0x04)=0x07 (after busrx+IRQ, align LicheeRV bus_start)");
    }
    // LicheeRV 上首包前有调度/中断延迟；给 bootrom 与 FLOW_CTRL 就绪时间，减少首包 submit_cmd_tx -110
    const POST_BUS_INIT_DELAY_MS: u64 = 200;
    axtask::sleep(core::time::Duration::from_millis(POST_BUS_INIT_DELAY_MS));

    const CMD_TIMEOUT_MS: u32 = 2000;

    // 1. 读 0x40500000 得 chip_rev（多线程：由 busrx 线程收 CFM，本线程仅 wait_done_until）。与 LicheeRV 一致：读失败则返回错误，不使用默认 chip_rev
    let (chip_rev_raw, is_chip_id_h) = match (if product_id == ProductId::Aic8801 {
        let mut after_delay = || {
            with_sdio(|sdio| {
                let bm = sdio.read_byte(0x102).unwrap_or(0xff);
                let bc = sdio.read_byte(0x112).unwrap_or(0xff);
                let fc = sdio.read_byte(0x10A).unwrap_or(0xff);
                log::info!(target: "wireless::bsp::sdio", "dbg_mem_read: after 100ms F1 BLOCK_CNT(0x12)=0x{:02x} BYTEMODE_LEN(0x02)=0x{:02x} FLOW_CTRL(0x0A)=0x{:02x}", bc, bm, fc);
            });
        };
        send_dbg_mem_read_busrx(CHIP_REV_MEM_ADDR, CMD_TIMEOUT_MS, Some(&mut after_delay))
    } else {
        send_dbg_mem_read_busrx(CHIP_REV_MEM_ADDR, CMD_TIMEOUT_MS, None)
    }) {
        Ok(memdata) => match product_id {
            // 8801：与 LicheeRV aic_bsp_driver.c:2019 一致，无掩码 chip_rev = (u8)(memdata >> 16)
            // 8800DC/D80：LicheeRV 用 (memdata>>16)&0x3F / is_chip_id_h=(memdata>>16)&0xC0
            ProductId::Aic8801 => {
                info.chip_rev = (memdata >> 16) as u8;
                (info.chip_rev, false)
            }
            ProductId::Aic8800Dc | ProductId::Aic8800Dw => {
                info.chip_rev = ((memdata >> 16) & 0x3F) as u8;
                let is_h = ((memdata >> 16) & 0xC0) == 0xC0;
                (info.chip_rev, is_h)
            }
            ProductId::Aic8800D80 => {
                info.chip_rev = ((memdata >> 16) & 0x3F) as u8;
                let is_h = ((memdata >> 16) & 0xC0) == 0xC0;
                (info.chip_rev, is_h)
            }
            ProductId::Aic8800D80X2 => {
                info.chip_rev = ((memdata >> 16) & 0x3F) as u8;
                (info.chip_rev, false)
            }
        },
        Err(e) => {
            // 与 LicheeRV 一致：读 chip_rev 失败（超时或发送失败）则直接返回错误，不使用默认 chip_rev 继续
            log::error!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: dbg_mem_read 0x40500000 failed {}", e);
            return Err(AxError::BadState);
        }
    };

    // 与 LicheeRV 一致：后续 DBG_* 发送经 bustx 线程；tx_fn/poll 先定义，step 2.5 用 send_one 内 with_cmd_mgr 短暂持锁，避免长期持 CMD_MGR 导致 busrx 无法收 CFM 而超时
    // 主线程直接 with_sdio(send_msg)，不经过 bustx，避免与 busrx 轮询争用 SDIO 导致超时（与 LicheeRV 发送时 bus 独占语义一致）
    // 发送缓冲：8801 与 LicheeRV 一致每块 1024 字节，消息 16+1032=1048，ipc_send_len_8801 向上取整到 1536
    let mut block_write_first_log = false;
    let mut mem_write_first_log = false;
    let mut tx_fn = |msg: &LmacMsg| -> Result<(), i32> {
        let mut buf = [0u8; 1536];
        let len = if product_id == ProductId::Aic8801 {
            msg.serialize_8801(&mut buf)
        } else {
            msg.serialize(&mut buf)
        };
        // 与 LicheeRV rwnx_set_cmd_tx 对齐：首包 DBG_MEM_WRITE（system_config 第一笔 0x40500014,0x00000101）打前 24B 便于逐字节对照
        if product_id == ProductId::Aic8801 && msg.header.id == DBG_MEM_WRITE_REQ && !mem_write_first_log && len >= 24 {
            mem_write_first_log = true;
            log::warn!(target: "wireless::bsp::sdio",
                "DBG_MEM_WRITE_REQ first 24B (LicheeRV rwnx_set_cmd_tx): {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15], buf[16], buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23]);
        }
        // 与 LicheeRV rwnx_set_cmd_tx 对齐：首包 DBG_MEM_BLOCK_WRITE 打前 24B 便于对照 [len+4 LE2][0x11 0x00][dummy4][id dest src param_len LE2][memaddr LE4]
        if product_id == ProductId::Aic8801 && msg.header.id == DBG_MEM_BLOCK_WRITE_REQ && !block_write_first_log && len >= 24 {
            block_write_first_log = true;
            log::warn!(target: "wireless::bsp::sdio",
                "DBG_MEM_BLOCK_WRITE_REQ first 24B (LicheeRV rwnx_set_cmd_tx): {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15], buf[16], buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23]);
        }
        let send_len = if product_id == ProductId::Aic8801 {
            ipc_send_len_8801(len)
        } else {
            len
        };
        if send_len > len {
            buf[len..send_len].fill(0);
        }
        // 8801 固件块写与 sysconfig 一致：经 bustx 发送（submit_cmd_tx_and_wait_tx_done），bustx 内 flow_ctrl+send_pkt，避免主线程直连 send_msg 时与 busrx 争用导致芯片未回 DBG_MEM_BLOCK_WRITE_CFM（BLOCK_CNT 恒 0）
        if product_id == ProductId::Aic8801 {
            submit_cmd_tx_and_wait_tx_done(&buf[..len], len).map_err(|e| {
                log::warn!(target: "wireless::bsp::sdio", "tx_fn submit_cmd_tx (bustx) err={} (e.g. -110=FLOW_CTRL)", e);
                e
            })
        } else {
            match with_sdio(|sdio| sdio.send_msg(&buf[..send_len], send_len)) {
                Some(Ok(_)) => Ok(()),
                Some(Err(e)) => {
                    log::warn!(target: "wireless::bsp::sdio", "tx_fn send_msg err={} (e.g. -110=FLOW_CTRL/-5=EIO)", e);
                    Err(e)
                }
                None => {
                    log::warn!(target: "wireless::bsp::sdio", "tx_fn with_sdio None (SDIO_DEVICE not set?)");
                    Err(-5)
                }
            }
        }
    };
    let mut poll = |c: &mut RwnxCmdMgr| {
        with_sdio(|sdio| poll_rx_one(sdio, c));
    };

    log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: product_id={:?} chip_rev={} (from (memdata>>16), LicheeRV accepts only 3/7)", product_id, info.chip_rev);

    // 2. 选固件表（与 LicheeRV aic_bsp_driver.c 2019-2030 一致：8801 仅接受 U02/U03/U04，否则返回错误）
    let fw_list = get_firmware_list(product_id, chip_rev_raw, is_chip_id_h).ok_or_else(|| {
        log::error!(target: "wireless::bsp::sdio", "aicbsp: aicbsp_driver_fw_init, unsupport chip rev: {} (LicheeRV same check; chip returned (memdata>>16)=0x{:02x}, expected 3 or 7)", info.chip_rev, info.chip_rev);
        AxError::BadState
    })?;

    let cpmode = (info.cpmode as usize).min(1);
    let fw = &fw_list[cpmode];
    log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: fw_list[{}] wl_fw={}", cpmode, fw.wl_fw);

    // 2.5. 8801：aicbsp_system_config。与 LicheeRV 完全对齐：
    // - 时序：LicheeRV 在 driver_fw_init 中 mem_read 返回后立即调用 aicbsp_system_config，无中间 delay。
    // - 发送：经 bustx 线程写 WR_FIFO；收 CFM：仅 busrx+PLIC，主线程不传 poll_fn。
    if matches!(product_id, ProductId::Aic8801) {
        log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: step 2.5 aicbsp_system_config_8801");
        const SYS_CONFIG_CFM_WAIT_MS: u32 = 3000;
        const SYS_CONFIG_LOG_EVERY_MS: u32 = 500;
        let mut tick_sys_config = |waited_ms: u32| {
            log_f1_block_cnt_flow_ctrl(waited_ms);
        };
        let mut syscfg_first_24b_log = false;
        let mut send_one = |addr: u32, data: u32| -> Result<(), i32> {
            let msg = build_dbg_mem_write_req(addr, data);
            let mut buf = [0u8; PENDING_CMD_TX_CAP];
            let len = msg.serialize_8801(&mut buf);
            let send_len = ipc_send_len_8801(len);
            if send_len > buf.len() {
                return Err(-22);
            }
            const TAIL_LEN_SYS: usize = 4; // 与 LicheeRV aicwf_sdio_tx_msg TAIL_LEN 一致
            if send_len > len {
                buf[len..len + TAIL_LEN_SYS].fill(0);
                if send_len > len + TAIL_LEN_SYS {
                    buf[len + TAIL_LEN_SYS..send_len].fill(0);
                }
            }
            log::warn!(
                target: "wireless::bsp",
                "send_dbg_mem_write: id={} dest={} src={} param_len={} addr=0x{:08x} data=0x{:08x}",
                msg.header.id, msg.header.dest_id, msg.header.src_id, msg.header.param_len,
                addr, data
            );
            if !syscfg_first_24b_log && send_len >= 24 {
                syscfg_first_24b_log = true;
                log::warn!(target: "wireless::bsp::sdio",
                    "DBG_MEM_WRITE_REQ first 24B (LicheeRV rwnx_set_cmd_tx): {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
                    buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15], buf[16], buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23]);
            }
            let token = match with_cmd_mgr(|mgr| mgr.push(DBG_MEM_WRITE_CFM).ok_or(-12)) {
                Some(Ok(t)) => t,
                Some(Err(e)) => return Err(e),
                None => return Err(-5),
            };
            submit_cmd_tx_and_wait_tx_done(&buf[..send_len], send_len)?;
            RwnxCmdMgr::wait_done_until(
                SYS_CONFIG_CFM_WAIT_MS,
                || with_cmd_mgr(|c| c.is_done(token)).unwrap_or(false),
                Some(&mut tick_sys_config),
                None,
                Some(SYS_CONFIG_LOG_EVERY_MS),
            )
            .map_err(|_| -62)?;
            // 与 LicheeRV 一致：wait 返回后释放 cmd 槽位（LicheeRV 在 rwnx_send_msg 返回前 kfree(cmd)），否则 10 条 syscfg 会占满 8 个 slot 导致第 9 次 push 返回 -ENOMEM(-12)
            let _ = with_cmd_mgr(|c| c.take_cfm(token, &mut [0u8; 16]));
            axtask::sleep(core::time::Duration::from_millis(10));
            Ok(())
        };
        aicbsp_system_config_8801(&mut send_one).map_err(|e| {
            log::error!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: aicbsp_system_config_8801 failed {}", e);
            AxError::BadState
        })?;
    }

    // 3a / 3b：固件上传。
    // 根因（等不到 CFM）：原实现整段 3a/3b 持 CMD_MGR，主线程 wait_done 时只由主线程 poll_rx_one 收包；busrx 线程需持 CMD_MGR 才能
    // 调 on_cfm，故被阻塞无法运行，CFM 仅能靠主线程轮询。与 LicheeRV 不同：LicheeRV 为中断+独立 busrx 线程收包并 complete()，主线程
    // 只 wait_for_completion。修复：用 push_fn/wait_fn 仅在 push 与每轮 condition/poll 时短暂持锁，wait_done_until 内 1ms sleep 时不持锁，
    // busrx 可运行并 poll_rx_one → on_cfm，主线程被唤醒后 condition() 为 true。
    let mut poll_impl = || {
        with_cmd_mgr(|m| {
            with_sdio(|s| poll_rx_one(s, m));
        });
    };
    const BLOCK_WRITE_CFM_TIMEOUT_MS: u32 = 500; // 每块等 CFM 超时，到时间未返回即报错
    const BLOCK_WRITE_LOG_EVERY_MS: u32 = 100; // 等待 CFM 时每 100ms 打 F1 BLOCK_CNT/FLOW_CTRL 便于确认设备是否回包
    let mut block_wait_tick = |waited_ms: u32| log_f1_block_cnt_flow_ctrl(waited_ms);
    let mut push_fn = || with_cmd_mgr(|m| m.push(DBG_MEM_BLOCK_WRITE_CFM)).flatten();
    let mut block_cfm_dummy = [0u8; 16];
    let mut wait_fn = |token: usize| {
        RwnxCmdMgr::wait_done_until(
            BLOCK_WRITE_CFM_TIMEOUT_MS,
            || with_cmd_mgr(|m| m.is_done(token)).unwrap_or(false),
            Some(&mut block_wait_tick),
            Some(&mut poll_impl),
            Some(BLOCK_WRITE_LOG_EVERY_MS),
        )
        .map_err(|_| -62)?;
        // 与 sysconfig 一致：wait 后 take_cfm 释放 slot，否则多块上传会占满 8 个 slot
        let _ = with_cmd_mgr(|c| c.take_cfm(token, &mut block_cfm_dummy));
        Ok(())
    };
    if let Some(data) = get_firmware_by_name(fw.wl_fw) {
        log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: step 3a wl_fw upload ({} bytes, {})", data.len(), fw.wl_fw);
        fw_upload_blocks(
            &mut tx_fn,
            RAM_FMAC_FW_ADDR,
            data,
            &mut push_fn,
            &mut wait_fn,
        )
        .map_err(|e| {
            log::error!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: fw_upload_blocks wl_fw failed, err={}", e);
            AxError::BadState
        })?;
        log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: step 3a wl_fw done");
    } else {
        log::warn!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: step 3a no firmware for {}, skip", fw.wl_fw);
    }
    if matches!(product_id, ProductId::Aic8801) {
        const RAM_FMAC_FW_PATCH_NAME: &str = "fmacfw_patch.bin";
        if let Some(data) = get_firmware_by_name(RAM_FMAC_FW_PATCH_NAME) {
            log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: step 3b patch upload ({} bytes, {})", data.len(), RAM_FMAC_FW_PATCH_NAME);
            fw_upload_blocks(
                &mut tx_fn,
                RAM_FMAC_FW_PATCH_ADDR,
                data,
                &mut push_fn,
                &mut wait_fn,
            )
            .map_err(|e| {
                log::error!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: patch upload failed {}", e);
                AxError::BadState
            })?;
            log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: step 3b patch done");
        } else {
            log::warn!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: step 3b no {} skip", RAM_FMAC_FW_PATCH_NAME);
        }
    }

    // 3c. 8801：aicwifi_patch_config，不持 CMD_MGR（主线程直接发 + wait_done_until），避免 busrx 无法收 CFM
    if matches!(product_id, ProductId::Aic8801) {
        log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: step 3c aicwifi_patch_config_8801");
        aicwifi_patch_config_8801_init(product_id, CMD_TIMEOUT_MS).map_err(|e| {
            log::error!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: aicwifi_patch_config_8801 failed {}", e);
            AxError::BadState
        })?;
        log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: step 3c aicwifi_patch_config_8801 done");
    }

    // 4. 8801：sys_config（持 cmd_guard 用于 4、5）
    let mut cmd_guard = CMD_MGR.lock();
    let cmd_mgr = cmd_guard.as_mut().ok_or(AxError::BadState)?;
    if matches!(product_id, ProductId::Aic8801) {
        log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: step 4 aicwifi_sys_config_8801");
        aicwifi_sys_config_8801(cmd_mgr, &mut tx_fn, CMD_TIMEOUT_MS, &mut poll).map_err(|e| {
            log::error!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: aicwifi_sys_config_8801 failed {}", e);
            AxError::BadState
        })?;
        log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: step 4 aicwifi_sys_config_8801 done");
    }

    // 5. START_APP（从 bootrom 启动）
    log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: step 5 fw_start_app");
    fw_start_app(
        cmd_mgr,
        &mut tx_fn,
        RAM_FMAC_FW_ADDR,
        HOST_START_APP_AUTO,
        CMD_TIMEOUT_MS,
        &mut poll,
    ).map_err(|e| {
        log::error!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: fw_start_app failed at step 5, err={}", e);
        AxError::BadState
    })?;
    log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: step 5 fw_start_app done");

    log::info!(target: "wireless::bsp::sdio", "aicbsp_driver_fw_init: done (all steps ok)");
    Ok(())
}

/// **aicbsp_sdio_release** — 释放 SDIO host 占用（保持设备已 probe 状态）
///
/// **作用**：在 `aicbsp_driver_fw_init` 成功后、FDRV 使用 SDIO 前，释放 BSP 对 SDIO host 的占用
///（release_irq、release_host），使 FDRV 可独立 claim host 进行读写。设备仍处于已 probe 状态。
///
/// **执行过程（LicheeRV）**：设置 `bus_if->state = BUS_DOWN_ST`，对 func（及 8800DC/DW 的 func_msg）
/// 做 `sdio_claim_host` → `sdio_release_irq` → `sdio_release_host`。
///
/// **本实现**：无 host/IRQ 抽象，空操作；由上层在 Linux 上按需 release host。
#[inline]
pub fn aicbsp_sdio_release() -> AxResult<()> {
    log::debug!(target: "wireless::bsp::sdio", "aicbsp_sdio_release (stub)");
    Ok(())
}

/// **aicbsp_sdio_exit** — 注销 BSP SDIO 驱动
///
/// **作用**：在模块卸载或下电时调用，与 LicheeRV `sdio_unregister_driver` 后内核调用 remove 等价：
/// 先停线程，再 **sdio_disable_func**（F1/F2），最后清空设备与 cmd_mgr。
///
/// **执行过程（LicheeRV）**：sdio_unregister_driver → remove → aicwf_sdio_release、**aicwf_sdio_func_deinit**（内 **sdio_disable_func**）、kfree。
///
/// **本实现**：无 register_driver，故无 unregister_driver 调用；在清 SDIO_DEVICE 前对 F1/F2 写 CCCR 0x02 关闭（与 sdio_disable_func 等价）。
#[inline]
pub fn aicbsp_sdio_exit() {
    // 与 LicheeRV 一致：先停 bustx 再停 busrx，避免退出时仍有 CMD 在队列
    BUSTX_RUNNING.store(false, Ordering::Relaxed);
    crate::sdio_irq::notify_bustx();
    BUSRX_RUNNING.store(false, Ordering::Relaxed);
    crate::sdio_irq::notify_wait_done();

    // LicheeRV：sdio_unregister_driver 前先 remove。调用 mmc::sdio_driver_remove 再 mmc::sdio_unregister_driver
    {
        let guard = SDIO_DEVICE.lock();
        if let Some(ref sdio) = *guard {
            let func_ref = super::mmc_impl::BspSdioFuncRef::new(sdio, 1);
            let _ = mmc::sdio_driver_remove(&func_ref);
        }
    }
    super::mmc_impl::unregister_aicbsp_sdio_driver();

    // LicheeRV remove → aicwf_sdio_func_deinit → sdio_disable_func(F1/F2)。在清 SDIO_DEVICE 前写 CCCR 0x02 关闭 F1(bit1)、F2(bit2)。
    {
        let guard = SDIO_DEVICE.lock();
        if let Some(ref sdio) = *guard {
            let host = sdio.host();
            if let Ok(io_enable) = host.read_byte(0x02) {
                let io_enable = io_enable & !0x02 & !0x04; // clear F1 and F2 (CCCR IO_ENABLE)
                let _ = host.write_byte(0x02, io_enable);
                log::debug!(target: "wireless::bsp::sdio", "aicbsp_sdio_exit: CCCR F1/F2 disabled (align sdio_disable_func)");
            }
        }
    }

    CURRENT_PRODUCT_ID.store(PRODUCT_ID_NONE, Ordering::SeqCst);
    SDIO_DEVICE.lock().take();
    CMD_MGR.lock().take();
    log::debug!(target: "wireless::bsp::sdio", "aicbsp_sdio_exit");
}

/// 返回当前已 probe 的产品 ID（若未 probe 则为 None）
#[inline]
pub fn aicbsp_current_product_id() -> Option<ProductId> {
    let id = CURRENT_PRODUCT_ID.load(Ordering::SeqCst);
    if id == PRODUCT_ID_NONE {
        return None;
    }
    match id {
        0 => Some(ProductId::Aic8801),
        1 => Some(ProductId::Aic8800Dc),
        2 => Some(ProductId::Aic8800Dw),
        3 => Some(ProductId::Aic8800D80),
        4 => Some(ProductId::Aic8800D80X2),
        _ => None,
    }
}
