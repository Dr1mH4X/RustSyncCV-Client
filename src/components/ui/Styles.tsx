import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import React from "react";

// --- Utils ---

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

// --- Components ---

export const BaseInput = React.forwardRef<
  HTMLInputElement,
  React.InputHTMLAttributes<HTMLInputElement>
>(({ className, ...props }, ref) => (
  <input
    ref={ref}
    className={cn(
      // Layout & Typography
      "w-full px-3 py-2 text-sm",
      // Borders & Radius (Unified)
      "rounded-lg border border-slate-700/80",
      // Colors & Backgrounds
      "bg-slate-800/50 text-slate-200 placeholder:text-slate-600",
      // States
      "focus:outline-none focus:border-blue-500/50 focus:bg-slate-800 focus:ring-1 focus:ring-blue-500/20",
      "transition-all duration-200",
      className
    )}
    {...props}
  />
));
BaseInput.displayName = "BaseInput";

export const BaseButton = React.forwardRef<
  HTMLButtonElement,
  React.ButtonHTMLAttributes<HTMLButtonElement>
>(({ className, disabled, children, ...props }, ref) => (
  <button
    ref={ref}
    disabled={disabled}
    className={cn(
      // Layout
      "flex items-center justify-center px-4 py-2",
      // Typography
      "text-sm font-medium select-none",
      // Borders & Radius (Unified)
      "rounded-lg border",
      // Default Theme (Neutral)
      "bg-slate-800/80 border-slate-700/60 text-slate-200 shadow-sm",
      // Hover States
      "hover:bg-slate-700/90 hover:border-slate-600",
      // Active/Focus States
      "focus:outline-none focus:ring-2 focus:ring-slate-500/20 active:scale-[0.98]",
      // Disabled State
      "disabled:opacity-50 disabled:cursor-not-allowed disabled:active:scale-100",
      // Transitions
      "transition-all duration-200",
      className
    )}
    {...props}
  >
    {children}
  </button>
));
BaseButton.displayName = "BaseButton";

export const BaseLabel = ({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) => (
  <label
    className={cn(
      "block text-xs font-medium text-slate-400 mb-1.5 ml-0.5 tracking-wide",
      className
    )}
  >
    {children}
  </label>
);
