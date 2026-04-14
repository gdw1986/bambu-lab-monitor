import { useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore, PrinterState } from "../stores/appStore";

declare global {
  interface Window {
    __BACKEND_BASE__?: string;
  }
}

export function usePrinter() {
  const { setPrinterState, setConfig, setConnected } = useAppStore();

  const eventSourceRef = useRef<EventSource | null>(null);
  const reconnectTimerRef = useRef<number | undefined>(undefined);
  const reconnectCountRef = useRef(0);
  const baseRef = useRef<string>("");

  const clearReconnect = useCallback(() => {
    if (reconnectTimerRef.current !== undefined) {
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = undefined;
    }
  }, []);

  const stopSSE = useCallback(() => {
    clearReconnect();
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
  }, [clearReconnect]);

  const startSSE = useCallback((base: string) => {
    // Always stop existing before starting new
    stopSSE();

    console.log("[usePrinter] Connecting SSE to:", `${base}/events`);
    const src = new EventSource(`${base}/events`);
    eventSourceRef.current = src;

    src.onopen = () => {
      console.log("[usePrinter] ✅ SSE opened");
      reconnectCountRef.current = 0;
    };

    src.onmessage = (e) => {
      console.log("[usePrinter] 📥 SSE message:", e.data.substring(0, 200));
      try {
        const data = JSON.parse(e.data) as PrinterState;
        setPrinterState(data);
        setConnected(data.online);
        reconnectCountRef.current = 0;
      } catch (err) {
        console.error("[usePrinter] Parse error:", err);
      }
    };

    src.onerror = () => {
      console.warn("[usePrinter] ⚠️ SSE error/disconnected");
      // Stop current connection — prevents browser's auto-reconnect from fighting our logic
      src.close();
      eventSourceRef.current = null;

      // Cancel any pending reconnect timer first
      clearReconnect();

      const delay = Math.min(1000 * Math.pow(2, reconnectCountRef.current), 30000);
      reconnectCountRef.current += 1;

      console.log(`[usePrinter] Retry #${reconnectCountRef.current} in ${delay}ms`);
      reconnectTimerRef.current = window.setTimeout(() => {
        reconnectTimerRef.current = undefined;
        if (baseRef.current) {
          startSSE(baseRef.current);
        }
      }, delay);
    };
  }, [stopSSE, clearReconnect, setPrinterState, setConnected]);

  const init = useCallback(async () => {
    let retries = 0;
    const maxRetries = 10;

    while (retries < maxRetries) {
      try {
        const port = await invoke<number>("get_http_port");
        console.log("[usePrinter] get_http_port =", port);

        if (port > 0) {
          const base = `http://localhost:${port}`;
          window.__BACKEND_BASE__ = base;
          baseRef.current = base;

          // Load config from backend
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
            console.warn("[usePrinter] /api/config failed:", e);
          }

          startSSE(base);
          return;
        }
      } catch (e) {
        console.error(`[usePrinter] attempt ${retries + 1}/${maxRetries} failed:`, e);
      }

      retries++;
      const delay = Math.min(1000 * Math.pow(2, retries), 8000);
      console.log(`[usePrinter] Waiting ${delay}ms before retry...`);
      await new Promise(resolve => setTimeout(resolve, delay));
    }

    console.error("[usePrinter] All retries exhausted");
    setConnected(false);
  }, [setPrinterState, setConfig, setConnected, startSSE]);

  useEffect(() => {
    init();
    return () => stopSSE();
  }, [init, stopSSE]);

  return {};
}
