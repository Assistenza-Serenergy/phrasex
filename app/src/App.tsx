import { useDeferredValue, useEffect, useMemo, useRef, useState, type Dispatch, type DragEvent, type HTMLAttributes, type MouseEvent, type RefObject, type SetStateAction } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type Entry, type Folder, type Library, type Settings, type TrashItem } from "./api";
import { errText, resolveLang, translator, type Key, type T } from "./i18n";
import { HotkeyInput } from "./HotkeyInput";
import { exportLibrary, importPicked, IMPORT_ACCEPT, SettingsView } from "./Settings";
import logo from "../src-tauri/icons/icon.png";

type Sel = { kind: "folder" | "entry"; id: number } | null;
type Item = NonNullable<Sel>;
type Band = { x0: number; y0: number; x1: number; y1: number };
const keyOf = (i: Item) => (i.kind === "folder" ? "f" : "e") + i.id;
type Draft = Omit<Entry, "tags"> & { tagsText: string };
type Tree = { folders: Map<number | null, Folder[]>; entries: Map<number | null, Entry[]> };
type FolderOption = { id: number; label: string };
type TrashSel = number | "all" | null; // a trashed item, the trash node itself, or nothing
type Menu = { x: number; y: number; target: Sel; trash?: TrashSel };
type MenuItem = [Key, () => unknown] | null;
/** Props spread on a tree row: drag source, drop target, context menu. */
type RowAttrs = (item: Sel, dropFolder?: number | null) => HTMLAttributes<HTMLDivElement>;
const DRAG_TYPE = "application/x-phrasex";

const toDraft = ({ tags, ...rest }: Entry): Draft => ({ ...rest, tagsText: tags.join(", ") });
const fromDraft = ({ tagsText, ...rest }: Draft): Entry => ({
  ...rest,
  tags: tagsText.split(",").map((s) => s.trim()).filter(Boolean),
});
const sameDraft = (a: Draft, b: Draft) => JSON.stringify(a) === JSON.stringify(b);

function push<K, V>(map: Map<K, V[]>, key: K, value: V) {
  const list = map.get(key);
  if (list) list.push(value);
  else map.set(key, [value]);
}

const FolderIcon = (
  <svg className="icon folder-icon" viewBox="0 0 16 16" aria-hidden="true">
    <path d="M1.5 3.5h4.2l1.4 1.5h7.4v7.5h-13z" fill="currentColor" />
  </svg>
);
const PhraseIcon = (
  <svg className="icon" viewBox="0 0 16 16" aria-hidden="true">
    <path d="M3.5 2.5h6l3 3v8h-9z M5.5 7.5h5 M5.5 10h5" fill="none" stroke="currentColor" strokeWidth="1.1" />
  </svg>
);
const TrashIcon = (
  <svg className="icon" viewBox="0 0 16 16" aria-hidden="true">
    <path d="M2.5 4h11M6 4V2.5h4V4M3.8 4l.8 9.5h6.8l.8-9.5M6.5 6.5v5M9.5 6.5v5" fill="none" stroke="currentColor" strokeWidth="1.1" strokeLinejoin="round" />
  </svg>
);
const GearIcon = (
  <svg className="icon" viewBox="0 0 16 16" aria-hidden="true">
    <circle cx="8" cy="8" r="2.2" fill="none" stroke="currentColor" strokeWidth="1.2" />
    <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
  </svg>
);

export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const lang = resolveLang(settings?.lang ?? "");
  const t = useMemo(() => translator(lang), [lang]);
  const [view, setView] = useState<"library" | "settings">("library");
  const [lib, setLib] = useState<Library | null>(null);
  const [sel, setSel] = useState<Sel>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [folderName, setFolderName] = useState("");
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query);
  const [collapsed, setCollapsed] = useState<Set<number>>(() => new Set());
  const [msg, setMsg] = useState<{ text: string; error: boolean; undo?: number[] } | null>(null); // undo = trash ids to restore
  const nameRef = useRef<HTMLInputElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const dragged = useRef<Item[] | null>(null); // dataTransfer can't be read during dragover
  const [multi, setMulti] = useState<Item[]>([]); // group selection (box or Ctrl+click), shown over the editor
  const [band, setBand] = useState<Band | null>(null); // selection box being drawn, in tree content coordinates
  const [moveTarget, setMoveTarget] = useState("");
  const multiKeys = useMemo(() => new Set(multi.map(keyOf)), [multi]);
  const isSel = (kind: Item["kind"], id: number) =>
    multi.length ? multiKeys.has(kind[0] + id) : sel?.kind === kind && sel.id === id;
  const [dropKey, setDropKey] = useState<string | null>(null); // "f<id>" or "root"
  const [trash, setTrash] = useState<TrashItem[]>([]);
  const [trashSel, setTrashSel] = useState<TrashSel>(null);
  const [trashOpen, setTrashOpen] = useState(true);
  const [trashMulti, setTrashMulti] = useState<number[]>([]); // group selection inside the trash
  const loadTrash = () => api.trash().then(setTrash).catch(() => {});
  useEffect(() => void loadTrash(), []);

  async function refresh() {
    const l = await api.library();
    setLib(l);
    return l;
  }

  useEffect(() => {
    // Phrases expand in this window too, except in fields marked data-noexpand (abbreviation, shortcut, search).
    const onFocus = (e: FocusEvent) => void api.setNoExpand(!!(e.target as Element | null)?.closest?.("[data-noexpand]")).catch(() => {});
    document.addEventListener("focusin", onFocus);
    return () => document.removeEventListener("focusin", onFocus);
  }, []);

  useEffect(() => {
    Promise.all([api.library(), api.settings()])
      .then(([l, s]) => {
        setLib(l);
        setSettings(s);
        // First run: store the system language so the tray menu matches.
        if (!s.lang) void api.saveSettings({ ...s, lang: resolveLang("") }).catch(() => {});
      })
      .catch((e) => setMsg({ text: String(e), error: true }));
    const unPaused = listen<boolean>("paused", (ev) => setLib((l) => l && { ...l, paused: ev.payload }));
    const unSettings = listen("open-settings", () => setView("settings"));
    // Cloud sync brought changes from another PC.
    const unSynced = listen("library-changed", () => void api.library().then(setLib).catch(() => {}));
    // A clipboard item turned into a phrase from the popup: show it in the editor.
    const unEntry = listen<number>("open-entry", async (ev) => {
      const l = await api.library();
      setLib(l);
      setView("library");
      setSel({ kind: "entry", id: ev.payload });
      const e = l.entries.find((x) => x.id === ev.payload);
      setDraft(e ? toDraft(e) : null);
    });
    return () => {
      void unPaused.then((f) => f());
      void unSettings.then((f) => f());
      void unSynced.then((f) => f());
      void unEntry.then((f) => f());
    };
  }, []);

  const tree = useMemo<Tree>(() => {
    const folders = new Map<number | null, Folder[]>();
    const entries = new Map<number | null, Entry[]>();
    for (const f of lib?.folders ?? []) push(folders, f.parentId, f);
    for (const e of lib?.entries ?? []) push(entries, e.folderId, e);
    return { folders, entries };
  }, [lib]);

  const folderOptions = useMemo(() => {
    const out: FolderOption[] = [];
    const walk = (parent: number | null, depth: number) => {
      for (const f of tree.folders.get(parent) ?? []) {
        out.push({ id: f.id, label: "   ".repeat(depth) + f.name });
        walk(f.id, depth + 1);
      }
    };
    walk(null, 0);
    return out;
  }, [tree]);

  const results = useMemo(() => {
    const q = deferredQuery.trim().toLowerCase();
    if (!q) return null;
    return (lib?.entries ?? []).filter((e) => [e.name, e.trigger, e.body, e.hotkey, ...e.tags].some((s) => s.toLowerCase().includes(q)));
  }, [lib, deferredQuery]);

  const selectedEntry = sel?.kind === "entry" ? lib?.entries.find((e) => e.id === sel.id) : undefined;
  const selectedFolder = sel?.kind === "folder" ? lib?.folders.find((f) => f.id === sel.id) : undefined;
  const dirty = !!draft && !!selectedEntry && !sameDraft(draft, toDraft(selectedEntry));
  const folderDirty = !!selectedFolder && folderName.trim() !== "" && folderName.trim() !== selectedFolder.name;
  const targetFolderId = sel?.kind === "folder" ? sel.id : (selectedEntry?.folderId ?? null);
  const popupKey = settings?.popupHotkey ?? "Ctrl+Shift+Space";

  function applySelection(next: Sel, l: Library | null) {
    setSel(next);
    const e = next?.kind === "entry" ? l?.entries.find((x) => x.id === next.id) : undefined;
    const f = next?.kind === "folder" ? l?.folders.find((x) => x.id === next.id) : undefined;
    setDraft(e ? toDraft(e) : null);
    setFolderName(f?.name ?? "");
  }

  function fail(e: unknown): false {
    setMsg({ text: errText(t, e), error: true });
    return false;
  }

  /** Saves pending edits. Returns the up-to-date library, or false if saving failed. */
  async function saveCurrent(): Promise<Library | null | false> {
    let l = lib;
    try {
      if (dirty && draft) {
        let id = draft.id;
        let warning: unknown = null;
        try {
          id = await api.saveEntry(fromDraft(draft));
        } catch (e) {
          if (!String(e).startsWith("hotkey_unavailable")) throw e;
          warning = e; // saved, only the shortcut is taken
        }
        l = await refresh();
        const saved = l.entries.find((x) => x.id === id);
        if (saved) setDraft(toDraft(saved)); // pick up normalized trigger/tags
        if (warning) fail(warning);
        else setMsg({ text: t("saved"), error: false });
      }
      if (folderDirty && selectedFolder) {
        await api.renameFolder(selectedFolder.id, folderName);
        l = await refresh();
        setMsg({ text: t("saved"), error: false });
      }
      return l;
    } catch (e) {
      return fail(e);
    }
  }

  async function select(next: Sel) {
    const l = await saveCurrent();
    if (l !== false) {
      applySelection(next, l);
      setMulti([]);
      setView("library");
      setTrashSel(null);
      setTrashMulti([]);
    }
    return l !== false;
  }

  async function openTrash(which: TrashSel, group: number[] = []) {
    if (!(await select(null))) return;
    setTrashSel(which);
    setTrashMulti(group);
  }

  /** Ctrl+click on a trashed row: add it to / remove it from the trash group. */
  function toggleTrash(id: number) {
    const base = trashMulti.length ? trashMulti : typeof trashSel === "number" ? [trashSel] : [];
    const next = base.includes(id) ? base.filter((x) => x !== id) : [...base, id];
    void openTrash(next.length === 1 ? next[0] : "all", next.length > 1 ? next : []);
  }

  async function restoreTrash(ids: number[]) {
    let phrases = 0, folders = 0;
    try {
      for (const id of ids) {
        const r = await api.trashRestore(id);
        phrases += r.phrases;
        folders += r.folders;
      }
      setMsg({ text: t("restored", { phrases, folders }), error: false });
      await refresh();
    } catch (e) {
      fail(e);
    }
    setTrashMulti([]);
    if (trashSel !== null) setTrashSel("all");
    void loadTrash();
  }

  async function deleteForever(ids: number[] | "all") {
    try {
      if (ids === "all") await api.trashClear();
      else for (const id of ids) await api.trashDelete(id);
    } catch (e) {
      fail(e);
    }
    setTrashMulti([]);
    if (trashSel !== null) setTrashSel("all");
    void loadTrash();
  }

  function expand(id: number | null) {
    if (id !== null) setCollapsed((c) => (c.has(id) ? new Set([...c].filter((x) => x !== id)) : c));
  }

  async function newEntry(folderId = targetFolderId) {
    if ((await saveCurrent()) === false) return;
    try {
      const id = await api.saveEntry({
        id: 0, folderId, name: t("newEntry"), trigger: "", body: "",
        immediate: false, caseSensitive: false, enabled: true, hotkey: "", tags: [], uses: 0,
      });
      expand(folderId);
      applySelection({ kind: "entry", id }, await refresh());
      setView("library");
      requestAnimationFrame(() => nameRef.current?.select());
    } catch (e) {
      fail(e);
    }
  }

  async function newFolder(parentId = targetFolderId) {
    if ((await saveCurrent()) === false) return;
    try {
      const id = await api.addFolder(parentId, t("newFolder"));
      expand(parentId);
      applySelection({ kind: "folder", id }, await refresh());
      setView("library");
      requestAnimationFrame(() => nameRef.current?.select());
    } catch (e) {
      fail(e);
    }
  }

  /** Moves to the trash right away, no confirmation: the status bar offers Undo (the trash keeps a snapshot). */
  async function remove(target: Sel = sel) {
    const group = multi.length && (!target || multiKeys.has(keyOf(target))) ? topLevel(multi) : null;
    const items = group ?? (target ? [target] : []);
    if (!items.length) return;
    try {
      for (const it of items) {
        if (it.kind === "entry") await api.deleteEntry(it.id);
        else await api.deleteFolder(it.id);
      }
      setMulti([]);
      const l = await refresh();
      const fresh = await api.trash();
      setTrash(fresh);
      const undo = fresh.slice(0, items.length).map((x) => x.id); // newest first = the ones just deleted
      setMsg({ text: t("deletedN", { n: items.length }), error: false, undo });
      // keep the current selection unless it was deleted (a folder takes its content with it)
      const still = sel && (sel.kind === "entry" ? l.entries.some((e) => e.id === sel.id) : l.folders.some((f) => f.id === sel.id));
      if (!still) applySelection(null, l);
    } catch (e) {
      fail(e);
    }
  }

  async function undoDelete(ids: number[]) {
    try {
      for (const id of [...ids].reverse()) await api.trashRestore(id);
      setMsg({ text: t("undone"), error: false });
      await refresh();
    } catch (e) {
      fail(e);
    }
    void loadTrash();
  }

  // Del/Canc deletes the selection (group first), unless typing in a field. On window: after a box selection
  // the focus is on no row, so the app's own onKeyDown wouldn't see the key.
  const onDelete = useRef(() => {});
  onDelete.current = () => {
    if (view === "library" && trashSel === null) void remove(multi.length ? null : sel);
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (e.key !== "Delete" || el?.closest?.("input, textarea, select, [contenteditable=true]")) return;
      e.preventDefault();
      onDelete.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  async function rename(target: NonNullable<Sel>) {
    await select(target);
    requestAnimationFrame(() => nameRef.current?.select());
  }

  /** Copy of a phrase without abbreviation and shortcut (both must be unique). */
  async function duplicate(id: number) {
    const l = await saveCurrent();
    const e = l && l.entries.find((x) => x.id === id);
    if (!e) return;
    try {
      const copy = await api.saveEntry({ ...e, id: 0, name: `${e.name} ${t("copySuffix")}`.trim(), trigger: "", hotkey: "", uses: 0 });
      applySelection({ kind: "entry", id: copy }, await refresh());
      setView("library");
      requestAnimationFrame(() => nameRef.current?.select());
    } catch (err) {
      fail(err);
    }
  }

  /** True if folder `id` is `ancestor` or lies somewhere below it. */
  function isInside(id: number | null, ancestor: number) {
    for (let cur = id; cur !== null; cur = lib?.folders.find((f) => f.id === cur)?.parentId ?? null) {
      if (cur === ancestor) return true;
    }
    return false;
  }

  /** Folder that contains an item (null = top level). */
  function parentOf(i: Item) {
    return (i.kind === "entry" ? lib?.entries.find((e) => e.id === i.id)?.folderId : lib?.folders.find((f) => f.id === i.id)?.parentId) ?? null;
  }

  /** Drops items that are already inside another selected folder (the folder takes them along). */
  function topLevel(items: Item[]) {
    const folders = items.filter((i) => i.kind === "folder").map((i) => i.id);
    return items.filter((i) => !folders.some((f) => isInside(parentOf(i), f)));
  }

  function canDrop(items: Item[] | null, folderId: number | null) {
    if (!items?.length || items.some((i) => i.kind === "folder" && isInside(folderId, i.id))) return false;
    return items.some((i) => parentOf(i) !== folderId);
  }

  async function moveTo(items: Item[], folderId: number | null) {
    const l = await saveCurrent();
    if (!l) return;
    try {
      for (const item of items) {
        if (parentOf(item) === folderId) continue;
        if (item.kind === "entry") {
          const e = l.entries.find((x) => x.id === item.id);
          if (e) await api.saveEntry({ ...e, folderId });
        } else {
          await api.moveFolder(item.id, folderId);
        }
      }
      setMsg({ text: t("moved"), error: false });
    } catch (e) {
      fail(e);
    }
    expand(folderId);
    try {
      applySelection(sel, await refresh()); // re-read the editor: its folder may have changed
    } catch (e) {
      fail(e);
    }
  }

  function dragOver(e: DragEvent, folderId: number | null, key: string) {
    e.stopPropagation();
    if (!canDrop(dragged.current, folderId)) return; // no preventDefault = drop refused
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    if (dropKey !== key) setDropKey(key);
  }

  function drop(e: DragEvent, folderId: number | null) {
    e.preventDefault();
    e.stopPropagation();
    const items = dragged.current;
    dragged.current = null;
    setDropKey(null);
    if (items && canDrop(items, folderId)) void moveTo(items, folderId);
  }

  function openMenu(e: MouseEvent, target: Sel) {
    e.preventDefault();
    e.stopPropagation();
    if (target && !multiKeys.has(keyOf(target))) void select(target); // right-click inside the group keeps it
    setMenu({ x: e.clientX, y: e.clientY, target });
  }

  const rowAttrs: RowAttrs = (item, dropFolder) => {
    const attrs: HTMLAttributes<HTMLDivElement> = {
      onContextMenu: (e) => openMenu(e, item),
      onClickCapture: (e) => {
        if (!item || !(e.ctrlKey || e.metaKey)) return;
        e.preventDefault();
        e.stopPropagation(); // Ctrl+click: add to / remove from the group instead of opening
        const base = multi.length ? multi : sel ? [sel] : [];
        const k = keyOf(item);
        setMulti(base.some((i) => keyOf(i) === k) ? base.filter((i) => keyOf(i) !== k) : [...base, item]);
      },
    };
    if (dropFolder === undefined) return attrs; // search results: menu only
    const key = dropFolder === null ? "root" : `f${dropFolder}`;
    return {
      ...attrs,
      draggable: true,
      onDragStart: (e) => {
        e.stopPropagation();
        dragged.current = item && multiKeys.has(keyOf(item)) ? topLevel(multi) : item ? [item] : null;
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData(DRAG_TYPE, JSON.stringify(item));
      },
      onDragEnd: () => {
        dragged.current = null;
        setDropKey(null);
      },
      onDragOver: (e) => dragOver(e, dropFolder, key),
      onDrop: (e) => drop(e, dropFolder),
    };
  };

  async function importFile(input: HTMLInputElement) {
    const r = await importPicked(t, input);
    if (!r) return;
    setMsg(r);
    if (!r.error) await refresh().catch(fail);
  }

  async function exportData(folderId: number | null = null) {
    setMsg(await exportLibrary(t, folderId));
  }

  function menuItems(target: Sel, trashTarget?: TrashSel): MenuItem[] {
    if (trashTarget === "all") return [["trashClear", () => deleteForever("all")]];
    if (typeof trashTarget === "number") {
      const ids = trashMulti.length > 1 && trashMulti.includes(trashTarget) ? trashMulti : [trashTarget];
      return [["trashRestore", () => restoreTrash(ids)], null, ["trashDeleteForever", () => deleteForever(ids)]];
    }
    if (target && multiKeys.has(keyOf(target)) && multi.length > 1) {
      return [["ctxDeleteSelected", () => remove(target)]];
    }
    if (!target) {
      return [
        ["ctxNewRootFolder", () => newFolder(null)],
        ["newEntry", () => newEntry(null)],
        null,
        ["ctxImport", () => fileRef.current?.click()],
        ["ctxExportAll", () => exportData()],
      ];
    }
    if (target.kind === "folder") {
      return [
        ["ctxNewSubfolder", () => newFolder(target.id)],
        ["ctxNewPhraseHere", () => newEntry(target.id)],
        ["ctxNewRootFolder", () => newFolder(null)],
        null,
        ["ctxRename", () => rename(target)],
        ["ctxExportFolder", () => exportData(target.id)],
        null,
        ["delete", () => remove(target)],
      ];
    }
    const folderId = lib?.entries.find((e) => e.id === target.id)?.folderId ?? null;
    return [
      ["ctxNewPhraseHere", () => newEntry(folderId)],
      ["ctxNewRootFolder", () => newFolder(null)],
      null,
      ["ctxRename", () => rename(target)],
      ["ctxDuplicate", () => duplicate(target.id)],
      null,
      ["delete", () => remove(target)],
    ];
  }

  /** Rows touched by the selection box: library items, or else trashed items (never both). */
  function bandHits(nav: HTMLElement, b: Band): { items: Item[]; trashed: number[] } {
    const r = nav.getBoundingClientRect();
    const top = Math.min(b.y0, b.y1), bottom = Math.max(b.y0, b.y1);
    const items: Item[] = [], trashed: number[] = [];
    nav.querySelectorAll<HTMLElement>("[data-key], [data-tkey]").forEach((el) => {
      const q = el.getBoundingClientRect();
      const y0 = q.top - r.top + nav.scrollTop, y1 = q.bottom - r.top + nav.scrollTop;
      if (y1 <= top || y0 >= bottom) return;
      const k = el.dataset.key;
      if (k) items.push({ kind: k[0] === "f" ? "folder" : "entry", id: Number(k.slice(1)) });
      else trashed.push(Number(el.dataset.tkey));
    });
    return items.length ? { items, trashed: [] } : { items, trashed };
  }

  function bandPoint(nav: HTMLElement, e: { clientX: number; clientY: number }) {
    const r = nav.getBoundingClientRect();
    return { x: e.clientX - r.left + nav.scrollLeft, y: e.clientY - r.top + nav.scrollTop };
  }

  async function togglePause() {
    if (!lib) return;
    try {
      await api.setPaused(!lib.paused);
      await refresh();
    } catch (e) {
      fail(e);
    }
  }

  function toggleFolder(id: number) {
    setCollapsed((c) => {
      const next = new Set(c);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  }

  const treeProps = { tree, collapsed, isSel, onSelect: select, onToggle: toggleFolder, row: rowAttrs, dropKey, t };
  const group = topLevel(multi);
  const groupTarget = moveTarget === "" ? null : Number(moveTarget);

  return (
    <div
      className="app"
      onKeyDown={(e) => {
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
          e.preventDefault();
          void saveCurrent();
        }
        if (e.key === "Escape") {
          setMulti([]);
          setTrashMulti([]);
        }
      }}
    >
      <header className="toolbar">
        <div className="brand">
          <img src={logo} alt="" width={23} height={23} />
          <span>Phrase X</span>
        </div>
        <button className="accent" onClick={() => newEntry()}>{PhraseIcon}<span>{t("newEntry")}</span></button>
        <button onClick={() => newFolder()}>{FolderIcon}<span>{t("newFolder")}</span></button>
        <button onClick={() => remove()} disabled={!sel && !multi.length}>{t("delete")}</button>
        <button onClick={() => fileRef.current?.click()} title={t("importTitle")}>{t("importShort")}</button>
        <button onClick={() => exportData()} title={t("exportTitle")}>{t("exportShort")}</button>
        <input ref={fileRef} type="file" accept={IMPORT_ACCEPT} hidden onChange={(e) => void importFile(e.currentTarget)} />
        <input
          className="search"
          type="search"
          data-noexpand
          value={query}
          placeholder={t("search")}
          aria-label={t("search")}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button className={view === "settings" ? "icon-button on" : "icon-button"} aria-label={t("settings")} title={t("settings")} onClick={() => setView(view === "settings" ? "library" : "settings")}>
          {GearIcon}
        </button>
      </header>

      <main className="panes">
        <nav
          className={dropKey === "root" ? "tree drop" : "tree"}
          aria-label={t("search")}
          onPointerDown={(e) => {
            if (e.target !== e.currentTarget || e.button !== 0 || results) return; // empty area only
            e.currentTarget.setPointerCapture(e.pointerId);
            const { x, y } = bandPoint(e.currentTarget, e);
            setBand({ x0: x, y0: y, x1: x, y1: y });
          }}
          onPointerMove={(e) => {
            if (!band) return;
            const { x, y } = bandPoint(e.currentTarget, e);
            const next = { ...band, x1: x, y1: y };
            setBand(next);
            const h = bandHits(e.currentTarget, next);
            setMulti(h.items);
            setTrashMulti(h.trashed);
          }}
          onPointerUp={(e) => {
            if (!band) return;
            const { items: hits, trashed } = bandHits(e.currentTarget, band);
            const click = Math.abs(band.x1 - band.x0) < 4 && Math.abs(band.y1 - band.y0) < 4;
            setBand(null);
            if (!click && trashed.length) void openTrash(trashed.length === 1 ? trashed[0] : "all", trashed.length > 1 ? trashed : []);
            else if (click || hits.length < 2) {
              setMulti([]);
              void select(click ? null : (hits[0] ?? null)); // plain click on empty area: new items go to the top level
            }
          }}
          onContextMenu={(e) => openMenu(e, null)}
          onDragOver={(e) => (results ? undefined : dragOver(e, null, "root"))}
          onDragLeave={(e) => {
            if (e.target === e.currentTarget) setDropKey(null);
          }}
          onDrop={(e) => drop(e, null)}
        >
          {band ? (
            <div
              className="band"
              style={{ left: Math.min(band.x0, band.x1), top: Math.min(band.y0, band.y1), width: Math.abs(band.x1 - band.x0), height: Math.abs(band.y1 - band.y0) }}
            />
          ) : null}
          {results ? (
            results.length ? (
              results.map((e) => <EntryRow key={e.id} entry={e} depth={0} selected={isSel("entry", e.id)} onSelect={select} attrs={rowAttrs({ kind: "entry", id: e.id })} t={t} />)
            ) : (
              <p className="muted pad">{t("noResults")}</p>
            )
          ) : (
            <>
              {(tree.folders.get(null) ?? []).map((f) => <TreeFolder key={f.id} folder={f} depth={0} {...treeProps} />)}
              {(tree.entries.get(null) ?? []).map((e) => (
                <EntryRow key={e.id} entry={e} depth={0} selected={isSel("entry", e.id)} onSelect={select} attrs={rowAttrs({ kind: "entry", id: e.id }, null)} t={t} />
              ))}
              <TrashNode
                items={trash}
                open={trashOpen}
                selected={trashSel}
                group={trashMulti}
                onToggle={() => setTrashOpen(!trashOpen)}
                onSelect={(w) => void openTrash(w)}
                onCtrl={toggleTrash}
                onMenu={(e, w) => {
                  e.preventDefault();
                  e.stopPropagation();
                  if (!(typeof w === "number" && trashMulti.length > 1 && trashMulti.includes(w))) void openTrash(w); // right-click in the group keeps it
                  setMenu({ x: e.clientX, y: e.clientY, target: null, trash: w });
                }}
                t={t}
              />
            </>
          )}
        </nav>
        {menu ? <ContextMenu menu={menu} items={menuItems(menu.target, menu.trash)} onClose={() => setMenu(null)} t={t} /> : null}

        <section className="detail">
          {view === "settings" && settings ? (
            <SettingsView key={lang} initial={settings} t={t} onSaved={setSettings} onImported={() => void refresh()} />
          ) : trashSel !== null ? (
            <TrashView key={String(trashSel) + trashMulti.join()} items={trash} selected={trashSel} group={trashMulti} onSelect={(w) => void openTrash(w)} onCancelGroup={() => setTrashMulti([])} onRestore={restoreTrash} onDelete={deleteForever} t={t} />
          ) : multi.length > 1 ? (
            <div className="editor">
              <h2>{t("selectedCount", { n: group.length })}</h2>
              <p className="muted">{t("groupHint")}</p>
              <label className="field">
                <span>{t("moveTo")}</span>
                <select value={moveTarget} onChange={(e) => setMoveTarget(e.target.value)}>
                  <option value="">{t("noFolder")}</option>
                  {folderOptions.map((o) => <option key={o.id} value={o.id}>{o.label}</option>)}
                </select>
              </label>
              <div className="row">
                <button className="primary" disabled={!canDrop(group, groupTarget)} onClick={() => void moveTo(group, groupTarget)}>{t("moveHere")}</button>
                <button onClick={() => void remove()}>{t("delete")}</button>
                <button onClick={() => setMulti([])}>{t("cancel")}</button>
              </div>
            </div>
          ) : draft ? (
            <Editor draft={draft} setDraft={setDraft} folderOptions={folderOptions} dirty={dirty} nameRef={nameRef} onSave={saveCurrent} entries={lib?.entries ?? []} t={t} />
          ) : selectedFolder ? (
            <form className="editor" onSubmit={(e) => { e.preventDefault(); void saveCurrent(); }}>
              <label className="field">
                <span>{t("folderName")}</span>
                <input ref={nameRef} value={folderName} onChange={(e) => setFolderName(e.target.value)} />
              </label>
              <p className="muted">{t("folderHint")}</p>
              <div className="row end">
                <button className="primary" type="submit" disabled={!folderDirty}>{t("save")}</button>
              </div>
            </form>
          ) : (
            <Welcome t={t} popupKey={popupKey} entries={lib?.entries ?? []} />
          )}
        </section>
      </main>

      <footer className="status">
        <span>{t("count", { n: lib?.entries.length ?? 0 })}</span>
        <button className={lib?.paused ? "pill off" : "pill on"} onClick={togglePause}>
          {lib?.paused ? t("paused") : t("active")}
        </button>
        <span className="muted">{t("popupKey", { key: popupKey })}</span>
        {msg ? (
          <span className={msg.error ? "msg error" : "msg"} role="status">
            {msg.text}
            {msg.undo?.length ? <button className="link" onClick={() => void undoDelete(msg.undo!)}>{t("undo")}</button> : null}
          </span>
        ) : null}
      </footer>
    </div>
  );
}

function Welcome({ t, popupKey, entries }: { t: T; popupKey: string; entries: Entry[] }) {
  const tips: Key[] = ["tip1", "tip2", "tip3", "tip4"];
  let uses = 0;
  let chars = 0;
  for (const e of entries) {
    uses += e.uses;
    chars += e.uses * Math.max(0, e.body.length - e.trigger.length);
  }
  const top = entries.filter((e) => e.uses > 0).toSorted((a, b) => b.uses - a.uses).slice(0, 3);
  return (
    <div className="welcome">
      <img src={logo} alt="" width={68} height={68} />
      <h1>{t("emptyTitle")}</h1>
      <p className="muted">{t("emptyHint")}</p>
      {uses > 0 ? (
        <div className="stats">
          <strong>{t("statsTitle")}</strong>
          <span>{t("stats", { n: uses, chars: chars.toLocaleString() })}</span>
          <span className="muted">{t("statsTop")} {top.map((e) => `${e.name} (${e.uses})`).join(" · ")}</span>
        </div>
      ) : null}
      <ul>
        {tips.map((k) => <li key={k}>{t(k, { key: popupKey })}</li>)}
      </ul>
    </div>
  );
}

type TreeProps = {
  tree: Tree;
  collapsed: Set<number>;
  isSel: (kind: Item["kind"], id: number) => boolean;
  onSelect: (s: Sel) => void;
  onToggle: (id: number) => void;
  row: RowAttrs;
  dropKey: string | null;
  t: T;
};

function ContextMenu({ menu, items, onClose, t }: { menu: Menu; items: MenuItem[]; onClose: () => void; t: T }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => ref.current?.querySelector("button")?.focus(), []);
  // keep it on screen (~28px per row, 220px wide)
  const left = Math.max(4, Math.min(menu.x, window.innerWidth - 230));
  const top = Math.max(4, Math.min(menu.y, window.innerHeight - items.length * 28 - 12));
  return (
    <div className="menu-backdrop" onMouseDown={onClose} onContextMenu={(e) => { e.preventDefault(); onClose(); }}>
      <div
        ref={ref}
        className="menu"
        role="menu"
        style={{ left, top }}
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          const buttons = [...(ref.current?.querySelectorAll("button") ?? [])];
          const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
          if (e.key === "Escape") onClose();
          else if (e.key === "ArrowDown") buttons[(i + 1) % buttons.length]?.focus();
          else if (e.key === "ArrowUp") buttons[(i - 1 + buttons.length) % buttons.length]?.focus();
          else return;
          e.preventDefault();
        }}
      >
        {items.map((it, i) =>
          it ? (
            <button key={it[0]} role="menuitem" className={DANGER.has(it[0]) ? "danger" : ""} onClick={() => { onClose(); void it[1](); }}>
              {t(it[0])}
            </button>
          ) : (
            <hr key={`sep${i}`} />
          ),
        )}
      </div>
    </div>
  );
}

const DANGER = new Set<Key>(["delete", "trashDeleteForever", "trashClear"]);

/** Days left before a trashed item is purged (the store drops it after 30). */
function daysLeft(deletedAt: string) {
  const age = (Date.now() - new Date(deletedAt.replace(" ", "T")).getTime()) / 86_400_000;
  return Math.max(0, Math.ceil(30 - age));
}

type TrashNodeProps = {
  items: TrashItem[];
  open: boolean;
  selected: TrashSel;
  group: number[];
  onToggle: () => void;
  onSelect: (w: TrashSel) => void;
  onCtrl: (id: number) => void;
  onMenu: (e: MouseEvent, w: TrashSel) => void;
  t: T;
};

/** The trash at the bottom of the tree: deleted folders and phrases, greyed, restorable. */
function TrashNode({ items, open, selected, group, onToggle, onSelect, onCtrl, onMenu, t }: TrashNodeProps) {
  const isSel = (id: number) => (group.length ? group.includes(id) : selected === id);
  return (
    <>
      <div className={selected === "all" ? "node trash-node selected" : "node trash-node"} style={{ paddingLeft: 6 }} onContextMenu={(e) => onMenu(e, "all")}>
        <button className={open ? "chevron open" : "chevron"} aria-label={t("trashTitle")} aria-expanded={open} onClick={onToggle}>
          ›
        </button>
        <button className="label" onClick={() => onSelect("all")} onDoubleClick={onToggle}>
          {TrashIcon}
          <span className="name">{t("trashTitle")}</span>
          {items.length ? <kbd>{items.length}</kbd> : null}
        </button>
      </div>
      {open
        ? items.map((it) => (
            <div key={it.id} className={isSel(it.id) ? "node trashed selected" : "node trashed"} style={{ paddingLeft: 6 + 16 + 18 }} data-tkey={it.id} onContextMenu={(e) => onMenu(e, it.id)}>
              <button className="label" onClick={(e) => (e.ctrlKey || e.metaKey ? onCtrl(it.id) : onSelect(it.id))}>
                {it.isFolder ? FolderIcon : PhraseIcon}
                <span className="name">{it.label || t("unnamed")}</span>
              </button>
            </div>
          ))
        : null}
    </>
  );
}

/** Two clicks for what cannot be undone: the first arms it, the second does it (no native popup). */
function ConfirmButton({ label, confirm, onConfirm, t }: { label: Key; confirm: Key; onConfirm: () => unknown; t: T }) {
  const [armed, setArmed] = useState(false);
  return armed ? (
    <>
      <button className="danger-fill" onClick={() => void onConfirm()}>{t(confirm)}</button>
      <button onClick={() => setArmed(false)}>{t("cancel")}</button>
    </>
  ) : (
    <button onClick={() => setArmed(true)}>{t(label)}</button>
  );
}

type TrashViewProps = {
  items: TrashItem[];
  selected: TrashSel;
  group: number[];
  onSelect: (w: TrashSel) => void;
  onCancelGroup: () => void;
  onRestore: (ids: number[]) => unknown;
  onDelete: (ids: number[] | "all") => unknown;
  t: T;
};

function TrashView({ items, selected, group, onSelect, onCancelGroup, onRestore, onDelete, t }: TrashViewProps) {
  if (group.length > 1) {
    return (
      <div className="editor">
        <h2>{t("trashSelected", { n: group.length })}</h2>
        <p className="muted">{t("trashGroupHint")}</p>
        <div className="row">
          <button className="primary" onClick={() => void onRestore(group)}>{t("trashRestore")}</button>
          <ConfirmButton label="trashDeleteForever" confirm="trashConfirmForever" onConfirm={() => onDelete(group)} t={t} />
          <button onClick={onCancelGroup}>{t("cancel")}</button>
        </div>
      </div>
    );
  }
  const it = typeof selected === "number" ? items.find((x) => x.id === selected) : undefined;
  if (it) {
    return (
      <div className="editor">
        <h2>{it.label || t("unnamed")}</h2>
        <p className="muted">{t("trashItemInfo", { kind: t(it.isFolder ? "trashFolder" : "trashPhrase"), date: it.deletedAt.slice(0, 16), days: daysLeft(it.deletedAt) })}</p>
        <div className="row">
          <button className="primary" onClick={() => void onRestore([it.id])}>{t("trashRestore")}</button>
          <ConfirmButton label="trashDeleteForever" confirm="trashConfirmForever" onConfirm={() => onDelete([it.id])} t={t} />
        </div>
      </div>
    );
  }
  return (
    <div className="editor">
      <h2>{t("trashTitle")}</h2>
      <p className="muted">{t("trashHint")}</p>
      {items.length ? (
        <>
          <ul className="trash">
            {items.map((x) => (
              <li key={x.id}>
                {x.isFolder ? FolderIcon : PhraseIcon}
                <button className="name link-row" onClick={() => onSelect(x.id)}>{x.label || t("unnamed")}</button>
                <span className="muted">{x.deletedAt.slice(0, 16)}</span>
                <button onClick={() => void onRestore([x.id])}>{t("trashRestore")}</button>
              </li>
            ))}
          </ul>
          <div className="row">
            <ConfirmButton label="trashClear" confirm="trashConfirmClear" onConfirm={() => onDelete("all")} t={t} />
          </div>
        </>
      ) : (
        <p className="muted">{t("trashEmpty")}</p>
      )}
    </div>
  );
}

function TreeFolder({ folder, depth, ...p }: TreeProps & { folder: Folder; depth: number }) {
  const open = !p.collapsed.has(folder.id);
  const selected = p.isSel("folder", folder.id);
  return (
    <>
      <div
        className={["node", selected ? "selected" : "", p.dropKey === `f${folder.id}` ? "drop" : ""].join(" ")}
        style={{ paddingLeft: 6 + depth * 16 }}
        data-key={`f${folder.id}`}
        {...p.row({ kind: "folder", id: folder.id }, folder.id)}
      >
        <button className={open ? "chevron open" : "chevron"} aria-label={folder.name} aria-expanded={open} onClick={() => p.onToggle(folder.id)}>
          ›
        </button>
        <button className="label" onClick={() => p.onSelect({ kind: "folder", id: folder.id })} onDoubleClick={() => p.onToggle(folder.id)}>
          {FolderIcon}
          <span className="name">{folder.name}</span>
        </button>
      </div>
      {open ? (
        <>
          {(p.tree.folders.get(folder.id) ?? []).map((f) => <TreeFolder key={f.id} folder={f} depth={depth + 1} {...p} />)}
          {(p.tree.entries.get(folder.id) ?? []).map((e) => (
            <EntryRow key={e.id} entry={e} depth={depth + 1} selected={p.isSel("entry", e.id)} onSelect={p.onSelect} attrs={p.row({ kind: "entry", id: e.id }, folder.id)} t={p.t} />
          ))}
        </>
      ) : null}
    </>
  );
}

type EntryRowProps = { entry: Entry; depth: number; selected: boolean; onSelect: (s: Sel) => void; attrs: HTMLAttributes<HTMLDivElement>; t: T };

function EntryRow({ entry, depth, selected, onSelect, attrs, t }: EntryRowProps) {
  const cls = ["node", selected ? "selected" : "", entry.enabled ? "" : "disabled"].join(" ");
  return (
    <div className={cls} style={{ paddingLeft: 6 + depth * 16 + 18 }} data-key={`e${entry.id}`} {...attrs}>
      <button className="label" onClick={() => onSelect({ kind: "entry", id: entry.id })}>
        {PhraseIcon}
        <span className="name">{entry.name || t("unnamed")}</span>
        {entry.trigger ? <kbd>{entry.trigger}</kbd> : entry.hotkey ? <kbd>{entry.hotkey}</kbd> : null}
      </button>
    </div>
  );
}

type EditorProps = {
  draft: Draft;
  setDraft: Dispatch<SetStateAction<Draft | null>>;
  folderOptions: FolderOption[];
  dirty: boolean;
  nameRef: RefObject<HTMLInputElement>;
  onSave: () => unknown;
  entries: Entry[];
  t: T;
};

/** [label, literal macro or a locale key holding a localized example] */
const MACRO_TEMPLATES: [Key, string][] = [
  ["mCursor", "{#cursor}"],
  ["mEnter", "{#enter}"],
  ["mTab", "{#tab}"],
  ["mTomorrow", "{#date +1d}"],
  ["mWeekday", "{#date dddd}"],
  ["mLongDate", "tLongDate"],
  ["mCalc", "tCalc"],
  ["mCounter", "tCounter"],
  ["mRandom", "tRandom"],
  ["mPhrase", "tPhrase"],
  ["mOpen", "tOpen"],
  ["mWait", "{#wait 500}"],
];

const FIELD_TEMPLATES: [Key, Key][] = [
  ["fieldText", "fieldTextLabel"],
  ["fieldMultiline", "fieldMultilineLabel"],
  ["fieldChoice", "fieldChoiceLabel"],
  ["fieldCheck", "fieldCheckLabel"],
  ["fieldDate", "fieldDateLabel"],
];

function Editor({ draft, setDraft, folderOptions, dirty, nameRef, onSave, entries, t }: EditorProps) {
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  // Other phrases with the same abbreviation / shortcut: typing it opens a menu to pick one.
  const sharing = (key: "trigger" | "hotkey") => {
    const v = draft[key].trim().toLowerCase();
    return v ? entries.filter((e) => e.id !== draft.id && e[key].toLowerCase() === v).map((e) => e.name || t("unnamed")) : [];
  };
  const sameTrigger = sharing("trigger");
  const sameHotkey = sharing("hotkey");
  const set = <K extends keyof Draft>(key: K, value: Draft[K]) => setDraft((d) => d && { ...d, [key]: value });

  function insert(token: string) {
    const ta = bodyRef.current;
    const start = ta?.selectionStart ?? draft.body.length;
    const end = ta?.selectionEnd ?? start;
    set("body", draft.body.slice(0, start) + token + draft.body.slice(end));
    requestAnimationFrame(() => {
      ta?.focus();
      ta?.setSelectionRange(start + token.length, start + token.length);
    });
  }

  return (
    <form className="editor" onSubmit={(e) => { e.preventDefault(); onSave(); }}>
      <label className="field">
        <span>{t("name")}</span>
        <input ref={nameRef} value={draft.name} onChange={(e) => set("name", e.target.value)} />
      </label>
      <label className="field grow">
        <span>{t("body")}</span>
        <textarea ref={bodyRef} spellCheck value={draft.body} onChange={(e) => set("body", e.target.value)} />
      </label>
      <div className="macros">
        <span className="muted">{t("insertMacro")}</span>
        <button type="button" onClick={() => insert("{#date}")}>{t("macroDate")}</button>
        <button type="button" onClick={() => insert("{#time}")}>{t("macroTime")}</button>
        <button type="button" onClick={() => insert("{#clipboard}")}>{t("macroClipboard")}</button>
        <select
          aria-label={t("macroField")}
          value=""
          onChange={(e) => {
            if (e.target.value) insert(`{#input ${t(e.target.value as Key)}}`);
          }}
        >
          <option value="">{t("macroField")}</option>
          {FIELD_TEMPLATES.map(([label, template]) => <option key={label} value={template}>{t(label)}</option>)}
        </select>
        <select
          aria-label={t("macroMore")}
          value=""
          onChange={(e) => {
            const v = e.target.value;
            if (v) insert(v.startsWith("{") ? v : t(v as Key));
          }}
        >
          <option value="">{t("macroMore")}</option>
          {MACRO_TEMPLATES.map(([label, template]) => <option key={label} value={template}>{t(label)}</option>)}
        </select>
      </div>
      <div className="row">
        <label className="field">
          <span>{t("trigger")}</span>
          <input value={draft.trigger} placeholder={t("triggerHint")} spellCheck={false} data-noexpand onChange={(e) => set("trigger", e.target.value)} />
        </label>
        <label className="field">
          <span>{t("mode")}</span>
          <select value={draft.immediate ? "i" : "w"} onChange={(e) => set("immediate", e.target.value === "i")}>
            <option value="w">{t("modeWord")}</option>
            <option value="i">{t("modeImmediate")}</option>
          </select>
        </label>
        <label className="field">
          <span>{t("hotkey")}</span>
          <HotkeyInput value={draft.hotkey} placeholder={t("hotkeyHint")} onChange={(v) => set("hotkey", v)} />
        </label>
      </div>
      {sameTrigger.length || sameHotkey.length ? (
        <p className="muted shared-note">
          {sameTrigger.length ? t("sharedTrigger", { names: sameTrigger.join(", ") }) : null}
          {sameTrigger.length && sameHotkey.length ? " " : null}
          {sameHotkey.length ? t("sharedHotkey", { names: sameHotkey.join(", ") }) : null}
        </p>
      ) : null}
      <div className="row">
        <label className="field">
          <span>{t("folder")}</span>
          <select value={draft.folderId ?? ""} onChange={(e) => set("folderId", e.target.value ? Number(e.target.value) : null)}>
            <option value="">{t("noFolder")}</option>
            {folderOptions.map((o) => <option key={o.id} value={o.id}>{o.label}</option>)}
          </select>
        </label>
        <label className="field">
          <span>{t("tags")}</span>
          <input value={draft.tagsText} placeholder={t("tagsHint")} onChange={(e) => set("tagsText", e.target.value)} />
        </label>
      </div>
      <div className="row checks">
        <label className="check">
          <input type="checkbox" checked={draft.caseSensitive} onChange={(e) => set("caseSensitive", e.target.checked)} />
          {t("caseSensitive")}
        </label>
        <label className="check">
          <input type="checkbox" checked={draft.enabled} onChange={(e) => set("enabled", e.target.checked)} />
          {t("enabled")}
        </label>
        <span className="spacer" />
        {dirty ? <span className="muted">{t("unsaved")}</span> : null}
        <button className="primary" type="submit" disabled={!dirty}>{t("save")}</button>
      </div>
    </form>
  );
}
