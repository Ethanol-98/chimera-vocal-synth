//! Speech text normalization, before dictionary lookup.
//!
//! `to_phrases` used to spell only bare digit runs and then throw away every other non-letter,
//! so "$12.50", "3rd", "7:30pm", "Dr." and "a@b.com" lost pieces or became the wrong words.
//! This pass rewrites those into words a dictionary can say, and copies plain words through
//! unchanged (same characters, so "Hello", "it's" and "__proto__" still take the old path).
//!
//! Phrase punctuation `.!?;:,` is left in place: that is what splits phrases and sets the pause.
//! A parenthesis, em dash or ellipsis becomes one of those marks. Anything else (emoji, a
//! character the rules don't know) is copied and the word splitter drops it, which is a skip,
//! not an error.

const ONES: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];
const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
/// Enough for "example.com" and "a@b.com". Unknown suffixes stay words split on the dot.
const TLDS: &[&str] = &[
    "com", "org", "net", "edu", "gov", "mil", "int", "io", "co", "uk", "us", "ca", "de", "fr",
    "au", "nz", "jp", "cn", "ru", "br", "in", "info", "dev", "app", "ai", "me", "tv", "cc", "ly",
    "biz", "fm", "gg", "xyz",
];

struct Hit {
    end: usize,
    spoken: String,
}

/// Rewrite `text` into words plus the phrase punctuation `to_phrases` already understands.
pub fn normalize_speech(text: &str) -> String {
    let folded: String = text.chars().map(fold_char).collect();
    let chars: Vec<char> = folded.chars().collect();
    let mut out = String::with_capacity(folded.len() + 8);
    let mut i = 0;
    while i < chars.len() {
        if let Some(hit) = match_at(&chars, i)
            && hit.end > i
        {
            emit(&mut out, &hit.spoken);
            i = hit.end;
            continue;
        }
        let c = chars[i];
        if let Some(hit) = match_symbol(&chars, i, c) {
            emit(&mut out, &hit.spoken);
            i = hit.end;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn fold_char(c: char) -> char {
    match c {
        // already what `to_phrases` did; also the opening quote used as an apostrophe
        '’' | '‘' | 'ʼ' => '\'',
        '０'..='９' => char::from_u32('0' as u32 + (c as u32 - '０' as u32)).unwrap(),
        _ => c,
    }
}

fn emit(out: &mut String, words: &str) {
    let words = words.trim();
    if words.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with(char::is_whitespace) {
        out.push(' ');
    }
    out.push_str(words);
    out.push(' ');
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '\'' || c == '_'
}

fn is_boundary(chars: &[char], i: usize) -> bool {
    i == 0 || !is_word_char(chars[i - 1])
}

fn ends_token(chars: &[char], end: usize) -> bool {
    end >= chars.len() || !chars[end].is_ascii_alphanumeric()
}

fn match_at(chars: &[char], i: usize) -> Option<Hit> {
    let c = chars[i];
    if c == '$' || c == '£' || c == '€' {
        return try_currency(chars, i, false);
    }
    if is_boundary(chars, i) && (c == '-' || c == '−') {
        return try_signed(chars, i);
    }
    if c.is_ascii_alphabetic() && is_boundary(chars, i) {
        return try_email(chars, i)
            .or_else(|| try_url(chars, i))
            .or_else(|| try_dotted(chars, i))
            .or_else(|| try_abbrev(chars, i))
            .or_else(|| try_acronym(chars, i));
    }
    if c.is_ascii_digit() && (is_boundary(chars, i) || is_inline_decimal(chars, i)) {
        if !is_boundary(chars, i) {
            return try_decimal(chars, i);
        }
        return try_time(chars, i)
            .or_else(|| try_slash(chars, i))
            .or_else(|| try_dash(chars, i))
            .or_else(|| try_ordinal(chars, i))
            .or_else(|| try_comma_number(chars, i))
            .or_else(|| try_decimal(chars, i))
            .or_else(|| try_leading_zeros(chars, i))
            .or_else(|| try_year(chars, i))
            .or_else(|| try_big_int(chars, i));
    }
    if c == '.' && is_leading_dot_decimal(chars, i) {
        return try_leading_dot_decimal(chars, i);
    }
    None
}

/// "v1.2": the digits are glued to a letter, but the dot would otherwise end a phrase.
fn is_inline_decimal(chars: &[char], i: usize) -> bool {
    let (j, _) = read_digits(chars, i);
    j > i
        && j < chars.len()
        && chars[j] == '.'
        && j + 1 < chars.len()
        && chars[j + 1].is_ascii_digit()
}

fn is_leading_dot_decimal(chars: &[char], i: usize) -> bool {
    chars[i] == '.'
        && i + 1 < chars.len()
        && chars[i + 1].is_ascii_digit()
        && (i + 2 >= chars.len() || chars[i + 2] != '.')
        && (i == 0 || !chars[i - 1].is_ascii_digit())
}

fn try_signed(chars: &[char], i: usize) -> Option<Hit> {
    let n = i + 1;
    if n >= chars.len() {
        return None;
    }
    if chars[n] == '$' || chars[n] == '£' || chars[n] == '€' {
        return try_currency(chars, n, true);
    }
    if !chars[n].is_ascii_digit() {
        return None;
    }
    // Leave the digits for the number rules (or for expand_numbers). Only the sign was silent.
    Some(Hit {
        end: n,
        spoken: "minus".into(),
    })
}

// ---------------------------------------------------------------- cardinals

fn cardinal(n: u64) -> String {
    if n < 1000 {
        return under_1000(n);
    }
    let scales = ["", "thousand", "million", "billion", "trillion"];
    let mut x = n;
    let mut parts = Vec::new();
    let mut i = 0;
    while x > 0 && i < scales.len() {
        let chunk = x % 1000;
        if chunk != 0 {
            let mut p = under_1000(chunk);
            if i > 0 {
                p.push(' ');
                p.push_str(scales[i]);
            }
            parts.push(p);
        }
        x /= 1000;
        i += 1;
    }
    if x > 0 {
        // past trillions: spell digits rather than invent a scale name
        return digit_words(&n.to_string());
    }
    parts.reverse();
    parts.join(" ")
}

fn under_1000(n: u64) -> String {
    if n < 20 {
        return ONES[n as usize].to_string();
    }
    if n < 100 {
        let t = TENS[(n / 10) as usize];
        return if n % 10 != 0 {
            format!("{t} {}", ONES[(n % 10) as usize])
        } else {
            t.to_string()
        };
    }
    let mut s = format!("{} hundred", ONES[(n / 100) as usize]);
    if n % 100 != 0 {
        s.push(' ');
        s.push_str(&under_1000(n % 100));
    }
    s
}

fn digit_words(digits: &str) -> String {
    digits
        .chars()
        .filter_map(|c| c.to_digit(10).map(|d| ONES[d as usize]))
        .collect::<Vec<_>>()
        .join(" ")
}

fn ordinal(n: u64) -> String {
    if n == 0 {
        return "zeroth".into();
    }
    let card = cardinal(n);
    let mut words: Vec<&str> = card.split(' ').collect();
    let last = words.pop().unwrap_or("zero");
    let ow = ordinal_word(last);
    let mut s = words.join(" ");
    if !s.is_empty() {
        s.push(' ');
    }
    s.push_str(&ow);
    s
}

fn ordinal_word(w: &str) -> String {
    match w {
        "zero" => "zeroth",
        "one" => "first",
        "two" => "second",
        "three" => "third",
        "four" => "fourth",
        "five" => "fifth",
        "six" => "sixth",
        "seven" => "seventh",
        "eight" => "eighth",
        "nine" => "ninth",
        "ten" => "tenth",
        "eleven" => "eleventh",
        "twelve" => "twelfth",
        "twenty" => "twentieth",
        "thirty" => "thirtieth",
        "forty" => "fortieth",
        "fifty" => "fiftieth",
        "sixty" => "sixtieth",
        "seventy" => "seventieth",
        "eighty" => "eightieth",
        "ninety" => "ninetieth",
        "hundred" => "hundredth",
        "thousand" => "thousandth",
        "million" => "millionth",
        "billion" => "billionth",
        "trillion" => "trillionth",
        _ => return format!("{w}th"),
    }
    .to_string()
}

fn suffix_for(n: u64) -> &'static str {
    let n = n % 100;
    if (11..=13).contains(&n) {
        return "th";
    }
    match n % 10 {
        1 => "st",
        2 => "nd",
        3 => "rd",
        _ => "th",
    }
}

/// 1100–2099 as years ("1999" → "nineteen ninety nine"). 2000–2009 stay cardinals, which is
/// the same phrase ("two thousand five"). 1000–1099 stay cardinals so "1000" is still
/// "one thousand", the reading the number goldens lock down.
fn year_words(y: u64) -> String {
    if !(1100..=2099).contains(&y) || (2000..2010).contains(&y) {
        return cardinal(y);
    }
    let head = cardinal(y / 100);
    let lo = y % 100;
    if lo == 0 {
        format!("{head} hundred")
    } else if lo < 10 {
        format!("{head} oh {}", ONES[lo as usize])
    } else {
        format!("{head} {}", cardinal(lo))
    }
}

fn speak_yearish(digits: &str) -> String {
    if digits.len() > 1 && digits.starts_with('0') || digits.len() >= 16 {
        return digit_words(digits);
    }
    let Ok(n) = digits.parse::<u64>() else {
        return digit_words(digits);
    };
    if digits.len() == 4 && (1100..=2099).contains(&n) {
        year_words(n)
    } else {
        cardinal(n)
    }
}

// ---------------------------------------------------------------- letters, urls, abbreviations

fn letter_word(c: char) -> &'static str {
    match c.to_ascii_lowercase() {
        'a' => "ay",
        'b' => "b",
        'c' => "c",
        'd' => "d",
        'e' => "e",
        'f' => "f",
        'g' => "g",
        'h' => "h",
        'i' => "eye",
        'j' => "j",
        'k' => "k",
        'l' => "l",
        'm' => "m",
        'n' => "n",
        'o' => "o",
        'p' => "p",
        'q' => "q",
        'r' => "r",
        's' => "s",
        't' => "t",
        'u' => "u",
        'v' => "v",
        'w' => "w",
        'x' => "x",
        'y' => "y",
        'z' => "z",
        _ => "",
    }
}

fn has_vowel(word: &str) -> bool {
    word.chars()
        .any(|c| matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u'))
}

fn spell_letters(word: &str) -> String {
    word.chars()
        .filter_map(|c| {
            let w = letter_word(c);
            (!w.is_empty()).then_some(w)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_tld(label: &str) -> bool {
    TLDS.iter().any(|t| label.eq_ignore_ascii_case(t))
}

fn strip_prefix_ci(chars: &[char], i: usize, prefix: &str) -> Option<usize> {
    let p: Vec<char> = prefix.chars().collect();
    if i + p.len() > chars.len() {
        return None;
    }
    if chars[i..i + p.len()]
        .iter()
        .zip(&p)
        .all(|(a, b)| a.eq_ignore_ascii_case(b))
    {
        Some(i + p.len())
    } else {
        None
    }
}

fn try_email(chars: &[char], i: usize) -> Option<Hit> {
    let mut j = i;
    let mut alnum = false;
    while j < chars.len() && is_email_local(chars[j]) {
        if chars[j].is_ascii_alphanumeric() {
            alnum = true;
        }
        j += 1;
    }
    if !alnum || j >= chars.len() || chars[j] != '@' {
        return None;
    }
    let local_end = j;
    j += 1;
    let dom_end = read_email_domain(chars, j)?;
    let mut spoken = speak_url_body(&chars[i..local_end]);
    spoken.push_str(" at ");
    spoken.push_str(&speak_url_body(&chars[j..dom_end]));
    Some(Hit {
        end: dom_end,
        spoken,
    })
}

fn is_email_local(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
}

fn read_email_domain(chars: &[char], i: usize) -> Option<usize> {
    let mut j = i;
    let mut labels = 0;
    let mut cur = 0usize;
    while j < chars.len() {
        if chars[j].is_ascii_alphanumeric() || chars[j] == '-' {
            cur += 1;
            j += 1;
        } else if chars[j] == '.'
            && cur > 0
            && j + 1 < chars.len()
            && (chars[j + 1].is_ascii_alphanumeric() || chars[j + 1] == '-')
        {
            labels += 1;
            cur = 0;
            j += 1;
        } else {
            break;
        }
    }
    if cur == 0 {
        return None;
    }
    labels += 1;
    let last_alpha = chars[j - cur..j].iter().all(|c| c.is_ascii_alphabetic());
    if labels < 2 || cur < 2 || !last_alpha {
        return None;
    }
    Some(j)
}

fn try_url(chars: &[char], i: usize) -> Option<Hit> {
    if let Some(j) = strip_prefix_ci(chars, i, "https://") {
        return finish_url(chars, j);
    }
    if let Some(j) = strip_prefix_ci(chars, i, "http://") {
        return finish_url(chars, j);
    }
    if let Some(j) = strip_prefix_ci(chars, i, "mailto:") {
        return try_email(chars, j);
    }
    try_bare_domain(chars, i)
}

fn finish_url(chars: &[char], body: usize) -> Option<Hit> {
    let end = scan_url_end(chars, body);
    if end <= body {
        return None;
    }
    let spoken = speak_url_body(&chars[body..end]);
    if spoken.trim().is_empty() {
        None
    } else {
        Some(Hit { end, spoken })
    }
}

fn scan_url_end(chars: &[char], i: usize) -> usize {
    let mut j = i;
    while j < chars.len()
        && !chars[j].is_whitespace()
        && !matches!(chars[j], '<' | '>' | '"' | '\'')
    {
        j += 1;
    }
    while j > i && matches!(chars[j - 1], '.' | ',' | ';' | ':' | '!' | '?' | ')') {
        j -= 1;
    }
    j
}

fn try_bare_domain(chars: &[char], i: usize) -> Option<Hit> {
    let mut j = i;
    let mut labels = 0;
    let mut cur = 0usize;
    let mut last_start = i;
    let mut saw_letter = false;
    while j < chars.len() {
        if chars[j].is_ascii_alphanumeric() || chars[j] == '-' {
            if cur == 0 {
                last_start = j;
            }
            if chars[j].is_ascii_alphabetic() {
                saw_letter = true;
            }
            cur += 1;
            j += 1;
        } else if chars[j] == '.'
            && cur > 0
            && j + 1 < chars.len()
            && (chars[j + 1].is_ascii_alphanumeric() || chars[j + 1] == '-')
        {
            labels += 1;
            cur = 0;
            j += 1;
        } else {
            break;
        }
    }
    if cur == 0 {
        return None;
    }
    labels += 1;
    if labels < 2 || !saw_letter {
        return None;
    }
    let last: String = chars[last_start..j].iter().collect();
    if !is_tld(&last) {
        return None;
    }
    let mut end = j;
    if end < chars.len() && chars[end] == '/' {
        end = scan_url_end(chars, end);
    }
    let spoken = speak_url_body(&chars[i..end]);
    Some(Hit { end, spoken })
}

fn speak_url_body(chars: &[char]) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphanumeric() {
            let s = i;
            while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                i += 1;
            }
            speak_alnum(&chars[s..i], &mut words);
        } else {
            let w = match c {
                '.' => "dot",
                '/' => "slash",
                '?' => "question",
                '=' => "equals",
                '&' => "and",
                '#' => "hash",
                '_' => "underscore",
                '-' => "dash",
                '+' => "plus",
                '%' => "percent",
                ':' => "colon",
                '~' => "tilde",
                '@' => "at",
                _ => "",
            };
            if !w.is_empty() {
                words.push(w.into());
            }
            i += 1;
        }
    }
    words.join(" ")
}

fn speak_alnum(piece: &[char], words: &mut Vec<String>) {
    let raw: String = piece.iter().collect();
    if raw.eq_ignore_ascii_case("www") {
        words.extend(["w", "w", "w"].into_iter().map(str::to_string));
        return;
    }
    let mut i = 0;
    while i < piece.len() {
        if piece[i].is_ascii_digit() {
            let s = i;
            while i < piece.len() && piece[i].is_ascii_digit() {
                i += 1;
            }
            let d: String = piece[s..i].iter().collect();
            words.push(speak_url_digits(&d));
        } else {
            let s = i;
            while i < piece.len() && piece[i].is_ascii_alphabetic() {
                i += 1;
            }
            let w: String = piece[s..i].iter().collect();
            if w.chars().count() == 1 {
                words.push(letter_word(w.chars().next().unwrap()).into());
            } else if w.chars().all(|c| c.is_ascii_uppercase())
                && (w.chars().count() <= 3 || !has_vowel(&w))
            {
                for c in w.chars() {
                    words.push(letter_word(c).into());
                }
            } else {
                words.push(w.to_ascii_lowercase());
            }
        }
    }
}

fn speak_url_digits(d: &str) -> String {
    if d.len() == 1 {
        cardinal(d.parse().unwrap_or(0))
    } else if d.starts_with('0') || d.len() >= 4 {
        digit_words(d)
    } else {
        d.parse::<u64>()
            .map(cardinal)
            .unwrap_or_else(|_| digit_words(d))
    }
}

fn try_dotted(chars: &[char], i: usize) -> Option<Hit> {
    let mut j = i;
    let mut letters = String::new();
    loop {
        if j >= chars.len() || !chars[j].is_ascii_alphabetic() {
            break;
        }
        if j + 1 < chars.len() && chars[j + 1] == '.' {
            letters.push(chars[j].to_ascii_lowercase());
            j += 2;
            continue;
        }
        if !letters.is_empty() && (j + 1 >= chars.len() || !chars[j + 1].is_ascii_alphanumeric()) {
            letters.push(chars[j].to_ascii_lowercase());
            j += 1;
        }
        break;
    }
    if letters.len() < 2 {
        return None;
    }
    let spoken = match letters.as_str() {
        "eg" => "for example".into(),
        "ie" => "that is".into(),
        "am" => "ay m".into(),
        "pm" => "pm".into(),
        "cf" => "compare".into(),
        "vs" => "versus".into(),
        _ => spell_letters(&letters),
    };
    Some(Hit { end: j, spoken })
}

fn try_abbrev(chars: &[char], i: usize) -> Option<Hit> {
    let mut j = i;
    while j < chars.len() && chars[j].is_ascii_alphabetic() {
        j += 1;
    }
    if j == i || j >= chars.len() || chars[j] != '.' {
        return None;
    }
    // "e.g." is a dotted initialism, not the abbreviation "e."
    if j - i == 1 && j + 1 < chars.len() && chars[j + 1].is_ascii_alphabetic() {
        return None;
    }
    let word: String = chars[i..j].iter().map(|c| c.to_ascii_lowercase()).collect();
    let end = j + 1;
    if word == "no" {
        if !next_nonspace_is_digit(chars, end) {
            return None;
        }
        return Some(Hit {
            end,
            spoken: "number".into(),
        });
    }
    let spoken = abbrev_word(&word, chars, i)?;
    if is_month(&spoken)
        && let Some((nend, day)) = following_day(chars, end)
    {
        return Some(Hit {
            end: nend,
            spoken: format!("{spoken} {day}"),
        });
    }
    Some(Hit {
        end,
        spoken: spoken.into(),
    })
}

fn abbrev_word(word: &str, chars: &[char], i: usize) -> Option<&'static str> {
    Some(match word {
        "dr" => "doctor",
        "mr" => "mister",
        "mrs" => "missus",
        "ms" => "ms",
        "jr" => "junior",
        "sr" => "senior",
        "prof" => "professor",
        "sgt" => "sergeant",
        "rev" => "reverend",
        "gov" => "governor",
        "sen" => "senator",
        "rep" => "representative",
        "ave" => "avenue",
        "blvd" => "boulevard",
        "rd" => "road",
        "mt" => "mount",
        "dept" => "department",
        "inc" => "incorporated",
        "ltd" => "limited",
        "corp" => "corporation",
        "co" => "company",
        "vs" => "versus",
        "etc" => "etc",
        "eg" => "for example",
        "ie" => "that is",
        "fig" => "figure",
        "vol" => "volume",
        "gen" => "general",
        "jan" => "january",
        "feb" => "february",
        "mar" => "march",
        "apr" => "april",
        "jun" => "june",
        "jul" => "july",
        "aug" => "august",
        "sep" | "sept" => "september",
        "oct" => "october",
        "nov" => "november",
        "dec" => "december",
        // "123 St." is the street; "St. Louis" is the saint. "am." is the verb, not "a.m."
        "st" => {
            if prev_token_is_number(chars, i) {
                "street"
            } else {
                "saint"
            }
        }
        _ => return None,
    })
}

fn is_month(w: &str) -> bool {
    MONTHS.contains(&w)
}

fn prev_token_is_number(chars: &[char], i: usize) -> bool {
    let mut j = i;
    while j > 0 && chars[j - 1].is_whitespace() {
        j -= 1;
    }
    if j == 0 {
        return false;
    }
    let mut saw_digit = false;
    let mut saw_other = false;
    while j > 0 && !chars[j - 1].is_whitespace() {
        j -= 1;
        if chars[j].is_ascii_digit() {
            saw_digit = true;
        } else if chars[j] != ',' && chars[j] != '-' {
            saw_other = true;
        }
    }
    saw_digit && !saw_other
}

fn next_nonspace_is_digit(chars: &[char], i: usize) -> bool {
    let mut j = i;
    if j < chars.len() && chars[j] == ' ' {
        j += 1;
    }
    j < chars.len() && chars[j].is_ascii_digit()
}

fn following_day(chars: &[char], i: usize) -> Option<(usize, String)> {
    let mut j = i;
    if j < chars.len() && chars[j] == ' ' {
        j += 1;
    }
    if j >= chars.len() || !chars[j].is_ascii_digit() {
        return None;
    }
    let (k, digs) = read_digits(chars, j);
    if digs.len() > 2 || (digs.len() > 1 && digs.starts_with('0')) {
        return None;
    }
    if k < chars.len() && chars[k].is_ascii_alphabetic() {
        return None; // "14th" is the ordinal rule
    }
    let n: u64 = digs.parse().ok()?;
    if !(1..=31).contains(&n) {
        return None;
    }
    Some((k, ordinal(n)))
}

fn try_acronym(chars: &[char], i: usize) -> Option<Hit> {
    let mut j = i;
    while j < chars.len() && chars[j].is_ascii_alphabetic() {
        j += 1;
    }
    if j - i < 2 {
        return None;
    }
    let word: String = chars[i..j].iter().collect();
    if word.eq_ignore_ascii_case("www") && ends_token(chars, j) {
        return Some(Hit {
            end: j,
            spoken: "w w w".into(),
        });
    }
    if !word.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    // NASA, ZORBLAT: a vowel and 4+ letters is a word (dictionary or letter-to-sound).
    // FBI, US, HTML: short, or no vowel, so spell the letters. "us" in the dictionary is
    // the pronoun, which is why the capitals have to be decided here.
    if word.len() >= 4 && has_vowel(&word) {
        return None;
    }
    let mut end = j;
    let mut spoken = spell_letters(&word);
    if end + 1 < chars.len()
        && chars[end] == '\''
        && (chars[end + 1] == 's' || chars[end + 1] == 'S')
    {
        end += 2;
        spoken.push_str(" s");
    }
    Some(Hit { end, spoken })
}

// ---------------------------------------------------------------- numbers

fn read_digits(chars: &[char], i: usize) -> (usize, String) {
    let mut j = i;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    (j, chars[i..j].iter().collect())
}

fn read_exact(chars: &[char], i: usize, n: usize) -> Option<(usize, String)> {
    if i + n > chars.len() || (i + n < chars.len() && chars[i + n].is_ascii_digit()) {
        return None;
    }
    if !chars[i..i + n].iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((i + n, chars[i..i + n].iter().collect()))
}

fn parse_u(d: &str) -> Option<u64> {
    if d.is_empty() || d.len() > 18 {
        None
    } else {
        d.parse().ok()
    }
}

fn try_time(chars: &[char], i: usize) -> Option<Hit> {
    let (j, hour_s) = read_digits(chars, i);
    if hour_s.is_empty() || hour_s.len() > 2 {
        return None;
    }
    let hour: u64 = hour_s.parse().ok()?;
    if j < chars.len() && chars[j] == ':' {
        let (k, min_s) = read_exact(chars, j + 1, 2)?;
        let min: u64 = min_s.parse().ok()?;
        if min > 59 {
            return None;
        }
        let mut end = k;
        let mut sec = 0u64;
        if end < chars.len() && chars[end] == ':' {
            let (k2, sec_s) = read_exact(chars, end + 1, 2)?;
            sec = sec_s.parse().ok()?;
            if sec > 59 {
                return None;
            }
            end = k2;
        }
        let (end, ampm) = take_ampm(chars, end);
        if hour > 23 || (ampm.is_some() && hour > 12) {
            return None;
        }
        return Some(Hit {
            end,
            spoken: speak_clock(hour, min, sec, ampm),
        });
    }
    let (end, ampm) = take_ampm(chars, j);
    if ampm.is_none() || hour > 12 {
        return None;
    }
    Some(Hit {
        end,
        spoken: speak_clock(hour, 0, 0, ampm),
    })
}

fn take_ampm(chars: &[char], i: usize) -> (usize, Option<&'static str>) {
    let mut j = i;
    let mut spaces = 0;
    while j < chars.len() && chars[j] == ' ' && spaces < 2 {
        j += 1;
        spaces += 1;
    }
    if j >= chars.len() {
        return (i, None);
    }
    let c = chars[j].to_ascii_lowercase();
    if c != 'a' && c != 'p' {
        return (i, None);
    }
    let which = if c == 'a' { "ay m" } else { "pm" };
    let mut k = j + 1;
    if k < chars.len() && chars[k] == '.' {
        k += 1;
    }
    if k >= chars.len() || chars[k].to_ascii_lowercase() != 'm' {
        return (i, None);
    }
    k += 1;
    if k < chars.len() && chars[k] == '.' {
        k += 1;
    }
    if k < chars.len() && chars[k].is_ascii_alphanumeric() {
        return (i, None);
    }
    (k, Some(which))
}

fn speak_clock(hour: u64, min: u64, sec: u64, ampm: Option<&str>) -> String {
    let mut s = if hour == 0 && ampm.is_some() {
        "twelve".to_string()
    } else {
        cardinal(hour)
    };
    if min == 0 && sec == 0 {
        if ampm.is_none() {
            s.push_str(" o'clock");
        }
    } else if min < 10 {
        s.push_str(" oh ");
        s.push_str(ONES[min as usize]);
    } else {
        s.push(' ');
        s.push_str(&cardinal(min));
    }
    if sec > 0 {
        s.push_str(" and ");
        s.push_str(&cardinal(sec));
        s.push_str(if sec == 1 { " second" } else { " seconds" });
    }
    if let Some(ap) = ampm {
        s.push(' ');
        s.push_str(ap);
    }
    s
}

fn sep_groups(chars: &[char], i: usize, sep: char) -> Option<(usize, Vec<String>)> {
    let (mut j, first) = read_digits(chars, i);
    if first.is_empty() {
        return None;
    }
    let mut groups = vec![first];
    while j < chars.len() && chars[j] == sep && j + 1 < chars.len() && chars[j + 1].is_ascii_digit()
    {
        let (k, g) = read_digits(chars, j + 1);
        if g.is_empty() {
            break;
        }
        groups.push(g);
        j = k;
    }
    if groups.len() < 2 {
        return None;
    }
    if j < chars.len() && chars[j].is_ascii_alphabetic() {
        return None;
    }
    Some((j, groups))
}

fn try_slash(chars: &[char], i: usize) -> Option<Hit> {
    let (end, groups) = sep_groups(chars, i, '/')?;
    if groups.len() == 2 && groups[0] == "24" && groups[1] == "7" {
        return Some(Hit {
            end,
            spoken: "twenty four seven".into(),
        });
    }
    if let Some(spoken) = slash_date(&groups) {
        return Some(Hit { end, spoken });
    }
    if let Some(spoken) = slash_fraction(&groups) {
        return Some(Hit { end, spoken });
    }
    None
}

fn slash_date(groups: &[String]) -> Option<String> {
    if groups.len() == 3 && groups[0].len() == 4 {
        let y = parse_u(&groups[0])?;
        let m = parse_u(&groups[1])?;
        let d = parse_u(&groups[2])?;
        if groups[1].len() <= 2 && groups[2].len() <= 2 && valid_day(m, d) {
            return Some(speak_date(m, d, Some(expand_year(y, groups[0].len()))));
        }
        return None;
    }
    if groups.len() == 3
        && groups[0].len() <= 2
        && groups[1].len() <= 2
        && (groups[2].len() == 2 || groups[2].len() == 4)
    {
        let a = parse_u(&groups[0])?;
        let b = parse_u(&groups[1])?;
        let y = expand_year(parse_u(&groups[2])?, groups[2].len());
        if valid_day(a, b) {
            return Some(speak_date(a, b, Some(y)));
        }
        if valid_day(b, a) {
            return Some(speak_date(b, a, Some(y)));
        }
        return None;
    }
    if groups.len() != 2 || groups[0].len() > 2 || groups[1].len() > 2 {
        return None;
    }
    let a = parse_u(&groups[0])?;
    let b = parse_u(&groups[1])?;
    let zeros = groups.iter().any(|g| g.len() > 1 && g.starts_with('0'));
    if !zeros && is_simple_fraction(a, b) {
        return None;
    }
    if valid_day(a, b) {
        Some(speak_date(a, b, None))
    } else {
        None
    }
}

fn is_simple_fraction(n: u64, d: u64) -> bool {
    matches!((n, d), (1, 2) | (1, 3) | (2, 3) | (1, 4) | (2, 4) | (3, 4))
}

fn slash_fraction(groups: &[String]) -> Option<String> {
    if groups.len() != 2
        || groups
            .iter()
            .any(|g| g.len() > 6 || (g.len() > 1 && g.starts_with('0')))
    {
        return None;
    }
    let n = parse_u(&groups[0])?;
    let d = parse_u(&groups[1])?;
    if d == 0 {
        return None;
    }
    Some(match (n, d) {
        (1, 2) | (2, 4) => "one half".into(),
        (1, 3) => "one third".into(),
        (2, 3) => "two thirds".into(),
        (1, 4) => "one quarter".into(),
        (3, 4) => "three quarters".into(),
        _ => format!("{} over {}", cardinal(n), cardinal(d)),
    })
}

fn valid_day(month: u64, day: u64) -> bool {
    let max = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 29,
        _ => return false,
    };
    (1..=max).contains(&day)
}

fn expand_year(y: u64, digits: usize) -> u64 {
    if digits <= 2 {
        if y <= 29 { 2000 + y } else { 1900 + y }
    } else {
        y
    }
}

fn speak_date(month: u64, day: u64, year: Option<u64>) -> String {
    let mut s = format!("{} {}", MONTHS[(month - 1) as usize], ordinal(day));
    if let Some(y) = year {
        s.push(' ');
        if (1100..=2099).contains(&y) {
            s.push_str(&year_words(y));
        } else {
            s.push_str(&cardinal(y));
        }
    }
    s
}

fn try_dash(chars: &[char], i: usize) -> Option<Hit> {
    let (end, groups) = sep_groups(chars, i, '-')?;
    if let Some(spoken) = dash_date(&groups) {
        return Some(Hit { end, spoken });
    }
    if let Some(spoken) = dash_phone(&groups) {
        return Some(Hit { end, spoken });
    }
    if let Some(spoken) = dash_range(&groups) {
        return Some(Hit { end, spoken });
    }
    None
}

fn dash_date(groups: &[String]) -> Option<String> {
    if groups.len() == 3 && groups[0].len() == 4 {
        let y = parse_u(&groups[0])?;
        let m = parse_u(&groups[1])?;
        let d = parse_u(&groups[2])?;
        if groups[1].len() <= 2 && groups[2].len() <= 2 && valid_day(m, d) {
            return Some(speak_date(m, d, Some(y)));
        }
        return None;
    }
    if groups.len() == 3
        && groups[0].len() <= 2
        && groups[1].len() <= 2
        && matches!(groups[2].len(), 2 | 4)
    {
        let a = parse_u(&groups[0])?;
        let b = parse_u(&groups[1])?;
        let y = expand_year(parse_u(&groups[2])?, groups[2].len());
        if valid_day(a, b) {
            return Some(speak_date(a, b, Some(y)));
        }
        if valid_day(b, a) {
            return Some(speak_date(b, a, Some(y)));
        }
        return None;
    }
    if groups.len() != 2 || groups[0].len() > 2 || groups[1].len() > 2 {
        return None;
    }
    let m = parse_u(&groups[0])?;
    let d = parse_u(&groups[1])?;
    let forced = groups.iter().any(|g| g.len() > 1 && g.starts_with('0')) || d > 12;
    if forced && valid_day(m, d) {
        Some(speak_date(m, d, None))
    } else {
        None
    }
}

fn dash_phone(groups: &[String]) -> Option<String> {
    if groups.len() < 2 || groups.iter().any(|g| g.is_empty() || g.len() > 4) {
        return None;
    }
    let total: usize = groups.iter().map(|g| g.len()).sum();
    if total < 7 {
        return None;
    }
    if groups.len() == 2 && both_yearish(groups) {
        return None;
    }
    let parts: Vec<String> = groups.iter().map(|g| digit_words(g)).collect();
    Some(parts.join(", "))
}

fn both_yearish(groups: &[String]) -> bool {
    groups.len() == 2
        && groups
            .iter()
            .all(|g| g.len() == 4 && parse_u(g).is_some_and(|n| (1000..=2099).contains(&n)))
}

fn dash_range(groups: &[String]) -> Option<String> {
    if groups.len() != 2
        || groups
            .iter()
            .any(|g| g.len() > 6 || (g.len() > 1 && g.starts_with('0')))
    {
        return None;
    }
    Some(format!(
        "{} to {}",
        speak_yearish(&groups[0]),
        speak_yearish(&groups[1])
    ))
}

fn try_ordinal(chars: &[char], i: usize) -> Option<Hit> {
    let (j, digs) = read_digits(chars, i);
    if digs.is_empty() || digs.len() > 15 || (digs.len() > 1 && digs.starts_with('0')) {
        return None;
    }
    let suf = ordinal_suffix_at(chars, j)?;
    let n: u64 = digs.parse().ok()?;
    if !suf.eq_ignore_ascii_case(suffix_for(n)) {
        return None;
    }
    let end = j + suf.len();
    if end < chars.len() && chars[end].is_ascii_alphanumeric() {
        return None;
    }
    Some(Hit {
        end,
        spoken: ordinal(n),
    })
}

fn ordinal_suffix_at(chars: &[char], i: usize) -> Option<&str> {
    let rest: String = chars.get(i..i + 2)?.iter().collect();
    if rest.eq_ignore_ascii_case("st")
        || rest.eq_ignore_ascii_case("nd")
        || rest.eq_ignore_ascii_case("rd")
        || rest.eq_ignore_ascii_case("th")
    {
        Some(if rest.eq_ignore_ascii_case("st") {
            "st"
        } else if rest.eq_ignore_ascii_case("nd") {
            "nd"
        } else if rest.eq_ignore_ascii_case("rd") {
            "rd"
        } else {
            "th"
        })
    } else {
        None
    }
}

fn try_comma_number(chars: &[char], i: usize) -> Option<Hit> {
    let mut j = i;
    let mut first = 0;
    while j < chars.len() && chars[j].is_ascii_digit() && first < 3 {
        j += 1;
        first += 1;
    }
    if first == 0 {
        return None;
    }
    let start = i;
    let mut groups = 0;
    loop {
        if j + 3 < chars.len()
            && chars[j] == ','
            && chars[j + 1].is_ascii_digit()
            && chars[j + 2].is_ascii_digit()
            && chars[j + 3].is_ascii_digit()
            && (j + 4 >= chars.len() || !chars[j + 4].is_ascii_digit())
        {
            j += 4;
            groups += 1;
        } else {
            break;
        }
    }
    if groups == 0 {
        return None;
    }
    let raw: String = chars[start..j]
        .iter()
        .filter(|c| c.is_ascii_digit())
        .collect();
    let spoken = if raw.len() >= 16 {
        digit_words(&raw)
    } else {
        raw.parse::<u64>()
            .map(cardinal)
            .unwrap_or_else(|_| digit_words(&raw))
    };
    Some(Hit { end: j, spoken })
}

fn try_decimal(chars: &[char], i: usize) -> Option<Hit> {
    let (j, int_s) = read_digits(chars, i);
    if j == i || j >= chars.len() || chars[j] != '.' {
        return None;
    }
    let (k, frac) = read_digits(chars, j + 1);
    if frac.is_empty() {
        return None;
    }
    let spoken = format!("{} point {}", speak_decimal_int(&int_s), digit_words(&frac));
    Some(Hit { end: k, spoken })
}

fn speak_decimal_int(digits: &str) -> String {
    if digits.len() > 1 && digits.starts_with('0') || digits.len() >= 16 {
        digit_words(digits)
    } else {
        digits
            .parse::<u64>()
            .map(cardinal)
            .unwrap_or_else(|_| digit_words(digits))
    }
}

fn try_leading_dot_decimal(chars: &[char], i: usize) -> Option<Hit> {
    let (k, frac) = read_digits(chars, i + 1);
    if frac.is_empty() {
        return None;
    }
    Some(Hit {
        end: k,
        spoken: format!("point {}", digit_words(&frac)),
    })
}

fn try_leading_zeros(chars: &[char], i: usize) -> Option<Hit> {
    let (j, digs) = read_digits(chars, i);
    if digs.len() < 2 || !digs.starts_with('0') || !ends_token(chars, j) {
        return None;
    }
    Some(Hit {
        end: j,
        spoken: digit_words(&digs),
    })
}

fn try_year(chars: &[char], i: usize) -> Option<Hit> {
    let (j, digs) = read_digits(chars, i);
    if digs.len() != 4 || !ends_token(chars, j) {
        return None;
    }
    let n: u64 = digs.parse().ok()?;
    if !(1100..=2099).contains(&n) {
        return None;
    }
    Some(Hit {
        end: j,
        spoken: year_words(n),
    })
}

fn try_big_int(chars: &[char], i: usize) -> Option<Hit> {
    let (j, digs) = read_digits(chars, i);
    if digs.starts_with('0') || !ends_token(chars, j) {
        return None;
    }
    if digs.len() >= 16 {
        return Some(Hit {
            end: j,
            spoken: digit_words(&digs),
        });
    }
    if digs.len() >= 7 {
        let n: u64 = digs.parse().ok()?;
        return Some(Hit {
            end: j,
            spoken: cardinal(n),
        });
    }
    None
}

fn try_currency(chars: &[char], i: usize, neg: bool) -> Option<Hit> {
    let (one, many, cent_one, cent_many) = match chars[i] {
        '$' => ("dollar", "dollars", "cent", "cents"),
        '£' => ("pound", "pounds", "penny", "pence"),
        '€' => ("euro", "euros", "cent", "cents"),
        _ => return None,
    };
    let mut j = i + 1;
    if j < chars.len() && chars[j] == ' ' {
        j += 1;
    }
    let mut neg = neg;
    if j < chars.len() && (chars[j] == '-' || chars[j] == '−') {
        neg = true;
        j += 1;
    }
    if j >= chars.len() || !chars[j].is_ascii_digit() {
        return None;
    }
    let start = j;
    let (mut end, _) = read_digits(chars, j);
    // 1,234 or 1,234,567 glued to the symbol
    if end < chars.len()
        && chars[end] == ','
        && let Some(hit) = try_comma_number(chars, start)
    {
        end = hit.end;
    }
    let mut frac = String::new();
    if end < chars.len()
        && chars[end] == '.'
        && end + 1 < chars.len()
        && chars[end + 1].is_ascii_digit()
    {
        let (k, f) = read_digits(chars, end + 1);
        frac = f;
        end = k;
    }
    let dollar_end = if frac.is_empty() {
        end
    } else {
        chars[start..end]
            .iter()
            .position(|c| *c == '.')
            .map(|p| start + p)
            .unwrap_or(end)
    };
    let dollars_s: String = chars[start..dollar_end]
        .iter()
        .filter(|c| c.is_ascii_digit())
        .collect();
    let spoken_dollars = if dollars_s.len() >= 16 {
        digit_words(&dollars_s)
    } else {
        dollars_s
            .parse::<u64>()
            .map(cardinal)
            .unwrap_or_else(|_| digit_words(&dollars_s))
    };
    let dollars_n = if dollars_s.len() >= 16 {
        None
    } else {
        dollars_s.parse::<u64>().ok()
    };
    let one_major = dollars_n == Some(1);
    let body = if frac.len() == 2 {
        let cents: u64 = frac.parse().unwrap_or(0);
        if dollars_n == Some(0) && cents == 0 {
            format!("zero {many}")
        } else if dollars_n == Some(0) {
            format!(
                "{} {}",
                cardinal(cents),
                if cents == 1 { cent_one } else { cent_many }
            )
        } else if cents == 0 {
            format!("{spoken_dollars} {}", if one_major { one } else { many })
        } else {
            format!(
                "{spoken_dollars} {} and {} {}",
                if one_major { one } else { many },
                cardinal(cents),
                if cents == 1 { cent_one } else { cent_many }
            )
        }
    } else if frac.is_empty() {
        format!("{spoken_dollars} {}", if one_major { one } else { many })
    } else {
        format!(
            "{spoken_dollars} point {} {}",
            digit_words(&frac),
            if one_major && frac.chars().all(|c| c == '0') {
                one
            } else {
                many
            }
        )
    };
    let spoken = if neg { format!("minus {body}") } else { body };
    Some(Hit { end, spoken })
}

/// Symbols that are not phrase punctuation. A return of `None` means "copy this character".
fn match_symbol(chars: &[char], i: usize, c: char) -> Option<Hit> {
    let next = chars.get(i + 1).copied();
    let spoken: Option<&str> = match c {
        '&' => Some("and"),
        '@' => Some("at"),
        '%' => Some("percent"),
        '+' => Some("plus"),
        '=' => Some("equals"),
        '*' => Some("asterisk"),
        '/' => Some("slash"),
        '\\' => Some("backslash"),
        '|' => Some("pipe"),
        '^' => Some("caret"),
        '~' => Some("tilde"),
        '°' => Some("degrees"),
        '×' => Some("times"),
        '÷' => Some("divided by"),
        '#' => Some(
            if next == Some(' ') && chars.get(i + 2).is_some_and(|d| d.is_ascii_digit())
                || next.is_some_and(|d| d.is_ascii_digit())
            {
                "number"
            } else {
                "hash"
            },
        ),
        '<' => {
            if next == Some('=') {
                return Some(Hit {
                    end: i + 2,
                    spoken: "less than or equal".into(),
                });
            }
            Some("less than")
        }
        '>' => {
            if next == Some('=') {
                return Some(Hit {
                    end: i + 2,
                    spoken: "greater than or equal".into(),
                });
            }
            Some("greater than")
        }
        '!' if next == Some('=') => {
            return Some(Hit {
                end: i + 2,
                spoken: "not equal".into(),
            });
        }
        '(' | ')' | '[' | ']' | '{' | '}' | '—' | '–' | '•' => Some(","),
        '…' => {
            return Some(Hit {
                end: i + 1,
                spoken: "...".into(),
            });
        }
        '.' => {
            let prev_alnum = i > 0 && chars[i - 1].is_ascii_alphanumeric();
            let next_alnum = next.is_some_and(|n| n.is_ascii_alphanumeric());
            if prev_alnum && next_alnum {
                Some("dot")
            } else {
                return None;
            }
        }
        '_' => {
            let prev_ident = i > 0 && (chars[i - 1].is_ascii_alphanumeric() || chars[i - 1] == '_');
            let next_ident = next.is_some_and(|n| n.is_ascii_alphanumeric() || n == '_');
            if !prev_ident && !next_ident {
                Some("underscore")
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(Hit {
        end: i + 1,
        spoken: spoken.unwrap_or("").into(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::number_words;
    use super::*;

    #[test]
    fn cardinals_match_the_legacy_spelling_under_a_million() {
        for n in [
            0, 1, 7, 10, 11, 19, 20, 21, 42, 99, 100, 101, 110, 999, 1000, 1001, 1010, 1100, 1999,
            2000, 2005, 2026, 9999, 10000, 100000, 999999,
        ] {
            assert_eq!(cardinal(n), number_words(n as f64), "{n}");
        }
    }
}
