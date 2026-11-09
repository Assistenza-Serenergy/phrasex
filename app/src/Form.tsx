import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type FormField, type Lang } from "./api";
import { resolveLang, translator } from "./i18n";

type Values = Record<string, string | boolean>;

const pad = (n: number) => String(n).padStart(2, "0");
const today = () => {
  const d = new Date(); // local date, not UTC
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};

function initialValues(fields: FormField[]): Values {
  return Object.fromEntries(
    fields.map((f) => [f.label, f.kind === "check" ? false : f.kind === "choice" ? (f.options[0] ?? "") : f.kind === "date" ? today() : ""]),
  );
}

/** Final text for each field: dates as dd/MM/yyyy, ticked boxes as their text. */
function answers(fields: FormField[], values: Values): Record<string, string> {
  return Object.fromEntries(
    fields.map((f) => {
      const v = values[f.label];
      if (f.kind === "check") return [f.label, v ? f.checkText : ""];
      if (f.kind === "date" && typeof v === "string" && v) {
        const [y, m, d] = v.split("-");
        return [f.label, `${d}/${m}/${y}`];
      }
      return [f.label, String(v ?? "")];
    }),
  );
}

/** Fill-in form shown while expanding a phrase with {#input} fields. */
export function FormWindow() {
  const [lang, setLang] = useState<Lang>("it");
  const t = useMemo(() => translator(lang), [lang]);
  const [fields, setFields] = useState<FormField[] | null>(null);
  const [values, setValues] = useState<Values>({});

  useEffect(() => {
    const load = (f: FormField[] | null) => {
      setFields(f);
      setValues(f ? initialValues(f) : {});
      void api.settings().then((s) => setLang(resolveLang(s.lang)));
    };
    void api.formPending().then(load);
    const un = listen<FormField[]>("form-request", (ev) => load(ev.payload));
    return () => void un.then((f) => f());
  }, []);

  if (!fields) return <div className="form-window" />;

  const set = (label: string, v: string | boolean) => setValues((s) => ({ ...s, [label]: v }));
  const submit = () => {
    void api.formSubmit(answers(fields, values));
    setFields(null);
  };
  const cancel = () => {
    void api.formSubmit(null);
    setFields(null);
  };

  return (
    <form
      className="form-window"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") cancel();
        if (e.key === "Enter" && e.ctrlKey) submit();
      }}
    >
      <h1>{t("formTitle")}</h1>
      <div className="form-fields">
        {fields.map((f, i) => {
          const v = values[f.label];
          const id = `f${i}`;
          if (f.kind === "check") {
            return (
              <label key={f.label} className="check">
                <input type="checkbox" autoFocus={i === 0} checked={v === true} onChange={(e) => set(f.label, e.target.checked)} />
                {f.label}
              </label>
            );
          }
          return (
            <label key={f.label} className="field" htmlFor={id}>
              <span>{f.label}</span>
              {f.kind === "multiline" ? (
                <textarea id={id} rows={4} autoFocus={i === 0} value={String(v)} onChange={(e) => set(f.label, e.target.value)} />
              ) : f.kind === "choice" ? (
                <select id={id} autoFocus={i === 0} value={String(v)} onChange={(e) => set(f.label, e.target.value)}>
                  {f.options.map((o) => <option key={o}>{o}</option>)}
                </select>
              ) : (
                <input id={id} type={f.kind === "date" ? "date" : "text"} autoFocus={i === 0} value={String(v)} onChange={(e) => set(f.label, e.target.value)} />
              )}
            </label>
          );
        })}
      </div>
      <div className="row end">
        <button type="button" onClick={cancel}>{t("cancel")}</button>
        <button type="submit" className="primary">{t("insert")}</button>
      </div>
    </form>
  );
}
