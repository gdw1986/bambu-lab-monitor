import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export interface AmsSlot {
  color: string;
  material: string;
  remaining: number;
}

export interface PrinterState {
  gcode_state: string;
  mode: string;
  action: string;
  progress: number;
  remaining_time: number;
  nozzle_temp: number;
  nozzle_target: number;
  bed_temp: number;
  bed_target: number;
  layer_current: number;
  layer_total: number;
  speed: string;
  filament_type: string;
  live_speed: number;
  light: string;
  online: boolean;
  ams: Record<string, AmsSlot>;
  job_name: string;
  last_update: string;
}

interface AppStore {
  printerState: PrinterState;
  config: {
    host: string;
    serial: string;
    access_code: string;
  } | null;
  isConnected: boolean;
  setPrinterState: (state: Partial<PrinterState>) => void;
  setConfig: (config: { host: string; serial: string; access_code: string } | null) => void;
  setConnected: (connected: boolean) => void;
  saveConfig: (host: string, serial: string, accessCode: string) => Promise<boolean>;
}

export const useAppStore = create<AppStore>((set) => ({
  printerState: {
    gcode_state: "UNKNOWN",
    mode: "unknown",
    action: "unknown",
    progress: 0,
    remaining_time: 0,
    nozzle_temp: 0,
    nozzle_target: 0,
    bed_temp: 0,
    bed_target: 0,
    layer_current: 0,
    layer_total: 0,
    speed: "100",
    filament_type: "",
    ams: {},
    job_name: "",
    live_speed: 0,
    light: "off",
    online: false,
    last_update: "",
  },
  config: null,
  isConnected: false,
  setPrinterState: (state) =>
    set((prev) => ({ printerState: { ...prev.printerState, ...state } })),
  setConfig: (config) => set({ config }),
  setConnected: (connected) => set({ isConnected: connected }),
  saveConfig: async (host: string, serial: string, accessCode: string) => {
    console.log("[saveConfig] Calling Tauri command with host:", host, "serial:", serial, "accessCode:", accessCode);
    try {
      const result = await invoke<boolean>("save_config", {
        host,
        serial,
        accessCode,
      });
      console.log("[saveConfig] Result:", result);
      if (result) {
        set({ config: { host, serial, access_code: accessCode } });
      }
      return result;
    } catch (err) {
      console.error("[saveConfig] Error:", err);
      return false;
    }
  },
}));
