//! MMC 主机抽象
//!
//! 对应 Linux：include/linux/mmc/host.h、drivers/mmc/core 中的 host 占用与 set_ios。
//! aic8800 使用：sdio_claim_host/sdio_release_host（通过 func 访问 host）、host->ops->set_ios(clock)。

/// MMC 主机控制器抽象
///
/// 对应 Linux mmc_host：提供 host 占用（串行化）与可选的总线配置。
/// 在 Linux 上 sdio_claim_host(func) 实际 claim 的是 func->card->host。
/// `claim_host` 返回的 Guard 应在使用后 drop，以释放占用；调用方应使用 `with_host_claimed` 避免忘记释放。
pub trait MmcHost {
    /// 占用期间持有的 guard，实现方应在 drop 时释放锁（与 sdio_release_host 语义一致）
    type Guard;

    /// 占用 host（在发起 CMD52/CMD53 前调用，与 sdio_claim_host 语义一致）
    fn claim_host(&self) -> Self::Guard;

    /// 配置接口时钟与总线宽度（对应 host->ops->set_ios(host, &host->ios)）
    /// 默认实现不做任何事；平台可实现 FREQ_SEL、HOST_CTRL1 等。
    fn set_ios(&self, _ios: &crate::types::MmcIos) -> Result<(), i32> {
        Ok(())
    }
}

/// 在持 host 时执行闭包（Guard 在闭包返回后 drop，避免忘记 release）
pub fn with_host_claimed<H: MmcHost, R, F>(host: &H, f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = host.claim_host();
    f()
}
