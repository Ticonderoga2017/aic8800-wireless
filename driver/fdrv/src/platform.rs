//! FDRV 平台初始化与全局 Wiphy 存储
//!
//! 对应 LicheeRV rwnx_cfg80211_init 后持有的 rwnx_hw（wiphy + cmd_mgr 引用）。
//! 本实现将 WiphyOpsImpl 存于静态，命令通过 BSP 的 with_cmd_mgr 发送。
//! 初始化时创建“wlan0”等价接口（add_interface(Station)），与 rwnx_interface_add("wlan%d", STATION) 对应。

use spin::Mutex;

use crate::e2a_dispatch::e2a_indication_handler;
use crate::wiphy::{IfaceType, InterfaceId, WiphyOps};
use crate::wiphy_impl::WiphyOpsImpl;

static PLATFORM_WIPHY: Mutex<Option<WiphyOpsImpl>> = Mutex::new(None);
/// 首个接口（wlan0 等价）的 inst_nbr，由 platform_init 时 add_interface(Station) 得到
static DEFAULT_IFACE_ID: Mutex<Option<InterfaceId>> = Mutex::new(None);

/// 平台初始化：创建 WiphyOpsImpl、注册 E2A 回调，并创建 wlan0 等价接口（MM_START + MM_ADD_IF）。
///
/// 应在 BSP 完成 aicbsp_set_subsys(Wifi, On) 之后调用（即 CMD_MGR 与 SDIO 已就绪）。
/// 与 LicheeRV rwnx_cfg80211_init + rwnx_interface_add("wlan%d", STATION) 及首 VIF up（rwnx_open）对应。
pub fn platform_init() -> Result<(), i32> {
    let mut guard = PLATFORM_WIPHY.lock();
    if guard.is_some() {
        log::info!(target: "wireless::fdrv", "platform_init: already inited");
        return Ok(());
    }
    guard.replace(WiphyOpsImpl::new());
    drop(guard);
    bsp::set_e2a_indication_cb(Some(e2a_indication_handler));
    log::info!(target: "wireless::fdrv", "platform_init: WiphyOpsImpl stored, E2A callback registered");

    // 与 LicheeRV rwnx_interface_add("wlan%d", NL80211_IFTYPE_STATION) + 首 VIF up 一致：创建并 up 一个 STA 接口
    let iface_id = match with_wiphy_mut(|w| w.add_interface(IfaceType::Station)) {
        Some(Ok(id)) => id,
        Some(Err(e)) => return Err(e),
        None => return Err(-5),
    };
    *DEFAULT_IFACE_ID.lock() = Some(iface_id);
    log::info!(target: "wireless::fdrv", "platform_init: wlan0 equivalent created, iface_id={}", iface_id);
    Ok(())
}

/// 返回初始化时创建的默认接口 id（wlan0 等价），供 scan/connect 使用。
pub fn default_interface_id() -> Option<InterfaceId> {
    *DEFAULT_IFACE_ID.lock()
}

/// 在持有全局 Wiphy 时执行闭包，用于 scan/connect/add_interface 等。
///
/// 若平台未初始化则返回 None。
pub fn with_wiphy_mut<R, F: FnOnce(&mut WiphyOpsImpl) -> R>(f: F) -> Option<R> {
    let mut guard = PLATFORM_WIPHY.lock();
    guard.as_mut().map(f)
}
