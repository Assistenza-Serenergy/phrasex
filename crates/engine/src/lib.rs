//! Pattern matching engine: typing buffer + trigger detection. Pure, OS-independent.

pub mod macros;

/// One keystroke, already translated by the platform listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,
    /// Caret moved or focus changed (arrows, Enter, click, window switch...): forget what was typed.
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Expand as soon as the last trigger character is typed.
    Immediate,
    /// Expand when a delimiter (space, punctuation) follows a whole-word trigger.
    WordEnd,
}

#[derive(Debug, Clone)]
pub struct Snippet {
    /// Library id, reported back when several phrases share one trigger.
    pub id: i64,
    pub trigger: String,
    pub body: String,
    pub mode: Mode,
    pub case_sensitive: bool,
}

impl Snippet {
    pub fn word(trigger: &str, body: &str) -> Self {
        Self { id: 0, trigger: trigger.into(), body: body.into(), mode: Mode::WordEnd, case_sensitive: false }
    }
    pub fn immediate(trigger: &str, body: &str) -> Self {
        Self { mode: Mode::Immediate, ..Self::word(trigger, body) }
    }
}

/// What the injector must do: erase `backspaces` characters, then type `text`.
#[derive(Debug, PartialEq, Eq)]
pub struct Expansion {
    pub backspaces: usize,
    pub text: String,
    /// Exactly what the user typed (trigger + delimiter), to undo the expansion.
    pub typed: String,
    /// Case to apply to the final text (after macros), following how the trigger was typed.
    pub case: Case,
    /// Every phrase sharing the matched trigger (`text` is the first one's). More than one: let the user choose.
    pub ids: Vec<i64>,
    /// What follows the phrase body in `text` (the delimiter for word triggers).
    pub suffix: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    AsIs,
    /// `Btw` -> "By the way"
    Capitalize,
    /// `BTW` -> "BY THE WAY"
    Upper,
}

impl Case {
    pub fn apply(self, text: &str) -> String {
        match self {
            Case::AsIs => text.to_string(),
            Case::Upper => text.to_uppercase(),
            Case::Capitalize => {
                let mut c = text.chars();
                c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
            }
        }
    }

    /// Only for case-insensitive triggers, and only when the typed case differs from the stored one.
    fn detect(typed: &[char], s: &Snippet) -> Case {
        if s.case_sensitive {
            return Case::AsIs;
        }
        let letters: Vec<char> = typed.iter().copied().filter(|c| c.is_alphabetic()).collect();
        let stored_has_lower = s.trigger.chars().any(char::is_lowercase);
        let stored_first_lower = s.trigger.chars().find(|c| c.is_alphabetic()).is_some_and(char::is_lowercase);
        if letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase()) && stored_has_lower {
            Case::Upper
        } else if letters.first().is_some_and(|c| c.is_uppercase()) && stored_first_lower {
            Case::Capitalize
        } else {
            Case::AsIs
        }
    }
}

/// Writing-assistance switches (both off by default).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// "DOmani" -> "Domani" when the word ends.
    pub fix_double_caps: bool,
    /// Upper-case the first letter after ". ", "! " or "? ".
    pub capitalize_sentences: bool,
}

pub struct Engine {
    snippets: Vec<Snippet>,
    buf: Vec<char>,
    cap: usize,
    opts: Options,
}

impl Engine {
    pub fn new(snippets: Vec<Snippet>) -> Self {
        Self::with_options(snippets, Options::default())
    }

    pub fn with_options(mut snippets: Vec<Snippet>, opts: Options) -> Self {
        snippets.retain(|s| !s.trigger.is_empty());
        // Longest trigger wins when several match at once.
        snippets.sort_by_key(|s| std::cmp::Reverse(s.trigger.chars().count()));
        // + boundary char before the trigger + delimiter after it.
        // At least 32 so whole words fit for the double-capitals fix.
        let cap = (snippets.iter().map(|s| s.trigger.chars().count()).max().unwrap_or(0) + 2).max(32);
        Self { snippets, buf: Vec::with_capacity(cap + 1), cap, opts }
    }

    pub fn feed(&mut self, key: Key) -> Option<Expansion> {
        match key {
            Key::Reset => self.buf.clear(),
            Key::Backspace => _ = self.buf.pop(),
            Key::Char(c) => {
                self.buf.push(c);
                if self.buf.len() > self.cap {
                    self.buf.remove(0); // ponytail: O(cap) shift, cap is ~trigger length so irrelevant
                }
                let hit = self.find(c).or_else(|| self.assist(c));
                if hit.is_some() {
                    self.buf.clear();
                }
                return hit;
            }
        }
        None
    }

    fn find(&self, last: char) -> Option<Expansion> {
        let delim = !last.is_alphanumeric();
        let len = self.buf.len();
        let hit = |s: &Snippet| {
            let n = s.trigger.chars().count();
            match s.mode {
                Mode::Immediate => self.ends_with(len, s),
                Mode::WordEnd => delim && self.ends_with(len - 1, s) && self.boundary_before(len - 1 - n, s),
            }
        };
        let s = self.snippets.iter().find(|s| hit(s))?;
        let n = s.trigger.chars().count();
        // Same trigger on several phrases: all of them, whatever their mode (an immediate one fires
        // first, so word-mode twins must be offered right then or they could never be picked).
        let twin = |o: &Snippet| match s.mode {
            Mode::Immediate => o.trigger.chars().count() == n && self.ends_with(len, o),
            Mode::WordEnd => o.mode == Mode::WordEnd && o.trigger.chars().count() == n && hit(o),
        };
        let ids = self.snippets.iter().filter(|o| twin(o)).map(|o| o.id).collect();
        Some(match s.mode {
            Mode::Immediate => Expansion {
                backspaces: n,
                text: s.body.clone(),
                typed: self.buf[len - n..].iter().collect(),
                case: Case::detect(&self.buf[len - n..], s),
                ids,
                suffix: String::new(),
            },
            Mode::WordEnd => Expansion {
                backspaces: n + 1,
                text: format!("{}{last}", s.body),
                typed: self.buf[len - 1 - n..].iter().collect(),
                case: Case::detect(&self.buf[len - 1 - n..len - 1], s),
                ids,
                suffix: last.to_string(),
            },
        })
    }

    /// Writing assistance, applied only when no phrase matched.
    fn assist(&self, last: char) -> Option<Expansion> {
        let len = self.buf.len();
        if self.opts.capitalize_sentences && last.is_lowercase() && len >= 4 {
            let (end, space, prev) = (self.buf[len - 3], self.buf[len - 2], self.buf[len - 4]);
            if matches!(end, '.' | '!' | '?') && space == ' ' && prev.is_alphabetic() {
                return Some(Expansion { backspaces: 1, text: last.to_uppercase().collect(), typed: last.to_string(), case: Case::AsIs, ids: vec![], suffix: String::new() });
            }
        }
        if self.opts.fix_double_caps && !last.is_alphanumeric() {
            let word: Vec<char> = self.buf[..len - 1].iter().rev().take_while(|c| c.is_alphabetic()).copied().collect::<Vec<_>>().into_iter().rev().collect();
            let starts_at = len - 1 - word.len();
            let glued = starts_at > 0 && self.buf[starts_at - 1].is_alphanumeric();
            if word.len() >= 3 && !glued && word[0].is_uppercase() && word[1].is_uppercase() && word[2..].iter().all(|c| c.is_lowercase()) {
                let fixed: String = std::iter::once(word[0]).chain(word[1..].iter().flat_map(|c| c.to_lowercase())).chain([last]).collect();
                let typed: String = word.iter().chain([&last]).collect();
                return Some(Expansion { backspaces: word.len() + 1, text: fixed, typed, case: Case::AsIs, ids: vec![], suffix: String::new() });
            }
        }
        None
    }

    /// `buf[..end]` ends with the trigger.
    fn ends_with(&self, end: usize, s: &Snippet) -> bool {
        let n = s.trigger.chars().count();
        end >= n
            && self.buf[end - n..end].iter().zip(s.trigger.chars()).all(|(&a, b)| {
                if s.case_sensitive { a == b } else { a.to_lowercase().eq(b.to_lowercase()) }
            })
    }

    /// The trigger starting at `start` is not glued to a preceding word ("abtw" must not fire "btw").
    fn boundary_before(&self, start: usize, s: &Snippet) -> bool {
        start == 0
            || !self.buf[start - 1].is_alphanumeric()
            || !s.trigger.starts_with(char::is_alphanumeric)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(e: &mut Engine, s: &str) -> Option<(usize, String)> {
        s.chars().fold(None, |_, c| e.feed(Key::Char(c))).map(|x| (x.backspaces, x.text))
    }
    fn exp(backspaces: usize, text: &str) -> Option<(usize, String)> {
        Some((backspaces, text.into()))
    }

    #[test]
    fn word_trigger_expands_on_delimiter() {
        let mut e = Engine::new(vec![Snippet::word("btw", "by the way")]);
        assert_eq!(typed(&mut e, "btw"), None);
        assert_eq!(typed(&mut e, " "), exp(4, "by the way "));
        assert_eq!(typed(&mut e, "ok btw."), exp(4, "by the way."));
    }

    #[test]
    fn word_trigger_needs_word_boundary() {
        let mut e = Engine::new(vec![Snippet::word("btw", "x")]);
        assert_eq!(typed(&mut e, "abtw "), None);
        assert_eq!(typed(&mut e, "btwx "), None);
    }

    #[test]
    fn immediate_fires_without_delimiter() {
        let mut e = Engine::new(vec![Snippet::immediate(";mail", "a@b.it")]);
        assert_eq!(typed(&mut e, "scrivi;mail"), exp(5, "a@b.it"));
    }

    #[test]
    fn backspace_and_reset() {
        let mut e = Engine::new(vec![Snippet::word("btw", "x")]);
        e.feed(Key::Char('b'));
        e.feed(Key::Char('t'));
        e.feed(Key::Char('q'));
        e.feed(Key::Backspace);
        assert_eq!(typed(&mut e, "w "), exp(4, "x "));
        typed(&mut e, "bt");
        e.feed(Key::Reset);
        assert_eq!(typed(&mut e, "w "), None);
    }

    #[test]
    fn case_sensitivity() {
        let mut e = Engine::new(vec![Snippet::word("btw", "x")]);
        assert_eq!(typed(&mut e, "BTW "), exp(4, "x "));
        let mut e = Engine::new(vec![Snippet { case_sensitive: true, ..Snippet::word("btw", "x") }]);
        assert_eq!(typed(&mut e, "BTW "), None);
    }

    #[test]
    fn longest_trigger_wins_and_unicode() {
        let mut e = Engine::new(vec![Snippet::immediate("mail", "1"), Snippet::immediate("gmail", "2")]);
        assert_eq!(typed(&mut e, "gmail"), exp(5, "2"));
        let mut e = Engine::new(vec![Snippet::word("però", "tuttavia")]);
        assert_eq!(typed(&mut e, "Però,"), exp(5, "tuttavia,"));
    }

    #[test]
    fn case_adaptation_and_typed() {
        let mut e = Engine::new(vec![Snippet::word("btw", "by the way")]);
        for (input, case) in [("btw ", Case::AsIs), ("Btw ", Case::Capitalize), ("BTW ", Case::Upper)] {
            let x = input.chars().fold(None, |_, c| e.feed(Key::Char(c))).unwrap();
            assert_eq!((x.case, x.typed.as_str()), (case, input));
        }
        assert_eq!(Case::Capitalize.apply("by the way "), "By the way ");
        assert_eq!(Case::Upper.apply("però"), "PERÒ");
        let mut e = Engine::new(vec![Snippet { case_sensitive: true, ..Snippet::word("Btw", "x") }]);
        assert_eq!(typed_case(&mut e, "Btw "), Some(Case::AsIs));
    }

    fn typed_case(e: &mut Engine, s: &str) -> Option<Case> {
        s.chars().fold(None, |_, c| e.feed(Key::Char(c))).map(|x| x.case)
    }

    #[test]
    fn writing_assistance() {
        let opts = Options { fix_double_caps: true, capitalize_sentences: true };
        let mut e = Engine::with_options(vec![], opts);
        assert_eq!(typed(&mut e, "DOmani "), exp(7, "Domani "));
        assert_eq!(typed(&mut e, "USA "), None); // all caps: an acronym, left alone
        assert_eq!(typed(&mut e, "Ciao. c"), exp(1, "C"));
        assert_eq!(typed(&mut e, "v1.2 x"), None); // no letter before the dot
        let mut off = Engine::new(vec![]);
        assert_eq!(typed(&mut off, "DOmani "), None);
    }

    #[test]
    fn shared_trigger_reports_every_phrase() {
        let snip = |id, t: &str, b: &str| Snippet { id, ..Snippet::word(t, b) };
        let mut e = Engine::new(vec![snip(1, "eee", "a@x.it"), snip(2, "zzz", "-"), snip(3, "EEE", "b@x.it")]);
        let x = "eee.".chars().fold(None, |_, c| e.feed(Key::Char(c))).unwrap();
        assert_eq!((x.ids, x.suffix.as_str(), x.backspaces), (vec![1, 3], ".", 4));
        let x = "zzz ".chars().fold(None, |_, c| e.feed(Key::Char(c))).unwrap();
        assert_eq!(x.ids, vec![2]);
        // one immediate + one word phrase on the same trigger: both offered as soon as it is typed
        let mut e = Engine::new(vec![Snippet { id: 1, ..Snippet::immediate("eee", "a") }, snip(2, "eee", "b")]);
        let x = "eee".chars().fold(None, |_, c| e.feed(Key::Char(c))).unwrap();
        assert_eq!((x.ids, x.suffix.as_str(), x.backspaces), (vec![1, 2], "", 3));
    }

    #[test]
    fn long_text_before_trigger() {
        let mut e = Engine::new(vec![Snippet::word("btw", "x")]);
        assert_eq!(typed(&mut e, "una frase molto lunga prima del trigger btw "), exp(4, "x "));
    }
}
