//! # mmc — SDIO/MMC 总线抽象
//!
//! 对应 Linux 内核 **MMC 子系统**（drivers/mmc/core、include/linux/mmc），
//! 实现 aic8800 所依赖的所有功能模块接口，便于 BSP 与 FDRV 与 LicheeRV 逻辑对齐。
//!
//! ## 模块与 Linux 对应
//!
//! | 模块      | Linux 位置                    | 说明 |
//! |-----------|-------------------------------|------|
//! | types     | mmc/card.h, host.h, sdio_ids.h | SdioDeviceId、MmcIos、MmcBusWidth、sdio_class |
//! | host      | mmc/host.h, core 占用          | MmcHost：claim_host、set_ios |
//! | card      | mmc/card.h                    | MmcCard（RCA） |
//! | sdio_func | mmc/sdio_func.h, sdio_ops.c   | SdioFunc：readb/writeb、readsb/writesb、set_block_size、enable_func 等 |
//! | driver    | sdio_func.h sdio_driver       | SdioDriver：id_table、probe、remove |
//!
//! ## aic8800 依赖清单（对照文档见 wireless/docs/aic8800_MMC调用与wireless_逐项对照.md）
//!
//! - sdio_claim_host / sdio_release_host → MmcHost::claim_host（Guard drop）
//! - sdio_readb / sdio_writeb → SdioFunc::readb / writeb
//! - sdio_readsb / sdio_writesb → SdioFunc::readsb / writesb
//! - sdio_set_block_size → SdioFunc::set_block_size
//! - sdio_enable_func / sdio_disable_func → SdioFunc::enable_func / disable_func
//! - sdio_claim_irq / sdio_release_irq → 软中断替代时可为空实现
//! - host->ops->set_ios(clock, bus_width) → MmcHost::set_ios
//! - sdio_register_driver / id_table / probe → SdioDriver

#![no_std]

pub mod card;
pub mod cccr;
pub mod driver;
pub mod host;
pub mod sdio_func;
pub mod types;

pub use card::{MmcCard, Rca};
pub use driver::{
    sdio_driver_remove, sdio_register_driver, sdio_try_probe, sdio_unregister_driver, SdioDriver,
};
pub use host::{MmcHost, with_host_claimed};
pub use sdio_func::{default_func_blocksize, SdioFunc, SdioIrqHandler};
pub use types::{
    sdio_class, MmcBusWidth, MmcIos, SdioDeviceId, SDIO_ANY_ID, SDIO_ANY_ID_U16,
    SDIO_FUNC_BLOCKSIZE_DEFAULT,
};
pub use cccr::{
    sdio_disable_function, sdio_enable_function, CccrAccess, DelayMs,
    sdio_f0_reg, SDIO_CCCR_IO_ENABLE, SDIO_CCCR_IO_READY,
};
