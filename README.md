# FloatDo

> 极致轻量的 Windows 桌面悬浮球待办工具 — 纯 Rust + Win32 API，无 GUI 框架。

Exe **210 KB**，内存占用 **~12 MB**（Working Set）/ **~1.7 MB**（Private）。

对比同等功能的 PyQt6 版本（exe 37 MB，内存 60–150 MB），体积缩小 **175×**，内存降低 **10×**。

## 特性

- 🟣 **悬浮球** — 分层窗口逐像素 GDI 自绘，呼吸光晕动画，可拖拽、贴边吸附
- 📋 **任务窗口** — 全自绘列表（无 Edit/ListBox 控件），圆角卡片 + 复选 + 删除
- 🔔 **系统托盘** — 独立隐藏窗口接收托盘消息，右键菜单（打开悬浮球 / 退出）
- 💾 **数据持久化** — JSON 格式，与 Python 旧版数据完全兼容
- ⚡ **零依赖运行** — 单 exe，无运行时、无框架、无 DLL

## 技术栈

| 组件 | 选择 | 说明 |
|------|------|------|
| 语言 | Rust | 系统级安全 + 零运行时 |
| GUI | Win32 API + GDI | 直接调用 `user32` / `gdi32` |
| Win32 绑定 | `windows` crate 0.58 | 微软官方维护 |
| 序列化 | `serde` + `serde_json` | 任务数据 JSON 存储 |
| 时间 | `chrono` | 任务创建时间戳 |
| 构建工具 | `winres` | 嵌入 .ico 图标到 exe |

## 构建

### 前置要求

- [Rust 工具链](https://rustup.rs)（stable，MSVC target）
- Windows 10+

### 编译

```powershell
cd FloatDo_native
.\build.ps1
# 或直接
cargo build --release
```

产物：`target/release/floatdo_native.exe`（约 210 KB）

### Release 优化

```toml
[profile.release]
opt-level = "z"      # 最小体积
lto = true            # 全量链接优化
codegen-units = 1     # 单编译单元
strip = true          # 剥离调试符号
panic = "abort"       # 禁用 unwind
```

## 项目结构

```
src/
├── main.rs       # 入口：托盘初始化 + 消息循环
├── ball.rs       # 悬浮球窗口（分层窗口 + 逐像素渲染 + 拖拽 + 贴边）
├── taskwin.rs    # 任务窗口（自绘列表 + 文本输入 + 滚轮滚动）
├── gdi.rs        # GDI 辅助（字体/圆角/椭圆/分层推送/托盘图标）
├── store.rs      # 数据层（JSON 读写）
├── tray.rs       # 托盘右键菜单
└── emoji.rs      # ➕ emoji BGRA 像素（twemoji）
```

## 数据格式

任务存储在 exe 同目录的 `tasks.txt`：

```json
[
  {
    "id": "20260815143000000123",
    "title": "完成项目文档",
    "completed": false,
    "created_at": "2026-08-15 14:30:00"
  }
]
```

## 使用

1. 双击 `floatdo_native.exe` 运行
2. 桌面出现紫色悬浮球
3. **左键点击**悬浮球 → 打开任务窗口
4. **右键点击**悬浮球 → 隐藏悬浮球 / 退出程序
5. **拖拽**悬浮球 → 移动位置（松手自动贴边）
6. 任务窗口右上角 **➕** → 输入新任务，回车添加
7. 任务卡片 **圆圈** → 切换完成状态
8. 任务卡片 **×** → 删除任务
9. 系统托盘右键 → 打开悬浮球 / 退出

## 从 PyQt6 迁移

本项目前身是 PyQt6 + Python 实现的悬浮球待办。如果你有旧版 `tasks.txt`，直接放到 exe 同目录即可，数据格式 100% 兼容。

## License

MIT
