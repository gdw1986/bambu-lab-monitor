# 🖨 Bambu Lab Monitor

Bambu Lab 3D 打印机实时监控桌面应用。macOS 状态栏图标实时显示打印状态，打印完成时推送系统通知。

## 功能

- 🔥 实时温度监控（喷头 / 热床）
- 📊 打印进度 + 层数显示
- 🎨 AMS 耗材槽位状态
- 🖥 macOS 菜单栏图标 — 颜色随状态变化
  - 🟡 琥珀色 = 打印中
  - 🟢 青绿 = 已完成
  - 🟠 橙色 = 已暂停
  - 🔴 红色 = 错误
- 🔔 打印完成系统通知
- 🌐 同时提供 Web 仪表盘（Flask SSE）

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面端 | Tauri 2 (Rust) + TypeScript + Vite |
| 后端 | Python 3 + Flask + paho-mqtt |
| 通信 | MQTT (TLS 8883) + SSE (Server-Sent Events) |
| 协议 | Bambu Lab LAN Mode MQTT |

## 快速开始

### 1. 启动 Python 后端

```bash
cd backend
python3 -m venv venv
source venv/bin/activate
pip install flask paho-mqtt
```

编辑 `server.py` 修改打印机配置：

```python
PRINTER_IP = "192.168.1.87"      # 你的打印机 IP
ACCESS_CODE = "your_access_code"  # Bambu Studio → 设备 → 局域网访问码
DEVICE_SN   = "your_serial_no"    # 设备序列号
```

```bash
python3 server.py
# Dashboard: http://localhost:5001
```

### 2. 启动 Tauri 桌面端

```bash
# 前置：Rust 工具链
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

npm install
npm run tauri dev
```

### 3. 单独使用 Web 仪表盘

不需要 Tauri，直接运行 Python 后端，浏览器打开 `http://localhost:5001`。

## 项目结构

```
├── backend/
│   ├── server.py              # Flask + MQTT 后端
│   └── templates/index.html   # Web 仪表盘 (备用)
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs             # Tauri 入口，Tray/菜单/窗口
│   │   └── python_backend.rs  # Python 子进程管理 + 状态轮询 + 图标更新
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/
│   └── main.ts
├── index.html                 # 仪表盘前端
├── package.json
└── vite.config.ts
```

## 打印机配置

1. 在 Bambu Studio 中开启 **局域网模式** (LAN Mode)
2. 获取 **访问码** (Access Code)：设备设置 → 网络信息 → 局域网访问码
3. 确认打印机固件版本 ≥ 1.7.0.86

## License

MIT
