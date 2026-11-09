import { invoke } from "@tauri-apps/api/core";

export type Folder = { id: number; parentId: number | null; name: string };
export type Entry = {
  id: number;
  folderId: number | null;
  name: string;
  trigger: string;
  body: string;
  immediate: boolean;
  caseSensitive: boolean;
  enabled: boolean;
  hotkey: string;
  tags: string[];
  uses: number;
};
export type Library = { folders: Folder[]; entries: Entry[]; paused: boolean };
export type Lang = "it" | "en";
export type Settings = {
  lang: string;
  popupHotkey: string;
  autostart: boolean;
  runAsAdmin: boolean;
  excludedApps: string[];
  clipHistory: boolean;
  clipHotkey: string;
  fixDoubleCaps: boolean;
  capitalizeSentences: boolean;
};
export type SyncStatus = { configured: boolean; email: string; last: string; error: string };
export type Clip = { id: number; text: string };
export type TrashItem = { id: number; label: string; isFolder: boolean; deletedAt: string };
export type FormField = {
  label: string;
  kind: "text" | "multiline" | "date" | "check" | "choice";
  options: string[];
  checkText: string;
};

export const api = {
  library: () => invoke<Library>("library"),
  saveEntry: (entry: Entry) => invoke<number>("save_entry", { entry }),
  deleteEntry: (id: number) => invoke<void>("delete_entry", { id }),
  addFolder: (parentId: number | null, name: string) => invoke<number>("add_folder", { parentId, name }),
  renameFolder: (id: number, name: string) => invoke<void>("rename_folder", { id, name }),
  moveFolder: (id: number, parentId: number | null) => invoke<void>("move_folder", { id, parentId }),
  deleteFolder: (id: number) => invoke<void>("delete_folder", { id }),
  setPaused: (paused: boolean) => invoke<void>("set_paused", { paused }),
  settings: () => invoke<Settings>("settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  runningApps: () => invoke<string[]>("running_apps"),
  popupInsert: (id: number) => invoke<void>("popup_insert", { id }),
  popupHide: () => invoke<void>("popup_hide"),
  formPending: () => invoke<FormField[] | null>("form_pending"),
  clips: () => invoke<Clip[]>("clips"),
  clipInsert: (id: number) => invoke<void>("clip_insert", { id }),
  clipDelete: (id: number) => invoke<void>("clip_delete", { id }),
  clipClear: () => invoke<void>("clip_clear"),
  clipToPhrase: (id: number) => invoke<number>("clip_to_phrase", { id }),
  exportData: (folderId: number | null = null) => invoke<string>("export_data", { folderId }),
  importData: (content: string, fileName: string) =>
    invoke<{ phrases: number; folders: number }>("import_data", { content, fileName }),
  trash: () => invoke<TrashItem[]>("trash"),
  trashRestore: (id: number) => invoke<{ phrases: number; folders: number }>("trash_restore", { id }),
  choicePending: () => invoke<number[] | null>("choice_pending"),
  choicePick: (id: number | null) => invoke<void>("choice_pick", { id }),
  setNoExpand: (on: boolean) => invoke<void>("set_no_expand", { on }),
  setRecordingHotkey: (on: boolean) => invoke<void>("set_recording_hotkey", { on }),
  trashClear: () => invoke<void>("trash_clear"),
  trashDelete: (id: number) => invoke<void>("trash_delete", { id }),
  installAutocorrect: () => invoke<number>("install_autocorrect"),
  openBackups: () => invoke<void>("open_backups"),
  syncStatus: () => invoke<SyncStatus>("sync_status"),
  syncLogin: () => invoke<SyncStatus>("sync_login"),
  syncLogout: () => invoke<SyncStatus>("sync_logout"),
  syncNow: () => invoke<SyncStatus>("sync_now"),
  formSubmit: (answers: Record<string, string> | null) => invoke<void>("form_submit", { answers }),
};
