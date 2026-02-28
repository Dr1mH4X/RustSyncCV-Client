import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useTranslation } from "react-i18next";
import { MainWindow, type LanPeer } from "./components/MainWindow";
import SettingsWindow, { type SettingsForm } from "./components/SettingsWindow";

// --- Types ---

interface ConnectionStatePayload {
  paused: boolean;
  state: string;
}

interface InitialState {
  paused: boolean;
  config: SettingsForm;
  logs: string[];
}

// --- Main App ---

export default function App() {
  const { t, i18n } = useTranslation();
  const [isSettingsWindow, setIsSettingsWindow] = useState(false);
  const [paused, setPaused] = useState(true);
  const [statusText, setStatusText] = useState("");
  const [connectionMode, setConnectionMode] = useState("server");
  const [lanPeers, setLanPeers] = useState<LanPeer[]>([]);

  useEffect(() => {
    const win = getCurrentWindow();
    if (win.label === "settings") {
      setIsSettingsWindow(true);
    }
  }, []);

  useEffect(() => {
    if (isSettingsWindow) return;

    // Load initial state (effects, paused status, connection mode)
    invoke<InitialState>("get_initial_state")
      .then((state) => {
        setPaused(state.paused);
        setConnectionMode(state.config.connection_mode || "server");
        applyLanguage(state.config.language);
        invoke("apply_window_effects", {
          effect: state.config.material_effect,
          theme: state.config.theme_mode,
        });
      })
      .catch((err) => console.error("Failed to load initial state:", err));

    const unlistenStatus = listen<string>("status-update", (event) => {
      setStatusText(event.payload);
    });

    const unlistenConnection = listen<ConnectionStatePayload>(
      "connection-state",
      (event) => {
        setPaused(event.payload.paused);
      },
    );

    const unlistenConfig = listen<SettingsForm>("config-changed", (event) => {
      setConnectionMode(event.payload.connection_mode || "server");
      applyLanguage(event.payload.language);
      invoke("apply_window_effects", {
        effect: event.payload.material_effect,
        theme: event.payload.theme_mode,
      });
      // Clear LAN peers when switching modes so stale data doesn't linger.
      if (event.payload.connection_mode !== "lan") {
        setLanPeers([]);
      }
    });

    const unlistenLanPeers = listen<string>("lan-peers-changed", (event) => {
      try {
        const peers: LanPeer[] = JSON.parse(event.payload);
        setLanPeers(peers);
      } catch {
        setLanPeers([]);
      }
    });

    return () => {
      unlistenStatus.then((f) => f());
      unlistenConnection.then((f) => f());
      unlistenConfig.then((f) => f());
      unlistenLanPeers.then((f) => f());
    };
  }, [isSettingsWindow]);

  const applyLanguage = (mode: string) => {
    if (mode === "system") {
      const sysLang = navigator.language.split("-")[0];
      i18n.changeLanguage(sysLang === "zh" ? "zh" : "en");
    } else {
      i18n.changeLanguage(mode);
    }
  };

  const handleTogglePause = async () => {
    try {
      await invoke("toggle_pause");
    } catch (e) {
      console.error(e);
      setStatusText(`Error: ${e}`);
    }
  };

  const handleOpenLogFolder = async () => {
    try {
      await invoke("open_log_folder");
    } catch (e) {
      console.error(e);
    }
  };

  const handleOpenSettings = async () => {
    try {
      const existing = await WebviewWindow.getByLabel("settings");
      if (existing) {
        await existing.setFocus();
        return;
      }

      const webview = new WebviewWindow("settings", {
        url: "index.html",
        title: t("settings.window_title"),
        width: 400,
        height: 600,
        resizable: false,
        minimizable: false,
        transparent: true,
        decorations: true,
      });

      webview.once("tauri://error", (e) => {
        console.error("Failed to create settings window", e);
        setStatusText(`Failed to create settings window: ${e}`);
      });
    } catch (e) {
      console.error("Error opening settings window:", e);
      setStatusText(`Error opening settings: ${e}`);
    }
  };

  if (isSettingsWindow) {
    return <SettingsWindow />;
  }

  return (
    <div className="relative w-full h-full bg-slate-900/60 text-slate-200 font-sans">
      <MainWindow
        paused={paused}
        statusText={statusText}
        connectionMode={connectionMode}
        lanPeers={lanPeers}
        onTogglePause={handleTogglePause}
        onOpenSettings={handleOpenSettings}
        onOpenLogFolder={handleOpenLogFolder}
      />
    </div>
  );
}
