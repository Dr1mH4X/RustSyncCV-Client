import { useTranslation } from "react-i18next";
import { BaseButton, cn } from "./ui/Styles";

// --- Types ---

interface MainWindowProps {
  paused: boolean;
  statusText: string;
  onTogglePause: () => void;
  onOpenSettings: () => void;
  onOpenLogFolder: () => void;
}

// --- Component ---

export function MainWindow({
  paused,
  statusText,
  onTogglePause,
  onOpenSettings,
  onOpenLogFolder,
}: MainWindowProps) {
  const { t } = useTranslation();

  return (
    <div className="flex flex-col h-full w-full p-6 gap-6 overflow-hidden select-none">
      {/* Status Card */}
      <div className="flex items-center gap-3 bg-slate-800/40 p-3.5 rounded-lg border border-slate-700/50 backdrop-blur-md shadow-sm shrink-0">
        <div className="relative shrink-0">
          <div
            className={cn(
              "w-3 h-3 rounded-full shadow-lg transition-colors duration-300",
              paused
                ? "bg-slate-400 shadow-slate-500/20"
                : "bg-emerald-500 shadow-emerald-500/40",
            )}
          />
          {!paused && (
            <div className="absolute inset-0 bg-emerald-400 rounded-full animate-ping opacity-75" />
          )}
        </div>
        <div className="flex flex-col min-w-0 overflow-hidden">
          <span
            className={cn(
              "text-xs font-bold uppercase tracking-wider mb-0.5",
              paused ? "text-slate-400" : "text-emerald-400",
            )}
          >
            {paused ? t("status.paused") : t("status.running")}
          </span>
          <span
            className="text-xs text-slate-300 truncate font-medium"
            title={statusText}
          >
            {statusText}
          </span>
        </div>
      </div>

      {/* Primary Actions */}
      <div className="flex flex-col gap-3 shrink-0">
        <BaseButton
          onClick={onTogglePause}
          className={cn(
            "w-full py-3 text-base border transition-all duration-300",
            paused
              ? "bg-emerald-500/10 text-emerald-400 border-emerald-500/20 hover:bg-emerald-500/20 hover:border-emerald-500/30"
              : "bg-amber-500/10 text-amber-400 border-amber-500/20 hover:bg-amber-500/20 hover:border-amber-500/30",
          )}
        >
          {paused ? t("action.resume") : t("action.pause")}
        </BaseButton>
        <div className="grid grid-cols-2 gap-3">
          <BaseButton onClick={onOpenSettings}>
            {t("action.settings")}
          </BaseButton>
          <BaseButton onClick={onOpenLogFolder}>
            {t("action.open_logs")}
          </BaseButton>
        </div>
      </div>
    </div>
  );
}
