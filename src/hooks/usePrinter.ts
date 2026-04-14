import { useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore, PrinterState } from "../stores/appStore";

declare global {
  interface Window {
    __BACKEND_BASE__?: string;
  }
}

export function usePrinter() {
  const { setPrinterState, setConfig, setConnected } = useAppStore();

  const init = useCallback(async () => {
    try {
      const port = await invoke<number>("get_http_port");
      if (port > 0) {
        const base = `http://localhost:${port}`;
        window.__BACKEND_BASE__ = base;
        console.log("[usePrinter] Backend port:", port);
        
        try {
          const resp = await fetch(`${base}/api/config`);
          if (resp.ok) {
            const cfg = await resp.json();
            setConfig({
              host: cfg.host || "",
              serial: cfg.serial || "",
              access_code: "",
            });
          }
        } catch (e) {
          console.log("[usePrinter] fetch config failed:", e);
        }

        startSSE(base);
      }
    } catch (e) {
      console.log("[usePrinter] get_http_port failed:", e);
    }
  }, [setPrinterState, setConfig, setConnected]);

  const startSSE = (base: string) => {
    const src = new EventSource(`${base}/events`);

    src.onmessage = (e) => {
      try {
        const data = JSON.parse(e.data) as PrinterState;
        setPrinterState(data);
        setConnected(data.online);
      } catch {}
    };

    src.onerror = () => {
      console.log("[usePrinter] SSE error, closing");
      src.close();
    };
  };

  useEffect(() => {
    init();
  }, [init]);

  return {};
}
