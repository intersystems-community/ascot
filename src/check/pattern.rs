//! ObjectScript pattern matching, `value ? pattern`, for the `PATTERN` a declared type carries.

use std::cell::Cell;

use tygr::{Parse, Print};
use tygr_objectscript::cos::{
    AllowIndirection, AllowMacro, Digits, OSString, PatternCode, PatternCount, PatternElement,
    PatternItem,
};

// The grammar's `Pattern`, which it doesn't export.
type Pattern = Vec<AllowMacro<AllowIndirection<PatternElement>>>;

struct Atom {
    min: usize,
    max: Option<usize>,
    item: Item,
}

// Over UTF-16 units, which IRIS counts.
enum Item {
    Literal(Vec<u16>),
    Codes(Vec<Code>),
    Alternatives(Vec<Vec<Atom>>),
}

#[derive(Clone, Copy, PartialEq)]
enum Code {
    Alpha,
    Upper,
    Lower,
    Numeric,
    Punctuation,
    Control,
    Any,
}

type Then<'a> = &'a dyn Fn(usize) -> bool;

/// Whether `value` matches `pattern`; `None` when that can't be told here: the pattern doesn't
/// parse, uses a macro, indirection or a locale-specific code, classifies a character outside
/// ASCII (IRIS does so by locale), the value is over 500 units, or backtracks past a budget.
pub(crate) fn matches(pattern: &str, value: &str) -> Option<bool> {
    let atoms = atoms(&Pattern::parse(pattern).ok()?)?;
    if !value.is_ascii() && classifies(&atoms) {
        return None;
    }
    let value: Vec<u16> = value.encode_utf16().collect();
    // The match recurses about three frames per unit, so the length bounds the stack.
    if value.len() > 500 {
        return None;
    }
    let steps = Cell::new(10_000);
    let matched = matches_at(&atoms, &value, 0, &|end| end == value.len(), &steps);
    (steps.get() > 0).then_some(matched)
}

/// The text an ObjectScript string literal stands for.
pub(super) fn unquote(literal: &OSString) -> String {
    literal.wrapped.print().replace("\"\"", "\"")
}

fn atoms(pattern: &Pattern) -> Option<Vec<Atom>> {
    (pattern.iter())
        .map(|element| {
            let AllowMacro::Plain(AllowIndirection::Plain(element)) = element else {
                return None;
            };
            let (min, max) = match &element.count {
                PatternCount::Exact(n) => {
                    let n = digits(n)?;
                    (n, Some(n))
                }
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
                    Item::Literal(unquote(literal).encode_utf16().collect())
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

fn code(code: &PatternCode) -> Option<Code> {
    Some(match code {
        PatternCode::Alpha(_) => Code::Alpha,
        PatternCode::Uppercase(_) => Code::Upper,
        PatternCode::Lowercase(_) => Code::Lower,
        PatternCode::Numeric(_) => Code::Numeric,
        PatternCode::Punctuation(_) => Code::Punctuation,
        PatternCode::Control(_) => Code::Control,
        PatternCode::Everything(_) => Code::Any,
        PatternCode::Cyrillic(_)
        | PatternCode::CyrillicUpper(_)
        | PatternCode::CyrillicLower(_)
        | PatternCode::FullWidth(_)
        | PatternCode::HalfWidthKatakana(_) => return None,
    })
}

// Whether any code but `E` classifies a character.
fn classifies(atoms: &[Atom]) -> bool {
    atoms.iter().any(|atom| match &atom.item {
        Item::Codes(codes) => codes.iter().any(|&code| code != Code::Any),
        Item::Alternatives(alternatives) => alternatives.iter().any(|a| classifies(a)),
        Item::Literal(_) => false,
    })
}

fn accepts(code: Code, unit: u16) -> bool {
    let Some(c) = char::from_u32(unit.into()).filter(char::is_ascii) else {
        return code == Code::Any;
    };
    match code {
        Code::Alpha => c.is_ascii_alphabetic(),
        Code::Upper => c.is_ascii_uppercase(),
        Code::Lower => c.is_ascii_lowercase(),
        Code::Numeric => c.is_ascii_digit(),
        Code::Punctuation => matches!(unit, 32..=47 | 58..=64 | 91..=96 | 123..=126),
        Code::Control => c.is_ascii_control(),
        Code::Any => true,
    }
}

// Whether `atoms` match `value` from `pos` up to an end `then` accepts; a pattern can match in
// several lengths, so every one is tried.
fn matches_at(atoms: &[Atom], value: &[u16], pos: usize, then: Then, steps: &Cell<u32>) -> bool {
    let Some((atom, rest)) = atoms.split_first() else {
        return then(pos);
    };
    // One occurrence of the item at `at`, `next` taking its end.
    let once = |at: usize, next: Then| match &atom.item {
        Item::Codes(codes) => {
            value
                .get(at)
                .is_some_and(|&unit| codes.iter().any(|&code| accepts(code, unit)))
                && next(at + 1)
        }
        Item::Literal(literal) => value[at..].starts_with(literal) && next(at + literal.len()),
        Item::Alternatives(alternatives) => {
            (alternatives.iter()).any(|alternative| matches_at(alternative, value, at, next, steps))
        }
    };
    let rest_from = |end| matches_at(rest, value, end, then, steps);
    repeat(&once, atom.min, atom.max, 0, pos, &rest_from, steps)
}

// The `n`th occurrence of an item at `pos`, `then` taking the end of the last one.
fn repeat(
    once: &dyn Fn(usize, Then) -> bool,
    min: usize,
    max: Option<usize>,
    n: usize,
    pos: usize,
    then: Then,
    steps: &Cell<u32>,
) -> bool {
    if steps.get() == 0 {
        return false;
    }
    steps.set(steps.get() - 1);
    if n >= min && then(pos) {
        return true;
    }
    if max.is_some_and(|max| n >= max) {
        return false;
    }
    once(pos, &|end| {
        // An empty occurrence repeats for free, so the count is reached whatever `min` is.
        if end == pos {
            then(pos)
        } else {
            repeat(once, min, max, n + 1, end, then, steps)
        }
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
        assert_eq!(matches(accessor, "имяGet"), Some(true));
        assert_eq!(matches(accessor, "Get"), Some(false));
        assert_eq!(matches(accessor, "nameget"), Some(false));
        assert_eq!(matches("3N", "123"), Some(true));
        assert_eq!(matches("3N", "12"), Some(false));
        assert_eq!(matches(".A1N", "ab1"), Some(true));
        assert_eq!(matches(r#"1.3AN1"-"2N"#, "ab-12"), Some(true));
        assert_eq!(matches(r#"1(1"a",2"b")"#, "bb"), Some(true));
        assert_eq!(matches(r#".(1"ab")"#, "ababab"), Some(true));
        assert_eq!(matches(r#"3"""#, ""), Some(true));
        assert_eq!(matches(r#"1""3A"#, "abc"), Some(true));
    }

    #[test]
    fn what_cannot_be_told() {
        assert_eq!(matches("1ZFWCHARZ", "x"), None);
        assert_eq!(matches("1R", "Я"), None);
        assert_eq!(matches("1Q", "x"), None);
        assert_eq!(matches("1A", "é"), None);
        assert_eq!(matches(r#".(.E)1"x""#, &"a".repeat(40)), None);
        assert_eq!(matches("1.E", &"a".repeat(500)), Some(true));
        assert_eq!(matches("1.E", &"a".repeat(501)), None);
    }
}
