import { useState } from "react";
import { useAppStore } from "./stores/appStore";
import Header from "./components/Header";
import ProgressHero from "./components/ProgressHero";
import TempGauges from "./components/TempGauges";
import Stats from "./components/Stats";
import AmsGrid from "./components/AmsGrid";
import SettingsModal from "./components/SettingsModal";
import { usePrinter } from "./hooks/usePrinter";

export default function App() {
  const { printerState, config, saveConfig } = useAppStore();
  usePrinter();
  const [showSettings, setShowSettings] = useState(true);

  return (
    <div className="wrap">
      <Header onSettingsClick={() => setShowSettings(true)} />
      
      {!printerState.online && config && (config.host || config.serial) && (
        <div className="offline-banner show">
          ⚠ 连接中断，正在等待打印机重连…
        </div>
      )}

      <ProgressHero state={printerState} />
      <TempGauges state={printerState} />
      <Stats state={printerState} />
      <AmsGrid ams={printerState.ams} />

      {showSettings && (
        <SettingsModal
          initialConfig={config}
          onSave={async (host: string, serial: string, code: string) => {
            const ok = await saveConfig(host, serial, code);
            if (ok) {
              setShowSettings(false);
            }
            return ok;
          }}
          onClose={() => setShowSettings(false)}
        />
      )}
    </div>
  );
}
