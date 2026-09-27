import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

type OverlayState =
  | { state: "hidden" }
  | { state: "listening" }
  | { state: "processing" }
  | { state: "message"; text: string };

// `overlay-level` arrives once per 32 ms audio window as dBFS -60..0 mapped to
// 0..1. Speech lives around 0.5–0.8 and room noise around 0.2, so stretch
// that band to the full height; silence stays a thin line.
const GATE = 0.3;
const SPAN = 0.5;
const shape = (v: number) => 0.08 + 0.92 * Math.min(1, Math.max(0, (v - GATE) / SPAN));

const INTERVAL_MS = 32; // one audio window
const BAR = 2.5;
const PITCH = 4.5; // bar + gap, in CSS px
const INK = "#1e1b18";

/** Scrolling waveform: each audio window becomes a bar entering on the right. */
function Waveform() {
  const canvas = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const bars: number[] = [];
    let lastAt = performance.now();
    const off = listen<number>("overlay-level", (e) => {
      bars.push(shape(e.payload));
      if (bars.length > 80) bars.shift();
      lastAt = performance.now();
    });

    let raf = 0;
    const draw = () => {
      raf = requestAnimationFrame(draw);
      const c = canvas.current;
      const g = c?.getContext("2d");
      if (!c || !g) return;
      const dpr = window.devicePixelRatio || 1;
      const w = c.clientWidth, h = c.clientHeight;
      if (c.width !== Math.round(w * dpr)) { c.width = Math.round(w * dpr); c.height = Math.round(h * dpr); }
      g.setTransform(dpr, 0, 0, dpr, 0, 0);
      g.clearRect(0, 0, w, h);
      g.fillStyle = INK;
      // Glide one pitch between samples so the stream scrolls smoothly even
      // when IPC events arrive unevenly.
      const glide = Math.min(1, (performance.now() - lastAt) / INTERVAL_MS);
      for (let i = bars.length - 1; i >= 0; i--) {
        const x = w - (bars.length - 1 - i + glide) * PITCH - BAR;
        if (x < -BAR) break;
        const bh = Math.max(2, bars[i] * h);
        g.beginPath();
        g.roundRect(x, (h - bh) / 2, BAR, bh, BAR / 2);
        g.fill();
      }
    };
    raf = requestAnimationFrame(draw);
    return () => { cancelAnimationFrame(raf); off.then((f) => f()); };
  }, []);

  return <canvas ref={canvas} className="wave" aria-hidden="true" />;
}

export default function Overlay() {
  const [s, setS] = useState<OverlayState>({ state: "hidden" });

  useEffect(() => {
    const off = listen<OverlayState>("overlay-state", (e) => setS(e.payload));
    return () => { off.then((f) => f()); };
  }, []);

  if (s.state === "hidden") return null;
  return (
    <div className="overlay-wrap">
      <div className={`pill ${s.state}`} role="status" aria-live="polite">
        {s.state === "listening" && (
          <>
            <span className="rec" aria-hidden="true" />
            <span>Listening</span>
            <Waveform />
          </>
        )}
        {s.state === "processing" && (
          <>
            <span className="hop" aria-hidden="true"><span /><span /><span /></span>
            <span>Writing it up…</span>
          </>
        )}
        {s.state === "message" && <span className="msg">{s.text}</span>}
      </div>
    </div>
  );
}
