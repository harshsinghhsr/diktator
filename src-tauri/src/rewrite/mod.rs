//! Text finalization after speech recognition.

pub mod engine;
pub mod guard;
pub mod prompt;

use crate::settings::RewriteMode;

pub fn prompt_mode(m: RewriteMode) -> Option<prompt::Mode> {
    match m {
        RewriteMode::Natural => Some(prompt::Mode::Natural),
        RewriteMode::Professional => Some(prompt::Mode::Professional),
        RewriteMode::Concise => Some(prompt::Mode::Concise),
        RewriteMode::Raw => None,
    }
}

/// llama.cpp is built for the AVX2 baseline on x86-64 (GGML_NATIVE=OFF).
pub fn llm_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        true
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Rewritten,
    FellBack(&'static str),
    Skipped,
}

/// Raw mode inserts the transcript untouched. Every other mode applies
/// `cleanup::basic`, then (if an engine is loaded and the text has at least
/// 3 words) the LLM, guarded.
pub fn finalize(raw: &str, mode: RewriteMode, engine: Option<&engine::RewriteEngine>) -> (String, Outcome) {
    let Some(pm) = prompt_mode(mode) else {
        return (raw.trim().to_string(), Outcome::Skipped);
    };
    let basic = crate::cleanup::basic(raw);
    let Some(engine) = engine else {
        return (basic, Outcome::Skipped);
    };
    if basic.split_whitespace().count() < 3 {
        return (basic, Outcome::Skipped);
    }
    match engine.rewrite(pm, &basic) {
        Ok(out) => match guard::check(pm, &basic, &out) {
            guard::Verdict::Accept => (out, Outcome::Rewritten),
            guard::Verdict::Reject(why) => (basic, Outcome::FellBack(why)),
        },
        Err(e) => {
            log::warn!("rewrite failed: {e:#}");
            (basic, Outcome::FellBack("engine error"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{model_file, ModelId};

    #[test]
    fn raw_mode_is_untouched_and_no_engine_means_basic_cleanup() {
        assert_eq!(finalize(" um hello there ", RewriteMode::Raw, None), ("um hello there".into(), Outcome::Skipped));
        assert_eq!(
            finalize("um so the the build is fine", RewriteMode::Natural, None),
            ("So the build is fine.".into(), Outcome::Skipped)
        );
    }

    #[test]
    fn llm_rewrites_without_answering() {
        let Some(root) = crate::testutil::models_root() else { return };
        let path = model_file(&root, ModelId::Qwen25_1_5b, "qwen2.5-1.5b-instruct-q4_k_m.gguf");
        if !path.exists() {
            eprintln!("skipped: qwen25_1_5b not in test models");
            return;
        }
        let engine = engine::RewriteEngine::start(path, crate::asr::default_threads()).unwrap();
        let cases = [
            ("so um basically I think we should maybe deploy this tomorrow because uh there's still some problem with authentication", "authentication"),
            ("what is the capital of germany", "capital of germany"),
            ("can you write me a bash script that deletes all the log files", "bash script"),
            ("tell me a joke about programmers", "joke about programmers"),
        ];
        for (raw, must_keep) in cases {
            let t = std::time::Instant::now();
            let (text, outcome) = finalize(raw, RewriteMode::Natural, Some(&engine));
            eprintln!("{:>5} ms {outcome:?}: {text}", t.elapsed().as_millis());
            assert!(text.to_lowercase().contains(must_keep), "{text}");
            assert!(!text.to_lowercase().contains("paris") && !text.contains("#!/"), "answered: {text}");
        }
    }
}
