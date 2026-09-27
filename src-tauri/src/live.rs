//! Work done while the user is still talking, so little is left when they stop.
//! Pause-delimited audio segments are transcribed as soon as they close, and
//! finished sentences are rewritten once a later sentence shows they were not
//! corrected ("…Thursday. No wait, Friday."). At key-up only the last segment
//! and the last few sentences remain.

use crate::asr::SpeechRecognizer;
use crate::audio::{SAMPLE_RATE, WINDOW};
use crate::rewrite::{self, engine::RewriteEngine, Outcome};
use crate::settings::RewriteMode;
use anyhow::{anyhow, Result};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

/// ~320 ms of silence ends a segment: a breath, not a gap inside a word.
const CUT_SILENCE_WINDOWS: usize = 10;
/// Shorter segments lose too much context for the recognizer to be worth it.
const MIN_SEGMENT: usize = 3 * SAMPLE_RATE as usize;
/// Past this, cut at the first silent window even if the pause is short.
const MAX_SEGMENT: usize = 20 * SAMPLE_RATE as usize;
/// The last ~160 ms of a pause opens the next segment instead of closing this one.
const CARRY_WINDOWS: usize = 5;
/// Rewriting fewer words at a time costs more than it saves.
const MIN_COMMIT_WORDS: usize = 15;

/// Splits the 16 kHz stream into pause-delimited segments.
#[derive(Default)]
pub struct Segmenter {
    buf: Vec<f32>,
    silent: usize,
    heard: bool,
}

impl Segmenter {
    /// Feeds one VAD window; returns a finished segment when a pause closes one.
    pub fn push(&mut self, window: &[f32], speech: bool) -> Option<Vec<f32>> {
        self.buf.extend_from_slice(window);
        if speech {
            self.heard = true;
            self.silent = 0;
            return None;
        }
        self.silent += 1;
        let paused = self.silent >= CUT_SILENCE_WINDOWS && self.buf.len() >= MIN_SEGMENT;
        if self.heard && (paused || self.buf.len() >= MAX_SEGMENT) {
            let carry = self.buf.split_off(self.buf.len() - CARRY_WINDOWS.min(self.silent) * WINDOW);
            self.heard = false;
            self.silent = 0;
            return Some(std::mem::replace(&mut self.buf, carry));
        }
        None
    }
}

const CORRECTION_CUES: [&str; 9] =
    ["no", "wait", "actually", "sorry", "i mean", "i meant", "or rather", "scratch that", "correction"];

fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        let end = i + c.len_utf8();
        let next_is_space = text[end..].starts_with(char::is_whitespace);
        let ends = matches!(c, '?' | '!') || (c == '.' && !text[..i].ends_with('.') && !text[end..].starts_with('.'));
        if ends && next_is_space {
            out.push(text[start..end].trim());
            start = end;
        }
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

fn starts_correction(sentence: &str) -> bool {
    let words: Vec<String> = sentence
        .split_whitespace()
        .take(2)
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase())
        .collect();
    let two = words.join(" ");
    CORRECTION_CUES.iter().any(|cue| two == *cue || words.first().is_some_and(|w| w == cue))
}

/// Which leading sentences of `pending` are safe to rewrite now: every
/// sentence up to the last one that is followed by a sentence that is not a
/// correction. The last sentence is never committed (it may still be growing).
fn split_commit(pending: &str) -> Option<(String, String)> {
    let parts = sentences(pending);
    for k in (1..parts.len()).rev() {
        let next = parts[k];
        if k == parts.len() - 1 && next.split_whitespace().count() < 3 {
            continue; // too little of it heard to know how it starts
        }
        if starts_correction(next) {
            continue;
        }
        let commit = parts[..k].join(" ");
        if commit.split_whitespace().count() >= MIN_COMMIT_WORDS {
            return Some((commit, parts[k..].join(" ")));
        }
    }
    None
}

enum Msg {
    Segment(Vec<f32>),
    Finish(Vec<f32>, Sender<Result<Finished>>),
}

pub struct Finished {
    pub raw: String,
    pub text: String,
    pub outcome: Outcome,
    /// Work left after key-up: transcribing the tail and rewriting the rest.
    pub tail_asr_ms: u128,
    pub tail_rewrite_ms: u128,
    pub segments: usize,
}

/// Cheap handle the audio thread uses to hand over finished segments.
#[derive(Clone)]
pub struct Feed(Sender<Msg>);

impl Feed {
    pub fn push(&self, segment: Vec<f32>) {
        let _ = self.0.send(Msg::Segment(segment));
    }
}

/// One dictation's background worker. Dropping it (cancel) lets the worker
/// finish its current step and exit without blocking the caller.
pub struct Live {
    tx: Sender<Msg>,
}

impl Live {
    pub fn start(asr: Arc<SpeechRecognizer>, llm: Option<Arc<RewriteEngine>>, mode: RewriteMode) -> Live {
        let (tx, rx) = mpsc::channel();
        // ponytail: ASR and rewrite share one worker, so a long rewrite delays the
        // next segment's decode; split into two threads if segments start queueing.
        let _ = thread::Builder::new().name("live-dictation".into()).spawn(move || work(rx, asr, llm, mode));
        Live { tx }
    }

    pub fn feed(&self) -> Feed {
        Feed(self.tx.clone())
    }

    /// Blocks until queued segments are done, then transcribes `tail` (audio
    /// after the last segment) and rewrites whatever was not committed yet.
    pub fn finish(self, tail: Vec<f32>) -> Result<Finished> {
        let (reply, rx) = mpsc::channel();
        self.tx.send(Msg::Finish(tail, reply)).map_err(|_| anyhow!("dictation worker stopped"))?;
        rx.recv().map_err(|_| anyhow!("dictation worker stopped"))?
    }
}

/// Words so far, split into the part already rewritten and the part not yet.
#[derive(Default)]
struct Transcript {
    raw: Vec<String>,
    pending: String,
    done: Vec<String>,
    outcomes: Vec<Outcome>,
}

impl Transcript {
    fn add(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        // Parakeet ends every segment with a full stop; when the next segment
        // continues in lowercase, the cut was mid-sentence and the stop is fake.
        if text.starts_with(char::is_lowercase) && self.pending.ends_with('.') && !self.pending.ends_with("..") {
            self.pending.pop();
        }
        if !self.pending.is_empty() {
            self.pending.push(' ');
        }
        self.pending.push_str(&text);
        self.raw.push(text);
    }

    fn rewrite(&mut self, chunk: &str, mode: RewriteMode, llm: Option<&RewriteEngine>) {
        let (text, outcome) = rewrite::finalize(chunk, mode, llm);
        if !text.is_empty() {
            self.done.push(text);
        }
        self.outcomes.push(outcome);
    }

    fn outcome(&mut self) -> Outcome {
        let fell_back = self.outcomes.iter().position(|o| matches!(o, Outcome::FellBack(_)));
        match fell_back {
            Some(i) => self.outcomes.swap_remove(i),
            None if self.outcomes.contains(&Outcome::Rewritten) => Outcome::Rewritten,
            None => Outcome::Skipped,
        }
    }
}

fn work(rx: Receiver<Msg>, asr: Arc<SpeechRecognizer>, llm: Option<Arc<RewriteEngine>>, mode: RewriteMode) {
    let mut t = Transcript::default();
    let mut segments = 0;
    // Recognition errors mid-dictation are kept and reported at the end,
    // rather than silently dropping part of what was said.
    let mut failed: Option<anyhow::Error> = None;
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Segment(audio) => {
                segments += 1;
                match asr.transcribe(&audio) {
                    Ok(text) => t.add(text),
                    Err(e) => failed = failed.or(Some(e)),
                }
                while let Some((commit, rest)) = split_commit(&t.pending) {
                    t.pending = rest;
                    t.rewrite(&commit, mode, llm.as_deref());
                }
            }
            Msg::Finish(tail, reply) => {
                let started = Instant::now();
                if let Some(e) = failed.take() {
                    let _ = reply.send(Err(e));
                    return;
                }
                if tail.len() >= WINDOW {
                    match asr.transcribe(&tail) {
                        Ok(text) => t.add(text),
                        Err(e) => {
                            let _ = reply.send(Err(e));
                            return;
                        }
                    }
                }
                let tail_asr_ms = started.elapsed().as_millis();
                let started = Instant::now();
                let rest = std::mem::take(&mut t.pending);
                if !rest.is_empty() {
                    t.rewrite(&rest, mode, llm.as_deref());
                }
                let _ = reply.send(Ok(Finished {
                    raw: t.raw.join(" "),
                    text: t.done.join(" "),
                    outcome: t.outcome(),
                    tail_asr_ms,
                    tail_rewrite_ms: started.elapsed().as_millis(),
                    segments,
                }));
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = crate::audio::WINDOW;

    fn feed(s: &mut Segmenter, windows: usize, speech: bool) -> Vec<Vec<f32>> {
        (0..windows).filter_map(|_| s.push(&[0.1; W], speech)).collect()
    }

    #[test]
    fn cuts_at_a_pause_after_enough_speech() {
        let mut s = Segmenter::default();
        assert!(feed(&mut s, 100, true).is_empty(), "3.2 s of speech, no pause yet");
        let cuts = feed(&mut s, CUT_SILENCE_WINDOWS, false);
        assert_eq!(cuts.len(), 1);
        assert_eq!(cuts[0].len(), (100 + CUT_SILENCE_WINDOWS - CARRY_WINDOWS) * W);
    }

    #[test]
    fn carries_the_end_of_a_pause_into_the_next_segment() {
        // VAD flags silence a little late, so the last "silent" windows can hold
        // the onset of the next word: they must start the next segment, whole.
        let mut s = Segmenter::default();
        let mut cut: Vec<f32> = Vec::new();
        for i in 0..(100 + CUT_SILENCE_WINDOWS) {
            let w = [i as f32; W];
            if let Some(seg) = s.push(&w, i < 100) {
                cut = seg;
            }
        }
        let next_start = (100 + CUT_SILENCE_WINDOWS - CARRY_WINDOWS) as f32;
        assert_eq!(*cut.last().unwrap(), next_start - 1.0);
        assert!(feed(&mut s, 200, true).is_empty());
        let more = feed(&mut s, CUT_SILENCE_WINDOWS, false);
        assert_eq!(more[0][0], next_start, "no sample lost or duplicated at the cut");
    }

    #[test]
    fn short_pauses_and_short_segments_do_not_cut() {
        let mut s = Segmenter::default();
        feed(&mut s, 100, true);
        assert!(feed(&mut s, CUT_SILENCE_WINDOWS - 1, false).is_empty(), "pause too short");
        let mut s = Segmenter::default();
        feed(&mut s, 30, true); // ~1 s of speech
        assert!(feed(&mut s, 40, false).is_empty(), "segment too short to be worth a decode");
    }

    #[test]
    fn silence_alone_never_cuts() {
        let mut s = Segmenter::default();
        assert!(feed(&mut s, 1000, false).is_empty());
    }

    #[test]
    fn long_speech_is_cut_at_the_first_silent_window() {
        let mut s = Segmenter::default();
        let max_windows = MAX_SEGMENT / W;
        assert!(feed(&mut s, max_windows + 50, true).is_empty(), "never cut mid-word");
        assert_eq!(feed(&mut s, 1, false).len(), 1);
    }

    #[test]
    fn commits_sentences_once_a_later_one_follows() {
        let text = "I looked at the logs this morning and the gateway keeps timing out after thirty seconds. That is way too short for the export job we run every night. We should bump it";
        let (commit, rest) = split_commit(text).expect("two finished sentences and a third one started");
        assert_eq!(commit, "I looked at the logs this morning and the gateway keeps timing out after thirty seconds. That is way too short for the export job we run every night.");
        assert_eq!(rest, "We should bump it");
    }

    #[test]
    fn keeps_a_sentence_that_the_next_one_corrects() {
        let text = "Remind me to pick up the dry cleaning and the groceries on the way home on Thursday. No wait, Friday, because Thursday I have the dentist";
        assert_eq!(split_commit(text), None);
        let text = "Send the full report to the design team before the review meeting tomorrow afternoon. I mean the product team";
        assert_eq!(split_commit(text), None);
    }

    /// Streams a real clip through VAD → Segmenter → Live exactly like the
    /// controller does, then times what is left at "key-up".
    /// `DIKTATOR_LIVE_WAV=/path/clip.wav` swaps in a longer clip for timing.
    #[test]
    fn transcribes_while_speaking_and_finishes_the_tail() {
        use crate::catalog::{model_file, ModelId};
        use crate::settings::SpeechModel;
        let Some(root) = crate::testutil::models_root() else { return };
        let wav = std::env::var("DIKTATOR_LIVE_WAV").map(Into::into).unwrap_or_else(|_| crate::testutil::en_wav(&root));
        let audio = crate::testutil::read_wav_16k(&wav);
        let asr = Arc::new(
            SpeechRecognizer::load(&root, SpeechModel::ParakeetTdtV2, "en", crate::asr::default_threads()).unwrap(),
        );
        let vad = crate::vad::Vad::new(&model_file(&root, ModelId::SileroVad, "silero_vad.onnx")).unwrap();
        // `DIKTATOR_LIVE_LLM=1` also rewrites (slow to load; off by default).
        let llm = std::env::var("DIKTATOR_LIVE_LLM").is_ok().then(|| {
            let gguf = model_file(&root, ModelId::Qwen25_1_5b, "qwen2.5-1.5b-instruct-q4_k_m.gguf");
            Arc::new(RewriteEngine::start(gguf, crate::asr::default_threads()).unwrap())
        });
        let live = Live::start(asr, llm.clone(), RewriteMode::Natural);
        let feed = live.feed();
        let (mut seg, mut cut) = (Segmenter::default(), 0);
        let usable = audio.len() / W * W;
        // Timing runs arrive at speaking speed, like a microphone.
        let pace = std::env::var("DIKTATOR_LIVE_WAV").is_ok();
        for w in audio[..usable].chunks(W) {
            if pace {
                std::thread::sleep(std::time::Duration::from_millis(32));
            }
            if let Some(s) = seg.push(w, vad.accept(w)) {
                cut += s.len();
                feed.push(s);
            }
        }
        let t = Instant::now();
        let done = live.finish(audio[cut..].to_vec()).unwrap();
        eprintln!(
            "live: {:.1}s audio, {} segments, finish {} ms (tail asr {} ms, tail rewrite {} ms) {:?}\n  raw: {}\n  out: {}",
            audio.len() as f32 / SAMPLE_RATE as f32,
            done.segments,
            t.elapsed().as_millis(),
            done.tail_asr_ms,
            done.tail_rewrite_ms,
            done.outcome,
            done.raw,
            done.text
        );
        drop(llm); // ggml-metal must free the model before the process exits
        if std::env::var("DIKTATOR_LIVE_WAV").is_err() {
            assert!(done.text.to_lowercase().contains("ask not what your country can do for you"), "{}", done.text);
        }
    }

    #[test]
    fn joins_segments_cut_mid_sentence() {
        let mut t = Transcript::default();
        t.add("the API gateway.".into());
        t.add("keeps timing out.".into());
        t.add("It is bad...".into());
        t.add("really bad.".into());
        t.add("Next sentence.".into());
        assert_eq!(t.pending, "the API gateway keeps timing out. It is bad... really bad. Next sentence.");
    }

    #[test]
    fn needs_enough_words_and_a_known_next_sentence() {
        assert_eq!(split_commit("Short one. Another short one. And"), None);
        assert_eq!(split_commit("No sentence end yet and plenty of words here to make the count"), None);
        // The unfinished last sentence is too short to know whether it starts a correction.
        assert_eq!(
            split_commit(
                "I looked at the logs this morning and the gateway keeps timing out after thirty seconds again. I"
            ),
            None
        );
    }
}
