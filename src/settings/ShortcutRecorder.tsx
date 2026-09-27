import { useEffect, useState } from "react";

const IS_MAC = navigator.userAgent.includes("Mac");

// Apple's modifier order; also the order we store them in.
const MODS = ["ctrl", "alt", "shift", "cmd"] as const;
type Mod = (typeof MODS)[number];

const MOD_LABEL: Record<Mod, string> = IS_MAC
  ? { ctrl: "⌃", alt: "⌥", shift: "⇧", cmd: "⌘" }
  : { ctrl: "Ctrl", alt: "Alt", shift: "Shift", cmd: "Win" };

const NAMED: Record<string, string> = {
  Space: "space", Enter: "return", Tab: "tab", Backspace: "backspace", Delete: "forwarddelete",
  Insert: "insert", Home: "home", End: "end", PageUp: "pageup", PageDown: "pagedown",
  ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down",
  Minus: "minus", Equal: "equal", BracketLeft: "leftbracket", BracketRight: "rightbracket",
  Backslash: "backslash", Semicolon: "semicolon", Quote: "quote", Comma: "comma",
  Period: "period", Slash: "slash", Backquote: "grave",
};

const KEY_LABEL: Record<string, string> = {
  space: "Space", return: "↩", tab: "⇥", backspace: "⌫", forwarddelete: "⌦",
  left: "←", right: "→", up: "↑", down: "↓", minus: "-", equal: "=",
  leftbracket: "[", rightbracket: "]", backslash: "\\", semicolon: ";", quote: "'",
  comma: ",", period: ".", slash: "/", grave: "`", pageup: "PgUp", pagedown: "PgDn",
};

/** Physical key (KeyboardEvent.code) → the stored shortcut key name (see hotkey.rs), or null if unsupported. */
export function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (/^F\d{1,2}$/.test(code)) return code.toLowerCase();
  if (/^Numpad\d$/.test(code)) return `keypad${code.slice(6)}`;
  return NAMED[code] ?? null;
}

function parse(shortcut: string): { mods: Mod[]; key: string } {
  const parts = shortcut.toLowerCase().split("+").map((p) => p.trim());
  const alias: Record<string, Mod> = { cmd: "cmd", command: "cmd", meta: "cmd", super: "cmd", win: "cmd", ctrl: "ctrl", control: "ctrl", alt: "alt", opt: "alt", option: "alt", shift: "shift" };
  const mods = MODS.filter((m) => parts.some((p) => alias[p] === m));
  const key = parts.find((p) => !(p in alias)) ?? "";
  return { mods, key };
}

const label = (key: string) => KEY_LABEL[key] ?? (key.length === 1 ? key.toUpperCase() : key.replace(/^f(\d)/, "F$1").replace(/^keypad/, "Num "));

export default function ShortcutRecorder({ value, onChange }: { value: string; onChange: (s: string) => void }) {
  const [recording, setRecording] = useState(false);
  const [held, setHeld] = useState<Mod[]>([]);
  const [hint, setHint] = useState<string | null>(null);

  useEffect(() => {
    if (!recording) return;
    const mods = (e: KeyboardEvent): Mod[] =>
      MODS.filter((m) => ({ ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, cmd: e.metaKey })[m]);
    const down = (e: KeyboardEvent) => {
      e.preventDefault();
      if (e.code === "Escape") { setRecording(false); return; }
      const m = mods(e);
      setHeld(m);
      const key = keyName(e.code);
      if (!key) return; // a modifier on its own, or a key we can't register
      if (m.length === 0 && !/^f\d/.test(key)) {
        setHint(`Add ${IS_MAC ? "⌘, ⌃, ⌥ or ⇧" : "Ctrl, Alt, Shift or Win"} so normal typing isn't caught.`);
        return;
      }
      setRecording(false);
      onChange([...m, key].join("+"));
    };
    const up = (e: KeyboardEvent) => setHeld(mods(e));
    const stop = () => setRecording(false);
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    window.addEventListener("blur", stop);
    return () => {
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
      window.removeEventListener("blur", stop);
    };
  }, [recording, onChange]);

  const start = () => { setHeld([]); setHint(null); setRecording(true); };
  const current = parse(value);

  return (
    <div className="field">
      <div className={`keys ${recording ? "recording" : ""}`} aria-live="polite">
        {recording ? (
          held.length ? held.map((m) => <kbd key={m} className="keycap mod held">{MOD_LABEL[m]}</kbd>)
            : <span className="prompt">Press your new shortcut…</span>
        ) : (
          <>
            <span className="sr-only">Current shortcut:</span>
            {current.mods.map((m, i) => (
              <kbd key={m} className="keycap mod" style={{ animationDelay: `${i * 60}ms` }}>{MOD_LABEL[m]}</kbd>
            ))}
            {current.key && <kbd className="keycap" style={{ animationDelay: `${current.mods.length * 60}ms` }}>{label(current.key)}</kbd>}
          </>
        )}
        {recording && <span className="caret" aria-hidden="true" />}
      </div>
      <div className="row-actions">
        {recording
          ? <button className="btn ghost" onClick={() => setRecording(false)}>Cancel (Esc)</button>
          : <button className="btn lavender" onClick={start}>Change shortcut</button>}
        <span className="muted small">
          {hint ?? "Hold to talk, or tap to start and tap again (or pause) to finish. Esc cancels a dictation."}
        </span>
      </div>
    </div>
  );
}
