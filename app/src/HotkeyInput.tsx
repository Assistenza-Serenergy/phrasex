import type { KeyboardEvent } from "react";
import { api } from "./api";

const MODIFIER_CODES = new Set(["ControlLeft", "ControlRight", "ShiftLeft", "ShiftRight", "AltLeft", "AltRight", "MetaLeft", "MetaRight"]);

/** "KeyK" -> "K", "Digit1" -> "1", "F5" -> "F5", "Space" -> "Space". */
function keyName(code: string) {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  return code;
}

function shortcutFrom(e: KeyboardEvent): string | null {
  if (MODIFIER_CODES.has(e.code)) return null;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
  const key = keyName(e.code);
  // Without modifiers only function keys make sense as global shortcuts.
  if (!mods.length && !/^F\d{1,2}$/.test(key)) return null;
  return [...mods, key].join("+");
}

/** Click, then press the key combination. Backspace/Delete clears it. */
export function HotkeyInput({ value, onChange, placeholder, id }: { value: string; onChange: (v: string) => void; placeholder: string; id?: string }) {
  return (
    <input
      id={id}
      className="hotkey"
      readOnly
      data-noexpand
      value={value}
      // global shortcuts would swallow the combination: switch them off while recording
      onFocus={() => void api.setRecordingHotkey(true).catch(() => {})}
      onBlur={() => void api.setRecordingHotkey(false).catch(() => {})}
      placeholder={placeholder}
      onKeyDown={(e) => {
        if (e.key === "Tab") return;
        e.preventDefault();
        if (!e.ctrlKey && !e.altKey && (e.key === "Backspace" || e.key === "Delete")) return onChange("");
        const s = shortcutFrom(e);
        if (s) onChange(s);
      }}
    />
  );
}
