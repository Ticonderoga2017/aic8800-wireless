//! FBR / CIS 读与解析（对应 Linux drivers/mmc/core/sdio_cis.c、include/linux/mmc/sdio.h）
//!
//! 对照 old-sg2002-wifi/wifi-driver detect_chip：优先从 CCCR 0x09-0x0B 读 CIS 指针（公共 CIS），
//! 若 CIS 中无 CISTPL_MANFID 则通过 probe_chip_type 探测寄存器推断芯片型号；不默认 D80。

use super::ops::SdioOps;
use super::types::{chipmatch, ProductId};

/// Function Basic Registers (FBR) 基址：function f 的 FBR 在地址 f*0x100
#[inline(always)]
pub fn sdio_fbr_base(func_num: u8) -> u32 {
    u32::from(func_num) * 0x100
}

/// FBR/CCCR 内 CIS 指针偏移（3 字节，小端）；CCCR 0x09-0x0B = 公共 CIS
pub const SDIO_FBR_CIS: u32 = 0x09;

/// CIS tuple：CISTPL_MANFID（制造商/设备 ID），4 字节，对应 cistpl_manfid
pub const CISTPL_MANFID: u8 = 0x20;

/// CIS tuple 结束标记
const CISTPL_END: u8 = 0xff;
const CISTPL_NULL: u8 = 0x00;

/// **从 CCCR 读出公共 CIS 指针**（CCCR 0x09-0x0B，3 字节小端）
/// 与 LicheeRV 一致：read_byte(0x09…) → backend 用 fn=0、addr=0x09/0x0a/0x0b。
pub fn read_cccr_cis_ptr<O: SdioOps>(ops: &O) -> Result<u32, i32> {
    let b0 = ops.read_byte(SDIO_FBR_CIS)?;
    let b1 = ops.read_byte(SDIO_FBR_CIS + 1)?;
    let b2 = ops.read_byte(SDIO_FBR_CIS + 2)?;
    Ok(u32::from(b0) | (u32::from(b1) << 8) | (u32::from(b2) << 16))
}

/// **从 FBR 读出 CIS 指针**（3 字节小端，24 位）
///
/// 调用链：`read_vendor_device(ops, 1)` → 本函数 `read_fbr_cis_ptr(ops, 1)`。
/// `func_num` 表示读哪个 function 的 FBR（1=F1），用于计算 base=0x109；**实际 CMD52 用 fn=0、addr=base**（统一 17 位地址，与 LicheeRV sdio_cis.c 一致）。
pub fn read_fbr_cis_ptr<O: SdioOps>(ops: &O, func_num: u8) -> Result<u32, i32> {
    let base = sdio_fbr_base(func_num) + SDIO_FBR_CIS; // F1 时 base=0x109，CMD52 用 fn=0
    log::debug!(target: "wireless::bsp::sdio", "read_fbr_cis_ptr: F{} FBR base=0x{:03x} (CMD52 fn=0)", func_num, base);
    let b0 = ops.read_byte(base)?;
    let b1 = ops.read_byte(base + 1)?;
    let b2 = ops.read_byte(base + 2)?;
    Ok(u32::from(b0) | (u32::from(b1) << 8) | (u32::from(b2) << 16))
}

/// **解析 CIS 中的 CISTPL_MANFID**，返回 (vendor, device)
///
/// 与 LicheeRV sdio_read_cis 一致：read_byte(ptr) → backend 对 0x1000+ 用 fn=0、17 位 addr。
pub fn parse_cis_for_manfid<O: SdioOps>(ops: &O, cis_ptr: u32) -> Result<Option<(u16, u16)>, i32> {
    const MAX_TUPLES: usize = 256;
    let mut ptr = cis_ptr & SDIO_ADDR_17BIT_MASK;
    for _ in 0..MAX_TUPLES {
        let tpl_code = ops.read_byte(ptr)?;
        ptr = ptr.wrapping_add(1);
        if tpl_code == CISTPL_END {
            break;
        }
        if tpl_code == CISTPL_NULL {
            continue;
        }
        let tpl_link = ops.read_byte(ptr)?;
        ptr = ptr.wrapping_add(1);
        if tpl_link == CISTPL_END {
            break;
        }
        let link = usize::from(tpl_link);
        if tpl_code == CISTPL_MANFID && link >= 4 {
            let d0 = ops.read_byte(ptr)?;
            let d1 = ops.read_byte(ptr + 1)?;
            let d2 = ops.read_byte(ptr + 2)?;
            let d3 = ops.read_byte(ptr + 3)?;
            let vendor = u16::from(d0) | (u16::from(d1) << 8);
            let device = u16::from(d2) | (u16::from(d3) << 8);
            return Ok(Some((vendor, device)));
        }
        ptr = ptr.wrapping_add(link as u32);
    }
    Ok(None)
}

/// SDIO CMD52 仅支持 17 位寄存器地址，cis_ptr 高 7 位会被截断。
const SDIO_ADDR_17BIT_MASK: u32 = 0x1_FFFF;

/// 在给定地址读前 N 字节并打日志（read_byte → backend fn=0）。
fn dump_cis_raw<O: SdioOps>(ops: &O, base: u32, n: usize, label: &str) {
    let eff = base & SDIO_ADDR_17BIT_MASK;
    let mut buf = [0u8; 32];
    let n = n.min(buf.len());
    for i in 0..n {
        buf[i] = ops.read_byte(eff.wrapping_add(i as u32)).unwrap_or(0xFF);
    }
    log::error!(target: "wireless::bsp::sdio", "{} 0x{:04x} 前{}字节: {:02x?}", label, eff, n, &buf[..n]);
}

/// 用指定 function 的地址空间读前 N 字节并打日志（调试 fn=1 路径时用）。
#[allow(dead_code)]
fn dump_cis_raw_at_func<O: SdioOps>(ops: &O, func_num: u8, base: u32, n: usize, label: &str) {
    let eff = base & SDIO_ADDR_17BIT_MASK;
    let mut buf = [0u8; 32];
    let n = n.min(buf.len());
    for i in 0..n {
        buf[i] = ops.read_byte_at_func(func_num, eff.wrapping_add(i as u32)).unwrap_or(0xFF);
    }
    log::error!(target: "wireless::bsp::sdio", "{} F{} 0x{:04x} 前{}字节: {:02x?}", label, func_num, eff, n, &buf[..n]);
}

/// 用指定 function 的地址空间解析 CIS 中的 CISTPL_MANFID（LicheeRV 仅用 fn=0；保留供调试）。
#[allow(dead_code)]
fn parse_cis_for_manfid_at_func<O: SdioOps>(ops: &O, func_num: u8, cis_ptr: u32) -> Result<Option<(u16, u16)>, i32> {
    const MAX_TUPLES: usize = 256;
    let mut ptr = cis_ptr & SDIO_ADDR_17BIT_MASK;
    for _ in 0..MAX_TUPLES {
        let tpl_code = ops.read_byte_at_func(func_num, ptr)?;
        ptr = ptr.wrapping_add(1);
        if tpl_code == CISTPL_END {
            break;
        }
        if tpl_code == CISTPL_NULL {
            continue;
        }
        let tpl_link = ops.read_byte_at_func(func_num, ptr)?;
        ptr = ptr.wrapping_add(1);
        if tpl_link == CISTPL_END {
            break;
        }
        let link = usize::from(tpl_link);
        if tpl_code == CISTPL_MANFID && link >= 4 {
            let d0 = ops.read_byte_at_func(func_num, ptr)?;
            let d1 = ops.read_byte_at_func(func_num, ptr + 1)?;
            let d2 = ops.read_byte_at_func(func_num, ptr + 2)?;
            let d3 = ops.read_byte_at_func(func_num, ptr + 3)?;
            let vendor = u16::from(d0) | (u16::from(d1) << 8);
            let device = u16::from(d2) | (u16::from(d3) << 8);
            return Ok(Some((vendor, device)));
        }
        ptr = ptr.wrapping_add(link as u32);
    }
    Ok(None)
}

/// **读 CIS 得到 vendor/device ID**（与 LicheeRV sdio_cis.c 完全一致，无备选无回退）
///
/// 仅：CCCR 指针 0x09-0x0B → 解析公共 CIS；若无 MANFID 则 F1 FBR 指针 0x109-0x10B → 解析 F1 CIS。
/// 全部 CMD52 fn=0 + 17 位地址。解析不到 CISTPL_MANFID 则返回 `Err(-2)`。
pub fn read_vendor_device<O: SdioOps>(ops: &O, func_num: u8) -> Result<(u16, u16), i32> {
    crate::sync::delay_spin_ms(5);

    // 1. 公共 CIS：CCCR 0x09-0x0B 取指针，fn=0 读 CIS 内容，解析 CISTPL_MANFID
    let cis_ptr_cccr = read_cccr_cis_ptr(ops)?;
    let eff_cccr = cis_ptr_cccr & SDIO_ADDR_17BIT_MASK;
    log::debug!(target: "wireless::bsp::sdio", "read_vendor_device: CCCR cis_ptr=0x{:06x} (fn=0 实际 0x{:04x})", cis_ptr_cccr, eff_cccr);
    if cis_ptr_cccr != 0 && cis_ptr_cccr != 0xFFFFFF {
        if let Some(ids) = parse_cis_for_manfid(ops, cis_ptr_cccr)? {
            return Ok(ids);
        }
    }

    // 2. F1 CIS：F1 FBR 0x109-0x10B 取指针，fn=0 读 CIS 内容，解析 CISTPL_MANFID（与 LicheeRV sdio_read_cis 一致）
    let cis_ptr_fbr = read_fbr_cis_ptr(ops, func_num)?;
    let eff_fbr = cis_ptr_fbr & SDIO_ADDR_17BIT_MASK;
    log::debug!(target: "wireless::bsp::sdio", "read_vendor_device: F1 FBR cis_ptr=0x{:06x} (fn=0 实际 0x{:04x})", cis_ptr_fbr, eff_fbr);
    if let Some(ids) = parse_cis_for_manfid(ops, cis_ptr_fbr)? {
        return Ok(ids);
    }

    log::error!(target: "wireless::bsp::sdio", "read_vendor_device: CIS 无 MANFID。CCCR cis_ptr=0x{:06x}(fn=0@0x{:04x}), F1 cis_ptr=0x{:06x}(fn=0@0x{:04x})", cis_ptr_cccr, eff_cccr, cis_ptr_fbr, eff_fbr);
    dump_cis_raw(ops, cis_ptr_cccr, 16, "CCCR CIS@");
    dump_cis_raw(ops, cis_ptr_fbr, 16, "F1 CIS@");
    Err(-2)
}

/// 通过探测寄存器推断芯片型号（LicheeRV 内核无此逻辑；保留供调试用）。
#[allow(dead_code)]
pub fn probe_chip_type<O: SdioOps>(ops: &O) -> Result<ProductId, i32> {
    // 1. 检查 Function 2 是否存在
    let io_enable = ops.read_byte(0x02).unwrap_or(0);
    let io_enable_new = io_enable | 0x04; // 使能 F2 (bit 2)
    let _ = ops.write_byte(0x02, io_enable_new);
    for _ in 0..1000 {
        core::hint::spin_loop();
    }
    let io_ready = ops.read_byte(0x03).unwrap_or(0);
    let _ = ops.write_byte(0x02, io_enable); // 恢复
    // 标准 F2 ready 为 bit2(0x04)；部分 AIC 卡用 bit4(0x10) 表示 F2/IO 就绪（与 flow.rs IO_READY 一致）
    let has_func2 = (io_ready & 0x04) != 0 || (io_ready & 0x10) != 0;

    if has_func2 {
        log::info!(target: "wireless::bsp::sdio", "probe_chip_type: F2 存在 -> 非 V3 (Aic8800Dc)");
        return Ok(ProductId::Aic8800Dc);
    }

    // 2. 无 F2，检查 V3 特征（F0 0xF0/0xF1/0xF2）
    let f0_f0 = ops.read_byte(0xF0).unwrap_or(0xFF);
    let f0_f1 = ops.read_byte(0xF1).unwrap_or(0xFF);
    let f0_f2 = ops.read_byte(0xF2).unwrap_or(0xFF);
    if f0_f0 != 0xFF || f0_f1 != 0xFF || f0_f2 != 0xFF {
        log::info!(target: "wireless::bsp::sdio", "probe_chip_type: F0 0xF0-0xF2 可访问 -> V3 (Aic8800D80)");
        return Ok(ProductId::Aic8800D80);
    }

    log::info!(target: "wireless::bsp::sdio", "probe_chip_type: 无法确定，默认 Aic8800Dc（不默认 D80）");
    Ok(ProductId::Aic8800Dc)
}

/// **从 SDIO 读 FBR/CIS 得到 vid/did 后做 chipmatch 并执行 BSP probe**
///
/// 对照 LicheeRV：内核 MMC 在枚举卡时读各 function 的 FBR/CIS，填充 `func->vendor`/`func->device`，
/// 再匹配 id_table 后调用 `aicbsp_sdio_probe(func)`；StarryOS 无 MMC 核心，由本函数完成「读 CIS → chipmatch → probe」。
///
/// ## 调用链（LicheeRV 对照）
///
/// **LicheeRV (aicsdio.c / aicwf_sdio.c)**：
/// 1. `aicbsp_sdio_init()` → `sdio_register_driver(&aicbsp_sdio_driver)`；
/// 2. 内核 MMC 枚举 SDIO 卡 → 读各 function 的 FBR/CIS → 填充 `func->vendor`、`func->device`；
/// 3. 匹配 id_table → 调用 `aicbsp_sdio_probe(func)`；
/// 4. `aicbsp_sdio_probe` 内：`aicwf_sdio_chipmatch(sdiodev, func->vendor, func->device)` → func_init → bus_init → `up(&aicbsp_probe_semaphore)`；
/// 5. `aicbsp_sdio_init` 里 `down_timeout(&aicbsp_probe_semaphore, 2000)` 返回。
///
/// **StarryOS 对应**：无内核 SDIO 总线时，由上电/主机初始化后**主动调用本函数**完成 probe：
/// 1. `aicbsp_power_on()` 后，创建 SDIO 主机与 ops；
/// 2. 调用 `probe_from_sdio_cis(&ops, func_num)`；
/// 3. 本函数内部：`read_vendor_device`（读 FBR/CIS）→ `chipmatch` → `aicbsp_sdio_probe(pid)`；
/// 4. 若上层在 `aicbsp_sdio_init(None)` 里等待，则 `probe_signal()` 会令其返回。
///
/// ## 从哪里开始调用
///
/// 在 **`aicbsp_power_on()` 返回之后**、且已有可用的 SDIO 主机并能做 `read_byte` 时调用。
/// 典型顺序：`aicbsp_power_on()` → 创建 `Aic8800SdioHost::new_sd1()` 和 `Aic8800Sdio`（临时 chipid 如 `Aic8801` 仅用于读 CIS）→ `probe_from_sdio_cis(&sdio, 1)`。
///
/// ## 参数
///
/// - **ops**：任意实现 `SdioOps` 的引用，用于读/写 CCCR、FBR、CIS。
/// - **func_num**：从哪个 function 的 FBR 读 CIS（用于 F1 的 0x109-0x10B）。`1` = F1（AIC8800 WiFi）。
///
/// ## 流程（与 LicheeRV 对齐，无回退）
///
/// 1. `read_vendor_device`：仅从 CCCR/F1 FBR CIS 解析 CISTPL_MANFID，无备选地址、无 probe_chip_type。
/// 2. 用 (vid, did) 做 chipmatch，得到 ProductId，再 `aicbsp_sdio_probe(pid)`。
/// ProductId → (vendor_id, device_id)，供 BspSdioFuncRef::device_id 与 id_table 匹配
pub fn product_id_to_vid_did(pid: ProductId) -> (u16, u16) {
    use super::types::sdio_ids::*;
    match pid {
        ProductId::Aic8801 => (VENDOR_AIC8801, DEVICE_AIC8801),
        ProductId::Aic8800Dc => (VENDOR_AIC8800DC, DEVICE_AIC8800DC),
        ProductId::Aic8800Dw => (VENDOR_AIC8800DC, DEVICE_AIC8800DC),
        ProductId::Aic8800D80 => (VENDOR_AIC8800D80, DEVICE_AIC8800D80),
        ProductId::Aic8800D80X2 => (VENDOR_AIC8800D80X2, DEVICE_AIC8800D80X2),
    }
}

pub fn probe_from_sdio_cis<O: SdioOps>(ops: &O, func_num: u8) -> Result<(), i32> {
    let (vid, did) = read_vendor_device(ops, func_num)?;
    let pid = chipmatch(vid, did).ok_or_else(|| {
        log::error!(target: "wireless::bsp::sdio", "probe_from_sdio_cis: vid=0x{:04x} did=0x{:04x} 不在 chipmatch 表内，非本驱动支持的 AIC 卡", vid, did);
        -2
    })?;
    log::info!(target: "wireless::bsp::sdio", "probe_from_sdio_cis: vid=0x{:04x} did=0x{:04x} -> {:?}", vid, did, pid);
    super::flow::aicbsp_sdio_probe(pid);
    Ok(())
}
