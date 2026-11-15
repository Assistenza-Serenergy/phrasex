import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Settings } from "./api";

/** Applies the chosen theme to the page (CSS reads data-theme) and to the window title bar. */
export function applyTheme(theme: Settings["theme"] | undefined) {
  const t = theme === "light" || theme === "dark" ? theme : null;
  if (t) document.documentElement.dataset.theme = t;
  else delete document.documentElement.dataset.theme;
  try {
    void getCurrentWindow().setTheme(t).catch(() => {});
  } catch {
    // outside Tauri (screenshot mode): the page alone is enough
  }
}
