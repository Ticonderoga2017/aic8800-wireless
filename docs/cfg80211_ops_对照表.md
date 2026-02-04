# cfg80211_ops 与 StarryOS 无线控制平面对照表

本文档列出 LicheeRV-Nano-Build aic8800_fdrv 中实现的 **cfg80211_ops** 回调及其作用，供在 StarryOS 下设计“无线控制平面”trait 时一一对应。

---

## 1. 接口与虚拟接口

| cfg80211_ops 成员 | 作用 | StarryOS 建议 |
|-------------------|------|----------------|
| add_virtual_intf | 创建虚拟接口（STA/AP/P2P 等） | WiphyOps::add_interface(type) → 返回 interface_id |
| del_virtual_intf | 删除虚拟接口 | WiphyOps::del_interface(interface_id) |
| change_virtual_intf | 修改接口类型 | WiphyOps::change_interface(interface_id, type) |
| start_p2p_device | 启动 P2P 设备 | 可选：WiphyOps::start_p2p_device(interface_id) |
| stop_p2p_device | 停止 P2P 设备 | 可选：WiphyOps::stop_p2p_device(interface_id) |

---

## 2. 扫描与连接

| cfg80211_ops 成员 | 作用 | StarryOS 建议 |
|-------------------|------|----------------|
| scan | 触发扫描，结果通过 cfg80211_scan_done 上报 | WiphyOps::scan(interface_id, params) → 异步结果回调 |
| connect | 连接到指定 BSS | WiphyOps::connect(interface_id, bssid, ssid, auth) → 异步结果 |
| disconnect | 断开连接 | WiphyOps::disconnect(interface_id) |
| sched_scan_start | 后台周期扫描（可选） | 可选：WiphyOps::sched_scan_start / sched_scan_stop |

---

## 3. 密钥与站管理

| cfg80211_ops 成员 | 作用 | StarryOS 建议 |
|-------------------|------|----------------|
| add_key | 添加 PTK/GTK 等 | WiphyOps::add_key(interface_id, key_index, key_data) |
| get_key | 查询密钥状态 | WiphyOps::get_key(interface_id, key_index) |
| del_key | 删除密钥 | WiphyOps::del_key(interface_id, key_index) |
| set_default_key | 设置默认数据密钥 | WiphyOps::set_default_key(interface_id, key_index) |
| set_default_mgmt_key | 设置默认管理帧密钥 | WiphyOps::set_default_mgmt_key(interface_id, key_index) |
| add_station | AP 模式下添加关联站 | WiphyOps::add_station(interface_id, mac, params) |
| del_station | 删除/踢掉站 | WiphyOps::del_station(interface_id, mac, reason) |
| change_station | 修改站参数 | WiphyOps::change_station(interface_id, mac, params) |
| get_station | 查询站信息（RSSI、速率等） | WiphyOps::get_station(interface_id, mac) → StationInfo |
| dump_station | 遍历所有站（可选） | 可选：WiphyOps::dump_stations(interface_id, callback) |

---

## 4. 管理帧与 AP

| cfg80211_ops 成员 | 作用 | StarryOS 建议 |
|-------------------|------|----------------|
| mgmt_tx | 发送管理帧（如 Action） | WiphyOps::mgmt_tx(interface_id, buf, freq) |
| start_ap | 启动 AP（beacon、信道等） | WiphyOps::start_ap(interface_id, beacon, channel) |
| change_beacon | 更新 beacon | WiphyOps::change_beacon(interface_id, beacon) |
| stop_ap | 关闭 AP | WiphyOps::stop_ap(interface_id) |
| probe_client | AP 下发 probe 给指定客户端 | WiphyOps::probe_client(interface_id, mac) |
| set_monitor_channel | Monitor 模式设信道 | 可选：WiphyOps::set_monitor_channel(interface_id, channel) |

---

## 5. 信道与功率

| cfg80211_ops 成员 | 作用 | StarryOS 建议 |
|-------------------|------|----------------|
| set_wiphy_params | 设置 wiphy 参数（如 RTS/分片阈值） | WiphyOps::set_wiphy_params(params) |
| set_txq_params | 设置 TX 队列参数 | 可选：WiphyOps::set_txq_params(interface_id, params) |
| set_tx_power | 设置发射功率 | WiphyOps::set_tx_power(interface_id, power) |
| get_tx_power | 查询发射功率 | WiphyOps::get_tx_power(interface_id) |
| set_power_mgmt | 设置省电模式 | WiphyOps::set_power_mgmt(interface_id, enabled) |
| get_channel | 查询当前信道 | WiphyOps::get_channel(interface_id) |
| remain_on_channel | P2P/侦听保持信道 | 可选：WiphyOps::remain_on_channel / cancel_remain_on_channel |
| dump_survey | 信道占用统计 | 可选：WiphyOps::dump_survey(interface_id, callback) |

---

## 6. DFS、雷达与监管

| cfg80211_ops 成员 | 作用 | StarryOS 建议 |
|-------------------|------|----------------|
| start_radar_detection | 启动雷达检测（DFS） | 可选：WiphyOps::start_radar_detection(interface_id, channel) |
| reg_notifier | 监管域变更（alpha2） | 可选：WiphyOps::reg_notifier(alpha2) → 内部下发 chan_config |

---

## 7. 漫游、CQM、FT

| cfg80211_ops 成员 | 作用 | StarryOS 建议 |
|-------------------|------|----------------|
| update_ft_ies | 更新 802.11r FT IEs | 可选：WiphyOps::update_ft_ies(interface_id, ies) |
| set_cqm_rssi_config | RSSI 门限与通知 | 可选：WiphyOps::set_cqm_rssi_config(interface_id, threshold) |
| channel_switch | 信道切换（AP） | 可选：WiphyOps::channel_switch(interface_id, params) |

---

## 8. TDLS

| cfg80211_ops 成员 | 作用 | StarryOS 建议 |
|-------------------|------|----------------|
| tdls_channel_switch | TDLS 信道切换 | 可选：WiphyOps::tdls_channel_switch |
| tdls_cancel_channel_switch | 取消 TDLS 信道切换 | 可选：WiphyOps::tdls_cancel_channel_switch |
| tdls_mgmt | TDLS 管理帧 | 可选：WiphyOps::tdls_mgmt |
| tdls_oper | TDLS 建立/拆除 | 可选：WiphyOps::tdls_oper |

---

## 9. 其它

| cfg80211_ops 成员 | 作用 | StarryOS 建议 |
|-------------------|------|----------------|
| change_bss | 修改 BSS 参数 | 可选：WiphyOps::change_bss(interface_id, params) |
| external_auth | 外部认证（WPA3 SAE 等） | 可选：WiphyOps::external_auth(interface_id, params) |

---

## 10. Mesh（可选，CONFIG_MESH）

| cfg80211_ops 成员 | 作用 | StarryOS 建议 |
|-------------------|------|----------------|
| get_station / dump_station | 同上，Mesh 下用于邻居 | 同上 |
| add_mpath / del_mpath / change_mpath / get_mpath / dump_mpath | Mesh 路径 | 可选：WiphyOps::mesh_* |
| get_mpp / dump_mpp | Mesh Proxy | 可选 |
| get_mesh_config / update_mesh_config | Mesh 配置 | 可选：WiphyOps::get_mesh_config / update_mesh_config |
| join_mesh / leave_mesh | 加入/离开 Mesh | 可选：WiphyOps::join_mesh / leave_mesh |

---

## 11. 最小可行集（MVP）

移植到 StarryOS 时，建议先实现以下子集，即可支撑“扫描 + 连接 + 简单 AP”：

- **接口**：add_virtual_intf、del_virtual_intf、change_virtual_intf
- **扫描与连接**：scan、connect、disconnect
- **密钥**：add_key、del_key、set_default_key
- **AP**：start_ap、change_beacon、stop_ap
- **站**：add_station、del_station、get_station
- **功率与信道**：set_tx_power、get_tx_power、get_channel、set_power_mgmt
- **管理帧**：mgmt_tx
- **wiphy 参数**：set_wiphy_params

其余（P2P、TDLS、Mesh、DFS、sched_scan、external_auth 等）可按需后续补齐。
