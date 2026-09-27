//! Deterministic cleanup that always runs: drops hesitation fillers and
//! immediately repeated words, capitalizes, and ends with punctuation.
//! Ambiguous words ("like", "basically", "so") are left to the LLM.

const FILLERS: [&str; 9] = ["um", "umm", "uh", "uhh", "uhm", "er", "erm", "ah", "hmm"];

fn core(token: &str) -> String {
    token.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase()
}

fn ends_sentence(s: &str) -> bool {
    s.ends_with(['?', '!']) || (s.ends_with('.') && !s.ends_with("..")) // "..." is a trail-off, not an end
}

/// Longest stutter collapsed: "I have a, I have a" is three words.
const MAX_REPEAT: usize = 4;

/// If `out` ends with a word or phrase said twice in a row ("I, I", "we should,
/// we should"), drops the first copy; the later one carries the punctuation
/// that belongs to the sentence. Never across a sentence end, and never for
/// numbers ("1 1 2"). Also collapses real doubles like "that that"; for
/// dictation that is almost always a stutter.
fn drop_repeat(out: &mut Vec<String>) {
    for n in (1..=MAX_REPEAT).rev() {
        if out.len() < 2 * n {
            continue;
        }
        let (first, second) = out[out.len() - 2 * n..].split_at(n);
        let same = first.iter().zip(second).all(|(a, b)| {
            let w = core(a);
            !w.is_empty() && !w.chars().all(|c| c.is_ascii_digit()) && w == core(b)
        });
        if same && !first.iter().any(|t| ends_sentence(t)) {
            let start = out.len() - 2 * n;
            out.drain(start..start + n);
            return;
        }
    }
}

const LEAD_INS: [&str; 9] = ["so", "okay", "ok", "well", "yeah", "basically", "like", "anyway", "alright"];

/// Drops spoken lead-ins at the start of a dictation ("So yeah,", "Okay, so
/// basically,", "I mean,"). Only up to the last comma ASR put in the run, so
/// "So the build…" and "I mean it" keep their words; never the whole text.
fn drop_lead_in(out: &mut Vec<String>) {
    let (mut i, mut cut) = (0, 0);
    while i < out.len() {
        let w = core(&out[i]);
        let step = if w == "i" && out.get(i + 1).is_some_and(|t| core(t) == "mean") { 2 } else { 1 };
        if step == 1 && !LEAD_INS.contains(&w.as_str()) {
            break;
        }
        i += step;
        if out[i - 1].ends_with(',') {
            cut = i;
        }
    }
    if cut > 0 && cut < out.len() {
        out.drain(..cut);
    }
}

pub fn basic(raw: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for tok in raw.split_whitespace() {
        let word = core(tok);
        if FILLERS.contains(&word.as_str()) {
            // Keep sentence-ending punctuation the filler carried ("ship it uh.").
            let tail: String = tok.chars().filter(|c| matches!(c, '.' | '?' | '!')).collect();
            if let (false, Some(prev)) = (tail.is_empty(), out.last_mut()) {
                if !ends_sentence(prev) {
                    *prev = format!("{}{tail}", prev.trim_end_matches(','));
                }
            }
            continue;
        }
        out.push(tok.to_string());
        drop_repeat(&mut out);
    }
    drop_lead_in(&mut out);
    let mut s = out.join(" ");
    if let Some((i, c)) = s.char_indices().find(|(_, c)| c.is_alphabetic()) {
        if c.is_lowercase() {
            s.replace_range(i..i + c.len_utf8(), &c.to_uppercase().to_string());
        }
    }
    if s.chars().last().is_some_and(|c| c.is_alphanumeric() || c == '"' || c == '\'') {
        s.push('.');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::basic;

    #[test]
    fn removes_fillers_and_doubled_words() {
        assert_eq!(basic("um so I uh think the the build is fine"), "So I think the build is fine.");
        assert_eq!(basic("Um, what time is it?"), "What time is it?");
        assert_eq!(basic("the deploy was was uhh pointing to staging"), "The deploy was pointing to staging.");
    }

    #[test]
    fn removes_repeats_that_asr_punctuated() {
        assert_eq!(basic("I, I think we should ship."), "I think we should ship.");
        assert_eq!(basic("The, the build is failing."), "The build is failing.");
        assert_eq!(basic("It's, it's fine."), "It's fine.");
        assert_eq!(basic("We should... we should wait."), "We should wait.");
    }

    #[test]
    fn removes_repeated_phrases() {
        assert_eq!(basic("I think I think we should wait."), "I think we should wait.");
        assert_eq!(basic("We should, we should move the standup."), "We should move the standup.");
        assert_eq!(basic("Can you, can you check the PR?"), "Can you check the PR?");
        assert_eq!(basic("I have a, I have a call at noon."), "I have a call at noon.");
    }

    #[test]
    fn drops_spoken_lead_ins() {
        assert_eq!(basic("So yeah, I wanted to ask about Thursday."), "I wanted to ask about Thursday.");
        assert_eq!(basic("I mean, maybe we should improve the prompt."), "Maybe we should improve the prompt.");
        assert_eq!(basic("Okay, so basically, the build is failing."), "The build is failing.");
        assert_eq!(basic("Okay, so the build is failing."), "So the build is failing.");
    }

    #[test]
    fn keeps_lead_in_words_that_are_content() {
        assert_eq!(basic("So the build is failing."), "So the build is failing.");
        assert_eq!(basic("I mean it."), "I mean it.");
        assert_eq!(basic("Okay."), "Okay.");
        assert_eq!(basic("Right, left, right."), "Right, left, right.");
    }

    #[test]
    fn keeps_repeats_that_carry_meaning() {
        assert_eq!(basic("Call 1 1 2 now."), "Call 1 1 2 now.");
        assert_eq!(basic("It works. It works on Linux too."), "It works. It works on Linux too.");
        assert_eq!(basic("Is it ready? Is it ready yet?"), "Is it ready? Is it ready yet?");
    }

    #[test]
    fn keeps_sentence_punctuation_when_a_filler_carried_it() {
        assert_eq!(basic("we should ship it uh."), "We should ship it.");
        assert_eq!(basic("is it ready hmm?"), "Is it ready?");
    }

    #[test]
    fn leaves_clean_text_alone() {
        assert_eq!(basic("Hello, world."), "Hello, world.");
        assert_eq!(basic("Ship it!"), "Ship it!");
        assert_eq!(basic(""), "");
        assert_eq!(basic("   "), "");
    }

    #[test]
    fn capitalizes_unicode_and_adds_final_period() {
        assert_eq!(basic("écrire le rapport"), "Écrire le rapport.");
        assert_eq!(basic("\"quoted\" text"), "\"Quoted\" text.");
    }
}
