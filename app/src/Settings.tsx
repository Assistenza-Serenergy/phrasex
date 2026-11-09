import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type Settings, type SyncStatus } from "./api";
import { errText, resolveLang, type T } from "./i18n";
import { HotkeyInput } from "./HotkeyInput";

type Msg = { text: string; error: boolean };
export const IMPORT_ACCEPT = ".json,.csv,.txt";

/** Exports the library (or one folder) to a shareable file; returns the status message. */
export async function exportLibrary(t: T, folderId: number | null = null): Promise<Msg> {
  try {
    return { text: t("exported", { path: await api.exportData(folderId) }), error: false };
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
  const error = fail || (st.error ? errText(t, st.error) : "");
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

export function SettingsView({ initial, onSaved, onImported, t }: { initial: Settings; onSaved: (s: Settings) => void; onImported: () => void; t: T }) {
  const [s, setS] = useState<Settings>(() => ({ ...initial, lang: resolveLang(initial.lang) }));
  const [recent, setRecent] = useState<string[]>([]);
  const [newApp, setNewApp] = useState("");
  const [msg, setMsg] = useState<Msg | null>(null);


  useEffect(() => {
    void api.runningApps().then(setRecent);
  }, []);

  const addApp = (name: string) => {
    const n = name.trim().toLowerCase();
    if (n && !s.excludedApps.includes(n)) setS({ ...s, excludedApps: [...s.excludedApps, n] });
    setNewApp("");
  };

  async function save() {
    try {
      await api.saveSettings(s);
      setMsg({ text: t("saved"), error: false });
      onSaved(s);
    } catch (e) {
      setMsg({ text: errText(t, e), error: true });
      onSaved(s); // saved anyway, only the shortcut registration failed
    }
  }

  const suggestions = recent.filter((r) => !s.excludedApps.includes(r) && r !== "phrasex-app.exe");

  return (
    <form className="editor settings" onSubmit={(e) => { e.preventDefault(); void save(); }}>
      <CloudSync t={t} />

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
      <label className="check">
        <input type="checkbox" checked={s.autostart} onChange={(e) => setS({ ...s, autostart: e.target.checked })} />
        {t("autostart")}
      </label>
      <label className="check">
        <input type="checkbox" checked={s.runAsAdmin} onChange={(e) => setS({ ...s, runAsAdmin: e.target.checked })} />
        {t("runAsAdmin")}
      </label>
      <p className="muted">{t("runAsAdminHint")}</p>

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

      <h2>{t("settingsClipboard")}</h2>
      <label className="check">
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

      <h2>{t("excludedApps")}</h2>
      <p className="muted">{t("excludedHint")}</p>
      <div className="chips" hidden={!s.excludedApps.length}>
        {s.excludedApps.map((a) => (
          <span key={a} className="chip">
            {a}
            <button type="button" aria-label={`${t("remove")} ${a}`} onClick={() => setS({ ...s, excludedApps: s.excludedApps.filter((x) => x !== a) })}>×</button>
          </span>
        ))}
      </div>
      <div className="row">
        <input
          className="grow-input"
          value={newApp}
          placeholder={t("excludedAdd")}
          onChange={(e) => setNewApp(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addApp(newApp);
            }
          }}
        />
        <button type="button" onClick={() => addApp(newApp)} disabled={!newApp.trim()}>{t("add")}</button>
      </div>
      {suggestions.length ? (
        <div className="chips">
          <span className="muted">{t("recentApps")}</span>
          {suggestions.map((a) => (
            <button key={a} type="button" className="chip suggestion" onClick={() => addApp(a)}>+ {a}</button>
          ))}
        </div>
      ) : null}

      <h2>{t("settingsData")}</h2>
      <p className="muted">{t("dataHint")}</p>
      <div className="row">
        <button type="button" onClick={async () => setMsg(await exportLibrary(t))}>{t("exportData")}</button>
        <label className="button-like">
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

      <div className="row checks">
        {msg ? <span className={msg.error ? "msg error" : "msg"} role="status">{msg.text}</span> : null}
        <span className="spacer" />
        <button className="primary" type="submit">{t("save")}</button>
      </div>
    </form>
  );
}
