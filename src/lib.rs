//! StarryOS wireless crate
//!
//! 整合 LicheeRV-Nano-Build AIC8800 WiFi 内核功能移植：BSP + FDRV
//! - BSP: 固件管理、SDIO 抽象、命令管理、固件下载与启动
//! - FDRV: IPC、控制平面 (WiphyOps)、管理器、私有命令、Vendor
//!
//! 详见 wireless/docs/LicheeRV_WiFi_Kernel_移植分析.md

#![no_std]

extern crate alloc;

pub use bsp;
pub use fdrv;

/// 无线驱动上下文：命令管理器 + 控制平面
/// 平台初始化时创建，可交给 api/syscall 或上层使用
pub struct WirelessDriver<W: fdrv::WiphyOps> {
    pub cmd_mgr: bsp::RwnxCmdMgr,
    pub wiphy: W,
}

impl<W: fdrv::WiphyOps> WirelessDriver<W> {
    pub fn new(wiphy: W) -> Self {
        Self {
            cmd_mgr: bsp::RwnxCmdMgr::new(),
            wiphy,
        }
    }

    pub fn cmd_mgr_mut(&mut self) -> &mut bsp::RwnxCmdMgr {
        &mut self.cmd_mgr
    }

    pub fn wiphy_mut(&mut self) -> &mut W {
        &mut self.wiphy
    }
}

/// 使用占位实现的驱动初始化（无平台 SDIO/平台时可用）
///
/// ## 对应 LicheeRV-Nano-Build 中的哪部分
///
/// 对应 **FDRV 侧“驱动上下文”的创建**，即 `rwnx_cfg80211_init()`（rwnx_main.c 约 5666 行）
/// 里创建 `struct rwnx_hw` 并挂上 wiphy 和 cmd_mgr 的那一步：
///
/// - LicheeRV 流程：BSP `aicbsp_init` → SDIO probe → `aicbsp_driver_fw_init`（固件加载、START_APP）
///   → FDRV 在 probe 路径里调 `rwnx_cfg80211_init()` → `wiphy_new()` 得到 wiphy，
///   `rwnx_hw = wiphy_priv(wiphy)`，`rwnx_hw->cmd_mgr = &sdiodev->cmd_mgr`，再设置 bands、注册 wiphy。
/// - 本函数只做“创建驱动上下文”（我们的 `WirelessDriver` = cmd_mgr + wiphy 抽象），
///   **不**做 SDIO 探测、固件加载、真实 wiphy 注册；wiphy 用 `WiphyOpsStub` 占位。
///
/// ## 预期实现的功能
///
/// 1. **无硬件/平台未就绪时**：系统仍能启动，并持有一个有效的 `WirelessDriver`，
///    上层（如 syscall）可统一通过该句柄调用 wiphy 接口，避免空指针。
/// 2. **占位行为**：scan/connect/start_ap 等通过 `WiphyOpsStub` 返回 `-ENOSYS`，
///    add_interface/del_interface 返回 Ok，便于联调与接口对接。
/// 3. **后续替换**：平台实现 SDIO + 固件加载后，可改为创建 `WirelessDriver<RealWiphyImpl>`，
///    并在初始化链中先执行 BSP 固件下载、再创建并注册该驱动上下文。
pub fn wireless_driver_init_stub() -> WirelessDriver<fdrv::WiphyOpsStub> {
    log::info!(target: "wireless", "wireless: init stub driver (WiphyOpsStub)");
    WirelessDriver::new(fdrv::WiphyOpsStub::default())
}
