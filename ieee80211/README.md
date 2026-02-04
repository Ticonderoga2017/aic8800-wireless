# ieee80211 crate

完整复刻 aic8800 依赖的 Linux 内核 **cfg80211** 与 **mac80211** 接口，便于 FDRV 与 LicheeRV `rwnx_main.c` / `rwnx_msg_*.c` 逻辑对齐。

## 模块与 Linux 对应

| 模块       | Linux 位置                 | 说明 |
|------------|----------------------------|------|
| **ieee80211** | include/linux/ieee80211.h | 信道、频段、速率、帧类型（is_beacon/is_assoc_req 等）、WLAN_EID |
| **cfg80211**  | net/cfg80211.h            | wiphy、**Cfg80211Ops**（与 struct cfg80211_ops 一一对应）、ScanRequest、ConnectParams、KeyParams、StationInfo、BeaconData、ChanDef 等 |
| **mac80211**  | net/mac80211.h            | **Hw** trait、Conf、SupportedBand、TxqParams、HtCap、VhtCap |

## 与 aic8800 的对应关系

- **rwnx_cfg80211_init** → 驱动构造实现 `Cfg80211Ops` + `Hw` 的类型并注册。
- **rwnx_cfg80211_scan / connect / disconnect** → `Cfg80211Ops::scan / connect / disconnect`。
- **rwnx_cfg80211_add_key / del_key / get_station** → `Cfg80211Ops::add_key / del_key / get_station`。
- **rwnx_cfg80211_start_ap / change_beacon / stop_ap** → `Cfg80211Ops::start_ap / change_beacon / stop_ap`。
- **ieee80211_scan_completed** → `Hw::report_scan_completed`。
- **struct ieee80211_supported_band** → `SupportedBand`；**struct ieee80211_channel** → `Channel`。

## 使用

- **wireless** 与 **fdrv** 依赖本 crate；fdrv 的 `WiphyOps` 为 cfg80211_ops 的 MVP 子集，完整接口为 `ieee80211::Cfg80211Ops`。
- 类型别名：fdrv 中 `IfaceType` = `Nl80211Iftype`，`InterfaceId` = `Ifindex`。

## 参考文档

- `wireless/docs/cfg80211_ops_对照表.md`
- `wireless/docs/LicheeRV_aic8800对Linux内核子系统依赖清单.md`
