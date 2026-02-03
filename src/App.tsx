import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useTranslation } from "react-i18next";
import { MainWindow } from "./components/MainWindow";
import SettingsWindow, { type SettingsForm } from "./components/SettingsWindow";

// --- Types ---

interface LogEntryPayload {
  line: string;
  level: string;
}

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
  const { t } = useTranslation();
  const [isSettingsWindow, setIsSettingsWindow] = useState(false);
  const [paused, setPaused] = useState(true);
  const [statusText, setStatusText] = useState("");

  useEffect(() => {
    const win = getCurrentWindow();
    if (win.label === "settings") {
      setIsSettingsWindow(true);
    }
  }, []);

  useEffect(() => {
    if (isSettingsWindow) return;

    // Load initial state (effects, paused status)
    invoke<InitialState>("get_initial_state")
      .then((state) => {
        setPaused(state.paused);
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

    return () => {
      unlistenStatus.then((f) => f());
      unlistenConnection.then((f) => f());
    };
  }, [isSettingsWindow]);

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
        onTogglePause={handleTogglePause}
        onOpenSettings={handleOpenSettings}
        onOpenLogFolder={handleOpenLogFolder}
      />
    </div>
  );
}
