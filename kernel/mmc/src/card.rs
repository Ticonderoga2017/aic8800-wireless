//! MMC 卡抽象
//!
//! 对应 Linux：include/linux/mmc/card.h。枚举完成后得到 card，其上挂载 sdio_func。
//! aic8800 通过 func 访问，card 主要用于“卡存在”与 RCA。

/// 相对卡地址（SD 规范 CMD3 返回）
pub type Rca = u16;

/// MMC/SD 卡信息（枚举结果）
///
/// 对应 Linux mmc_card 的简化视图：无 hotplug 时仅需 RCA 与“已选中”状态。
#[derive(Debug, Clone, Copy)]
pub struct MmcCard {
    /// 相对卡地址
    pub rca: Rca,
}

impl MmcCard {
    pub const fn new(rca: Rca) -> Self {
        Self { rca }
    }
}
