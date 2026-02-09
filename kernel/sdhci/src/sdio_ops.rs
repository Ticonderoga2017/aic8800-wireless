//! LicheeRV mmc_io_rw_extended 原样移植（drivers/mmc/core/sdio_ops.c 第 114-141 行）
//!
//! 供 writesb/readsb 经 sdio_io_rw_ext_helper 调用；wireless backend 的 CMD53 arg、blksz、blocks 须与此一致。
//!
//! ## addr 参数语义（与 Linux 一致）
//! Linux 中 `sdio_writesb(func, addr, buf, count)` 的 `addr` 是**函数内偏移**（如 aic8800 的 WR_FIFO=7、RD_FIFO=8），
//! 不是“func<<8|reg”的完整编码。`mmc_io_rw_extended(card, write, fn, addr, ...)` 的 `addr` 即该函数内偏移（0..0x1FFFF）。
//! 因此：若 backend 内部用 `addr = func*0x100 + reg` 编码，调用本模块的 arg 函数时**必须传入函数内偏移**，
//! 即 `addr & 0xFF`（F1/F2 仅用低 8 位），否则 CMD53 会发错地址导致设备无响应或 CFM 超时。

/// 地址合法性检查：与 sdio_ops.c 一致，addr 须在 17 位内
pub const SDIO_IO_RW_EXTENDED_ADDR_MASK: u32 = 0x1_FFFF;

/// 块模式时单次 CMD53 最大块数（SDIO 规范 IO_RW_EXTENDED 的 9 位块数 1..511）
pub const SDIO_IO_RW_EXTENDED_MAX_BLOCKS: u32 = 511;

/// 与 mmc_io_rw_extended 完全一致的 CMD53 argument 计算（块模式）。
///
/// - `write`: true=写，false=读
/// - `fn_num`: 函数号 1..7
/// - `addr`: 函数内地址（17 位，如 F1 0x07）
/// - `incr_addr`: FIFO 固定地址时为 false
/// - `blocks`: 块数 1..511；块模式时 arg 低 9 位 = blocks，bit27 = 1
#[inline(always)]
pub fn mmc_io_rw_extended_arg_block(
    write: bool,
    fn_num: u32,
    addr: u32,
    incr_addr: bool,
    blocks: u32,
) -> u32 {
    let mut arg = if write { 0x8000_0000u32 } else { 0x0000_0000u32 };
    arg |= fn_num << 28;
    arg |= if incr_addr { 0x0400_0000 } else { 0 };
    arg |= (addr & SDIO_IO_RW_EXTENDED_ADDR_MASK) << 9;
    arg |= 0x0800_0000 | (blocks & 0x1FF);
    arg
}

/// 与 mmc_io_rw_extended 完全一致的 CMD53 argument 计算（字节模式）。
///
/// - `addr`: **函数内寄存器地址**（与 Linux mmc_io_rw_extended 一致），例如 F1 的 0x07、0x08，不是 func<<8|reg。
/// - `blocks == 0` 表示字节模式；`blksz == 512` 时 arg 低 9 位为 0，否则为 blksz
#[inline(always)]
pub fn mmc_io_rw_extended_arg_byte(
    write: bool,
    fn_num: u32,
    addr: u32,
    incr_addr: bool,
    blksz: u32,
) -> u32 {
    let mut arg = if write { 0x8000_0000u32 } else { 0x0000_0000u32 };
    arg |= fn_num << 28;
    arg |= if incr_addr { 0x0400_0000 } else { 0 };
    arg |= (addr & SDIO_IO_RW_EXTENDED_ADDR_MASK) << 9;
    if blksz == 512 {
        arg |= 0; // byte mode, 512 bytes: low 9 bits = 0
    } else {
        arg |= blksz & 0x1FF;
    }
    arg
}

/// 与 sdio_ops.c 一致：data.blksz 与 data.blocks。
/// data.blocks = blocks ? blocks : 1（host 驱动假定至少 1 块）
#[inline(always)]
pub fn mmc_io_rw_extended_data_blocks(blocks: u32) -> u32 {
    if blocks == 0 {
        1
    } else {
        blocks
    }
}
