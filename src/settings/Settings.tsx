import { useCallback, useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import {
  api, onDownloadProgress, onEngineStatus,
  type DownloadProgress, type EngineStatus, type LoadState, type ModelId, type ModelView,
  type Permissions, type RewriteMode, type Settings as S, type WritingModel,
} from "../ipc";
import ShortcutRecorder from "./ShortcutRecorder";

const MODES: { id: RewriteMode; label: string; example: string }[] = [
  { id: "natural", label: "Natural", example: "Hey, can you check this PR when you get some time? I think there might be an issue with authentication." },
  { id: "professional", label: "Professional", example: "Could you review this PR when you have a chance? I believe there may be an issue with the authentication flow." },
  { id: "concise", label: "Concise", example: "Could you review this PR? There may be an issue with authentication." },
  { id: "raw", label: "Raw", example: "hey can you like check this PR whenever you get some time because I think there might be some issue with the auth thing" },
];
const AUTO_STOP = [0, 500, 700, 1000, 1500];
const LANGS: Record<string, string> = { en: "English", es: "Spanish", de: "German", fr: "French" };
const RECOMMENDED: ModelId[] = ["silero_vad", "parakeet_tdt_v2", "qwen25_1_5b"];

const Icon = {
  mic: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5 11a7 7 0 0 0 14 0M12 18v3" /></svg>,
  check: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3.2" strokeLinecap="round" strokeLinejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5" /></svg>,
  down: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round"><path d="M12 4v11m-5-5 5 5 5-5M5 20h14" /></svg>,
};

const LOAD: Record<LoadState, { text: string; tone: string }> = {
  ready: { text: "ready", tone: "teal" },
  loading: { text: "loading…", tone: "butter" },
  missing: { text: "not downloaded", tone: "coral" },
  error: { text: "unavailable", tone: "coral" },
};

function Panel({ title, swatch, i, aside, className = "", children }: {
  title: string; swatch: string; i: number; aside?: ReactNode; className?: string; children: ReactNode;
}) {
  return (
    <section className={`brutal panel rise ${className}`} style={{ "--i": i } as CSSProperties}>
      <header>
        <h2><span className="swatch" style={{ background: `var(--${swatch})` }} />{title}</h2>
        {aside}
      </header>
      {children}
    </section>
  );
}

export default function Settings() {
  const [settings, setSettings] = useState<S | null>(null);
  const [mics, setMics] = useState<string[]>([]);
  const [models, setModels] = useState<ModelView[]>([]);
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});
  const [engine, setEngine] = useState<EngineStatus | null>(null);
  const [perms, setPerms] = useState<Permissions | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The latest settings we asked for. `set` composes onto this rather than a
  // render closure, so two quick changes don't overwrite each other.
  const latest = useRef<S | null>(null);

  const refreshModels = useCallback(() => api.modelCatalog().then(setModels), []);
  const load = useCallback(() => api.getSettings().then((s) => { latest.current = s; setSettings(s); }), []);

  useEffect(() => {
    load();
    api.listMicrophones().then(setMics);
    api.engineStatus().then(setEngine);
    refreshModels();
    const offP = onDownloadProgress((p) => {
      setProgress((all) => ({ ...all, [p.id]: p }));
      if (p.status === "done") refreshModels();
    });
    const offE = onEngineStatus(setEngine);
    const poll = () => api.permissionStatus().then(setPerms);
    poll();
    const timer = window.setInterval(poll, 2000);
    return () => { offP.then((f) => f()); offE.then((f) => f()); window.clearInterval(timer); };
  }, [refreshModels, load]);

  const set = useCallback(<K extends keyof S>(k: K, v: S[K]) => {
    if (!latest.current) return;
    const next = { ...latest.current, [k]: v };
    latest.current = next;
    setSettings(next);
    setError(null);
    api.saveSettings(next).catch((e) => { setError(String(e)); load(); });
  }, [load]);
  const setShortcut = useCallback((s: string) => set("shortcut", s), [set]);

  if (!settings) return <main className="settings"><p className="muted">Loading…</p></main>;

  const speech = models.filter((m) => m.kind === "speech");
  const writing = models.filter((m) => m.kind === "writing");
  const vad = models.find((m) => m.id === "silero_vad");
  const installed = (id: ModelId) => models.find((m) => m.id === id)?.installed ?? false;
  const writingId = (w: WritingModel): ModelId | null => (w === "balanced" ? "qwen25_1_5b" : w === "max" ? "qwen35_2b" : null);
  const mode = MODES.find((m) => m.id === settings.mode) ?? MODES[0];
  const isMac = perms?.platform === "macos";

  const modelActions = (m: ModelView) => {
    const p = progress[m.id];
    if (p && ["downloading", "verifying", "extracting"].includes(p.status)) {
      const pct = p.total ? Math.floor((p.downloaded / p.total) * 100) : 0;
      return (
        <div className="model-actions">
          <div className="meter" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100} aria-label={`${m.name} download`}>
            <span style={{ width: `${Math.max(4, pct)}%` }} />
          </div>
          <span className="small muted">{p.status === "downloading" ? `${pct}%` : p.status === "verifying" ? "Checking…" : "Unpacking…"}</span>
          <button className="btn ghost" onClick={(e) => { e.preventDefault(); api.cancelDownload(m.id); }}>Cancel</button>
        </div>
      );
    }
    return (
      <div className="model-actions">
        {m.installed
          ? <button className="btn ghost" onClick={(e) => { e.preventDefault(); api.deleteModel(m.id).then(refreshModels).catch((err) => setError(String(err))); }}>Delete</button>
          : <button className="btn butter" onClick={(e) => { e.preventDefault(); api.downloadModel(m.id); }}>{Icon.down} Download {m.download_mb} MB</button>}
        {p?.status === "error" && <span className="err-text small">{p.error}</span>}
      </div>
    );
  };

  const modelCard = (key: string, name: string, selected: boolean, onSelect: () => void, body: ReactNode) => (
    <label key={key} className={`model ${selected ? "selected" : ""}`}>
      <input type="radio" name={name} checked={selected} onChange={onSelect} />
      <span className="tick" aria-hidden="true">{Icon.check}</span>
      <span className="model-body">{body}</span>
    </label>
  );

  const catalogCard = (m: ModelView, name: string, selected: boolean, onSelect: () => void) =>
    modelCard(m.id, name, selected, onSelect, <>
      <span className="model-title">{m.name} <span className="tag">{m.tier}</span>{m.installed && <span className="tag teal">on this device</span>}</span>
      <span className="muted">{m.summary}</span>
      <span className="facts"><span>{m.languages}</span><span>{m.download_mb} MB</span><span>~{(m.ram_mb / 1000).toFixed(1)} GB RAM</span><span>{m.license}</span></span>
      {modelActions(m)}
    </>);

  const hotkey = engine?.hotkey;
  const speechState = LOAD[engine?.speech ?? "missing"];
  const writingState = settings.writing_model === "lightweight" ? { text: "built-in rules", tone: "lavender" } : LOAD[engine?.writing ?? "missing"];

  return (
    <main className="settings">
      <header className="masthead rise">
        <span className="logo" aria-hidden="true">{Icon.mic}</span>
        <div>
          <h1>Diktator</h1>
          <p>Talk into any text field. Your voice and words stay on this {isMac ? "Mac" : "PC"}.</p>
        </div>
        <div className="chips" aria-label="Status">
          <span className={`tag ${hotkey ? "teal" : "coral"}`}><span className="dot" />Shortcut {hotkey ? "on" : "unavailable"}</span>
          <span className={`tag ${speechState.tone}`}><span className="dot" />Speech {speechState.text}</span>
          <span className={`tag ${writingState.tone}`}><span className="dot" />Writing {writingState.text}</span>
        </div>
      </header>

      {error && <p className="brutal alert" role="alert">{error}</p>}

      {!settings.onboarding_done && (
        <Panel title="Let's get you talking" swatch="coral" i={1} className="onboarding">
          <ol className="steps">
            {isMac && (
              <li className={perms?.accessibility ? "done" : ""}>
                <div className="step-body">
                  <span>Allow Accessibility so Diktator can paste into other apps.</span>
                  {!perms?.accessibility && <button className="btn" onClick={() => api.openPermissionPane("accessibility")}>Open Accessibility settings</button>}
                </div>
              </li>
            )}
            <li className={RECOMMENDED.every(installed) ? "done" : ""}>
              <div className="step-body">
                <span>Download the recommended models, about 1.6 GB, once.</span>
                {!RECOMMENDED.every(installed) && (
                  <button className="btn" onClick={() => RECOMMENDED.filter((id) => !installed(id)).forEach((id) => api.downloadModel(id))}>{Icon.down} Download models</button>
                )}
              </div>
            </li>
            <li>
              <div className="step-body">
                <span>Click into any text field, hold your shortcut, speak, and let go. The first time, macOS asks for the microphone.</span>
                <button className="btn" onClick={() => api.openPermissionPane("microphone")}>Microphone settings</button>
              </div>
            </li>
          </ol>
          <button className="btn coral" style={{ justifySelf: "start" }} onClick={() => set("onboarding_done", true)}>Finish setup</button>
        </Panel>
      )}

      <Panel title="Shortcut" swatch="lavender" i={2}>
        <ShortcutRecorder value={settings.shortcut} onChange={setShortcut} />
        {!hotkey && engine && (
          <p className="err-text small">That shortcut couldn't be registered; another app may be using it. Pick a different one.</p>
        )}
        {isMac && !perms?.accessibility && (
          <button className="btn coral" style={{ justifySelf: "start" }} onClick={() => api.openPermissionPane("accessibility")}>Allow pasting (Accessibility)</button>
        )}
      </Panel>

      <Panel title="Writing style" swatch="butter" i={3}>
        <div className="tabs" role="radiogroup" aria-label="Writing style">
          {MODES.map((m) => (
            <button key={m.id} role="radio" aria-checked={settings.mode === m.id} onClick={() => set("mode", m.id)}>{m.label}</button>
          ))}
        </div>
        <p className="bubble" key={mode.id}>
          <span className="said">You'll get something like</span>
          {mode.example}
        </p>
      </Panel>

      <Panel title="Listening" swatch="teal" i={4}
        aside={<button className="btn ghost" onClick={() => api.openPermissionPane("microphone")}>Microphone settings</button>}>
        <div className="field">
          <label className="label" htmlFor="mic">Microphone</label>
          <select id="mic" className="select" value={settings.microphone ?? ""} onChange={(e) => set("microphone", e.target.value || null)}>
            <option value="">System default</option>
            {mics.map((m) => <option key={m} value={m}>{m}</option>)}
          </select>
        </div>
        <div className="field">
          <span className="label" id="autostop">Stop after a pause (tap mode)</span>
          <div className="tabs" role="radiogroup" aria-labelledby="autostop">
            {AUTO_STOP.map((ms) => (
              <button key={ms} role="radio" aria-checked={settings.auto_stop_ms === ms} onClick={() => set("auto_stop_ms", ms)}>
                {ms === 0 ? "Never" : `${ms / 1000} s`}
              </button>
            ))}
          </div>
        </div>
        {vad && !vad.installed && (
          <div className="field">
            <span className="label">Pause detection</span>
            <span className="muted small">A tiny 0.6 MB model that notices when you stop talking.</span>
            {modelActions(vad)}
          </div>
        )}
      </Panel>

      <Panel title="Speech model" swatch="coral" i={5}>
        <div className="models" role="radiogroup" aria-label="Speech model">
          {speech.map((m) => catalogCard(m, "speech", settings.speech_model === m.id, () => set("speech_model", m.id as S["speech_model"])))}
        </div>
        {settings.speech_model === "canary180m_flash" && (
          <div className="field">
            <label className="label" htmlFor="lang">Language</label>
            <select id="lang" className="select" value={settings.language} onChange={(e) => set("language", e.target.value)}>
              {Object.entries(LANGS).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
            </select>
          </div>
        )}
      </Panel>

      <Panel title="Writing model" swatch="tangerine" i={6}>
        <div className="models" role="radiogroup" aria-label="Writing model">
          {modelCard("lightweight", "writing", settings.writing_model === "lightweight", () => set("writing_model", "lightweight"), <>
            <span className="model-title">Built-in rules <span className="tag">Lightweight</span></span>
            <span className="muted">Drops "um", "uh" and repeated words, and fixes capitals and full stops. Instant, no download.</span>
          </>)}
          {writing.map((m) => {
            const tier: WritingModel = m.id === "qwen25_1_5b" ? "balanced" : "max";
            return catalogCard(m, "writing", writingId(settings.writing_model) === m.id, () => set("writing_model", tier));
          })}
        </div>
      </Panel>

      <Panel title="General" swatch="blush" i={7}>
        <label className="toggle">
          <input type="checkbox" checked={settings.launch_at_login} onChange={(e) => set("launch_at_login", e.target.checked)} />
          <span className="switch" aria-hidden="true" />
          Start Diktator when I log in
        </label>
      </Panel>
    </main>
  );
}
