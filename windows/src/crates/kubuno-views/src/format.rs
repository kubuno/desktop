//! Typed binding conversions (`vskubuno/docs/DATA.md` §7, lot DATA-2): what a `{Binding …}` does
//! between the value a view model (or a data component) holds and the value a property wants —
//! WinForms' `Binding.Format`/`Parse` with `FormatString`, `NullValue` and `FormatInfo`.
//!
//! ```text
//! {Binding Source=orders, Path=amount, FormatString=N2, Mode=TwoWay}           1234.5 → "1 234,50" (fr-FR)
//! {Binding Source=customers, Path=born, FormatString=d, Culture=en-US}        "1815-12-10" → "12/10/1815"
//! {Binding Source=customers, Path=email, NullValue='(none)'}                   NULL → "(none)"
//! {Binding Path=Total, FormatString='#,##0.00 €'}
//! ```
//!
//! - **Reading** ([`to_target`]): a number shown in a text property is formatted with the
//!   `FormatString` (`N`, `F`, `D`, `C`, `P` with an optional precision, `G`, or a custom pattern
//!   such as `#,##0.00`); a date or a timestamp (ISO text) with a date pattern (`d`, `D`, `t`, `T`,
//!   `g`, `G`, `s`, or `dd/MM/yyyy HH:mm`…); an empty value shows the `NullValue`. A text property
//!   read by a numeric property (`NumericField.Value`, `Slider.Value`) is parsed; a boolean property
//!   accepts `true`/`false`/`1`/`0`.
//! - **Writing** ([`from_target`]): the text a user typed is parsed back with the same culture and
//!   pattern (`1 234,50` → `1234.5`, `10/12/1815` → `1815-12-10`), the `NullValue` text becomes an
//!   empty value.
//! - **Culture**: `Culture=fr-FR` on the binding, else the process default ([`set_default_culture`]),
//!   else the user's Windows locale. Without a `FormatString` and a `Culture`, values are shown
//!   exactly as they are held (no culture formatting), as before DATA-2.
//!
//! Data components (a `BindingSource`) know their columns' types and answer typed reads and writes
//! themselves (`crate::scope::BindingProvider`), with the same functions.

use std::sync::RwLock;

use crate::binding::{BindingFormat, Value};

/// The shape a bound property wants (what [`crate::binding::FromValue`] reads).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValueKind {
    /// Whatever the source holds (a custom property, an `ItemsSource`).
    #[default]
    Any,
    Bool,
    Number,
    Text,
    List,
}

/// The conventions of a culture for numbers and dates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Culture {
    /// `"fr-FR"`, `"en-US"`, `""` for the invariant culture.
    pub name: String,
    pub decimal: char,
    /// The digit group separator (`,` / `.` / narrow no-break space).
    pub group: char,
    pub currency: &'static str,
    /// `12,50 €` (after, with a no-break space) or `$12.50` (before).
    pub currency_after: bool,
    /// `12,50 %` (with a no-break space) or `12.50%`.
    pub percent_space: bool,
    /// The short date pattern (`d`).
    pub short_date: &'static str,
    /// The long date pattern (`D`).
    pub long_date: &'static str,
    /// The short time pattern (`t`).
    pub short_time: &'static str,
    /// The long time pattern (`T`).
    pub long_time: &'static str,
    months: &'static [&'static str; 12],
    months_abbr: &'static [&'static str; 12],
}

const EN_MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const EN_MONTHS_ABBR: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const FR_MONTHS: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
const FR_MONTHS_ABBR: [&str; 12] = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];
const DE_MONTHS: [&str; 12] = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
const DE_MONTHS_ABBR: [&str; 12] = ["Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.", "Dez."];

/// A narrow no-break space (what .NET uses between digit groups and before `%`/`€` in French).
const NNBSP: char = '\u{202F}';

impl Culture {
    /// The invariant culture: `.` decimals, `,` groups, ISO dates.
    pub fn invariant() -> Self {
        Self {
            name: String::new(),
            decimal: '.',
            group: ',',
            currency: "¤",
            currency_after: false,
            percent_space: false,
            short_date: "yyyy-MM-dd",
            long_date: "d MMMM yyyy",
            short_time: "HH:mm",
            long_time: "HH:mm:ss",
            months: &EN_MONTHS,
            months_abbr: &EN_MONTHS_ABBR,
        }
    }

    /// The culture named `name` (`fr-FR`, `fr`, `en-US`, `de`…; case-insensitive). Unknown
    /// names give the conventions of their language when it is known, else the invariant culture.
    pub fn named(name: &str) -> Self {
        let n = name.trim().replace('_', "-");
        let lower = n.to_ascii_lowercase();
        let lang = lower.split('-').next().unwrap_or("");
        let mut c = match lang {
            "fr" => Self {
                decimal: ',',
                group: NNBSP,
                currency: "€",
                currency_after: true,
                percent_space: true,
                short_date: "dd/MM/yyyy",
                long_date: "d MMMM yyyy",
                short_time: "HH:mm",
                long_time: "HH:mm:ss",
                months: &FR_MONTHS,
                months_abbr: &FR_MONTHS_ABBR,
                ..Self::invariant()
            },
            "de" => Self {
                decimal: ',',
                group: '.',
                currency: "€",
                currency_after: true,
                percent_space: true,
                short_date: "dd.MM.yyyy",
                long_date: "d. MMMM yyyy",
                months: &DE_MONTHS,
                months_abbr: &DE_MONTHS_ABBR,
                ..Self::invariant()
            },
            "es" | "it" | "nl" | "pt" => Self { decimal: ',', group: '.', currency: "€", currency_after: true, percent_space: true, short_date: "dd/MM/yyyy", ..Self::invariant() },
            "en" if lower == "en-us" || lower == "en" => {
                Self { currency: "$", short_date: "M/d/yyyy", long_date: "MMMM d, yyyy", short_time: "h:mm tt", long_time: "h:mm:ss tt", ..Self::invariant() }
            }
            "en" => Self { currency: if lower == "en-gb" { "£" } else { "$" }, short_date: "dd/MM/yyyy", ..Self::invariant() },
            _ => Self::invariant(),
        };
        if lang == "fr" && lower == "fr-ch" {
            c.group = '\u{2019}';
            c.decimal = '.';
            c.currency = "CHF";
        }
        if lang == "fr" && lower == "fr-ca" {
            c.currency = "$";
            c.short_date = "yyyy-MM-dd";
        }
        c.name = if lang.is_empty() || lang == "invariant" { String::new() } else { n };
        c
    }
}

static DEFAULT_CULTURE: RwLock<Option<String>> = RwLock::new(None);

/// Sets the culture bindings use when they name none (`None`: back to the user's locale).
pub fn set_default_culture(name: Option<&str>) {
    if let Ok(mut c) = DEFAULT_CULTURE.write() {
        *c = name.map(str::to_string);
    }
}

/// The culture of a binding: its own `Culture=`, else the default ([`set_default_culture`]), else
/// the user's Windows locale.
pub fn culture_of(format: &BindingFormat) -> Culture {
    if let Some(name) = format.culture.as_deref().filter(|n| !n.trim().is_empty()) {
        return Culture::named(name);
    }
    if let Some(name) = DEFAULT_CULTURE.read().ok().and_then(|c| c.clone()) {
        return Culture::named(&name);
    }
    Culture::named(&user_locale())
}

/// The user's locale name (`GetUserDefaultLocaleName`), `""` when unknown.
fn user_locale() -> String {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetUserDefaultLocaleName(name: *mut u16, len: i32) -> i32;
        }
        let mut buf = [0u16; 85];
        // SAFETY: the buffer is valid for `buf.len()` UTF-16 units (LOCALE_NAME_MAX_LENGTH is 85)
        // and the function writes at most that many, NUL included.
        let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
        if n > 1 {
            return String::from_utf16_lossy(&buf[..(n - 1) as usize]);
        }
    }
    String::new()
}

// ── Numbers ─────────────────────────────────────────────────────────────────────────────────

/// Groups the digits of `int` (no sign) with `sep` every three digits.
fn group_digits(int: &str, sep: char) -> String {
    let mut out = String::with_capacity(int.len() + int.len() / 3);
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

/// `n` with `decimals` decimals, grouped or not, in the culture's separators.
fn fixed(n: f64, decimals: usize, grouped: bool, c: &Culture) -> String {
    let text = format!("{:.*}", decimals, n.abs());
    let (int, frac) = text.split_once('.').unwrap_or((&text, ""));
    let int = if grouped { group_digits(int, c.group) } else { int.to_string() };
    let negative = n < 0.0 && text.chars().any(|ch| ch.is_ascii_digit() && ch != '0');
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&int);
    if !frac.is_empty() {
        out.push(c.decimal);
        out.push_str(frac);
    }
    out
}

/// The shortest text of `n` in the culture (`G`).
fn general(n: f64, c: &Culture) -> String {
    let text = if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{n}") };
    text.replace('.', &c.decimal.to_string())
}

/// Formats `n` with a .NET numeric format string (`N2`, `F0`, `D5`, `C`, `P1`, `G`, `#,##0.00`).
/// `None` when the format is not a numeric one (a date pattern).
pub fn format_number(n: f64, format: &str, c: &Culture) -> Option<String> {
    let f = format.trim();
    if f.is_empty() {
        return Some(general(n, c));
    }
    let mut chars = f.chars();
    let spec = chars.next()?;
    let rest: String = chars.collect();
    let precision = if rest.is_empty() { None } else { rest.parse::<usize>().ok() };
    if rest.is_empty() || precision.is_some() {
        let p = precision;
        match spec.to_ascii_uppercase() {
            'N' => return Some(fixed(n, p.unwrap_or(2), true, c)),
            'F' => return Some(fixed(n, p.unwrap_or(2), false, c)),
            'D' if n.fract() == 0.0 => {
                let digits = format!("{:0width$}", (n as i64).unsigned_abs(), width = p.unwrap_or(1));
                return Some(if n < 0.0 { format!("-{digits}") } else { digits });
            }
            'C' => {
                let body = fixed(n.abs(), p.unwrap_or(2), true, c);
                let sign = if n < 0.0 { "-" } else { "" };
                return Some(if c.currency_after { format!("{sign}{body}\u{00A0}{}", c.currency) } else { format!("{sign}{}{body}", c.currency) });
            }
            'P' => {
                let body = fixed(n * 100.0, p.unwrap_or(2), true, c);
                return Some(if c.percent_space { format!("{body}{NNBSP}%") } else { format!("{body}%") });
            }
            'G' => return Some(general(n, c)),
            _ => {}
        }
    }
    custom_number(n, f, c)
}

/// A custom numeric pattern: `0`/`#` digits, `,` grouping, `.` decimal point, `%` (×100), any
/// other text kept as a prefix or suffix (`'#,##0.00 €'`).
fn custom_number(n: f64, pattern: &str, c: &Culture) -> Option<String> {
    let is_num = |ch: char| matches!(ch, '0' | '#' | ',' | '.');
    let start = pattern.find(is_num)?;
    let end = pattern.rfind(is_num)? + 1;
    let (prefix, body, suffix) = (&pattern[..start], &pattern[start..end], &pattern[end..]);
    if !body.contains(['0', '#']) {
        return None;
    }
    let percent = prefix.contains('%') || suffix.contains('%');
    let (int_part, frac_part) = body.split_once('.').unwrap_or((body, ""));
    let grouped = int_part.contains(',');
    let max_dec = frac_part.chars().filter(|ch| *ch == '0' || *ch == '#').count();
    let min_dec = frac_part.chars().filter(|ch| *ch == '0').count();
    let min_int = int_part.chars().filter(|ch| *ch == '0').count();
    let value = if percent { n * 100.0 } else { n };
    let mut text = format!("{:.*}", max_dec, value.abs());
    if let Some((int, frac)) = text.clone().split_once('.') {
        let mut frac = frac.to_string();
        while frac.len() > min_dec && frac.ends_with('0') {
            frac.pop();
        }
        text = if frac.is_empty() { int.to_string() } else { format!("{int}.{frac}") };
    }
    let (int, frac) = text.split_once('.').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((text.clone(), String::new()));
    let mut int = int.trim_start_matches('0').to_string();
    while int.len() < min_int.max(if frac.is_empty() && int.is_empty() { 1 } else { 0 }) {
        int.insert(0, '0');
    }
    if grouped {
        int = group_digits(&int, c.group);
    }
    let mut out = String::new();
    let negative = value < 0.0 && (int.chars().any(|ch| ch.is_ascii_digit() && ch != '0') || frac.chars().any(|ch| ch != '0'));
    out.push_str(&prefix.replace('\'', ""));
    if negative {
        out.push('-');
    }
    out.push_str(&int);
    if !frac.is_empty() {
        out.push(c.decimal);
        out.push_str(&frac);
    }
    out.push_str(&suffix.replace('\'', ""));
    Some(out)
}

/// Whether `format` is a numeric format (as opposed to a date pattern).
pub fn is_numeric_format(format: &str) -> bool {
    let f = format.trim();
    let mut chars = f.chars();
    let Some(first) = chars.next() else { return false };
    let rest: String = chars.collect();
    if matches!(first.to_ascii_uppercase(), 'N' | 'F' | 'C' | 'P' | 'E') && (rest.is_empty() || rest.parse::<usize>().is_ok()) {
        return true;
    }
    if first == 'D' && !rest.is_empty() && rest.parse::<usize>().is_ok() {
        return true;
    }
    f.contains(['0', '#']) && !f.contains(['y', 'M', 'd', 'H', 'h', 'm', 's'])
}

/// Parses a number typed in the culture's conventions: digit groups, the culture's decimal
/// separator (and `.` when the culture does not use it for groups), a currency symbol, `%`
/// (divided by 100), a leading `+`/`-` or parentheses.
pub fn parse_number(text: &str, c: &Culture) -> Option<f64> {
    let mut t: String = text.trim().to_string();
    if t.is_empty() {
        return None;
    }
    let mut negative = false;
    if t.starts_with('(') && t.ends_with(')') {
        negative = true;
        t = t[1..t.len() - 1].to_string();
    }
    let percent = t.contains('%');
    for sym in [c.currency, "€", "$", "£", "¤", "%", "CHF"] {
        t = t.replace(sym, "");
    }
    let mut cleaned = String::with_capacity(t.len());
    for ch in t.chars() {
        if ch.is_whitespace() || ch == '\u{00A0}' || ch == NNBSP || ch == '\u{2019}' || ch == '\'' {
            continue;
        }
        if ch == c.group && !c.group.is_whitespace() && ch != c.decimal {
            continue;
        }
        if ch == c.decimal {
            cleaned.push('.');
        } else {
            cleaned.push(ch);
        }
    }
    // A culture whose decimal is `,` still accepts `.` when it does not group with it.
    if c.decimal != '.' && c.group == '.' && cleaned.matches('.').count() > 1 {
        return None;
    }
    let cleaned = cleaned.trim_start_matches('+');
    let value = cleaned.parse::<f64>().ok().filter(|v| v.is_finite())?;
    let value = if percent { value / 100.0 } else { value };
    Some(if negative { -value } else { value })
}

// ── Dates ───────────────────────────────────────────────────────────────────────────────────

/// A date and/or time, as the ISO text of a database value carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DateParts {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub has_date: bool,
    pub has_time: bool,
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

impl DateParts {
    fn valid(&self) -> bool {
        (!self.has_date || ((1..=12).contains(&self.month) && self.day >= 1 && self.day <= days_in_month(self.year, self.month)))
            && (!self.has_time || (self.hour < 24 && self.minute < 60 && self.second < 61))
    }

    /// The canonical text: `YYYY-MM-DD`, `HH:MM:SS` or `YYYY-MM-DD HH:MM:SS`.
    pub fn to_iso(&self) -> String {
        match (self.has_date, self.has_time) {
            (true, false) => format!("{:04}-{:02}-{:02}", self.year, self.month, self.day),
            (false, true) => format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second),
            _ => format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", self.year, self.month, self.day, self.hour, self.minute, self.second),
        }
    }
}

/// Reads a leading run of at most `max` digits.
fn digits(s: &[char], i: &mut usize, min: usize, max: usize) -> Option<u32> {
    let start = *i;
    while *i < s.len() && *i - start < max && s[*i].is_ascii_digit() {
        *i += 1;
    }
    if *i - start < min {
        return None;
    }
    s[start..*i].iter().collect::<String>().parse().ok()
}

/// Parses ISO text: `YYYY-MM-DD`, `YYYY-MM-DD[T ]HH:MM[:SS[.fff]][Z|±HH:MM]`, `HH:MM[:SS]`.
pub fn parse_iso(text: &str) -> Option<DateParts> {
    let s: Vec<char> = text.trim().chars().collect();
    let mut i = 0;
    let mut p = DateParts::default();
    let time_only = s.get(2) == Some(&':');
    if !time_only {
        p.year = i32::try_from(digits(&s, &mut i, 4, 4)?).ok()?;
        (s.get(i) == Some(&'-')).then_some(())?;
        i += 1;
        p.month = digits(&s, &mut i, 1, 2)?;
        (s.get(i) == Some(&'-')).then_some(())?;
        i += 1;
        p.day = digits(&s, &mut i, 1, 2)?;
        p.has_date = true;
        if i == s.len() {
            return p.valid().then_some(p);
        }
        if !matches!(s.get(i), Some('T' | ' ')) {
            return None;
        }
        i += 1;
    }
    p.hour = digits(&s, &mut i, 1, 2)?;
    (s.get(i) == Some(&':')).then_some(())?;
    i += 1;
    p.minute = digits(&s, &mut i, 2, 2)?;
    if s.get(i) == Some(&':') {
        i += 1;
        p.second = digits(&s, &mut i, 2, 2)?;
        if s.get(i) == Some(&'.') {
            i += 1;
            while i < s.len() && s[i].is_ascii_digit() {
                i += 1;
            }
        }
    }
    p.has_time = true;
    // A zone suffix is accepted (and kept by the caller's original text, not here).
    let rest: String = s[i..].iter().collect();
    let zone_ok = rest.is_empty() || rest == "Z" || ((rest.starts_with('+') || rest.starts_with('-')) && rest[1..].chars().all(|c| c.is_ascii_digit() || c == ':'));
    (zone_ok && p.valid()).then_some(p)
}

/// The tokens of a date pattern: `yyyy`, `MM`, `d`… and literal text (`'…'` quoted or not).
fn tokens(pattern: &str) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = Vec::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' || c == '"' {
            let mut lit = String::new();
            i += 1;
            while i < chars.len() && chars[i] != c {
                lit.push(chars[i]);
                i += 1;
            }
            i += 1;
            out.push((false, lit));
        } else if matches!(c, 'y' | 'M' | 'd' | 'H' | 'h' | 'm' | 's' | 't' | 'f') {
            let mut j = i;
            while j < chars.len() && chars[j] == c {
                j += 1;
            }
            out.push((true, chars[i..j].iter().collect()));
            i = j;
        } else {
            match out.last_mut() {
                Some((false, lit)) => lit.push(c),
                _ => out.push((false, c.to_string())),
            }
            i += 1;
        }
    }
    out
}

/// The pattern a standard date format letter stands for in `c` (`d` → `dd/MM/yyyy` in French).
pub fn date_pattern(format: &str, c: &Culture) -> String {
    match format.trim() {
        "d" => c.short_date.to_string(),
        "D" => c.long_date.to_string(),
        "t" => c.short_time.to_string(),
        "T" => c.long_time.to_string(),
        "g" => format!("{} {}", c.short_date, c.short_time),
        "G" => format!("{} {}", c.short_date, c.long_time),
        "f" => format!("{} {}", c.long_date, c.short_time),
        "F" => format!("{} {}", c.long_date, c.long_time),
        "s" => "yyyy'-'MM'-'dd'T'HH':'mm':'ss".to_string(),
        other => other.to_string(),
    }
}

/// Formats `p` with a date format (a standard letter or a pattern).
pub fn format_date(p: &DateParts, format: &str, c: &Culture) -> String {
    let pattern = date_pattern(format, c);
    let mut out = String::new();
    for (is_token, t) in tokens(&pattern) {
        if !is_token {
            out.push_str(&t);
            continue;
        }
        let n = t.len();
        let hour12 = if p.hour.is_multiple_of(12) { 12 } else { p.hour % 12 };
        let piece = match (t.chars().next().unwrap_or(' '), n) {
            ('y', 1) => format!("{}", p.year % 100),
            ('y', 2) => format!("{:02}", p.year % 100),
            ('y', _) => format!("{:04}", p.year),
            ('M', 1) => p.month.to_string(),
            ('M', 2) => format!("{:02}", p.month),
            ('M', 3) => c.months_abbr.get(p.month.saturating_sub(1) as usize).copied().unwrap_or("").to_string(),
            ('M', _) => c.months.get(p.month.saturating_sub(1) as usize).copied().unwrap_or("").to_string(),
            ('d', 1) => p.day.to_string(),
            ('d', _) => format!("{:02}", p.day),
            ('H', 1) => p.hour.to_string(),
            ('H', _) => format!("{:02}", p.hour),
            ('h', 1) => hour12.to_string(),
            ('h', _) => format!("{hour12:02}"),
            ('m', 1) => p.minute.to_string(),
            ('m', _) => format!("{:02}", p.minute),
            ('s', 1) => p.second.to_string(),
            ('s', _) => format!("{:02}", p.second),
            ('t', _) => (if p.hour < 12 { "AM" } else { "PM" }).to_string(),
            ('f', _) => "0".repeat(n),
            _ => t,
        };
        out.push_str(&piece);
    }
    out
}

/// Parses `text` with a date format (standard letter or pattern) in `c`. Lenient on the literal
/// separators (any non-digit separates), strict on the values (a 31st of April is refused).
pub fn parse_date(text: &str, format: &str, c: &Culture) -> Option<DateParts> {
    let pattern = date_pattern(format, c);
    let s: Vec<char> = text.trim().chars().collect();
    let mut i = 0;
    let mut p = DateParts::default();
    let mut pm: Option<bool> = None;
    for (is_token, t) in tokens(&pattern) {
        if !is_token {
            // Skip the separator (whatever it is), and spaces.
            while i < s.len() && !s[i].is_alphanumeric() {
                i += 1;
            }
            continue;
        }
        while i < s.len() && s[i].is_whitespace() {
            i += 1;
        }
        let n = t.len();
        match t.chars().next().unwrap_or(' ') {
            'y' => {
                let y = digits(&s, &mut i, 1, 4)?;
                p.year = if n <= 2 && y < 100 { 2000 + y as i32 } else { i32::try_from(y).ok()? };
                p.has_date = true;
            }
            'M' if n >= 3 => {
                let word: String = s[i..].iter().take_while(|ch| ch.is_alphabetic() || **ch == '.').collect();
                i += word.chars().count();
                let w = word.to_lowercase();
                let idx = c.months.iter().position(|m| m.to_lowercase() == w).or_else(|| c.months_abbr.iter().position(|m| m.to_lowercase() == w))?;
                p.month = idx as u32 + 1;
                p.has_date = true;
            }
            'M' => {
                p.month = digits(&s, &mut i, 1, 2)?;
                p.has_date = true;
            }
            'd' => {
                p.day = digits(&s, &mut i, 1, 2)?;
                p.has_date = true;
            }
            'H' | 'h' => {
                p.hour = digits(&s, &mut i, 1, 2)?;
                p.has_time = true;
            }
            'm' => {
                p.minute = digits(&s, &mut i, 1, 2)?;
                p.has_time = true;
            }
            's' => {
                p.second = digits(&s, &mut i, 1, 2)?;
                p.has_time = true;
            }
            't' => {
                let word: String = s[i..].iter().take_while(|ch| ch.is_alphabetic()).collect();
                i += word.chars().count();
                pm = match word.to_ascii_uppercase().as_str() {
                    "PM" | "P" => Some(true),
                    "AM" | "A" => Some(false),
                    _ => None,
                };
            }
            _ => {}
        }
    }
    while i < s.len() && s[i].is_whitespace() {
        i += 1;
    }
    if i != s.len() {
        return None;
    }
    if let Some(pm) = pm {
        p.hour = match (pm, p.hour) {
            (true, h) if h < 12 => h + 12,
            (false, 12) => 0,
            (_, h) => h,
        };
    }
    p.valid().then_some(p)
}

// ── Binding conversions ─────────────────────────────────────────────────────────────────────

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "oui" | "vrai" => Some(true),
        "false" | "0" | "no" | "non" | "faux" | "" => Some(false),
        _ => None,
    }
}

/// Formats text a source holds (`"1234.5"`, `"2026-09-30"`) with the binding's `FormatString`:
/// a number with a numeric format, a date/time with a date format; anything else unchanged.
pub fn format_text(text: &str, format: &BindingFormat) -> String {
    if text.is_empty() {
        return format.null_value.clone().unwrap_or_default();
    }
    let Some(f) = format.format_string.as_deref().filter(|f| !f.trim().is_empty()) else {
        return text.to_string();
    };
    let c = culture_of(format);
    if is_numeric_format(f) {
        if let Some(n) = text.trim().parse::<f64>().ok().filter(|n| n.is_finite()) {
            if let Some(s) = format_number(n, f, &c) {
                return s;
            }
        }
        return text.to_string();
    }
    match parse_iso(text) {
        Some(p) => format_date(&p, f, &c),
        None => text.to_string(),
    }
}

/// The value a property of shape `want` reads from `value` (see the module doc). `None`: it does
/// not convert (the property falls back to its default).
pub fn to_target(value: Value, want: ValueKind, format: &BindingFormat) -> Option<Value> {
    match (value, want) {
        (v, ValueKind::Any) if format.is_empty() => Some(v),
        (Value::List(l), ValueKind::List | ValueKind::Any) => Some(Value::List(l)),
        (Value::List(_), _) => None,
        (Value::Object(o), ValueKind::Any) => Some(Value::Object(o)),
        (Value::Object(_), _) => None,
        (v, ValueKind::List) => Some(v),
        (Value::Str(s), ValueKind::Text | ValueKind::Any) => Some(Value::Str(format_text(&s, format))),
        (Value::F32(f), ValueKind::Text | ValueKind::Any) => Some(Value::Str(match format.format_string.as_deref().filter(|f| !f.trim().is_empty()) {
            Some(fs) => format_number(f64::from(f), fs, &culture_of(format)).unwrap_or_else(|| f.to_string()),
            None if format.culture.is_some() => general(f64::from(f), &culture_of(format)),
            None => f.to_string(),
        })),
        (Value::Bool(b), ValueKind::Text | ValueKind::Any) => Some(Value::Str(b.to_string())),
        (Value::F32(f), ValueKind::Number) => Some(Value::F32(f)),
        (Value::Str(s), ValueKind::Number) => {
            let t = s.trim();
            t.parse::<f32>().ok().or_else(|| parse_number(t, &culture_of(format)).map(|n| n as f32)).map(Value::F32)
        }
        (Value::Bool(b), ValueKind::Number) => Some(Value::F32(if b { 1.0 } else { 0.0 })),
        (Value::Bool(b), ValueKind::Bool) => Some(Value::Bool(b)),
        (Value::F32(f), ValueKind::Bool) => Some(Value::Bool(f != 0.0)),
        (Value::Str(s), ValueKind::Bool) => parse_bool(&s).map(Value::Bool),
    }
}

/// What a two-way binding writes back to a plain view model for `value` (see the module doc): the
/// `NullValue` text becomes `""`; with a numeric `FormatString`, a text that parses becomes a
/// number; with a date format, a text that parses becomes its ISO text.
pub fn from_target(value: Value, format: &BindingFormat) -> Value {
    let Value::Str(s) = value else { return value };
    if format.null_value.as_deref().is_some_and(|n| n == s) {
        return Value::Str(String::new());
    }
    let Some(f) = format.format_string.as_deref().filter(|f| !f.trim().is_empty()) else { return Value::Str(s) };
    let c = culture_of(format);
    if is_numeric_format(f) {
        return match parse_number(&s, &c) {
            Some(n) => Value::F32(n as f32),
            None => Value::Str(s),
        };
    }
    match parse_date(&s, f, &c).or_else(|| parse_iso(&s)) {
        Some(p) => Value::Str(p.to_iso()),
        None => Value::Str(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(format: &str, culture: &str) -> BindingFormat {
        BindingFormat { format_string: Some(format.to_string()), null_value: None, culture: Some(culture.to_string()) }
    }

    #[test]
    fn standard_numeric_formats() {
        let fr = Culture::named("fr-FR");
        let us = Culture::named("en-US");
        assert_eq!(format_number(1234.5, "N2", &fr).as_deref(), Some("1\u{202F}234,50"));
        assert_eq!(format_number(1234.5, "N2", &us).as_deref(), Some("1,234.50"));
        assert_eq!(format_number(-1234.567, "N1", &us).as_deref(), Some("-1,234.6"));
        assert_eq!(format_number(3.0, "F3", &us).as_deref(), Some("3.000"));
        assert_eq!(format_number(42.0, "D5", &us).as_deref(), Some("00042"));
        assert_eq!(format_number(12.5, "C", &fr).as_deref(), Some("12,50\u{00A0}€"));
        assert_eq!(format_number(12.5, "C", &us).as_deref(), Some("$12.50"));
        assert_eq!(format_number(0.125, "P1", &us).as_deref(), Some("12.5%"));
        assert_eq!(format_number(0.125, "P0", &fr).as_deref(), Some("12\u{202F}%"));
        assert_eq!(format_number(2.5, "G", &fr).as_deref(), Some("2,5"));
        assert_eq!(format_number(-0.001, "N2", &us).as_deref(), Some("0.00"), "no negative zero");
    }

    #[test]
    fn custom_numeric_formats() {
        let us = Culture::named("en-US");
        assert_eq!(format_number(1234.5, "#,##0.00", &us).as_deref(), Some("1,234.50"));
        assert_eq!(format_number(1234.5, "0.##", &us).as_deref(), Some("1234.5"));
        assert_eq!(format_number(0.5, "#,##0.00 €", &Culture::named("de-DE")).as_deref(), Some("0,50 €"));
        assert_eq!(format_number(7.0, "000", &us).as_deref(), Some("007"));
        assert!(is_numeric_format("#,##0.00") && is_numeric_format("N2") && !is_numeric_format("dd/MM/yyyy") && !is_numeric_format("d"));
    }

    #[test]
    fn numbers_parse_back_in_the_culture() {
        let fr = Culture::named("fr-FR");
        let de = Culture::named("de-DE");
        let us = Culture::named("en-US");
        assert_eq!(parse_number("1\u{202F}234,50", &fr), Some(1234.5));
        assert_eq!(parse_number("1 234,5 €", &fr), Some(1234.5));
        assert_eq!(parse_number("3.5", &fr), Some(3.5), "a dot is a decimal point in French");
        assert_eq!(parse_number("1.234,5", &de), Some(1234.5));
        assert_eq!(parse_number("1,234.5", &us), Some(1234.5));
        assert_eq!(parse_number("12,5 %", &fr), Some(0.125));
        assert_eq!(parse_number("(4)", &us), Some(-4.0));
        assert_eq!(parse_number("abc", &us), None);
        assert_eq!(parse_number("", &us), None);
    }

    #[test]
    fn dates_format_and_parse() {
        let fr = Culture::named("fr-FR");
        let us = Culture::named("en-US");
        let p = parse_iso("2026-09-30 14:05:09+02:00").expect("iso");
        assert_eq!(format_date(&p, "d", &fr), "30/09/2026");
        assert_eq!(format_date(&p, "g", &fr), "30/09/2026 14:05");
        assert_eq!(format_date(&p, "d", &us), "9/30/2026");
        assert_eq!(format_date(&p, "t", &us), "2:05 PM");
        assert_eq!(format_date(&p, "D", &fr), "30 septembre 2026");
        assert_eq!(format_date(&p, "yyyy-MM-dd'T'HH:mm", &fr), "2026-09-30T14:05");
        assert_eq!(parse_date("10/12/1815", "d", &fr).map(|p| p.to_iso()).as_deref(), Some("1815-12-10"));
        assert_eq!(parse_date("12/10/1815", "d", &us).map(|p| p.to_iso()).as_deref(), Some("1815-12-10"));
        assert_eq!(parse_date("31/04/2026", "d", &fr), None, "no 31st of April");
        assert_eq!(parse_date("30/09/2026 2:05", "g", &fr).map(|p| p.to_iso()).as_deref(), Some("2026-09-30 02:05:00"));
        assert_eq!(parse_date("3 mars 2026", "D", &fr).map(|p| p.to_iso()).as_deref(), Some("2026-03-03"));
        assert_eq!(parse_date("2:05 PM", "t", &us).map(|p| p.to_iso()).as_deref(), Some("14:05:00"));
        assert!(parse_iso("2026-02-30").is_none());
        assert_eq!(parse_iso("08:30").map(|p| p.to_iso()).as_deref(), Some("08:30:00"));
    }

    #[test]
    fn binding_reads_and_writes() {
        let n2 = fmt("N2", "fr-FR");
        assert_eq!(to_target(Value::Str("1234.5".into()), ValueKind::Text, &n2), Some(Value::Str("1\u{202F}234,50".into())));
        assert_eq!(to_target(Value::F32(2.0), ValueKind::Text, &n2), Some(Value::Str("2,00".into())));
        assert_eq!(to_target(Value::Str("42".into()), ValueKind::Number, &BindingFormat::default()), Some(Value::F32(42.0)));
        assert_eq!(to_target(Value::Str("abc".into()), ValueKind::Number, &BindingFormat::default()), None);
        assert_eq!(to_target(Value::Str("1".into()), ValueKind::Bool, &BindingFormat::default()), Some(Value::Bool(true)));
        let null = BindingFormat { null_value: Some("(none)".into()), ..Default::default() };
        assert_eq!(to_target(Value::Str(String::new()), ValueKind::Text, &null), Some(Value::Str("(none)".into())));
        assert_eq!(from_target(Value::Str("(none)".into()), &null), Value::Str(String::new()));
        assert_eq!(from_target(Value::Str("1\u{202F}234,50".into()), &n2), Value::F32(1234.5));
        let d = fmt("d", "fr-FR");
        assert_eq!(to_target(Value::Str("1815-12-10".into()), ValueKind::Text, &d), Some(Value::Str("10/12/1815".into())));
        assert_eq!(from_target(Value::Str("11/12/1815".into()), &d), Value::Str("1815-12-11".into()));
        assert_eq!(from_target(Value::Str("not a date".into()), &d), Value::Str("not a date".into()));
        // No format, no culture: exactly what the source holds.
        assert_eq!(to_target(Value::Str("1234.5".into()), ValueKind::Text, &BindingFormat::default()), Some(Value::Str("1234.5".into())));
    }

    #[test]
    fn cultures() {
        assert_eq!(Culture::named("fr").decimal, ',');
        assert_eq!(Culture::named("FR_fr").name, "FR-fr");
        assert_eq!(Culture::named("invariant").name, "");
        assert_eq!(Culture::named("xx-YY").decimal, '.');
        assert_eq!(Culture::named("en-GB").short_date, "dd/MM/yyyy");
    }
}
