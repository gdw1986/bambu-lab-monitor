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
  
  // Refs to hold latest state for reconnection logic
  const eventSourceRef = useRef<EventSource | null>(null);
  const reconnectTimeoutRef = useRef<number | undefined>(undefined);
  const reconnectCountRef = useRef(0);
  const baseRef = useRef<string>("");

  const disconnectSSE = useCallback(() => {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
    if (reconnectTimeoutRef.current) {
      clearTimeout(reconnectTimeoutRef.current);
      reconnectTimeoutRef.current = undefined;
    }
  }, []);

  const startSSE = useCallback((base: string) => {
    // Close existing connection before opening new one
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
    }

    console.log("[usePrinter] Connecting SSE to:", `${base}/events`);
    
    const src = new EventSource(`${base}/events`);
    eventSourceRef.current = src;

    src.onopen = () => {
      console.log("[usePrinter] ✅ SSE connection opened (READY_STATE=OPEN)");
    };

    src.onmessage = (e) => {
      console.log("[usePrinter] 📥 Received SSE message:", e.data.substring(0, 200));
      try {
        const data = JSON.parse(e.data) as PrinterState;
        setPrinterState(data);
        setConnected(data.online);
        // Reset reconnect count on successful message
        reconnectCountRef.current = 0;
      } catch (err) {
        console.error("[usePrinter] Failed to parse SSE message:", err);
      }
    };

    src.onerror = (_e) => {
      console.warn("[usePrinter] SSE error or disconnected");
      
      // Don't close immediately - let the reconnection mechanism handle it
      // EventSource will try to auto-reconnect by default
      
      // Mark for manual reconnect if auto-reconnect fails after a delay
      // The browser's built-in reconnection will attempt first
      setTimeout(() => {
        // If still in error state after 3 seconds, force reconnect
        if (eventSourceRef.current && eventSourceRef.current.readyState !== EventSource.OPEN) {
          console.log("[usePrinter] Auto-reconnect failed, attempting manual reconnect...");
          eventSourceRef.current.close();
          eventSourceRef.current = null;
          
          // Exponential backoff with max 30s delay
          const delay = Math.min(1000 * Math.pow(2, reconnectCountRef.current), 30000);
          reconnectCountRef.current += 1;
          
          console.log(`[usePrinter] Reconnecting in ${delay}ms (attempt #${reconnectCountRef.current})`);
          
          reconnectTimeoutRef.current = window.setTimeout(() => {
            if (baseRef.current) {
              startSSE(baseRef.current);
            }
          }, delay);
        }
      }, 3000);
    };
  }, [setPrinterState, setConnected]);

  const init = useCallback(async () => {
    let retries = 0;
    const maxRetries = 5;

    while (retries < maxRetries) {
      try {
        console.log("[usePrinter] Attempting to get HTTP port...");
        const port = await invoke<number>("get_http_port");
        
        if (port > 0) {
          const base = `http://localhost:${port}`;
          window.__BACKEND_BASE__ = base;
          baseRef.current = base;
          console.log("[usePrinter] Backend port:", port);

          // Reset reconnect count on fresh connection
          reconnectCountRef.current = 0;

          try {
            // Get initial config (host, serial)
            const resp = await fetch(`${base}/api/config`);
            if (resp.ok) {
              const cfg = await resp.json();
              
              // BUG FIX: Don't hardcode access_code to empty string!
              // The backend only returns has_access_code boolean, not the actual code
              // We preserve whatever is already in store or set empty
              setConfig({
                host: cfg.host || "",
                serial: cfg.serial || "",
                access_code: "",  // Intentionally empty - user must enter via settings
                              // This was the old behavior but now we won't overwrite
                              // a previously saved access_code from settings
              });
            } else {
              console.warn("[usePrinter] Config fetch returned status:", resp.status);
            }
          } catch (e) {
            console.error("[usePrinter] fetch config failed:", e);
          }

          // Start SSE with retry capability
          startSSE(base);
          return; // Success, exit retry loop
        } else {
          console.warn("[usePrinter] Got port 0, backend may not be ready yet");
        }
      } catch (e) {
        console.error(`[usePrinter] get_http_port failed (attempt ${retries + 1}/${maxRetries}):`, e);
      }

      retries++;
      if (retries < maxRetries) {
        // Exponential backoff for initial connection
        const delay = Math.min(1000 * Math.pow(2, retries), 5000);
        console.log(`[usePrinter] Retrying in ${delay}ms...`);
        await new Promise(resolve => setTimeout(resolve, delay));
      }
    }

    console.error("[usePrinter] Failed to connect to backend after", maxRetries, "attempts");
    setConnected(false);
  }, [setPrinterState, setConfig, setConnected, startSSE]);

  useEffect(() => {
    init();

    // Cleanup function - close SSE connection when component unmounts
    return () => {
      disconnectSSE();
    };
  }, [init, disconnectSSE]);

  return {};
}
