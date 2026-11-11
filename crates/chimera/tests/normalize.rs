//! What Speak mode says for text the old front end skipped or split apart.
use chimera::phonetics::Intonation;
use chimera::phonetics::english::{EnglishG2P, normalize_speech};
use std::io::Read;

fn words(text: &str) -> Vec<String> {
    normalize_speech(text)
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '\'')
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

fn g2p() -> EnglishG2P {
    let path = format!(
        "{}/../../public/data/cmudict.txt.gz",
        env!("CARGO_MANIFEST_DIR")
    );
    let gz = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut text = String::new();
    flate2::read::GzDecoder::new(&gz[..])
        .read_to_string(&mut text)
        .unwrap();
    let mut g = EnglishG2P::new();
    g.load_dict(&text);
    g
}

fn said(text: &str) -> Vec<String> {
    g2p()
        .to_phrases(text)
        .into_iter()
        .flat_map(|p| p.words)
        .collect()
}

fn ipa(text: &str) -> String {
    g2p()
        .to_phrases(text)
        .into_iter()
        .flat_map(|p| p.segments.into_iter().map(|s| s.ipa))
        .collect()
}

#[test]
fn plain_words_are_copied_through() {
    for s in [
        "Hello world",
        "water",
        "Is it on? Yes, it is.",
        "42",
        "The __proto__ is ready.",
        "x1y2z3",
        "ZORBLAT quixotical fnord!",
        "0 1 19 20 21 99 100 101 110 999 1000 1001",
        "Rock'n'roll isn't dead",
    ] {
        assert_eq!(normalize_speech(s), s, "{s:?}");
    }
    // the curly apostrophe was already folded before lookup; the words do not move
    assert_eq!(normalize_speech("it’s well-known"), "it's well-known");
}

#[test]
fn normalizer_rewrites_the_tokens_lookup_used_to_drop() {
    // bare integers below a million stay digits; expand_numbers spells them later
    assert_eq!(words("42"), ["42"]);
    assert_eq!(words("-12"), ["minus", "12"]);
    assert_eq!(words("-3.14"), ["minus", "three", "point", "one", "four"]);
    assert_eq!(words("3.14"), ["three", "point", "one", "four"]);
    assert_eq!(words(".5"), ["point", "five"]);
    assert_eq!(words("3rd"), ["third"]);
    assert_eq!(words("21st"), ["twenty", "first"]);
    assert_eq!(words("22nd"), ["twenty", "second"]);
    assert_eq!(words("11th"), ["eleventh"]);
    assert_eq!(words("103rd"), ["one", "hundred", "third"]);
    assert_eq!(words("1999"), ["nineteen", "ninety", "nine"]);
    assert_eq!(words("2026"), ["twenty", "twenty", "six"]);
    assert_eq!(words("2005"), ["two", "thousand", "five"]);
    assert_eq!(words("1900"), ["nineteen", "hundred"]);
    assert_eq!(words("1905"), ["nineteen", "oh", "five"]);
    assert_eq!(words("1000"), ["1000"]);
    assert_eq!(words("007"), ["zero", "zero", "seven"]);
    assert_eq!(words("0042"), ["zero", "zero", "four", "two"]);
    assert_eq!(words("1000000"), ["one", "million"]);
    assert_eq!(words("1,000,000"), ["one", "million"]);
    assert_eq!(
        words("1234567"),
        [
            "one", "million", "two", "hundred", "thirty", "four", "thousand", "five", "hundred",
            "sixty", "seven"
        ]
    );
    assert_eq!(
        words("555-1234"),
        ["five", "five", "five", "one", "two", "three", "four"]
    );
    assert_eq!(words("5-10"), ["five", "to", "ten"]);
    assert_eq!(words(&"9".repeat(20)), vec!["nine"; 20]);
}

#[test]
fn money_percents_times_and_dates() {
    assert_eq!(
        words("$5.20"),
        ["five", "dollars", "and", "twenty", "cents"]
    );
    assert_eq!(
        words("$12.50"),
        ["twelve", "dollars", "and", "fifty", "cents"]
    );
    assert_eq!(words("$1"), ["one", "dollar"]);
    assert_eq!(words("$0.05"), ["five", "cents"]);
    assert_eq!(
        words("$1,234"),
        [
            "one", "thousand", "two", "hundred", "thirty", "four", "dollars"
        ]
    );
    assert_eq!(words("-$5"), ["minus", "five", "dollars"]);
    assert_eq!(words("20%"), ["20", "percent"]);
    assert_eq!(words("3.5%"), ["three", "point", "five", "percent"]);
    assert_eq!(words("7:30"), ["seven", "thirty"]);
    assert_eq!(words("7:05"), ["seven", "oh", "five"]);
    assert_eq!(words("7:00"), ["seven", "o'clock"]);
    assert_eq!(words("7:30pm"), ["seven", "thirty", "pm"]);
    assert_eq!(words("7:30 p.m."), ["seven", "thirty", "pm"]);
    assert_eq!(words("0:30am"), ["twelve", "thirty", "ay", "m"]);
    assert_eq!(
        words("7:30:15"),
        ["seven", "thirty", "and", "fifteen", "seconds"]
    );
    assert_eq!(words("3/14"), ["march", "fourteenth"]);
    assert_eq!(
        words("3/14/26"),
        ["march", "fourteenth", "twenty", "twenty", "six"]
    );
    assert_eq!(
        words("2026-03-14"),
        ["march", "fourteenth", "twenty", "twenty", "six"]
    );
    assert_eq!(words("9/11"), ["september", "eleventh"]);
    assert_eq!(words("1/2"), ["one", "half"]);
    assert_eq!(words("3/4"), ["three", "quarters"]);
    assert_eq!(words("22/7"), ["twenty", "two", "over", "seven"]);
    assert_eq!(words("24/7"), ["twenty", "four", "seven"]);
    assert_eq!(
        words("1/2/2026"),
        ["january", "second", "twenty", "twenty", "six"]
    );
}

#[test]
fn symbols_abbreviations_acronyms_urls_and_email() {
    assert_eq!(words("A & B"), ["a", "and", "b"]);
    assert_eq!(words("C++"), ["c", "plus", "plus"]);
    assert_eq!(words("2 < 3"), ["2", "less", "than", "3"]);
    assert_eq!(words("a=b"), ["a", "equals", "b"]);
    assert_eq!(words("and/or"), ["and", "slash", "or"]);
    assert_eq!(words("#1"), ["number", "1"]);
    assert_eq!(words("#"), ["hash"]);
    assert_eq!(words("file.txt"), ["file", "dot", "txt"]);
    assert_eq!(words("Dr. Lee"), ["doctor", "lee"]);
    assert_eq!(
        words("Mr. Smith vs. Dr. Jones"),
        ["mister", "smith", "versus", "doctor", "jones"]
    );
    assert_eq!(words("e.g. this"), ["for", "example", "this"]);
    assert_eq!(words("i.e. that"), ["that", "is", "that"]);
    assert_eq!(words("St. Louis"), ["saint", "louis"]);
    assert_eq!(words("5 St."), ["5", "street"]);
    assert_eq!(words("NASA"), ["nasa"]);
    assert_eq!(words("FBI"), ["f", "b", "eye"]);
    assert_eq!(words("HTML"), ["h", "t", "m", "l"]);
    assert_eq!(words("US"), ["u", "s"]);
    assert_eq!(words("a@b.com"), ["ay", "at", "b", "dot", "com"]);
    assert_eq!(
        words("https://example.com/a"),
        ["example", "dot", "com", "slash", "ay"]
    );
    assert_eq!(
        words("www.example.com"),
        ["w", "w", "w", "dot", "example", "dot", "com"]
    );
    assert_eq!(words("I am here"), ["i", "am", "here"]);
}

#[test]
fn punctuation_pauses_and_unknown_text_does_not_drop_the_words_around_it() {
    let g = g2p();
    let wait = g.to_phrases("Wait. Really?");
    assert_eq!(wait.len(), 2);
    assert_eq!(wait[0].intonation, Intonation::Statement);
    assert_eq!(wait[1].intonation, Intonation::Question);
    assert!(wait[0].pause > wait[1].pause || wait[0].pause >= 0.3);

    let hi = g.to_phrases("hi 😀 there 🎉 now");
    assert_eq!(hi[0].words, ["hi", "there", "now"]);
    assert!(hi[0].segments.iter().any(|s| s.word_end == Some(true)));

    let zh = g.to_phrases("Hello 你好 world");
    assert_eq!(zh[0].words, ["hello", "world"]);

    assert!(g.to_phrases("😀🎉").is_empty());
    assert!(g.to_phrases("").is_empty());

    // a parenthesis is a pause, not a deleted word
    let paren = g.to_phrases("go (now) home");
    assert_eq!(
        paren
            .iter()
            .flat_map(|p| p.words.clone())
            .collect::<Vec<_>>(),
        ["go", "now", "home"]
    );
    assert!(paren.len() >= 2);
}

#[test]
fn speak_mode_says_the_mixed_sentence_with_nothing_dropped() {
    let g = g2p();
    let text = "On 3/14 at 7:30pm, Dr. Lee paid $12.50 (20% off) & emailed a@b.com #1!";
    let phrases = g.to_phrases(text);
    let said: Vec<&str> = phrases
        .iter()
        .flat_map(|p| p.words.iter().map(String::as_str))
        .collect();
    assert_eq!(
        said,
        [
            "on",
            "march",
            "fourteenth",
            "at",
            "seven",
            "thirty",
            "pm",
            "doctor",
            "lee",
            "paid",
            "twelve",
            "dollars",
            "and",
            "fifty",
            "cents",
            "twenty",
            "percent",
            "off",
            "and",
            "emailed",
            "ay",
            "at",
            "b",
            "dot",
            "com",
            "number",
            "one"
        ]
    );
    for p in &phrases {
        let ends = p
            .segments
            .iter()
            .filter(|s| s.word_end == Some(true))
            .count();
        assert_eq!(p.words.len(), ends);
        assert!(!p.segments.is_empty());
    }
    assert_eq!(phrases.last().unwrap().intonation, Intonation::Statement);
    assert!(phrases.last().unwrap().pause >= 0.3);

    // dictionary readings, not letter-to-sound guesses
    assert_eq!(ipa("NASA"), "næsə");
    assert_eq!(ipa("FBI"), "ɛfbiaɪ");
    assert_eq!(ipa("Dr. Lee"), "dɑktɚli");
}

#[test]
fn lookup_still_spells_the_plain_integers_the_goldens_lock_down() {
    let g = g2p();
    assert_eq!(g.to_phrases("7:00")[0].words, ["seven", "o'clock"]);
    assert_eq!(g.to_phrases("42")[0].words, ["forty", "two"]);
    assert_eq!(g.to_phrases("-12")[0].words, ["minus", "twelve"]);
    assert_eq!(g.to_phrases("20%")[0].words, ["twenty", "percent"]);
    assert_eq!(g.to_phrases("#1")[0].words, ["number", "one"]);
    assert_eq!(
        g.to_phrases("x1y2z3")[0].words,
        ["x", "one", "y", "two", "z", "three"]
    );
    assert_eq!(
        g.to_phrases("1000 1001")[0].words,
        ["one", "thousand", "one", "thousand", "one"]
    );
    assert_eq!(g.to_phrases("Hello world")[0].words, ["hello", "world"]);
    assert_eq!(ipa("Hello world"), "həloʊwɝɫd");
    assert_eq!(
        g.to_phrases("The __proto__ is ready.")[0].words,
        ["the", "proto", "is", "ready"]
    );
    assert_eq!(
        said("007 and 0042"),
        [
            "zero", "zero", "seven", "and", "zero", "zero", "four", "two"
        ]
    );
}
