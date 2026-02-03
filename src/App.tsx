import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

// --- Types ---

interface SettingsForm {
  server_url: string;
  token: string;
  username: string;
  password: string;
  max_image_kb: number;
  material_effect: string;
  theme_mode: string;
}

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

// --- Utils ---

function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

// --- Components ---

const Button = ({
  className,
  disabled,
  onClick,
  children,
}: React.ButtonHTMLAttributes<HTMLButtonElement>) => (
  <button
    disabled={disabled}
    onClick={onClick}
    className={cn(
      "px-4 py-1.5 rounded text-sm font-medium transition-colors select-none",
      "bg-slate-700 text-slate-100 hover:bg-slate-600 active:bg-slate-800",
      "disabled:opacity-50 disabled:cursor-not-allowed",
      className,
    )}
  >
    {children}
  </button>
);

const Input = ({
  className,
  ...props
}: React.InputHTMLAttributes<HTMLInputElement>) => (
  <input
    className={cn(
      "w-full px-3 py-1.5 bg-slate-800 border border-slate-600 rounded text-slate-200 text-sm focus:outline-none focus:border-blue-500",
      className,
    )}
    {...props}
  />
);

const Select = ({
  className,
  children,
  ...props
}: React.SelectHTMLAttributes<HTMLSelectElement>) => (
  <select
    className={cn(
      "w-full px-3 py-1.5 bg-slate-800 border border-slate-600 rounded text-slate-200 text-sm focus:outline-none focus:border-blue-500",
      className,
    )}
    {...props}
  >
    {children}
  </select>
);

const Label = ({ children }: { children: React.ReactNode }) => (
  <label className="block text-sm text-slate-300 mb-1">{children}</label>
);

// --- Main App ---

export default function App() {
  // State
  const [paused, setPaused] = useState(true);
  const [statusText, setStatusText] = useState("准备启动");
  const [logs, setLogs] = useState<string[]>([]);
  const [settingsVisible, setSettingsVisible] = useState(false);
  const [settingsError, setSettingsError] = useState("");

  // Form State
  const [formData, setFormData] = useState<SettingsForm>({
    server_url: "",
    token: "",
    username: "",
    password: "",
    max_image_kb: 512,
    material_effect: "mica",
    theme_mode: "system",
  });

  const logsEndRef = useRef<HTMLDivElement>(null);

  // Auto-scroll logs
  useEffect(() => {
    logsEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  // Initial Load & Event Listeners
  useEffect(() => {
    // Load initial state
    invoke<InitialState>("get_initial_state")
      .then((state) => {
        setPaused(state.paused);
        setFormData(state.config);
        setLogs(state.logs);
        // Apply initial window effects
        invoke("apply_window_effects", {
          effect: state.config.material_effect,
          theme: state.config.theme_mode,
        });
      })
      .catch((err) => console.error("Failed to load initial state:", err));

    // Listeners
    const unlistenStatus = listen<string>("status-update", (event) => {
      setStatusText(event.payload);
    });

    const unlistenConnection = listen<ConnectionStatePayload>(
      "connection-state",
      (event) => {
        setPaused(event.payload.paused);
      },
    );

    const unlistenLog = listen<LogEntryPayload>("log-entry", (event) => {
      setLogs((prev) => {
        const newLogs = [...prev, event.payload.line];
        if (newLogs.length > 2000) return newLogs.slice(newLogs.length - 2000);
        return newLogs;
      });
    });

    return () => {
      unlistenStatus.then((f) => f());
      unlistenConnection.then((f) => f());
      unlistenLog.then((f) => f());
    };
  }, []);

  // Handlers
  const handleTogglePause = async () => {
    try {
      await invoke("toggle_pause");
    } catch (e) {
      console.error(e);
      setStatusText(`Error: ${e}`);
    }
  };

  const handleClearLogs = async () => {
    setLogs([]);
    await invoke("clear_logs");
  };

  const handleOpenSettings = () => {
    setSettingsVisible(true);
    setSettingsError("");
    // Re-fetch current config to ensure form is up to date?
    // Actually we keep formData in sync mostly, but let's re-fetch to be safe or just use current.
    // Let's use current state.
  };

  const handleCloseSettings = () => {
    setSettingsVisible(false);
    setSettingsError("");
    // Ideally we revert changes if not saved, but for simplicity we keep state.
    // To strictly follow "Cancel", we should reload initial state.
    // Let's reload from backend just in case.
    invoke<InitialState>("get_initial_state").then((state) => {
      setFormData(state.config);
    });
  };

  const handleSaveSettings = async () => {
    try {
      await invoke("save_settings", { form: formData });

      // Apply effects immediately
      await invoke("apply_window_effects", {
        effect: formData.material_effect,
        theme: formData.theme_mode,
      });

      setSettingsVisible(false);
      setStatusText("Configuration saved, reloading...");
    } catch (e: any) {
      setSettingsError(String(e));
    }
  };

  const handleFormChange = (
    field: keyof SettingsForm,
    value: string | number | boolean,
  ) => {
    setFormData((prev) => ({ ...prev, [field]: value }));
  };

  return (
    <div className="relative w-full h-full flex flex-col overflow-hidden bg-slate-900/60 text-slate-200 font-sans">
      {/* Main Content */}
      <div className="flex flex-col p-4 gap-3 h-full">
        {/* Status Line */}
        <div className="text-sm">
          <span className="text-slate-400 mr-1">状态:</span>
          <span
            className={cn(
              "font-semibold",
              paused ? "text-red-400" : "text-green-400",
            )}
          >
            {paused ? "已暂停" : "运行中"}
          </span>
          <span className="mx-2 text-slate-600">|</span>
          <span className="text-slate-300">{statusText}</span>
        </div>

        {/* Buttons */}
        <div className="flex gap-3">
          <Button onClick={handleTogglePause}>
            {paused ? "恢复同步" : "暂停同步"}
          </Button>
          <Button onClick={handleOpenSettings}>设置</Button>
          <Button onClick={handleClearLogs}>清空日志</Button>
        </div>

        {/* Log Area */}
        <div className="flex-1 flex flex-col min-h-0">
          <div className="text-xs font-bold text-slate-400 mb-1 uppercase tracking-wider">
            日志
          </div>
          <div className="flex-1 bg-slate-900/50 rounded-lg border border-slate-700/50 overflow-hidden relative">
            <div className="absolute inset-0 overflow-y-auto p-2 font-mono text-xs space-y-1">
              {logs.map((line, i) => (
                <div key={i} className="text-slate-400 break-words">
                  {line}
                </div>
              ))}
              <div ref={logsEndRef} />
            </div>
          </div>
        </div>
      </div>

      {/* Settings Modal Overlay */}
      {settingsVisible && (
        <div className="absolute inset-0 bg-black/60 z-50 flex items-center justify-center p-4 backdrop-blur-sm">
          <div className="bg-slate-800 w-full max-w-md max-h-full rounded-xl shadow-2xl flex flex-col border border-slate-700 overflow-hidden">
            {/* Header */}
            <div className="px-5 py-4 border-b border-slate-700 flex justify-between items-center bg-slate-800/50">
              <h2 className="text-lg font-semibold text-slate-100">设置</h2>
              <Button
                onClick={handleCloseSettings}
                className="text-xs py-1 px-2 bg-transparent hover:bg-slate-700 border border-slate-600"
              >
                关闭
              </Button>
            </div>

            {/* Scrollable Form */}
            <div className="p-5 overflow-y-auto space-y-4">
              <div>
                <Label>服务器地址</Label>
                <Input
                  value={formData.server_url}
                  onChange={(e) =>
                    handleFormChange("server_url", e.target.value)
                  }
                  placeholder="wss://example.com/..."
                />
              </div>

              <div>
                <Label>认证 Token (可选)</Label>
                <Input
                  type="password"
                  value={formData.token}
                  onChange={(e) => handleFormChange("token", e.target.value)}
                />
              </div>

              <div>
                <Label>用户名 (Token 留空时必填)</Label>
                <Input
                  value={formData.username}
                  onChange={(e) => handleFormChange("username", e.target.value)}
                />
              </div>

              <div>
                <Label>密码</Label>
                <Input
                  type="password"
                  value={formData.password}
                  onChange={(e) => handleFormChange("password", e.target.value)}
                />
              </div>

              <div>
                <Label>图片大小上限 (KB)</Label>
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    min={32}
                    max={8192}
                    value={formData.max_image_kb}
                    onChange={(e) =>
                      handleFormChange(
                        "max_image_kb",
                        parseInt(e.target.value) || 512,
                      )
                    }
                  />
                </div>
              </div>

              <div>
                <Label>主题</Label>
                <Select
                  value={formData.theme_mode}
                  onChange={(e) =>
                    handleFormChange("theme_mode", e.target.value)
                  }
                >
                  <option value="system">跟随系统</option>
                  <option value="dark">深色</option>
                  <option value="light">浅色</option>
                </Select>
              </div>

              <div className="flex items-center gap-2 pt-2">
                <input
                  type="checkbox"
                  id="acrylic"
                  checked={formData.material_effect === "acrylic"}
                  onChange={(e) =>
                    handleFormChange(
                      "material_effect",
                      e.target.checked ? "acrylic" : "mica",
                    )
                  }
                  className="rounded bg-slate-700 border-slate-600 text-blue-500 focus:ring-offset-slate-800"
                />
                <label htmlFor="acrylic" className="text-sm text-slate-300">
                  使用 Acrylic 背景 (关闭为 Mica)
                </label>
              </div>

              {settingsError && (
                <div className="text-red-400 text-sm bg-red-900/20 p-2 rounded border border-red-900/50">
                  {settingsError}
                </div>
              )}
            </div>

            {/* Footer */}
            <div className="p-4 border-t border-slate-700 flex justify-end gap-3 bg-slate-800/50">
              <Button
                onClick={handleCloseSettings}
                className="bg-transparent border border-slate-600 hover:bg-slate-700"
              >
                取消
              </Button>
              <Button
                onClick={handleSaveSettings}
                className="bg-blue-600 hover:bg-blue-500 text-white"
                disabled={!formData.server_url}
              >
                保存
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
