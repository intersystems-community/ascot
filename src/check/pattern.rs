//! ObjectScript pattern matching, `value ? pattern`, for the `PATTERN` a declared type carries.

use tygr::Grammar;
use tygr_objectscript::cos::{
    AllowIndirection, AllowMacro, Digits, PatternCode, PatternCount, PatternElement, PatternItem,
};

// The grammar's `Pattern`, which it doesn't export.
type Pattern = Vec<AllowMacro<AllowIndirection<PatternElement>>>;

struct Atom {
    min: usize,
    max: Option<usize>,
    item: Item,
}

enum Item {
    Literal(Vec<char>),
    Codes(Vec<Code>),
    Alternatives(Vec<Vec<Atom>>),
}

#[derive(Clone, Copy)]
enum Code {
    Alpha,
    Upper,
    Lower,
    Numeric,
    Punctuation,
    Control,
    Any,
    Cyrillic,
    CyrillicUpper,
    CyrillicLower,
}

/// Whether `value` matches `pattern`; `None` when the pattern can't be evaluated here: it doesn't
/// parse, or uses a macro, indirection or a locale-specific code.
pub(crate) fn matches(pattern: &str, value: &str) -> Option<bool> {
    let atoms = atoms(&Pattern::parse(pattern).ok()?)?;
    let value: Vec<char> = value.chars().collect();
    Some(matches_at(&atoms, &value, 0, &|end| end == value.len()))
}

fn atoms(pattern: &Pattern) -> Option<Vec<Atom>> {
    (pattern.iter())
        .map(|element| {
            let AllowMacro::Plain(AllowIndirection::Plain(element)) = element else {
                return None;
            };
            let (min, max) = match &element.count {
                PatternCount::Exact(n) => (digits(n)?, Some(digits(n)?)),
                PatternCount::Range { min, max, .. } => {
                    let min = match min {
                        Some(min) => digits(min)?,
                        None => 0,
                    };
                    let max = match max {
                        Some(max) => Some(digits(max)?),
                        None => None,
                    };
                    (min, max)
                }
            };
            let item = match &element.item {
                PatternItem::Literal(literal) => {
                    Item::Literal(unquote(&literal.print()).chars().collect())
                }
                PatternItem::Codes(codes) => {
                    Item::Codes(codes.iter().map(code).collect::<Option<_>>()?)
                }
                PatternItem::Alter(alternatives) => Item::Alternatives(
                    (alternatives.wrapped.items.iter())
                        .map(atoms)
                        .collect::<Option<_>>()?,
                ),
            };
            Some(Atom { min, max, item })
        })
        .collect()
}

fn digits(digits: &Digits) -> Option<usize> {
    digits.print().parse().ok()
}

fn unquote(literal: &str) -> String {
    match literal.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(inner) => inner.replace("\"\"", "\""),
        None => literal.to_string(),
    }
}

fn code(code: &PatternCode) -> Option<Code> {
    Some(match code {
        PatternCode::Alpha(_) => Code::Alpha,
        PatternCode::Uppercase(_) => Code::Upper,
        PatternCode::Lowercase(_) => Code::Lower,
        PatternCode::Numeric(_) => Code::Numeric,
        PatternCode::Punctuation(_) => Code::Punctuation,
        PatternCode::Control(_) => Code::Control,
        PatternCode::Everything(_) => Code::Any,
        PatternCode::Cyrillic(_) => Code::Cyrillic,
        PatternCode::CyrillicUpper(_) => Code::CyrillicUpper,
        PatternCode::CyrillicLower(_) => Code::CyrillicLower,
        PatternCode::FullWidth(_) | PatternCode::HalfWidthKatakana(_) => return None,
    })
}

fn accepts(code: Code, c: char) -> bool {
    let cyrillic = ('\u{0400}'..='\u{04FF}').contains(&c);
    match code {
        Code::Alpha => c.is_alphabetic(),
        Code::Upper => c.is_uppercase(),
        Code::Lower => c.is_lowercase(),
        Code::Numeric => c.is_ascii_digit(),
        Code::Punctuation => matches!(c as u32, 32..=47 | 58..=64 | 91..=96 | 123..=126),
        Code::Control => c.is_control(),
        Code::Any => true,
        Code::Cyrillic => cyrillic && c.is_alphabetic(),
        Code::CyrillicUpper => cyrillic && c.is_uppercase(),
        Code::CyrillicLower => cyrillic && c.is_lowercase(),
    }
}

// Whether `atoms` match `value` from `pos` up to some end `then` accepts; a pattern can match a
// prefix in several lengths, so every one is tried.
fn matches_at(atoms: &[Atom], value: &[char], pos: usize, then: &dyn Fn(usize) -> bool) -> bool {
    let Some((atom, rest)) = atoms.split_first() else {
        return then(pos);
    };
    let rest_from = |end: usize| matches_at(rest, value, end, then);
    match &atom.item {
        Item::Codes(codes) => {
            let run = (value[pos..].iter())
                .take_while(|c| codes.iter().any(|&code| accepts(code, **c)))
                .count();
            let most = atom.max.map_or(run, |max| run.min(max));
            (atom.min..=most).rev().any(|n| rest_from(pos + n))
        }
        Item::Literal(literal) => {
            let (mut n, mut end) = (0, pos);
            loop {
                if n >= atom.min && rest_from(end) {
                    return true;
                }
                if literal.is_empty()
                    || atom.max.is_some_and(|max| n >= max)
                    || !value[end..].starts_with(literal)
                {
                    return false;
                }
                (n, end) = (n + 1, end + literal.len());
            }
        }
        Item::Alternatives(alternatives) => {
            repeat(alternatives, atom.min, atom.max, 0, value, pos, &rest_from)
        }
    }
}

// The `n`th repetition of one of `alternatives` at `pos`, `then` taking the end of the last one.
fn repeat(
    alternatives: &[Vec<Atom>],
    min: usize,
    max: Option<usize>,
    n: usize,
    value: &[char],
    pos: usize,
    then: &dyn Fn(usize) -> bool,
) -> bool {
    if n >= min && then(pos) {
        return true;
    }
    if max.is_some_and(|max| n >= max) {
        return false;
    }
    alternatives.iter().any(|alternative| {
        matches_at(alternative, value, pos, &|end| {
            // An empty match repeats for free, so the count is reached whatever `min` is.
            if end == pos {
                then(pos)
            } else {
                repeat(alternatives, min, max, n + 1, value, end, then)
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn counts_codes_literals_and_alternatives() {
        let accessor = r#"1.E1(1"Get",1"Set")"#;
        assert_eq!(matches(accessor, "nameGet"), Some(true));
        assert_eq!(matches(accessor, "%fooSet"), Some(true));
        assert_eq!(matches(accessor, "Get"), Some(false));
        assert_eq!(matches(accessor, "nameget"), Some(false));
        assert_eq!(matches("3N", "123"), Some(true));
        assert_eq!(matches("3N", "12"), Some(false));
        assert_eq!(matches(".A1N", "ab1"), Some(true));
        assert_eq!(matches(r#"1.3AN1"-"2N"#, "ab-12"), Some(true));
        assert_eq!(matches(r#"1(1"a",2"b")"#, "bb"), Some(true));
        assert_eq!(matches(r#".(1"ab")"#, "ababab"), Some(true));
        assert_eq!(matches("1ZFWCHARZ", "x"), None);
        assert_eq!(matches("1Q", "x"), None);
    }
}
