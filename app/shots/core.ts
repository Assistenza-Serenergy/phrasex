// Screenshot mode only (vite.shots.config.ts): a fake backend with sample phrases, never shipped.
const q = new URLSearchParams(location.search);
const F = (id: number, parentId: number | null, name: string, color = "", hotkey = "") => ({ id, parentId, name, hotkey, color, shared: false });
const E = (id: number, folderId: number | null, name: string, trigger: string, body: string, uses = 0) => ({
  id, folderId, name, trigger, body, immediate: true, caseSensitive: false, enabled: true, hotkey: "", color: id === 15 || id === 16 ? "folder" : "", uses, shared: false,
});
const library = {
  paused: false,
  folders: [F(1, null, "Clienti", "#2f7de1", "Ctrl+Alt+C"), F(2, null, "Email"), F(3, null, "Firme", "#3fa45b"), F(4, 1, "Preventivi", "#e8833a")],
  entries: [
    E(10, 2, "Grazie per la richiesta", "grz", "Gentile {#input Nome},\ngrazie per la richiesta. Le rispondo entro oggi con tutti i dettagli.\n\nCordiali saluti", 42),
    E(11, 2, "Conferma appuntamento", "app", "Le confermo l'appuntamento di {#date +1d} alle ore {#input Ora}.\nA presto!", 17),
    E(12, 2, "Documenti mancanti", "doc", "Per completare la pratica ci servono ancora:\n- documento d'identità\n- codice fiscale", 9),
    E(13, 3, "Firma completa", "ff", "Marco Spagnuolo\nX Holding\ntel. 0123 456789", 88),
    E(14, 3, "Firma breve", "fb", "Marco - X Holding", 31),
    E(15, 4, "Invio preventivo", "prev", "In allegato il preventivo n. {#counter preventivi} del {#date}.\nResto a disposizione per qualsiasi domanda.", 12),
    E(16, 1, "Benvenuto nuovo cliente", "benv", "Benvenuto in X Holding! Da oggi ha un referente dedicato.", 5),
    E(17, null, "Indirizzo ufficio", "ind", "Via Roma 1, 00100 Roma", 23),
  ],
};
const settings = {
  lang: "it", popupHotkey: "Ctrl+Space", clipHotkey: "Ctrl+Shift+V", autostart: true, runAsAdmin: false, excludedApps: ["keepass.exe"],
  clipHistory: true, fixDoubleCaps: true, capitalizeSentences: false, popupSort: "uses", theme: "system", shareWith: ["collega@x-holding.it"],
};
const answers: Record<string, unknown> = {
  library, settings,
  sync_status: { configured: true, email: "marco@x-holding.it", name: "Marco Spagnuolo", last: "2026-11-15 10:42", error: "", shareError: "" },
  trash: [], clips: [
    { id: 1, text: "ordine n. 2026/418 spedito oggi", image: "" },
    { id: 2, text: "IT60 X054 2811 1010 0000 0123 456", image: "" },
  ],
  running_apps: ["outlook.exe", "chrome.exe"], update_available: null, form_pending: null,
  choice_pending: q.get("w") === "choose" ? [13, 14] : null,
};
export async function invoke<T>(cmd: string): Promise<T> {
  return (cmd in answers ? answers[cmd] : null) as T;
}
