import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { api, type Clip, type Entry, type Lang } from "./api";
import { resolveLang, translator } from "./i18n";

type Item = Entry & { folder: string };
/** phrases/clips = search popup; choose = several phrases share what was typed, pick one. */
type Mode = "phrases" | "clips" | "choose";

/** Lower score = better match. */
function score(e: Item, q: string): number {
  const name = e.name.toLowerCase();
  const trig = e.trigger.toLowerCase();
  if (trig === q) return 0;
  if (trig.startsWith(q)) return 1;
  if (name.startsWith(q)) return 2;
  if (name.includes(q) || trig.includes(q)) return 3;
  if (e.body.toLowerCase().includes(q) || e.tags.some((t) => t.toLowerCase().includes(q)) || e.folder.toLowerCase().includes(q)) return 4;
  return 99;
}

/** Search-and-insert window opened by the global shortcuts: phrases or clipboard history. */
export function Popup() {
  const [lang, setLang] = useState<Lang>("it");
  const t = useMemo(() => translator(lang), [lang]);
  const [mode, setMode] = useState<Mode>("phrases");
  const [items, setItems] = useState<Item[]>([]);
  const [clips, setClips] = useState<Clip[]>([]);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const [chooseIds, setChooseIds] = useState<number[]>([]);
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);

  async function load(next: Mode) {
    setMode(next);
    setQuery("");
    setIndex(0);
    if (next === "choose") rootRef.current?.focus();
    else inputRef.current?.focus();
    const [lib, settings, history, ids] = await Promise.all([api.library(), api.settings(), api.clips(), next === "choose" ? api.choicePending() : null]);
    setChooseIds(ids ?? []);
    const byId = new Map(lib.folders.map((f) => [f.id, f]));
    const path = (id: number | null): string => {
      const f = id === null ? undefined : byId.get(id);
      return f ? [path(f.parentId), f.name].filter(Boolean).join(" › ") : "";
    };
    // Most used phrases first when nothing is typed.
    setItems(
      lib.entries
        .filter((e) => e.enabled)
        .map((e) => ({ ...e, folder: path(e.folderId) }))
        .toSorted((a, b) => b.uses - a.uses),
    );
    setClips(history);
    setLang(resolveLang(settings.lang));
  }

  useEffect(() => {
    void load("phrases");
    const unShow = listen<Mode>("popup-show", (ev) => void load(ev.payload === "clips" || ev.payload === "choose" ? ev.payload : "phrases"));
    const unFocus = getCurrentWebviewWindow().onFocusChanged(({ payload: focused }) => {
      if (!focused) void api.popupHide();
    });
    return () => {
      void unShow.then((f) => f());
      void unFocus.then((f) => f());
    };
  }, []);

  const phraseResults = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items
      .map((e) => ({ e, s: score(e, q) }))
      .filter((x) => x.s < 99)
      .sort((a, b) => a.s - b.s || b.e.uses - a.e.uses)
      .map((x) => x.e);
  }, [items, query]);

  const clipResults = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? clips.filter((c) => c.text.toLowerCase().includes(q)) : clips;
  }, [clips, query]);

  // the chooser and the search view are different elements: focus the right one after it renders
  useEffect(() => (mode === "choose" ? rootRef.current : inputRef.current)?.focus(), [mode, chooseIds]);

  const chosen = useMemo(() => chooseIds.flatMap((id) => items.filter((e) => e.id === id)), [chooseIds, items]);
  const count = mode === "phrases" ? phraseResults.length : mode === "clips" ? clipResults.length : chosen.length;
  const current = Math.min(index, Math.max(count - 1, 0));

  function move(delta: number) {
    const next = Math.max(0, Math.min(count - 1, current + delta));
    setIndex(next);
    listRef.current?.children[next]?.scrollIntoView({ block: "nearest" });
  }

  function choose(i: number) {
    if (mode === "choose" && chosen[i]) void api.choicePick(chosen[i].id);
    if (mode === "phrases" && phraseResults[i]) void api.popupInsert(phraseResults[i].id);
    if (mode === "clips" && clipResults[i]) void api.clipInsert(clipResults[i].id);
  }

  function switchMode(next: Mode) {
    setMode(next);
    setIndex(0);
    inputRef.current?.focus();
  }

  async function deleteClip(id: number) {
    await api.clipDelete(id);
    setClips((c) => c.filter((x) => x.id !== id));
  }

  if (mode === "choose") {
    const sel = chosen[current];
    return (
      <div
        className="popup chooser"
        ref={rootRef}
        tabIndex={-1}
        onKeyDown={(e) => {
          const digit = Number(e.key);
          if (e.key === "ArrowDown") move(1);
          else if (e.key === "ArrowUp") move(-1);
          else if (e.key === "Enter" || e.key === "Tab") choose(current);
          else if (e.key === "Escape") void api.choicePick(null);
          else if (digit >= 1 && digit <= 9) choose(digit - 1);
          else return;
          e.preventDefault();
        }}
      >
        <div className="chooser-title">{t("chooseTitle")}</div>
        <ul className="popup-list" ref={listRef} role="listbox" aria-label={t("chooseTitle")}>
          {chosen.map((e, i) => (
            <li key={e.id} role="option" aria-selected={i === current} className={i === current ? "active" : undefined} onMouseMove={() => i !== current && setIndex(i)} onClick={() => choose(i)}>
              <div className="popup-row">
                <span className="chooser-num">{i < 9 ? i + 1 : ""}</span>
                <span className="popup-name">{e.name || t("unnamed")}</span>
                {e.trigger ? <kbd>{e.trigger}</kbd> : null}
              </div>
            </li>
          ))}
        </ul>
        {sel ? (
          <div className="chooser-preview">
            <div className="chooser-path">{sel.folder || t("noFolderPath")}</div>
            <div className="chooser-body">{sel.body}</div>
          </div>
        ) : null}
        <footer className="popup-hint"><span>{t("chooseHint")}</span></footer>
      </div>
    );
  }

  return (
    <div
      className="popup"
      onKeyDown={(e) => {
        if (e.key === "ArrowDown") (e.preventDefault(), move(1));
        else if (e.key === "ArrowUp") (e.preventDefault(), move(-1));
        else if (e.key === "Enter") choose(current);
        else if (e.key === "Tab") (e.preventDefault(), switchMode(mode === "phrases" ? "clips" : "phrases"));
        else if (e.key === "Escape") void api.popupHide();
      }}
    >
      <div className="popup-tabs" role="tablist">
        <button role="tab" aria-selected={mode === "phrases"} className={mode === "phrases" ? "on" : undefined} onClick={() => switchMode("phrases")}>
          {t("tabPhrases")}
        </button>
        <button role="tab" aria-selected={mode === "clips"} className={mode === "clips" ? "on" : undefined} onClick={() => switchMode("clips")}>
          {t("tabClips")}
        </button>
      </div>
      <input
        ref={inputRef}
        className="popup-search"
        value={query}
        placeholder={t("popupPlaceholder")}
        aria-label={t("popupPlaceholder")}
        autoFocus
        onChange={(e) => {
          setQuery(e.target.value);
          setIndex(0);
        }}
      />
      <ul className="popup-list" ref={listRef} role="listbox">
        {mode === "phrases"
          ? phraseResults.map((e, i) => (
              <li key={e.id} role="option" aria-selected={i === current} className={i === current ? "active" : undefined} onMouseMove={() => i !== current && setIndex(i)} onClick={() => choose(i)}>
                <div className="popup-row">
                  <span className="popup-name">{e.name || t("unnamed")}</span>
                  {e.trigger ? <kbd>{e.trigger}</kbd> : null}
                </div>
                <div className="popup-preview">
                  {e.folder ? <span className="popup-folder">{e.folder} · </span> : null}
                  {e.body.split("\n")[0]}
                </div>
              </li>
            ))
          : clipResults.map((c, i) => (
              <li key={c.id} role="option" aria-selected={i === current} className={i === current ? "active" : undefined} onMouseMove={() => i !== current && setIndex(i)} onClick={() => choose(i)}>
                <div className="clip-text">{c.text}</div>
                <div className="clip-actions">
                  <button type="button" onClick={(ev) => (ev.stopPropagation(), void api.clipToPhrase(c.id))}>{t("clipToPhrase")}</button>
                  <button type="button" onClick={(ev) => (ev.stopPropagation(), void deleteClip(c.id))}>{t("clipDelete")}</button>
                </div>
              </li>
            ))}
        {count ? null : <li className="popup-empty">{mode === "clips" && !query ? t("clipEmpty") : t("noResults")}</li>}
      </ul>
      <footer className="popup-hint">
        <span>{mode === "clips" ? t("clipHint") : t("popupHint")}</span>
        {mode === "clips" && clips.length ? (
          <button type="button" onClick={() => void api.clipClear().then(() => setClips([]))}>{t("clipClear")}</button>
        ) : null}
      </footer>
    </div>
  );
}
