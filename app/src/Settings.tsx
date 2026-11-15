import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type Settings, type SyncStatus } from "./api";
import { errText, resolveLang, type Key, type T } from "./i18n";
import { HotkeyInput } from "./HotkeyInput";

type Tab = "general" | "account" | "writing" | "clipboard" | "excluded" | "data";
const TABS: [Tab, Key][] = [
  ["general", "settingsGeneral"],
  ["account", "settingsAccountTab"],
  ["writing", "settingsWriting"],
  ["clipboard", "settingsClipboard"],
  ["excluded", "excludedApps"],
  ["data", "settingsData"],
];
type Msg = { text: string; error: boolean; title?: string }; // title = tooltip (e.g. the exported file path)
export const IMPORT_ACCEPT = ".json,.csv,.txt";

/** Exports the library (or one folder) to a shareable file; returns the status message. */
export async function exportLibrary(t: T, folderId: number | null = null): Promise<Msg> {
  try {
    const path = await api.exportData(folderId);
    return { text: t("exported"), error: false, title: path };
  } catch (e) {
    return { text: errText(t, e), error: true };
  }
}

/** Imports the file picked in a file input (then clears it); null if nothing was picked. */
export async function importPicked(t: T, input: HTMLInputElement): Promise<Msg | null> {
  const file = input.files?.[0];
  input.value = "";
  if (!file) return null;
  try {
    const r = await api.importData(await file.text(), file.name);
    return { text: t("imported", { phrases: r.phrases, folders: r.folders }), error: false };
  } catch (e) {
    return { text: errText(t, e), error: true };
  }
}

/** "Accedi con Google": library synced through the hidden app folder on the user's Google Drive. */
function CloudSync({ t }: { t: T }) {
  const [st, setSt] = useState<SyncStatus | null>(null);
  const [busy, setBusy] = useState<"login" | "sync" | null>(null);
  const [fail, setFail] = useState("");
  const [loginUrl, setLoginUrl] = useState("");
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    void api.syncStatus().then(setSt);
    const un = listen<SyncStatus>("sync-status", (ev) => setSt(ev.payload));
    const unUrl = listen<string>("sync-login-url", (ev) => setLoginUrl(ev.payload));
    return () => {
      void un.then((f) => f());
      void unUrl.then((f) => f());
    };
  }, []);

  async function run(kind: "login" | "sync" | "logout") {
    setFail("");
    setLoginUrl("");
    setCopied(false);
    setBusy(kind === "logout" ? null : kind);
    try {
      setSt(await (kind === "login" ? api.syncLogin() : kind === "sync" ? api.syncNow() : api.syncLogout()));
    } catch (e) {
      if (String(e) !== "google_login_cancelled") setFail(errText(t, e));
    } finally {
      setBusy(null);
    }
  }

  if (!st) return null;
  const error = fail || (st.error ? errText(t, st.error) : "") || (st.shareError ? errText(t, st.shareError) : "");
  return (
    <>
      <h2>{t("settingsSync")}</h2>
      <p className="muted">{t("syncHint")}</p>
      {!st.configured ? (
        <p className="muted">{t("errGoogleNotConfigured")}</p>
      ) : !st.email ? (
        <>
          <div className="row">
            <button type="button" className="primary" disabled={busy !== null} onClick={() => void run("login")}>
              {busy === "login" ? t("syncLoggingIn") : t("syncLogin")}
            </button>
            {busy === "login" ? <button type="button" onClick={() => void api.syncCancelLogin()}>{t("cancel")}</button> : null}
          </div>
          {busy === "login" && loginUrl ? (
            <p className="muted">
              {t("syncNoBrowser")}{" "}
              <button type="button" onClick={() => void navigator.clipboard.writeText(loginUrl).then(() => setCopied(true))}>
                {copied ? t("syncLinkCopied") : t("syncCopyLink")}
              </button>
            </p>
          ) : null}
        </>
      ) : (
        <>
          <p>
            {t("syncAccount", { email: st.email })}
            <br />
            <span className="muted">{busy === "sync" ? t("syncRunning") : st.last ? t("syncLast", { when: st.last }) : t("syncNever")}</span>
          </p>
          <div className="row">
            <button type="button" disabled={busy !== null} onClick={() => void run("sync")}>{t("syncNow")}</button>
            <button type="button" disabled={busy !== null} onClick={() => void run("logout")}>{t("syncLogout")}</button>
          </div>
        </>
      )}
      {error ? <p className="msg error" role="status">{error}</p> : null}
    </>
  );
}

/** Removable chips plus a field to add more (Enter or "Aggiungi"); `parse` turns the typed text into new items. */
function ListEditor({ items, onChange, parse, placeholder, email, t }: {
  items: string[];
  onChange: (items: string[]) => void;
  parse: (text: string) => string[];
  placeholder: string;
  email?: boolean;
  t: T;
}) {
  const [text, setText] = useState("");
  const add = () => {
    const extra = parse(text).filter((x, i, all) => !items.includes(x) && all.indexOf(x) === i);
    if (extra.length) onChange([...items, ...extra]);
    setText("");
  };
  return (
    <>
      <div className="chips" hidden={!items.length}>
        {items.map((a) => (
          <span key={a} className="chip">
            {a}
            <button type="button" aria-label={`${t("remove")} ${a}`} onClick={() => onChange(items.filter((x) => x !== a))}>×</button>
          </span>
        ))}
      </div>
      <div className="row">
        <input
          className="grow-input"
          type={email ? "email" : "text"}
          data-noexpand={email || undefined}
          value={text}
          placeholder={placeholder}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add();
            }
          }}
        />
        <button type="button" onClick={add} disabled={!parse(text).length}>{t("add")}</button>
      </div>
    </>
  );
}

export function SettingsView({ initial, onSaved, onImported, t }: { initial: Settings; onSaved: (s: Settings) => void; onImported: () => void; t: T }) {
  const [s, setS] = useState<Settings>(() => ({ ...initial, lang: resolveLang(initial.lang) }));
  const [recent, setRecent] = useState<string[]>([]);
  const [msg, setMsg] = useState<Msg | null>(null);
  const [tab, setTab] = useState<Tab>("general");


  useEffect(() => {
    void api.runningApps().then(setRecent);
  }, []);

  // Autosave: every change is written shortly after it is made (no Save button).
  const last = useRef(s); // what is saved
  const latest = useRef(s); // what is on screen
  latest.current = s;
  async function save(next: Settings) {
    const prev = last.current;
    last.current = next;
    try {
      await api.saveSettings(next);
      setMsg((m) => (m?.error ? null : m)); // an old error is fixed now
      onSaved(next);
      if (next.shareWith.join() !== prev.shareWith.join()) void api.syncNow(); // publish the new list now, not in a few minutes
    } catch (e) {
      setMsg({ text: errText(t, e), error: true });
      if (String(e) === "admin_cancelled") {
        const back = { ...next, runAsAdmin: prev.runAsAdmin }; // the box goes back, nothing to save again
        last.current = back;
        setS(back);
      } else onSaved(next); // saved anyway, only the shortcut registration failed
    }
  }
  const saveRef = useRef(save);
  saveRef.current = save;
  useEffect(() => {
    if (s === last.current) return;
    const id = setTimeout(() => void saveRef.current(s), 600);
    return () => clearTimeout(id);
  }, [s]);
  // leaving Settings within the 600 ms still saves the last change
  useEffect(() => () => void (latest.current !== last.current && saveRef.current(latest.current)), []);
  // settings changed on another PC (sync): show them, unless something is waiting to be saved here
  useEffect(() => {
    if (initial === last.current || latest.current !== last.current) return;
    const next = { ...initial, lang: resolveLang(initial.lang) };
    last.current = next;
    setS(next);
  }, [initial]);

  const suggestions = recent.filter((r) => !s.excludedApps.includes(r) && r !== "phrasex-app.exe");

  return (
    <div className="settings-layout">
      <nav className="settings-tabs" aria-label={t("settings")}>
        {TABS.map(([k, label]) => (
          <button key={k} type="button" className={tab === k ? "on" : undefined} onClick={() => setTab(k)}>{t(label)}</button>
        ))}
      </nav>
      <form className="editor settings" onSubmit={(e) => e.preventDefault()}>
        {tab === "general" ? (
          <>
            <h2>{t("settingsGeneral")}</h2>
            <div className="row">
              <label className="field">
                <span>{t("language")}</span>
                <select value={s.lang} onChange={(e) => setS({ ...s, lang: e.target.value })}>
                  <option value="it">Italiano</option>
                  <option value="en">English</option>
                </select>
              </label>
              <label className="field">
                <span>{t("popupHotkey")}</span>
                <HotkeyInput value={s.popupHotkey} placeholder={t("hotkeyHint")} onChange={(v) => setS({ ...s, popupHotkey: v })} />
              </label>
            </div>
            <div className="row">
              <label className="field">
                <span>{t("popupSort")}</span>
                <select value={s.popupSort} onChange={(e) => setS({ ...s, popupSort: e.target.value as Settings["popupSort"] })}>
                  <option value="uses">{t("popupSortUses")}</option>
                  <option value="name">{t("popupSortName")}</option>
                  <option value="tree">{t("popupSortTree")}</option>
                </select>
              </label>
              <label className="field">
                <span>{t("theme")}</span>
                <select value={s.theme} onChange={(e) => setS({ ...s, theme: e.target.value as Settings["theme"] })}>
                  <option value="system">{t("themeSystem")}</option>
                  <option value="light">{t("themeLight")}</option>
                  <option value="dark">{t("themeDark")}</option>
                </select>
              </label>
            </div>
            <label className="check">
              <input type="checkbox" checked={s.autostart} onChange={(e) => setS({ ...s, autostart: e.target.checked })} />
              {t("autostart")}
            </label>
            <label className="check" title={t("runAsAdminHint")}>
              <input type="checkbox" checked={s.runAsAdmin} onChange={(e) => setS({ ...s, runAsAdmin: e.target.checked })} />
              {t("runAsAdmin")}
            </label>
          </>
        ) : null}
        {tab === "account" ? (
          <>
            <CloudSync t={t} />

            <h2>{t("settingsShare")}</h2>
            <p className="muted">{t("shareHint")}</p>
            <ListEditor
              items={s.shareWith}
              onChange={(shareWith) => setS({ ...s, shareWith })}
              parse={(text) => text.split(/[\s,;]+/).map((x) => x.trim().toLowerCase()).filter((x) => x.includes("@"))}
              placeholder={t("shareAdd")}
              email
              t={t}
            />
          </>
        ) : null}
        {tab === "writing" ? (
          <>
            <h2>{t("settingsWriting")}</h2>
            <label className="check">
              <input type="checkbox" checked={s.fixDoubleCaps} onChange={(e) => setS({ ...s, fixDoubleCaps: e.target.checked })} />
              {t("fixDoubleCaps")}
            </label>
            <label className="check">
              <input type="checkbox" checked={s.capitalizeSentences} onChange={(e) => setS({ ...s, capitalizeSentences: e.target.checked })} />
              {t("capitalizeSentences")}
            </label>
            <div className="row">
              <button type="button" onClick={async () => {
                try {
                  setMsg({ text: t("autocorrectAdded", { n: await api.installAutocorrect() }), error: false });
                  onImported();
                } catch (e) {
                  setMsg({ text: errText(t, e), error: true });
                }
              }}>{t("installAutocorrect")}</button>
            </div>
          </>
        ) : null}
        {tab === "clipboard" ? (
          <>
            <h2>{t("settingsClipboard")}</h2>
            <label className="check" title={t("clipHistoryTip")}>
              <input type="checkbox" checked={s.clipHistory} onChange={(e) => setS({ ...s, clipHistory: e.target.checked })} />
              {t("clipHistory")}
            </label>
            <div className="row">
              <label className="field">
                <span>{t("clipHotkey")}</span>
                <HotkeyInput value={s.clipHotkey} placeholder={t("hotkeyHint")} onChange={(v) => setS({ ...s, clipHotkey: v })} />
              </label>
              <span className="field" />
            </div>
          </>
        ) : null}
        {tab === "excluded" ? (
          <>
            <h2>{t("excludedApps")}</h2>
            <p className="muted">{t("excludedHint")}</p>
            <ListEditor
              items={s.excludedApps}
              onChange={(excludedApps) => setS({ ...s, excludedApps })}
              parse={(text) => [text.trim().toLowerCase()].filter(Boolean)}
              placeholder={t("excludedAdd")}
              t={t}
            />
            {suggestions.length ? (
              <div className="chips">
                <span className="muted">{t("recentApps")}</span>
                {suggestions.map((a) => (
                  <button key={a} type="button" className="chip suggestion" onClick={() => setS({ ...s, excludedApps: [...s.excludedApps, a] })}>+ {a}</button>
                ))}
              </div>
            ) : null}
          </>
        ) : null}
        {tab === "data" ? (
          <>
            <h2>{t("settingsData")}</h2>
            <p className="muted">{t("dataHint")}</p>
            <div className="row">
              <button type="button" onClick={async () => setMsg(await exportLibrary(t))}>{t("exportData")}</button>
              <label className="button-like" title={t("importTip")}>
                {t("importData")}
                <input type="file" accept={IMPORT_ACCEPT} hidden onChange={async (e) => {
                  const r = await importPicked(t, e.currentTarget);
                  if (r) {
                    setMsg(r);
                    if (!r.error) onImported();
                  }
                }} />
              </label>
              <button type="button" onClick={() => void api.openBackups()}>{t("openBackups")}</button>
            </div>
          </>
        ) : null}
        {msg ? <p className={msg.error ? "msg error" : "msg"} role="status" title={msg.title}>{msg.text}</p> : null}
      </form>
    </div>
  );
}
