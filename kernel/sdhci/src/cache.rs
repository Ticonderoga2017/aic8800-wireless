//! DMA 与 CPU 的 cache 一致性
//!
//! 当 DMA 缓冲区位于可缓存内存时，须保证：
//! - **DMA 写（CPU → 设备）前**：flush（clean）dcache，使设备能读到 CPU 刚写入的数据；
//! - **DMA 读（设备 → CPU）后**：invalidate dcache，使 CPU 能读到设备刚写入内存的数据。
//!
//! SG2002 使用 T-Head C906 核，**不支持 Zicbom**。采用 T-Head 自定义指令（与 LicheeRV
//! U-Boot `arch/riscv/cpu/generic/cache.c`、Linux arch/riscv/mm/cacheflush.c 一致）：
//! - dcache.cpa rs1（clean）：`.long 0x0295000b`，写前用；
//! - dcache.cipa rs1（clean+invalidate）：`.long 0x02b5000b`，读后用；
//! - sync.s：`.long 0x0190000b`。
//! 见 SG200X TRM（C906）、LicheeRV-docs。

/// 常见 RISC-V dcache line 大小（字节），用于按行 flush/invalidate
#[cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]
const CACHE_LINE_SIZE: usize = 64;

/// DMA 写前：将 [ptr, ptr+size) 的脏 cache 行写回内存，保证设备 DMA 读到最新数据。
///
/// 在 CPU 写完 DMA 缓冲区、启动 DMA 写（CMD53 写）之前调用。
#[inline]
pub fn dma_flush_before_write(ptr: *const u8, size: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }
    #[cfg(target_arch = "riscv64")]
    {
        riscv64_dcache_clean_range(ptr, size);
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        let _ = (ptr, size);
    }
}

/// DMA 读后：将 [ptr, ptr+size) 的 cache 行失效，保证 CPU 读到设备 DMA 写入内存的数据。
///
/// 在 DMA 读（CMD53 读）完成、CPU 读 DMA 缓冲区之前调用。
#[inline]
pub fn dma_invalidate_after_read(ptr: *const u8, size: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }
    #[cfg(target_arch = "riscv64")]
    {
        riscv64_dcache_invalidate_range(ptr, size);
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        let _ = (ptr, size);
    }
}

// T-Head C906 dcache 指令（SG2002 可用，LicheeRV U-Boot arch/riscv/cpu/generic/cache.c）
// dcache.cpa a0 (clean); dcache.cipa a0 (clean+invalidate); sync.s — 编码固定，用 .long 发出

#[cfg(target_arch = "riscv64")]
#[inline(never)]
fn riscv64_dcache_clean_range(ptr: *const u8, size: usize) {
    let start = ptr as usize;
    let end = start + size;
    let line = CACHE_LINE_SIZE;
    let start_aligned = start & !(line - 1);
    let mut addr = start_aligned;
    while addr < end {
        unsafe {
            // a0 = 地址，然后执行 dcache.cpa a0（0x0295000b）
            core::arch::asm!(
                "mv a0, {0}",
                ".long 0x0295000b",
                in(reg) addr,
                options(nostack, preserves_flags)
            );
        }
        addr += line;
    }
    unsafe { core::arch::asm!(".long 0x0190000b", options(nostack, preserves_flags)) }; // sync.s
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

#[cfg(target_arch = "riscv64")]
#[inline(never)]
fn riscv64_dcache_invalidate_range(ptr: *const u8, size: usize) {
    let start = ptr as usize;
    let end = start + size;
    let line = CACHE_LINE_SIZE;
    let start_aligned = start & !(line - 1);
    let mut addr = start_aligned;
    while addr < end {
        unsafe {
            // a0 = 地址，然后执行 dcache.cipa a0（0x02b5000b）
            core::arch::asm!(
                "mv a0, {0}",
                ".long 0x02b5000b",
                in(reg) addr,
                options(nostack, preserves_flags)
            );
        }
        addr += line;
    }
    unsafe { core::arch::asm!(".long 0x0190000b", options(nostack, preserves_flags)) }; // sync.s
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}
