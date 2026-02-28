import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { useTranslation } from "react-i18next";
import { BaseInput, BaseLabel, cn } from "./ui/Styles";

// --- Types ---

export interface SettingsForm {
  server_url: string;
  token: string;
  username: string;
  password: string;
  max_image_kb: number;
  material_effect: string;
  theme_mode: string;
  language: string;
  connection_mode: string;
  lan_device_name: string;
  close_behavior: string;
}

interface InitialState {
  config: SettingsForm;
}

export default function SettingsWindow() {
  const { t, i18n } = useTranslation();
  const [formData, setFormData] = useState<SettingsForm | null>(null);
  const [error, setError] = useState("");
  const [autoStart, setAutoStart] = useState(false);

  useEffect(() => {
    invoke<InitialState>("get_initial_state")
      .then((state) => {
        setFormData({
          ...state.config,
          connection_mode: state.config.connection_mode || "server",
          lan_device_name: state.config.lan_device_name || "",
          close_behavior: state.config.close_behavior || "minimize_to_tray",
        });
        applyLanguage(state.config.language);
      })
      .catch((err) => console.error("Failed to load settings:", err));

    isEnabled().then(setAutoStart).catch(console.error);
  }, []);

  const applyLanguage = (mode: string) => {
    if (mode === "system") {
      const sysLang = navigator.language.split("-")[0];
      i18n.changeLanguage(sysLang === "zh" ? "zh" : "en");
    } else {
      i18n.changeLanguage(mode);
    }
  };

  const changeLanguage = (lang: string) => {
    applyLanguage(lang);
    if (formData) {
      const newData = { ...formData, language: lang };
      setFormData(newData);
      handleSave(newData);
    }
  };

  const toggleAutoStart = async () => {
    try {
      if (autoStart) {
        await disable();
      } else {
        await enable();
      }
      setAutoStart(!autoStart);
    } catch (e) {
      console.error(e);
      setError("Failed to toggle auto-start");
    }
  };

  const handleSave = async (data: SettingsForm) => {
    try {
      await invoke("save_settings", { form: data });
      await invoke("apply_window_effects", {
        effect: data.material_effect,
        theme: data.theme_mode,
      });
      setError("");
    } catch (e: any) {
      console.error(e);
      setError(String(e));
    }
  };

  const handleBlur = () => {
    if (formData) {
      handleSave(formData);
    }
  };

  const handleChange = (field: keyof SettingsForm, value: string | number) => {
    setFormData((prev) => {
      if (!prev) return null;
      return { ...prev, [field]: value };
    });
  };

  const handleModeChange = (mode: string) => {
    if (formData) {
      const newData = { ...formData, connection_mode: mode };
      setFormData(newData);
      handleSave(newData);
    }
  };

  const handleCloseBehaviorChange = (behavior: string) => {
    if (formData) {
      const newData = { ...formData, close_behavior: behavior };
      setFormData(newData);
      handleSave(newData);
    }
  };

  const handleAutoDetectHostname = async () => {
    try {
      const hostname = await invoke<string>("get_hostname");
      if (formData) {
        const newData = { ...formData, lan_device_name: hostname };
        setFormData(newData);
        handleSave(newData);
      }
    } catch (e: any) {
      console.error(e);
      setError(String(e));
    }
  };

  if (!formData) {
    return (
      <div className="flex items-center justify-center h-screen bg-slate-900/60 text-slate-200 text-sm">
        {t("settings.loading")}
      </div>
    );
  }

  const isLan = formData.connection_mode === "lan";

  return (
    <div className="flex flex-col h-screen bg-slate-900/60 p-6 overflow-y-auto select-none text-slate-200">
      {/* Language Switcher */}
      <div className="space-y-2 mb-6">
        <BaseLabel>{t("settings.language")}</BaseLabel>
        <div className="grid grid-cols-3 gap-2">
          {["system", "zh", "en"].map((lang) => (
            <button
              key={lang}
              onClick={() => changeLanguage(lang)}
              className={cn(
                "px-3 py-2 rounded-lg text-sm transition-all border",
                formData.language === lang
                  ? "bg-blue-500/20 text-blue-100 border-blue-500/30 font-medium"
                  : "bg-slate-800/40 text-slate-400 border-transparent hover:bg-slate-800/60 hover:text-slate-200",
              )}
            >
              {lang === "system"
                ? t("settings.system")
                : lang === "en"
                  ? "English"
                  : "中文"}
            </button>
          ))}
        </div>
      </div>

      {/* Auto-start toggle */}
      <div className="flex items-center justify-between mb-6">
        <BaseLabel>{t("settings.auto_start")}</BaseLabel>
        <button
          onClick={toggleAutoStart}
          className={cn(
            "w-11 h-6 rounded-full transition-colors relative focus:outline-none",
            autoStart ? "bg-emerald-500" : "bg-slate-700",
          )}
        >
          <div
            className={cn(
              "absolute top-1 left-1 bg-white w-4 h-4 rounded-full transition-transform",
              autoStart ? "translate-x-5" : "translate-x-0",
            )}
          />
        </button>
      </div>

      {/* Close Behavior Selector */}
      <div className="space-y-2 mb-6">
        <BaseLabel>{t("settings.close_behavior")}</BaseLabel>
        <div className="grid grid-cols-3 gap-2">
          {(
            [
              {
                key: "minimize_to_tray",
                label: t("settings.close_behavior_minimize_to_tray"),
              },
              { key: "minimize", label: t("settings.close_behavior_minimize") },
              { key: "quit", label: t("settings.close_behavior_quit") },
            ] as const
          ).map(({ key, label }) => (
            <button
              key={key}
              onClick={() => handleCloseBehaviorChange(key)}
              className={cn(
                "px-2 py-2 rounded-lg text-xs transition-all border leading-tight",
                formData.close_behavior === key
                  ? "bg-blue-500/20 text-blue-100 border-blue-500/30 font-medium"
                  : "bg-slate-800/40 text-slate-400 border-transparent hover:bg-slate-800/60 hover:text-slate-200",
              )}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {/* Connection Mode Selector */}
      <div className="space-y-2 mb-6">
        <BaseLabel>{t("settings.connection_mode")}</BaseLabel>
        <div className="grid grid-cols-2 gap-2">
          {(["server", "lan"] as const).map((mode) => (
            <button
              key={mode}
              onClick={() => handleModeChange(mode)}
              className={cn(
                "px-3 py-2.5 rounded-lg text-sm transition-all border",
                formData.connection_mode === mode
                  ? "bg-blue-500/20 text-blue-100 border-blue-500/30 font-medium"
                  : "bg-slate-800/40 text-slate-400 border-transparent hover:bg-slate-800/60 hover:text-slate-200",
              )}
            >
              {mode === "server"
                ? t("settings.mode_server")
                : t("settings.mode_lan")}
            </button>
          ))}
        </div>
      </div>

      <div className="space-y-5">
        {/* ── Server Mode Settings ─────────────────────────────────────── */}
        {!isLan && (
          <>
            <div className="text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">
              {t("settings.server_section_title")}
            </div>

            {/* Server URL */}
            <div>
              <BaseLabel>{t("settings.server_url")}</BaseLabel>
              <BaseInput
                value={formData.server_url}
                onChange={(e) => handleChange("server_url", e.target.value)}
                onBlur={handleBlur}
                placeholder="wss://..."
              />
            </div>

            {/* Token */}
            <div>
              <BaseLabel>{t("settings.token")}</BaseLabel>
              <BaseInput
                type="password"
                value={formData.token}
                onChange={(e) => handleChange("token", e.target.value)}
                onBlur={handleBlur}
              />
            </div>

            {/* Username */}
            <div>
              <BaseLabel>{t("settings.username")}</BaseLabel>
              <BaseInput
                value={formData.username}
                onChange={(e) => handleChange("username", e.target.value)}
                onBlur={handleBlur}
              />
            </div>

            {/* Password */}
            <div>
              <BaseLabel>{t("settings.password")}</BaseLabel>
              <BaseInput
                type="password"
                value={formData.password}
                onChange={(e) => handleChange("password", e.target.value)}
                onBlur={handleBlur}
              />
            </div>
          </>
        )}

        {/* ── LAN Mode Settings ───────────────────────────────────────── */}
        {isLan && (
          <>
            <div className="text-xs font-semibold text-slate-500 uppercase tracking-wider mb-1">
              {t("settings.lan_section_title")}
            </div>

            {/* Security Warning */}
            <div className="flex gap-2.5 p-3 rounded-lg bg-amber-500/10 border border-amber-500/20 text-amber-200/90 text-xs leading-relaxed">
              <span className="shrink-0 text-sm">⚠</span>
              <span>{t("lan.security_warning")}</span>
            </div>

            {/* Device Name with Auto Detect button */}
            <div>
              <BaseLabel>{t("settings.lan_device_name")}</BaseLabel>
              <div className="flex gap-2">
                <BaseInput
                  value={formData.lan_device_name}
                  onChange={(e) =>
                    handleChange("lan_device_name", e.target.value)
                  }
                  onBlur={handleBlur}
                  placeholder={t("settings.lan_device_name_placeholder")}
                  className="flex-1"
                />
                <button
                  onClick={handleAutoDetectHostname}
                  className={cn(
                    "shrink-0 px-3 py-2 rounded-lg text-xs font-medium transition-all border",
                    "bg-slate-800/80 border-slate-700/60 text-slate-300",
                    "hover:bg-slate-700/90 hover:border-slate-600 hover:text-slate-100",
                    "active:scale-[0.97]",
                  )}
                >
                  {t("settings.lan_auto_detect")}
                </button>
              </div>
            </div>
          </>
        )}

        {/* ── Shared Settings ─────────────────────────────────────────── */}

        {/* Max Image Size */}
        <div>
          <BaseLabel>{t("settings.max_image_size")}</BaseLabel>
          <BaseInput
            type="number"
            min={1}
            max={524288}
            value={formData.max_image_kb}
            onChange={(e) =>
              handleChange("max_image_kb", parseInt(e.target.value) || 512)
            }
            onBlur={handleBlur}
          />
        </div>
      </div>

      {error && (
        <div className="mt-6 p-3 rounded bg-red-500/10 border border-red-500/20 text-red-200 text-xs">
          {error}
        </div>
      )}
    </div>
  );
}
