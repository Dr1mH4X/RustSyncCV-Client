import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
import "./i18n/config";
import { invoke } from "@tauri-apps/api/core";

// Bridge console methods to backend logger
const levels = ["log", "info", "warn", "error", "debug"] as const;
levels.forEach((level) => {
  const original = console[level];
  console[level] = (...args) => {
    original(...args);
    const message = args
      .map((arg) =>
        arg instanceof Error
          ? arg.toString()
          : typeof arg === "object"
            ? JSON.stringify(arg)
            : String(arg),
      )
      .join(" ");
    invoke("frontend_log", { level, message }).catch(() => {
      // Ignore errors if backend is not ready
    });
  };
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
