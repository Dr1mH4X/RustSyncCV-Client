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
}

interface InitialState {
  config: SettingsForm;
}

export default function SettingsWindow() {
  const { t, i18n } = useTranslation();
  const [formData, setFormData] = useState<SettingsForm | null>(null);
  const [error, setError] = useState("");
  // Retrieve language preference from localStorage or default to system
  const [languageMode, setLanguageMode] = useState<string>(
    localStorage.getItem("app-language") || "system",
  );
  const [autoStart, setAutoStart] = useState(false);

  useEffect(() => {
    // Load initial settings from Rust backend
    invoke<InitialState>("get_initial_state")
      .then((state) => {
        setFormData(state.config);
      })
      .catch((err) => console.error("Failed to load settings:", err));

    // Initialize language
    applyLanguage(languageMode);

    isEnabled().then(setAutoStart).catch(console.error);
  }, []);

  const applyLanguage = (mode: string) => {
    if (mode === "system") {
      const sysLang = navigator.language.split("-")[0];
      // Simple fallback mapping if needed, assuming resources has 'en' and 'zh'
      i18n.changeLanguage(sysLang === "zh" ? "zh" : "en");
    } else {
      i18n.changeLanguage(mode);
    }
  };

  const changeLanguage = (lang: string) => {
    setLanguageMode(lang);
    localStorage.setItem("app-language", lang);
    applyLanguage(lang);
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
      // We also apply effects here to ensure immediate feedback if changed
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

  if (!formData) {
    return (
      <div className="flex items-center justify-center h-screen bg-slate-900/60 text-slate-400 text-sm">
        {t("settings.loading")}
      </div>
    );
  }

  return (
    <div className="flex flex-col h-screen bg-slate-900/60 p-6 overflow-y-auto select-none">
      {/* Language Switcher */}
      <div className="space-y-2 mb-6">
        <BaseLabel>{t("settings.language")}</BaseLabel>
        <div className="grid grid-cols-3 gap-2">
          {["system", "en", "zh"].map((lang) => (
            <button
              key={lang}
              onClick={() => changeLanguage(lang)}
              className={cn(
                "px-3 py-2 rounded-lg text-sm transition-all border",
                languageMode === lang
                  ? "bg-blue-500/20 text-blue-100 border-blue-500/30 font-medium"
                  : "bg-slate-800/40 text-slate-500 border-transparent hover:bg-slate-800/60 hover:text-slate-300",
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

      <div className="space-y-5">
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
