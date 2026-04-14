interface SettingsModalProps {
  initialConfig: { host?: string; serial?: string; has_access_code?: boolean } | null;
  onSave: (host: string, serial: string, code: string) => Promise<boolean>;
  onClose: () => void;
}

import { useState } from "react";

export default function SettingsModal({ initialConfig, onSave, onClose }: SettingsModalProps) {
  const [host, setHost] = useState(initialConfig?.host || "");
  const [serial, setSerial] = useState(initialConfig?.serial || "");
  const [code, setCode] = useState("");
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);

  const handleSubmit = async () => {
    if (!host || !serial) {
      setError("请填写 IP 和序列号");
      return;
    }
    setError("连接中…");
    setSaving(true);
    const ok = await onSave(host, serial, code);
    setSaving(false);
    if (!ok) {
      setError("保存失败，请重试");
    }
  };

  return (
    <div
      id="cfg-modal"
      style={{
        display: "flex",
        position: "fixed",
        inset: 0,
        zIndex: 100,
        background: "rgba(0,0,0,.7)",
        backdropFilter: "blur(4px)",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <div
        style={{
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "16px",
          padding: "36px",
          width: "min(480px,92vw)",
          boxShadow: "0 24px 64px rgba(0,0,0,.5)",
        }}
      >
        <h2
          style={{
            fontFamily: "var(--mono)",
            fontSize: "15px",
            color: "var(--accent)",
            marginBottom: "24px",
            letterSpacing: ".08em",
            textTransform: "uppercase",
          }}
        >
          ⚙ 打印机配置
        </h2>
        <div style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
          <div>
            <label
              style={{
                fontFamily: "var(--mono)",
                fontSize: "11px",
                color: "var(--muted)",
                textTransform: "uppercase",
                letterSpacing: ".1em",
                display: "block",
                marginBottom: "6px",
              }}
            >
              打印机 IP / 主机名
            </label>
            <input
              id="cfg-host"
              type="text"
              placeholder="192.168.1.100"
              value={host}
              onChange={(e) => setHost(e.target.value)}
              style={{
                width: "100%",
                background: "var(--bg)",
                border: "1px solid var(--border)",
                borderRadius: "8px",
                padding: "10px 14px",
                color: "var(--text)",
                fontFamily: "var(--mono)",
                fontSize: "14px",
                outline: "none",
              }}
            />
          </div>
          <div>
            <label
              style={{
                fontFamily: "var(--mono)",
                fontSize: "11px",
                color: "var(--muted)",
                textTransform: "uppercase",
                letterSpacing: ".1em",
                display: "block",
                marginBottom: "6px",
              }}
            >
              设备序列号
            </label>
            <input
              id="cfg-serial"
              type="text"
              placeholder="CMA7GX123456"
              value={serial}
              onChange={(e) => setSerial(e.target.value)}
              style={{
                width: "100%",
                background: "var(--bg)",
                border: "1px solid var(--border)",
                borderRadius: "8px",
                padding: "10px 14px",
                color: "var(--text)",
                fontFamily: "var(--mono)",
                fontSize: "14px",
                outline: "none",
              }}
            />
          </div>
          <div>
            <label
              style={{
                fontFamily: "var(--mono)",
                fontSize: "11px",
                color: "var(--muted)",
                textTransform: "uppercase",
                letterSpacing: ".1em",
                display: "block",
                marginBottom: "6px",
              }}
            >
              访问码（局域网模式 PIN）
            </label>
            <input
              id="cfg-code"
              type="password"
              placeholder="12345678"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              style={{
                width: "100%",
                background: "var(--bg)",
                border: "1px solid var(--border)",
                borderRadius: "8px",
                padding: "10px 14px",
                color: "var(--text)",
                fontFamily: "var(--mono)",
                fontSize: "14px",
                outline: "none",
              }}
            />
          </div>
        </div>
        <div
          id="cfg-msg"
          style={{
            marginTop: "12px",
            fontFamily: "var(--mono)",
            fontSize: "12px",
            color: "var(--err)",
            minHeight: "18px",
          }}
        >
          {error}
        </div>
        <div
          style={{
            display: "flex",
            gap: "10px",
            marginTop: "20px",
            justifyContent: "flex-end",
          }}
        >
          <button
            onClick={onClose}
            style={{
              background: "transparent",
              border: "1px solid var(--border)",
              borderRadius: "8px",
              padding: "9px 20px",
              color: "var(--muted)",
              fontFamily: "var(--mono)",
              fontSize: "12px",
              cursor: "pointer",
              letterSpacing: ".06em",
            }}
          >
            取消
          </button>
          <button
            onClick={handleSubmit}
            disabled={saving}
            style={{
              background: "var(--accent)",
              border: "none",
              borderRadius: "8px",
              padding: "9px 20px",
              color: "#000",
              fontFamily: "var(--mono)",
              fontSize: "12px",
              fontWeight: "700",
              cursor: saving ? "not-allowed" : "pointer",
              letterSpacing: ".06em",
              opacity: saving ? 0.7 : 1,
            }}
          >
            {saving ? "保存中…" : "连接"}
          </button>
        </div>
      </div>
    </div>
  );
}
