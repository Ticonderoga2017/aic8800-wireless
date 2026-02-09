//! mmc crate 在 BSP 上的实现
//!
//! 实现 mmc::MmcHost（claim_host = SDIO_DEVICE 锁、set_ios）与 mmc::SdioFunc（F1/F2 的 readb/writeb/readsb/writesb 等）。
//! enable_func/disable_func 委托 mmc::cccr 的规范实现。
//! 静态 AicBspSdioDriver 实现 mmc::SdioDriver，在 aicbsp_init 时注册、aicbsp_sdio_exit 时反注册。

use mmc::{
    sdio_disable_function, sdio_enable_function, CccrAccess, DelayMs, MmcBusWidth, MmcHost, MmcIos,
    SdioDeviceId, SdioDriver, SdioFunc, sdio_class,
};
use spin::MutexGuard;

use super::flow;
use super::ops::Aic8800Sdio;
use super::types::sdio_ids;

const FUNC1_BASE: u32 = 0x100;
const FUNC2_BASE: u32 = 0x200;
const SDIO_FUNC_BLOCKSIZE: u16 = 512;

/// BSP 的 MMC 主机：claim_host 即持 SDIO_DEVICE 锁，与 with_sdio 语义一致
#[derive(Debug, Clone, Copy)]
pub struct BspSdioHost;

impl MmcHost for BspSdioHost {
    type Guard = MutexGuard<'static, Option<Aic8800Sdio>>;

    fn claim_host(&self) -> Self::Guard {
        flow::lock_sdio_device()
    }

    /// 与 LicheeRV sdhci_set_ios 顺序一致：先 set_clock(ios->clock)（0=关卡时钟），再 set_bus_width(ios->bus_width)。
    fn set_ios(&self, ios: &MmcIos) -> Result<(), i32> {
        let guard = flow::lock_sdio_device();
        let sdio = guard.as_ref().ok_or(-19)?;
        let four_bit = matches!(ios.bus_width, MmcBusWidth::FourBit);
        sdio.host().set_clock(ios.clock);
        sdio.host().set_bus_width(four_bit);
        Ok(())
    }
}

/// 通过 Aic8800Sdio 的 host 访问 F0（CCCR），供 mmc::sdio_enable_function 使用
struct CccrViaSdioHost<'a>(&'a Aic8800Sdio);
impl CccrAccess for CccrViaSdioHost<'_> {
    fn read_f0(&self, reg: u8) -> Result<u8, i32> {
        self.0.host().read_byte(reg as u32)
    }
    fn write_f0(&self, reg: u8, val: u8) -> Result<(), i32> {
        self.0.host().write_byte(reg as u32, val)
    }
}

/// BSP 提供的延时，供 mmc::sdio_enable_function 轮询 IO_READY 使用
struct BspDelay;
impl DelayMs for BspDelay {
    fn delay_ms(&mut self, ms: u32) {
        crate::sync::delay_spin_ms(ms);
    }
}

/// SDIO Function 的 BSP 实现：持有对 Aic8800Sdio 的引用与 function 号（1 或 2）
#[derive(Clone, Copy)]
pub struct BspSdioFuncRef<'a> {
    pub sdio: &'a Aic8800Sdio,
    pub num: u8,
}

impl<'a> BspSdioFuncRef<'a> {
    pub fn new(sdio: &'a Aic8800Sdio, num: u8) -> Self {
        Self { sdio, num }
    }

    fn base(&self) -> u32 {
        if self.num == 1 {
            FUNC1_BASE
        } else {
            FUNC2_BASE
        }
    }
}

impl SdioFunc for BspSdioFuncRef<'_> {
    fn num(&self) -> u8 {
        self.num
    }

    fn vendor(&self) -> u16 {
        super::cis::product_id_to_vid_did(self.sdio.product_id()).0
    }

    fn device(&self) -> u16 {
        super::cis::product_id_to_vid_did(self.sdio.product_id()).1
    }

    fn class(&self) -> u8 {
        sdio_class::WLAN
    }

    fn cur_blksize(&self) -> u16 {
        SDIO_FUNC_BLOCKSIZE
    }

    fn readb(&self, addr: u32) -> Result<u8, i32> {
        self.sdio.host().read_byte(self.base() + addr)
    }

    fn writeb(&self, addr: u32, b: u8) -> Result<(), i32> {
        self.sdio.host().write_byte(self.base() + addr, b)
    }

    fn read_f0(&self, reg: u8) -> Result<u8, i32> {
        self.sdio.host().read_byte(reg as u32)
    }

    fn write_f0(&self, reg: u8, val: u8) -> Result<(), i32> {
        self.sdio.host().write_byte(reg as u32, val)
    }

    fn readsb(&self, addr: u32, buf: &mut [u8]) -> Result<usize, i32> {
        let base = self.base() + addr;
        let n = buf.len();
        if n == 0 {
            return Ok(0);
        }
        self.sdio.host().read_block(base, buf)?;
        Ok(n)
    }

    fn writesb(&self, addr: u32, buf: &[u8]) -> Result<usize, i32> {
        let base = self.base() + addr;
        let n = buf.len();
        if n == 0 {
            return Ok(0);
        }
        self.sdio.host().write_block(base, buf)?;
        Ok(n)
    }

    fn set_block_size(&self, blksz: u16) -> Result<(), i32> {
        self.sdio.host().set_block_size(self.num as u32, blksz)
    }

    fn enable_func(&self) -> Result<(), i32> {
        let cccr = CccrViaSdioHost(self.sdio);
        let mut delay = BspDelay;
        sdio_enable_function(&cccr, self.num, 100, &mut delay)
    }

    fn disable_func(&self) -> Result<(), i32> {
        let cccr = CccrViaSdioHost(self.sdio);
        sdio_disable_function(&cccr, self.num)
    }
}

/// AIC BSP SDIO 驱动（对应 LicheeRV aicbsp_sdio_driver）
/// 在 aicbsp_init 时 mmc::sdio_register_driver；枚举并创建设备后 mmc::sdio_try_probe；aicbsp_sdio_exit 时 remove + unregister。
static AIC_BSP_SDIO_ID_TABLE: &[SdioDeviceId] = &[
    SdioDeviceId::new(sdio_class::WLAN, sdio_ids::VENDOR_AIC8801, sdio_ids::DEVICE_AIC8801),
    SdioDeviceId::new(sdio_class::WLAN, sdio_ids::VENDOR_AIC8800DC, sdio_ids::DEVICE_AIC8800DC),
    SdioDeviceId::new(sdio_class::WLAN, sdio_ids::VENDOR_AIC8800D80, sdio_ids::DEVICE_AIC8800D80),
    SdioDeviceId::new(sdio_class::WLAN, sdio_ids::VENDOR_AIC8800D80X2, sdio_ids::DEVICE_AIC8800D80X2),
];

struct AicBspSdioDriver;

impl SdioDriver for AicBspSdioDriver {
    fn name(&self) -> &'static str {
        "aicbsp_sdio"
    }

    fn id_table(&self) -> &[SdioDeviceId] {
        AIC_BSP_SDIO_ID_TABLE
    }

    fn probe(&self, func: &dyn SdioFunc) -> Result<(), i32> {
        // 实际 probe（F1/F2 使能、建 Aic8800Sdio、起线程）已在 aicbsp_sdio_init 内完成；
        // 此处仅满足 mmc 的 try_probe 调用链，无额外操作。
        let _ = func;
        Ok(())
    }

    fn remove(&self, func: &dyn SdioFunc) -> Result<(), i32> {
        let _ = func;
        Ok(())
    }
}

/// 供 aicbsp_init / aicbsp_sdio_exit 调用的 mmc 注册/反注册
pub fn register_aicbsp_sdio_driver() -> Result<(), i32> {
    mmc::sdio_register_driver(&AicBspSdioDriver)
}

pub fn unregister_aicbsp_sdio_driver() {
    mmc::sdio_unregister_driver();
}
