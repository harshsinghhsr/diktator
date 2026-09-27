//! Downloadable models. URLs are pinned to an immutable GitHub release asset
//! or a Hugging Face commit, and every file is verified by SHA-256.

use crate::settings::{SpeechModel, WritingModel};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelId {
    SileroVad,
    Canary180mFlash,
    ParakeetTdtV2,
    ParakeetTdtV3,
    Qwen25_1_5b,
    Qwen35_2b,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Vad,
    Speech,
    Writing,
}

pub struct ModelInfo {
    pub id: ModelId,
    pub kind: Kind,
    pub name: &'static str,
    pub tier: &'static str,
    pub summary: &'static str,
    pub languages: &'static str,
    pub license: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub download_bytes: u64,
    /// Approximate memory once loaded. An estimate, shown to the user as "~".
    pub ram_mb: u32,
    /// `true`: a `.tar.bz2` extracted into the model dir. `false`: a single file stored as `files[0]`.
    pub archive: bool,
    /// Paths relative to the model dir that must exist after install.
    pub files: &'static [&'static str],
}

macro_rules! sherpa_url {
    ($file:literal) => {
        concat!("https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/", $file)
    };
}

pub const CATALOG: &[ModelInfo] = &[
    ModelInfo {
        id: ModelId::SileroVad,
        kind: Kind::Vad,
        name: "Silero VAD",
        tier: "Required",
        summary: "Detects when you start and stop speaking.",
        languages: "Any",
        license: "MIT",
        url: sherpa_url!("silero_vad.onnx"),
        sha256: "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6",
        download_bytes: 643_854,
        ram_mb: 20,
        archive: false,
        files: &["silero_vad.onnx"],
    },
    ModelInfo {
        id: ModelId::Canary180mFlash,
        kind: Kind::Speech,
        name: "NVIDIA Canary 180M Flash",
        tier: "Lightweight",
        summary: "Smallest and fastest. Slightly less accurate.",
        languages: "English, Spanish, German, French",
        license: "CC-BY-4.0 (NVIDIA)",
        url: sherpa_url!("sherpa-onnx-nemo-canary-180m-flash-en-es-de-fr-int8.tar.bz2"),
        sha256: "7a38ed8b13f014ad632b09ff8d22e0c6f1359dd046af9235d281dfae841b9ab9",
        download_bytes: 153_692_328,
        ram_mb: 400,
        archive: true,
        files: &[
            "sherpa-onnx-nemo-canary-180m-flash-en-es-de-fr-int8/encoder.int8.onnx",
            "sherpa-onnx-nemo-canary-180m-flash-en-es-de-fr-int8/decoder.int8.onnx",
            "sherpa-onnx-nemo-canary-180m-flash-en-es-de-fr-int8/tokens.txt",
        ],
    },
    ModelInfo {
        id: ModelId::ParakeetTdtV2,
        kind: Kind::Speech,
        name: "NVIDIA Parakeet TDT 0.6B v2",
        tier: "Balanced",
        summary: "Most accurate English model. Recommended.",
        languages: "English",
        license: "CC-BY-4.0 (NVIDIA)",
        url: sherpa_url!("sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2"),
        sha256: "157c157bc51155e03e37d2466522a3a737dd9c72bb25f36eb18912964161e1ad",
        download_bytes: 482_468_385,
        ram_mb: 1000,
        archive: true,
        files: &[
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/encoder.int8.onnx",
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/decoder.int8.onnx",
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/joiner.int8.onnx",
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/tokens.txt",
        ],
    },
    ModelInfo {
        id: ModelId::ParakeetTdtV3,
        kind: Kind::Speech,
        name: "NVIDIA Parakeet TDT 0.6B v3",
        tier: "Multilingual",
        summary: "25 European languages, detected automatically.",
        languages: "25 European languages",
        license: "CC-BY-4.0 (NVIDIA)",
        url: sherpa_url!("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2"),
        sha256: "5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf",
        download_bytes: 487_170_055,
        ram_mb: 1000,
        archive: true,
        files: &[
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/encoder.int8.onnx",
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/decoder.int8.onnx",
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/joiner.int8.onnx",
            "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/tokens.txt",
        ],
    },
    ModelInfo {
        id: ModelId::Qwen25_1_5b,
        kind: Kind::Writing,
        name: "Qwen2.5 1.5B Instruct",
        tier: "Balanced",
        summary: "Cleans up dictation in about half a second on Apple Silicon. Recommended.",
        languages: "English and most major languages",
        license: "Apache-2.0",
        url: "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/91cad51170dc346986eccefdc2dd33a9da36ead9/qwen2.5-1.5b-instruct-q4_k_m.gguf",
        sha256: "6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e",
        download_bytes: 1_117_320_736,
        ram_mb: 1500,
        archive: false,
        files: &["qwen2.5-1.5b-instruct-q4_k_m.gguf"],
    },
    ModelInfo {
        id: ModelId::Qwen35_2b,
        kind: Kind::Writing,
        name: "Qwen3.5 2B",
        tier: "Max quality",
        summary: "Better filler removal, about twice as slow.",
        languages: "English and most major languages",
        license: "Apache-2.0",
        url: "https://huggingface.co/bartowski/Qwen_Qwen3.5-2B-GGUF/resolve/7d26695454df6de5fbcce2e58681e62dae06ce43/Qwen_Qwen3.5-2B-Q4_K_M.gguf",
        sha256: "57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8",
        download_bytes: 1_396_198_496,
        ram_mb: 1800,
        archive: false,
        files: &["Qwen_Qwen3.5-2B-Q4_K_M.gguf"],
    },
];

impl ModelId {
    pub fn info(self) -> &'static ModelInfo {
        CATALOG.iter().find(|m| m.id == self).expect("every ModelId has a CATALOG entry")
    }

    /// Directory name under the models root. Equals the serde wire name.
    pub fn slug(self) -> &'static str {
        match self {
            ModelId::SileroVad => "silero_vad",
            ModelId::Canary180mFlash => "canary180m_flash",
            ModelId::ParakeetTdtV2 => "parakeet_tdt_v2",
            ModelId::ParakeetTdtV3 => "parakeet_tdt_v3",
            ModelId::Qwen25_1_5b => "qwen25_1_5b",
            ModelId::Qwen35_2b => "qwen35_2b",
        }
    }
}

impl From<SpeechModel> for ModelId {
    fn from(m: SpeechModel) -> Self {
        match m {
            SpeechModel::Canary180mFlash => ModelId::Canary180mFlash,
            SpeechModel::ParakeetTdtV2 => ModelId::ParakeetTdtV2,
            SpeechModel::ParakeetTdtV3 => ModelId::ParakeetTdtV3,
        }
    }
}

/// `None` for Lightweight (rule-based cleanup, no model).
pub fn writing_model_id(m: WritingModel) -> Option<ModelId> {
    match m {
        WritingModel::Lightweight => None,
        WritingModel::Balanced => Some(ModelId::Qwen25_1_5b),
        WritingModel::Max => Some(ModelId::Qwen35_2b),
    }
}

pub fn model_dir(root: &Path, id: ModelId) -> PathBuf {
    root.join(id.slug())
}

pub fn model_file(root: &Path, id: ModelId, rel: &str) -> PathBuf {
    model_dir(root, id).join(rel)
}

pub fn is_installed(root: &Path, id: ModelId) -> bool {
    id.info().files.iter().all(|f| model_file(root, id, f).is_file())
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelView {
    pub id: ModelId,
    pub kind: Kind,
    pub name: &'static str,
    pub tier: &'static str,
    pub summary: &'static str,
    pub languages: &'static str,
    pub license: &'static str,
    pub download_mb: u64,
    pub ram_mb: u32,
    pub installed: bool,
}

pub fn views(root: &Path) -> Vec<ModelView> {
    CATALOG
        .iter()
        .map(|m| ModelView {
            id: m.id,
            kind: m.kind,
            name: m.name,
            tier: m.tier,
            summary: m.summary,
            languages: m.languages,
            license: m.license,
            download_mb: m.download_bytes.div_ceil(1_000_000),
            ram_mb: m.ram_mb,
            installed: is_installed(root, m.id),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_has_exactly_one_entry_with_sane_fields() {
        let ids = [
            ModelId::SileroVad,
            ModelId::Canary180mFlash,
            ModelId::ParakeetTdtV2,
            ModelId::ParakeetTdtV3,
            ModelId::Qwen25_1_5b,
            ModelId::Qwen35_2b,
        ];
        assert_eq!(CATALOG.len(), ids.len());
        for id in ids {
            let m = id.info();
            assert_eq!(CATALOG.iter().filter(|x| x.id == id).count(), 1);
            assert_eq!(m.sha256.len(), 64, "{id:?}");
            assert!(m.url.starts_with("https://"), "{id:?}");
            assert!(!m.files.is_empty());
            assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{}\"", id.slug()));
            if m.archive {
                assert!(m.url.ends_with(".tar.bz2"));
            }
        }
    }

    #[test]
    fn settings_map_to_models() {
        assert_eq!(ModelId::from(SpeechModel::ParakeetTdtV2), ModelId::ParakeetTdtV2);
        assert_eq!(writing_model_id(WritingModel::Lightweight), None);
        assert_eq!(writing_model_id(WritingModel::Balanced), Some(ModelId::Qwen25_1_5b));
        assert_eq!(ModelId::from(SpeechModel::Canary180mFlash).info().kind, Kind::Speech);
    }

    #[test]
    fn installed_only_when_all_files_exist() {
        let root = std::env::temp_dir().join(format!("diktator-catalog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(!is_installed(&root, ModelId::SileroVad));
        std::fs::create_dir_all(model_dir(&root, ModelId::SileroVad)).unwrap();
        std::fs::write(model_file(&root, ModelId::SileroVad, "silero_vad.onnx"), b"x").unwrap();
        assert!(is_installed(&root, ModelId::SileroVad));
        assert!(views(&root).iter().any(|v| v.id == ModelId::SileroVad && v.installed));
    }
}
