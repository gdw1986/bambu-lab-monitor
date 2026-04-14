# 🖨 Bambu Lab Monitor

Bambu Lab 3D 打印机实时监控桌面应用。托盘图标实时显示打印进度，浮窗显示进度环与详细信息，打印完成时推送系统通知。零依赖，单文件运行。

## 功能

- 🔥 实时温度监控（喷头 / 热床）
- 📊 打印进度 + 层数显示
- 🎨 AMS 耗材槽位状态
- 🖥 **托盘图标** — 颜色随状态变化，五档进度显示（0/25/50/75/100%）
  - 🟡 琥珀色 = 打印中
  - 🟢 青绿 = 已完成
  - 🟠 橙色 = 已暂停
  - 🔴 红色 = 错误/空闲
- 🪟 **桌面浮窗** — 圆形进度环，实时显示百分比，轮播喷头温度、热床温度、剩余时间、任务名称
  - 置顶显示，可拖拽移动
  - 打印中自动显示，可手动隐藏
  - 透明圆形窗口，不遮挡桌面内容
- 🔔 打印完成系统通知

## 截图

| 主界面 | 浮窗 |
|--------|------|
| （待补充） | 圆形进度环 + 信息轮播 |

## 下载

前往 [Releases](https://github.com/gdw1986/bambu-lab-monitor/releases) 页面下载对应平台的安装包。

| 平台 | 架构 | 格式 |
|------|------|------|
| Windows | x64 | `.exe` / `.msi` |
| macOS | x64 / ARM64 | `.dmg` |

## 配置

启动前设置以下环境变量：

| 变量 | 说明 | 示例 |
|------|------|------|
| `BAMBU_IP` | 打印机 IP 地址 | `192.168.1.87` |
| `BAMBU_SN` | 设备序列号 | `XX0XX0XX0XX0` |
| `BAMBU_CODE` | 局域网访问码 | `your_access_code` |

**获取方式：**
1. Bambu Studio → 设备 → 开启**局域网模式**
2. 网络信息 → 复制**局域网访问码**和**序列号**

### macOS / Linux

```bash
export BAMBU_IP=192.168.1.87
export BAMBU_SN=XX0XX0XX0XX0
export BAMBU_CODE=your_access_code
./BambuMonitor
```

### Windows (PowerShell)

```powershell
$env:BAMBU_IP="192.168.1.87"
$env:BAMBU_SN="XX0XX0XX0XX0"
$env:BAMBU_CODE="your_access_code"
.\BambuMonitor.exe
```

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面端 | Tauri 2 (Rust) + TypeScript + Vite |
| 后端 | 纯 Rust（axum + rumqttc） |
| 通信 | MQTT (TLS 8883) |
| 实时推送 | SSE (Server-Sent Events) |
| 协议 | Bambu Lab LAN Mode MQTT |

## 项目结构

```
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs             # Tauri 入口，Tray/菜单/浮窗
│   │   ├── server.rs          # Rust HTTP + MQTT 客户端
│   │   └── tray_icon.rs       # 托盘图标渲染（含五档进度）
│   ├── capabilities/          # Tauri 权限配置
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/
│   └── main.ts
├── floating.html              # 桌面浮窗（SVG 进度环）
├── index.html                 # 前端界面
├── package.json
└── vite.config.ts
```

## 版本历史

### v0.4.0
- ✨ 新增桌面浮窗：圆形进度环，实时显示打印百分比
- 📊 浮窗信息轮播：喷头温度、热床温度、剩余时间、任务名称
- 🖼 托盘图标五档进度（0/25/50/75/100%），竹叶元素设计
- 🔧 修复 MQTT TLS 连接问题（签名算法 + TLS 1.2 兼容）
- 🔧 修复 SSE 广播通道断连问题
- 🎨 温度仪表字体加粗加大

### v0.3.0
- 初始版本：主界面温度监控 + 进度显示 + AMS 耗材状态

## 开发

### 环境要求

- Node.js 20+
- Rust 1.70+
- Windows: Visual Studio Build Tools
- macOS: Xcode Command Line Tools

### 本地运行

```bash
npm install
npm run tauri dev
```

### 构建

```bash
npm run tauri build
```

## License

MIT
