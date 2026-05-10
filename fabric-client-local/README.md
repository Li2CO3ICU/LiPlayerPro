# Fabric 客户端 + 本地曲库最小集成骨架

本目录提供 Java 侧最小骨架，用于对接本仓库新增的 Rust C ABI（`liplayer_*`）。

## 当前模式

- 客户端播放
- 本地曲库扫描/播放
- 不依赖服务端

## 对接步骤

1. 在 Fabric 客户端初始化时加载 native 动态库（`liplayerpro_core`）。
2. 创建播放器句柄并调用 `liplayer_scan_local_library` 扫描本地音乐目录。
3. 通过命令/按键/HUD 操作调用：
   - `liplayer_play_track_at`
   - `liplayer_pause` / `liplayer_resume` / `liplayer_stop`
   - `liplayer_elapsed_millis` / `liplayer_state`
4. 客户端退出时调用 `liplayer_destroy` 释放句柄。

## 后续扩展

- 服务端同步：增加播放控制包广播（当前不启用）。
- 跨平台打包：按平台分发 `.so/.dll/.dylib` 并在客户端按 OS 加载。
