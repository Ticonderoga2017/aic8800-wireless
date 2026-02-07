//! SDIO 驱动抽象与注册
//!
//! 对应 Linux：sdio_register_driver、sdio_driver、probe/remove。
//! aic8800 使用 id_table 匹配，probe 时完成 func_init、bus_init 等。
//! 无内核时由平台在枚举到卡后调用 sdio_try_probe，本模块根据已注册驱动做 id_table 匹配并调用 probe。

use spin::Mutex;

use crate::sdio_func::SdioFunc;
use crate::types::SdioDeviceId;

/// SDIO 驱动接口
///
/// 对应 Linux struct sdio_driver：id_table + probe + remove。
/// 无内核时由平台在枚举到卡后调用 mmc::sdio_try_probe，内部根据 id_table 匹配并调用 probe。
pub trait SdioDriver {
    /// 驱动名
    fn name(&self) -> &'static str;

    /// 设备 ID 表（任一匹配则 probe）
    fn id_table(&self) -> &[SdioDeviceId];

    /// 探测：func 已 enable、block_size 已设，驱动完成芯片相关初始化
    fn probe(&self, func: &dyn SdioFunc) -> Result<(), i32>;

    /// 移除时清理
    fn remove(&self, func: &dyn SdioFunc) -> Result<(), i32> {
        let _ = func;
        Ok(())
    }

    /// 是否匹配给定设备 ID
    fn matches(&self, id: &SdioDeviceId) -> bool {
        self.id_table()
            .iter()
            .any(|tid| tid.matches(id))
    }
}

/// 已注册的 SDIO 驱动（单驱动槽，与 LicheeRV 单 aicbsp_sdio_driver 一致）
/// 要求 Sync 以便可放入 static Mutex。
static REGISTERED: Mutex<Option<&'static (dyn SdioDriver + Sync)>> = Mutex::new(None);

/// 注册 SDIO 驱动（对应 Linux sdio_register_driver）。
/// 若已有驱动登记则返回 Err(-16) (EBUSY)。
pub fn sdio_register_driver(driver: &'static (dyn SdioDriver + Sync)) -> Result<(), i32> {
    let mut guard = REGISTERED.lock();
    if guard.is_some() {
        return Err(-16); // EBUSY
    }
    *guard = Some(driver);
    Ok(())
}

/// 反注册当前 SDIO 驱动（对应 Linux sdio_unregister_driver）。
/// 应在 remove 完成后调用；本函数仅清除登记，不调用 remove。
pub fn sdio_unregister_driver() {
    let mut guard = REGISTERED.lock();
    *guard = None;
}

/// 若已注册驱动且 id_table 匹配 func，则调用 driver.probe(func)（对应内核枚举到卡后按 id_table 调用 probe）。
/// 无驱动登记返回 Err(-19)(ENODEV)；不匹配返回 Err(-19)；probe 失败返回驱动返回的 err。
/// 在调用 probe 前释放注册表锁，避免驱动内回调导致死锁。
pub fn sdio_try_probe(func: &dyn SdioFunc) -> Result<(), i32> {
    let id = func.device_id();
    let driver_opt = {
        let guard = REGISTERED.lock();
        (*guard).filter(|d| d.matches(&id))
    };
    let driver = driver_opt.ok_or(-19)?; // ENODEV or no match
    driver.probe(func)
}

/// 若已注册驱动，则调用 driver.remove(func)（对应 remove 回调）。
/// 无驱动登记返回 Ok(())（无操作）；remove 失败返回驱动返回的 err。
pub fn sdio_driver_remove(func: &dyn SdioFunc) -> Result<(), i32> {
    let driver = {
        let guard = REGISTERED.lock();
        *guard
    };
    if let Some(d) = driver {
        d.remove(func)
    } else {
        Ok(())
    }
}
