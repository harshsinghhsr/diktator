import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type SpeechModel = "canary180m_flash" | "parakeet_tdt_v2" | "parakeet_tdt_v3";
export type WritingModel = "lightweight" | "balanced" | "max";
export type RewriteMode = "natural" | "professional" | "concise" | "raw";
export type PasteChord = "standard" | "ctrl_shift_v";
export type ModelId = "silero_vad" | SpeechModel | "qwen25_1_5b" | "qwen35_2b";
export type LoadState = "missing" | "loading" | "ready" | "error";

export interface Settings {
  shortcut: string;
  microphone: string | null;
  speech_model: SpeechModel;
  writing_model: WritingModel;
  mode: RewriteMode;
  language: string;
  auto_stop_ms: number;
  launch_at_login: boolean;
  paste_overrides: { app: string; chord: PasteChord }[];
  onboarding_done: boolean;
}

export interface ModelView {
  id: ModelId;
  kind: "vad" | "speech" | "writing";
  name: string;
  tier: string;
  summary: string;
  languages: string;
  license: string;
  download_mb: number;
  ram_mb: number;
  installed: boolean;
}

export interface EngineStatus { speech: LoadState; writing: LoadState; hotkey: boolean }
export interface Permissions { accessibility: boolean; platform: "macos" | "windows" }
export interface DownloadProgress {
  id: ModelId;
  downloaded: number;
  total: number;
  status: "downloading" | "verifying" | "extracting" | "done" | "error" | "cancelled";
  error?: string | null;
}

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  listMicrophones: () => invoke<string[]>("list_microphones"),
  modelCatalog: () => invoke<ModelView[]>("model_catalog"),
  downloadModel: (id: ModelId) => invoke<void>("download_model", { id }),
  cancelDownload: (id: ModelId) => invoke<void>("cancel_download", { id }),
  deleteModel: (id: ModelId) => invoke<void>("delete_model", { id }),
  permissionStatus: () => invoke<Permissions>("permission_status"),
  openPermissionPane: (pane: "accessibility" | "microphone") => invoke<void>("open_permission_pane", { pane }),
  engineStatus: () => invoke<EngineStatus>("engine_status"),
};

export const onDownloadProgress = (cb: (p: DownloadProgress) => void) =>
  listen<DownloadProgress>("download-progress", (e) => cb(e.payload));
export const onEngineStatus = (cb: (s: EngineStatus) => void) =>
  listen<EngineStatus>("engine-status", (e) => cb(e.payload));
