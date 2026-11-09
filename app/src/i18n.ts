import it from "./locales/it.json";
import en from "./locales/en.json";
import type { Lang } from "./api";

export type Key = keyof typeof it;
const dict: Record<Lang, Record<Key, string>> = { it, en };

/** Stored language, or the system one on first run. */
export function resolveLang(stored: string): Lang {
  if (stored === "it" || stored === "en") return stored;
  return navigator.language.toLowerCase().startsWith("it") ? "it" : "en";
}

export type T = (key: Key, vars?: Record<string, string | number>) => string;

export function translator(lang: Lang): T {
  return (key, vars) =>
    dict[lang][key].replace(/\{(\w+)\}/g, (m, name) => (vars && name in vars ? String(vars[name]) : m));
}

/** Translates backend error codes. */
export function errText(t: T, e: unknown): string {
  const code = String(e);
  if (code === "invalid_hotkey") return t("errInvalidHotkey");
  if (code.startsWith("hotkey_unavailable")) return t("errHotkeyUnavailable");
  if (code === "popup_hotkey_unavailable") return t("errPopupHotkeyUnavailable");
  if (code === "folder_loop") return t("errFolderLoop");
  if (code === "import_invalid") return t("errImportInvalid");
  if (code === "import_phraseexpress") return t("errImportPhraseExpress");
  if (code === "admin_cancelled") return t("errAdminCancelled");
  if (code === "google_not_configured") return t("errGoogleNotConfigured");
  if (code === "google_login_timeout") return t("errGoogleTimeout");
  if (code === "google_scope_missing") return t("errGoogleScope");
  if (code === "google_relogin" || code === "google_signed_out") return t("errGoogleRelogin");
  if (code.startsWith("google_login_failed")) return t("errGoogleLogin");
  return t("errGeneric", { msg: code });
}
