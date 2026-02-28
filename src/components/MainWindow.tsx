import { useTranslation } from "react-i18next";
import { BaseButton, cn } from "./ui/Styles";

// --- Types ---

export interface LanPeer {
  device_id: string;
  device_name: string;
  addr: string;
  last_seen: number;
}

interface MainWindowProps {
  paused: boolean;
  statusText: string;
  connectionMode: string;
  lanPeers: LanPeer[];
  onTogglePause: () => void;
  onOpenSettings: () => void;
  onOpenLogFolder: () => void;
}

// --- Component ---

export function MainWindow({
  paused,
  statusText,
  connectionMode,
  lanPeers,
  onTogglePause,
  onOpenSettings,
  onOpenLogFolder,
}: MainWindowProps) {
  const { t } = useTranslation();

  const isLan = connectionMode === "lan";

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
        {/* Mode badge */}
        <div className="ml-auto shrink-0">
          <span
            className={cn(
              "text-[10px] font-semibold uppercase tracking-widest px-2 py-0.5 rounded-full border",
              isLan
                ? "text-violet-300 border-violet-500/30 bg-violet-500/10"
                : "text-sky-300 border-sky-500/30 bg-sky-500/10",
            )}
          >
            {isLan ? t("settings.mode_lan") : t("settings.mode_server")}
          </span>
        </div>
      </div>

      {/* LAN Peers Panel — only shown in LAN mode when not paused */}
      {isLan && !paused && (
        <div className="flex flex-col bg-slate-800/30 rounded-lg border border-slate-700/40 backdrop-blur-md overflow-hidden min-h-0 flex-1">
          <div className="flex items-center justify-between px-3.5 py-2 border-b border-slate-700/40 shrink-0">
            <span className="text-xs font-semibold text-slate-400 uppercase tracking-wider">
              {t("lan.peers_title")}
            </span>
            <span className="text-[10px] text-slate-500 font-medium tabular-nums">
              {lanPeers.length}
            </span>
          </div>

          <div className="flex-1 overflow-y-auto px-3.5 py-2 space-y-1.5">
            {lanPeers.length === 0 ? (
              <div className="flex items-center justify-center py-4">
                <div className="flex items-center gap-2 text-slate-500 text-xs">
                  <div className="w-1.5 h-1.5 rounded-full bg-slate-500 animate-pulse" />
                  <span>{t("lan.no_peers")}</span>
                </div>
              </div>
            ) : (
              lanPeers.map((peer) => (
                <div
                  key={peer.device_id}
                  className="flex items-center gap-2.5 px-2.5 py-2 rounded-md bg-slate-700/20 border border-slate-700/30 hover:bg-slate-700/30 transition-colors"
                >
                  <div className="relative shrink-0">
                    <div className="w-2 h-2 rounded-full bg-emerald-500 shadow-sm shadow-emerald-500/40" />
                  </div>
                  <div className="flex flex-col min-w-0 overflow-hidden">
                    <span className="text-xs font-medium text-slate-200 truncate">
                      {peer.device_name}
                    </span>
                    <span className="text-[10px] text-slate-500 truncate tabular-nums">
                      {peer.addr}
                    </span>
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      )}

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
