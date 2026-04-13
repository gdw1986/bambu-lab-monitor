import "./styles.css";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";

// ── Notifications ─────────────────────────────────────────────────────────────
async function ensurePermission(): Promise<boolean> {
  let granted = await isPermissionGranted();
  if (!granted) {
    const perm = await requestPermission();
    granted = perm === "granted";
  }
  return granted;
}

async function notify(title: string, body: string): Promise<void> {
  try {
    const ok = await ensurePermission();
    if (!ok) { console.warn("No notification permission"); return; }
    await invoke("send_notification", { title, body });
  } catch (e) {
    console.error("notify error:", e);
  }
}

// ── Tauri events → native OS notifications ───────────────────────────────────
export async function initTauriEvents(): Promise<void> {
  // Print finished → OS notification
  await listen<{ job: string }>("print-finished", ({ payload }) => {
    notify("🎉 打印完成", `「${payload.job}」已完成，快去看看吧！`);
  });

  // Backend online/offline status shown in the existing status bar
  await listen("backend-connected", () => {
    const el = document.getElementById("conn-status");
    if (el) el.textContent = "后端已连接";
  });
  await listen("backend-disconnected", () => {
    const el = document.getElementById("conn-status");
    if (el) el.textContent = "后端断开";
  });

  // Printer state changes (tray tooltip managed by Rust; just log)
  await listen("printer-state", ({ payload }) => {
    console.log("[tauri] printer-state", payload);
  });
}

// SSE + render is handled by the inline <script> in index.html
// This module only handles Tauri-native features (notifications, tray events)
initTauriEvents().catch(console.error);
