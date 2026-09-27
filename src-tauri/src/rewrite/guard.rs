//! Output guard: decides whether a rewrite is safe to insert. Anything it
//! rejects falls back to the cleaned transcript, so a bad rewrite can never
//! answer the user, invent content, or drop most of what they said.

use super::prompt::{examples, Mode};
use std::collections::HashSet;

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Accept,
    Reject(&'static str),
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).map(|w| w.to_lowercase()).collect()
}

pub fn check(mode: Mode, raw: &str, out: &str) -> Verdict {
    let out_t = out.trim();
    if out_t.is_empty() {
        return Verdict::Reject("empty");
    }
    if out_t.contains("```") || out_t.contains('\n') {
        return Verdict::Reject("multiline or code");
    }
    let raw_w = words(raw);
    for ex in examples(mode) {
        if out_t == ex.clean && raw_w != words(ex.raw) {
            return Verdict::Reject("copied a few-shot example");
        }
    }
    let out_w = words(out_t);
    if out_w.is_empty() {
        return Verdict::Reject("no words");
    }
    // A rewrite only removes and lightly rephrases; it never grows much.
    if out_w.len() > raw_w.len() + raw_w.len() / 2 + 3 {
        return Verdict::Reject("longer than input");
    }
    // Dropping most of the input changes the meaning.
    let min_len = match mode {
        Mode::Concise => raw_w.len() / 4,
        _ => raw_w.len() / 3,
    };
    if out_w.len() < min_len {
        return Verdict::Reject("dropped too much");
    }
    // Most output words must come from the input.
    let raw_set: HashSet<&str> = raw_w.iter().map(String::as_str).collect();
    let shared = out_w.iter().filter(|w| raw_set.contains(w.as_str())).count();
    let ratio = shared as f32 / out_w.len() as f32;
    let floor = match mode {
        Mode::Professional => 0.45,
        _ => 0.6,
    };
    if ratio < floor {
        return Verdict::Reject("low overlap with input");
    }
    Verdict::Accept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_clean_rewrite() {
        let raw = "hey rahul uh basically I was thinking maybe we can move the meeting tomorrow because I don't think I'll be able to join today";
        let out =
            "Hey Rahul, I was thinking we could move the meeting to tomorrow since I won't be able to join today.";
        assert_eq!(check(Mode::Natural, raw, out), Verdict::Accept);
    }

    #[test]
    fn rejects_an_answer() {
        assert_eq!(
            check(
                Mode::Natural,
                "what is the capital of france",
                "The capital of France is Paris, a city known for the Eiffel Tower and the Louvre."
            ),
            Verdict::Reject("longer than input")
        );
        assert!(matches!(
            check(Mode::Natural, "tell me a joke about cats", "Why did the cat sit on the computer?"),
            Verdict::Reject(_)
        ));
    }

    #[test]
    fn rejects_copied_example() {
        assert_eq!(
            check(Mode::Natural, "what is the capital of germany", "What is the capital of France?"),
            Verdict::Reject("copied a few-shot example")
        );
    }

    #[test]
    fn allows_dictating_an_example_sentence() {
        // `finalize` passes capitalized, punctuated text; the example raw may differ in case.
        assert_eq!(
            check(Mode::Natural, "What is the capital of france.", "What is the capital of France?"),
            Verdict::Accept
        );
    }

    #[test]
    fn rejects_dropped_clauses() {
        let raw = "hey can you like check this PR whenever you get some time because I think there might be some issue with the auth thing";
        assert_eq!(check(Mode::Natural, raw, "Hey."), Verdict::Reject("dropped too much"));
    }

    #[test]
    fn rejects_code_and_empty() {
        assert!(matches!(check(Mode::Natural, "write code", "```py"), Verdict::Reject(_)));
        assert_eq!(check(Mode::Natural, "hello", "   "), Verdict::Reject("empty"));
    }
}
