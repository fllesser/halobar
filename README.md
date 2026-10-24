# HaloBar

基于 Rust 2024、面向 Halo PixelBar 字幕屏和 QQ 音乐歌词的原型项目。该项目从协议层实现屏幕输出，并优先做两种显示样式：滚动与逐字展示。

## 目标

- 参考 Halo PixelBar / TempoHub 的 HID 屏幕协议，只保留歌词显示所需驱动行为。
- 读取 QQ 音乐本地歌词缓存中的 QRC 逐字时间轴；不请求在线歌词接口。
- 允许以两种模式展示歌词：
  - `scroll`：滚动字幕
  - `typewriter`：逐字展示
- 默认显示模式为 `typewriter`，可通过 `--mode scroll` 切换为滚动字幕。
- 当检测到设备时，向屏幕发送文本；没有设备时，可在终端中预览。

## 运行

```powershell
cd D:\PROJECTS\halobar
cargo build --release
Start-Process .\target\release\halobar.exe
Start-Process .\target\release\halobar.exe -ArgumentList '--mode typewriter'
```

Windows 版本以无控制台后台程序运行。首次正常启动会自动添加当前用户的开机启动项；运行 `Start-Process .\target\release\halobar.exe -ArgumentList '--remove-startup'` 可移除该启动项。运行状态和错误写入 `%LOCALAPPDATA%\HaloBar\halobar.log`。程序启动后会常驻监听 Windows 媒体会话，每 30ms 检查一次播放状态，并补偿读取媒体信息期间经过的时间；显示位置额外提前 150ms，以抵消歌词与音频之间的轻微滞后。检测到 QQ 音乐播放歌曲时，会按播放器提供的实际播放进度同步本地缓存歌词；曲目变化时重新读取缓存，首次未读到后等待 2 秒，只重读一次本地缓存。程序优先查找 `%APPDATA%\Tencent\QQMusic\QQMusicCache\QQMusicLyricNew` 中匹配歌曲的 `_qm.qrc` 缓存，使用 QMC1 XOR 与 QRC 解码；若缓存内容是行级 LRC，则按行级时间显示。程序不请求 QQ 音乐在线接口，因此没有本地缓存时不会显示该歌曲歌词。`typewriter` 优先使用 QRC 逐字时间轴，按屏幕字宽保留占位（英文半角空格、汉字全角空格），并将整句居中显示；最多支持 32 个半角英文字符或 16 个汉字，混排时按 32 列与 48 UTF-8 字节限制分页。文字布局使用设备左对齐并关闭设备端滚动，由程序按固定显示列定位。若歌曲没有 QRC 逐字时间轴，则退回行级时间轴并提示；此时整行按行时间显示，不会伪装成逐字音频同步。暂停、停止或关闭 QQ 音乐后，字幕屏切回设备内置时钟。可通过任务管理器结束后台进程。

运行要求：Windows 10/11、QQ 音乐桌面端已开始播放，并且 QQ 音乐向 Windows 提供了媒体会话信息。需要 QQ 音乐已将该歌曲歌词写入本地缓存；若屏幕未自动切换，请先确认系统音量/媒体面板能显示 QQ 音乐的曲名和播放状态。没有 PixelBar 时程序仍会监听 QQ 音乐，预览信息会写入日志。

## 设备协议

依照参考项目中确认的屏幕 HID 协议，使用以下关键参数：

- VID/PID：`2D99:A106`、`2D99:A160`
- 接口：4
- Usage page：`0xFF14`
- 文本设置命令：`0xE8`
- 屏幕设置命令：`0xEF`
- 每帧最大长度：64 字节
- 文本限制：最多 48 UTF-8 字节（最多 32 个半角字符或 16 个汉字）

当前实现仅保留歌词显示功能，不扩展其他音响状态、场景切换、颜色设置或音量控制。

## 开发说明

```powershell
cargo test
cargo run -- --help
```
