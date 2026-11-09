//! Dynamic macros inside snippet bodies. Pure: OS values, forms and storage come from a [`Context`].
//!
//! | Macro | Result |
//! |---|---|
//! | `{#date dd/MM/yyyy +1d}` | date with optional offset (`d` days, `w` weeks, `M` months, `y` years); `dddd`/`ddd` weekday, `MMMM`/`MMM` month names |
//! | `{#time HH:mm:ss}` | time |
//! | `{#clipboard}` | clipboard text |
//! | `{#input Label}` `…|multiline` `…|date` `…|check:text` `…|A;B;C` | fill-in form field |
//! | `{#phrase Name}` | another phrase (by name or abbreviation), nested up to 5 levels |
//! | `{#counter name}` | persistent counter, +1 each use |
//! | `{#random a;b;c}` | one item at random |
//! | `{#calc 12*3+(4/2)}` | arithmetic |
//! | `{#cursor}` | caret position after insertion |
//! | `{#key Tab}` `{#enter}` `{#tab}` `{#wait 500}` | keystrokes and pauses |
//! | `{#open https://… or C:\path}` | opens a URL, file or program |

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Multiline,
    Date,
    /// Checkbox: inserts this text when ticked, nothing otherwise.
    Check(String),
    Choice(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub label: String,
    pub kind: FieldKind,
}

/// What to do, in order, to insert a rendered phrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Text(String),
    Key(String),
    Wait(u64),
    /// Leave the caret here once everything is inserted.
    Cursor,
    Open(String),
}

pub trait Context {
    fn now(&self) -> DateTime;
    fn clipboard(&self) -> String;
    /// Shows one form with all fields; returns label -> final text, or `None` if cancelled.
    fn form(&self, fields: &[Field]) -> Option<HashMap<String, String>>;
    /// "it" or "en", for day/month names and decimal separator.
    fn lang(&self) -> &str {
        "it"
    }
    /// Body of another phrase, looked up by name or abbreviation.
    fn phrase(&self, _key: &str) -> Option<String> {
        None
    }
    /// Increments and returns a persistent counter.
    fn counter(&self, _name: &str) -> u64 {
        1
    }
    /// Random index in `0..n`.
    fn random(&self, _n: usize) -> usize {
        0
    }
}

enum Token<'a> {
    Text(&'a str),
    /// name, argument, raw text (kept as is when the macro is unknown)
    Macro(&'a str, &'a str, &'a str),
}

fn tokens(body: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("{#") {
        let Some(len) = rest[start..].find('}') else { break };
        out.push(Token::Text(&rest[..start]));
        let raw = &rest[start..=start + len];
        let inner = raw[2..raw.len() - 1].trim();
        let (name, arg) = inner.split_once(' ').map_or((inner, ""), |(n, a)| (n, a.trim()));
        out.push(Token::Macro(name, arg, raw));
        rest = &rest[start + len + 1..];
    }
    out.push(Token::Text(rest));
    out
}

/// Replaces `{#phrase X}` with that phrase's body, recursively (max 5 levels, so loops stop).
fn include_phrases(body: &str, ctx: &dyn Context, depth: u8) -> String {
    tokens(body)
        .into_iter()
        .map(|t| match t {
            Token::Text(s) => s.to_string(),
            Token::Macro("phrase", arg, raw) => match ctx.phrase(arg) {
                Some(inner) if depth < 5 => include_phrases(&inner, ctx, depth + 1),
                _ => raw.to_string(),
            },
            Token::Macro(_, _, raw) => raw.to_string(),
        })
        .collect()
}

fn parse_field(arg: &str) -> Field {
    let (label, spec) = arg.split_once('|').map_or((arg, ""), |(l, s)| (l.trim(), s.trim()));
    let kind = match spec {
        "" => FieldKind::Text,
        "multiline" => FieldKind::Multiline,
        "date" => FieldKind::Date,
        "check" => FieldKind::Check(label.to_string()),
        _ if spec.starts_with("check:") => FieldKind::Check(spec["check:".len()..].to_string()),
        _ => FieldKind::Choice(split_list(spec)),
    };
    Field { label: label.to_string(), kind }
}

fn split_list(s: &str) -> Vec<String> {
    s.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// The fill-in fields of a body, first occurrence of each label only.
pub fn fields(body: &str) -> Vec<Field> {
    let mut out: Vec<Field> = Vec::new();
    for t in tokens(body) {
        if let Token::Macro("input", arg, _) = t {
            let f = parse_field(arg);
            if !out.iter().any(|o| o.label == f.label) {
                out.push(f);
            }
        }
    }
    out
}

/// Expands a body into insertion steps. `None` if the user cancels the form.
pub fn render_actions(body: &str, ctx: &dyn Context) -> Option<Vec<Action>> {
    let body = include_phrases(body, ctx, 0);
    let form = fields(&body);
    let answers = if form.is_empty() { HashMap::new() } else { ctx.form(&form)? };
    let mut out: Vec<Action> = Vec::new();
    let text = |out: &mut Vec<Action>, s: &str| match out.last_mut() {
        Some(Action::Text(t)) => t.push_str(s),
        _ if !s.is_empty() => out.push(Action::Text(s.to_string())),
        _ => {}
    };
    for t in tokens(&body) {
        match t {
            Token::Text(s) => text(&mut out, s),
            Token::Macro("date", arg, _) => text(&mut out, &date_macro(arg, ctx, "dd/MM/yyyy")),
            Token::Macro("time", arg, _) => text(&mut out, &date_macro(arg, ctx, "HH:mm")),
            Token::Macro("clipboard", _, _) => text(&mut out, &ctx.clipboard()),
            Token::Macro("input", arg, _) => text(&mut out, answers.get(&parse_field(arg).label).map_or("", String::as_str)),
            Token::Macro("counter", arg, _) => text(&mut out, &ctx.counter(arg).to_string()),
            Token::Macro("random", arg, _) => {
                let items = split_list(arg);
                if !items.is_empty() {
                    text(&mut out, &items[ctx.random(items.len()).min(items.len() - 1)]);
                }
            }
            Token::Macro("calc", arg, raw) => match calc(arg) {
                Some(v) => text(&mut out, &format_number(v, ctx.lang())),
                None => text(&mut out, raw),
            },
            Token::Macro("cursor", _, _) => out.push(Action::Cursor),
            Token::Macro("enter", _, _) => out.push(Action::Key("Enter".into())),
            Token::Macro("tab", _, _) => out.push(Action::Key("Tab".into())),
            Token::Macro("key", arg, _) if !arg.is_empty() => out.push(Action::Key(arg.to_string())),
            Token::Macro("wait", arg, raw) => match arg.parse::<u64>() {
                Ok(ms) => out.push(Action::Wait(ms.min(10_000))),
                Err(_) => text(&mut out, raw),
            },
            Token::Macro("open", arg, _) if !arg.is_empty() => out.push(Action::Open(arg.to_string())),
            Token::Macro(_, _, raw) => text(&mut out, raw),
        }
    }
    Some(out)
}

/// Text-only rendering (keystrokes, waits and cursor marks are dropped).
pub fn render(body: &str, ctx: &dyn Context) -> Option<String> {
    Some(
        render_actions(body, ctx)?
            .into_iter()
            .filter_map(|a| if let Action::Text(t) = a { Some(t) } else { None })
            .collect(),
    )
}

// ---------- dates ----------

/// `{#date fmt +1d -2M}`: offsets anywhere in the argument, the rest is the format.
fn date_macro(arg: &str, ctx: &dyn Context, default_fmt: &str) -> String {
    let mut t = ctx.now();
    let mut fmt = Vec::new();
    for part in arg.split_whitespace() {
        match parse_offset(part) {
            Some((n, unit)) => t = shift(t, n, unit),
            None => fmt.push(part),
        }
    }
    let fmt = if fmt.is_empty() { default_fmt.to_string() } else { fmt.join(" ") };
    format_dt(&fmt, t, ctx.lang())
}

fn parse_offset(s: &str) -> Option<(i64, char)> {
    let unit = s.chars().last()?;
    if !matches!(unit, 'd' | 'w' | 'M' | 'y') || !(s.starts_with('+') || s.starts_with('-')) {
        return None;
    }
    s[..s.len() - 1].parse().ok().map(|n| (n, unit))
}

fn shift(t: DateTime, n: i64, unit: char) -> DateTime {
    let (y, m, d) = match unit {
        'd' | 'w' => civil(days(t.year as i64, t.month as i64, t.day as i64) + n * if unit == 'w' { 7 } else { 1 }),
        _ => {
            let months = t.year as i64 * 12 + t.month as i64 - 1 + if unit == 'y' { n * 12 } else { n };
            let (y, m) = (months.div_euclid(12), months.rem_euclid(12) + 1);
            (y, m, (t.day as i64).min(month_len(y, m)))
        }
    };
    DateTime { year: y as u16, month: m as u8, day: d as u8, ..t }
}

fn month_len(y: i64, m: i64) -> i64 {
    days(y + (m == 12) as i64, m % 12 + 1, 1) - days(y, m, 1)
}

/// Days since 1970-01-01 (proleptic Gregorian; H. Hinnant's algorithm).
fn days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

const DAYS_IT: [&str; 7] = ["domenica", "lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato"];
const DAYS_EN: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS_IT: [&str; 12] = ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"];
const MONTHS_EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// Tokens: yyyy yy MMMM MMM MM dddd ddd dd HH mm ss; everything else is copied.
fn format_dt(fmt: &str, t: DateTime, lang: &str) -> String {
    let en = lang == "en";
    let weekday = (days(t.year as i64, t.month as i64, t.day as i64) + 4).rem_euclid(7) as usize;
    let day_name = if en { DAYS_EN[weekday] } else { DAYS_IT[weekday] };
    let month_name = if en { MONTHS_EN } else { MONTHS_IT }[t.month.clamp(1, 12) as usize - 1];
    let short = |s: &str| s.chars().take(3).collect::<String>();
    let mut out = String::new();
    let mut s = fmt;
    while !s.is_empty() {
        let (val, len) = match s {
            _ if s.starts_with("yyyy") => (format!("{:04}", t.year), 4),
            _ if s.starts_with("yy") => (format!("{:02}", t.year % 100), 2),
            _ if s.starts_with("MMMM") => (month_name.to_string(), 4),
            _ if s.starts_with("MMM") => (short(month_name), 3),
            _ if s.starts_with("MM") => (format!("{:02}", t.month), 2),
            _ if s.starts_with("dddd") => (day_name.to_string(), 4),
            _ if s.starts_with("ddd") => (short(day_name), 3),
            _ if s.starts_with("dd") => (format!("{:02}", t.day), 2),
            _ if s.starts_with("HH") => (format!("{:02}", t.hour), 2),
            _ if s.starts_with("mm") => (format!("{:02}", t.minute), 2),
            _ if s.starts_with("ss") => (format!("{:02}", t.second), 2),
            _ => {
                let c = s.chars().next().unwrap();
                (c.to_string(), c.len_utf8())
            }
        };
        out += &val;
        s = &s[len..];
    }
    out
}

// ---------- calculator ----------

/// `+ - * / ( )`, decimals with `.` or `,`. `None` on syntax error or division by zero.
fn calc(expr: &str) -> Option<f64> {
    let chars: Vec<char> = expr.chars().filter(|c| !c.is_whitespace()).collect();
    let mut pos = 0;
    let v = sum(&chars, &mut pos)?;
    (pos == chars.len() && v.is_finite()).then_some(v)
}

fn sum(c: &[char], p: &mut usize) -> Option<f64> {
    let mut v = product(c, p)?;
    while let Some(&op @ ('+' | '-')) = c.get(*p) {
        *p += 1;
        let r = product(c, p)?;
        v = if op == '+' { v + r } else { v - r };
    }
    Some(v)
}

fn product(c: &[char], p: &mut usize) -> Option<f64> {
    let mut v = factor(c, p)?;
    while let Some(&op @ ('*' | '/' | 'x' | ':')) = c.get(*p) {
        *p += 1;
        let r = factor(c, p)?;
        v = if op == '/' || op == ':' { if r == 0.0 { return None } else { v / r } } else { v * r };
    }
    Some(v)
}

fn factor(c: &[char], p: &mut usize) -> Option<f64> {
    match c.get(*p)? {
        '-' => {
            *p += 1;
            factor(c, p).map(|v| -v)
        }
        '(' => {
            *p += 1;
            let v = sum(c, p)?;
            (c.get(*p) == Some(&')')).then(|| *p += 1)?;
            Some(v)
        }
        _ => {
            let start = *p;
            while c.get(*p).is_some_and(|ch| ch.is_ascii_digit() || *ch == '.' || *ch == ',') {
                *p += 1;
            }
            c[start..*p].iter().map(|&ch| if ch == ',' { '.' } else { ch }).collect::<String>().parse().ok()
        }
    }
}

fn format_number(v: f64, lang: &str) -> String {
    let s = if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v:.2}").trim_end_matches('0').to_string() };
    if lang == "en" { s } else { s.replace('.', ",") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct Fake {
        asked: RefCell<Vec<Vec<Field>>>,
        answers: Option<Vec<(&'static str, &'static str)>>,
        counter: Cell<u64>,
    }
    impl Context for Fake {
        fn now(&self) -> DateTime {
            DateTime { year: 2026, month: 3, day: 7, hour: 9, minute: 5, second: 2 } // Saturday
        }
        fn clipboard(&self) -> String {
            "copiato".into()
        }
        fn form(&self, fields: &[Field]) -> Option<HashMap<String, String>> {
            self.asked.borrow_mut().push(fields.to_vec());
            self.answers.as_ref().map(|a| a.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
        }
        fn phrase(&self, key: &str) -> Option<String> {
            match key {
                "firma" => Some("Saluti, {#input Nome}".into()),
                "loop" => Some("{#phrase loop}".into()),
                _ => None,
            }
        }
        fn counter(&self, _: &str) -> u64 {
            self.counter.set(self.counter.get() + 1);
            self.counter.get()
        }
        fn random(&self, n: usize) -> usize {
            n - 1
        }
    }
    fn fake(answers: Option<Vec<(&'static str, &'static str)>>) -> Fake {
        Fake { asked: RefCell::new(Vec::new()), answers, counter: Cell::new(0) }
    }

    #[test]
    fn date_time_clipboard() {
        let f = fake(None);
        assert_eq!(render("Oggi {#date}, ore {#time}", &f).unwrap(), "Oggi 07/03/2026, ore 09:05");
        assert_eq!(render("{#date yyyy-MM-dd HH:mm:ss}", &f).unwrap(), "2026-03-07 09:05:02");
        assert_eq!(render("[{#clipboard}]", &f).unwrap(), "[copiato]");
        assert!(f.asked.borrow().is_empty(), "no form without fields");
    }

    #[test]
    fn date_offsets_and_names() {
        let f = fake(None);
        assert_eq!(render("{#date dddd d MMMM}", &f).unwrap(), "sabato d marzo");
        assert_eq!(render("{#date dd/MM/yyyy +1d}", &f).unwrap(), "08/03/2026");
        assert_eq!(render("{#date +30d}", &f).unwrap(), "06/04/2026");
        assert_eq!(render("{#date -1w ddd dd}", &f).unwrap(), "sab 28");
        assert_eq!(render("{#date +11M}", &f).unwrap(), "07/02/2027");
        assert_eq!(render("{#date +1y}", &f).unwrap(), "07/03/2027");
        // month end clamps: 31 Jan + 1 month = 28 Feb
        assert_eq!(shift(DateTime { year: 2026, month: 1, day: 31, hour: 0, minute: 0, second: 0 }, 1, 'M').day, 28);
        assert_eq!(civil(days(2024, 2, 29)), (2024, 2, 29));
    }

    #[test]
    fn one_form_for_all_fields_and_cancel() {
        let f = fake(Some(vec![("Nome", "Mario"), ("Saluto", "Caro")]));
        let body = "{#input Saluto|Gentile;Caro} {#input Nome}, {#input Nome}!";
        assert_eq!(render(body, &f).unwrap(), "Caro Mario, Mario!");
        assert_eq!(f.asked.borrow().len(), 1);
        assert_eq!(
            f.asked.borrow()[0],
            vec![
                Field { label: "Saluto".into(), kind: FieldKind::Choice(vec!["Gentile".into(), "Caro".into()]) },
                Field { label: "Nome".into(), kind: FieldKind::Text },
            ]
        );
        assert_eq!(render("Ciao {#input Nome}", &fake(None)), None);
    }

    #[test]
    fn field_kinds() {
        let kinds: Vec<FieldKind> = fields("{#input A|multiline}{#input B|date}{#input C|check}{#input D|check:urgente}")
            .into_iter()
            .map(|f| f.kind)
            .collect();
        assert_eq!(
            kinds,
            [FieldKind::Multiline, FieldKind::Date, FieldKind::Check("C".into()), FieldKind::Check("urgente".into())]
        );
    }

    #[test]
    fn nested_phrases_counters_random_calc() {
        let f = fake(Some(vec![("Nome", "Anna")]));
        assert_eq!(render("Ciao.\n{#phrase firma}", &f).unwrap(), "Ciao.\nSaluti, Anna");
        assert_eq!(render("{#phrase loop}|{#phrase nope}", &f).unwrap(), "{#phrase loop}|{#phrase nope}");
        assert_eq!(render("N. {#counter fatture} e {#counter fatture}", &f).unwrap(), "N. 1 e 2");
        assert_eq!(render("{#random a; b ;c}", &f).unwrap(), "c");
        assert_eq!(render("{#calc 12*3+(4/2)} {#calc 10/4} {#calc 1,5x2} {#calc 1/0}", &f).unwrap(), "38 2,5 3 {#calc 1/0}");
    }

    #[test]
    fn actions_keys_cursor_open() {
        let f = fake(None);
        assert_eq!(
            render_actions("Ciao {#cursor}!{#tab}{#key Down}{#wait 200}{#open https://x-holding.it}", &f).unwrap(),
            vec![
                Action::Text("Ciao ".into()),
                Action::Cursor,
                Action::Text("!".into()),
                Action::Key("Tab".into()),
                Action::Key("Down".into()),
                Action::Wait(200),
                Action::Open("https://x-holding.it".into()),
            ]
        );
    }

    #[test]
    fn unknown_and_unclosed_kept_literally() {
        let f = fake(None);
        assert_eq!(render("a {#boh x} b {nope} {#date", &f).unwrap(), "a {#boh x} b {nope} {#date");
        assert_eq!(render("è {#date dd}", &f).unwrap(), "è 07");
    }
}
