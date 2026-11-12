//! Grammars that tygr 0.7 no longer derives: a list with its separators kept apart, and a type
//! parsed as another grammar and converted from it, with the conversion's own message when it
//! fails. [`Converted`] is implemented beside the `TryFrom`/`From`, and `converted!` writes its
//! `Parse`.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use tygr::bnf::{Expr, Rules};
use tygr::first::First;
use tygr::*;

/// One or more items, with a separator between each pair: `T { S T }`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VecSep<T, S> {
    pub items: Vec<T>,
    /// Always exactly one fewer than `items`.
    pub seps: Vec<S>,
}

impl<T, S> VecSep<T, S> {
    /// `(item, separator)` pairs, one per separator.
    pub fn into_item_sep(self) -> impl Iterator<Item = (T, S)> {
        self.items.into_iter().zip(self.seps)
    }
}

impl<T: Parse, S: Parse> Parse for VecSep<T, S> {
    type First = <T::First as First>::Seq<Vec<(S, T)>>;

    #[inline]
    fn parse_at<M: Mode>(
        input: &str,
        pos: usize,
        mut state: State,
    ) -> Option<(M::Output<Self>, usize)> {
        let (first, mut pos) = T::parse_at::<M>(input, pos, state.reborrow())?;
        let mut vec_sep = M::build(|_| VecSep {
            items: Vec::new(),
            seps: Vec::new(),
        });
        if let Some(mode) = M::building() {
            mode.get_mut(&mut vec_sep).items.push(mode.get(first));
        }
        loop {
            let Some((sep, after_sep)) = S::parse_at::<M>(input, pos, state.reborrow()) else {
                break;
            };
            let Some((item, end)) = T::parse_at::<M>(input, after_sep, state.reborrow()) else {
                break;
            };
            if end == pos {
                break; // a pair matching empty would loop forever
            }
            if let Some(mode) = M::building() {
                let VecSep { items, seps } = mode.get_mut(&mut vec_sep);
                seps.push(mode.get(sep));
                items.push(mode.get(item));
            }
            pos = end;
        }
        Some((vec_sep, pos))
    }

    fn fail_at(pos: usize, mut state: State) -> bool {
        T::fail_at(pos, state.reborrow()) || <Vec<(S, T)>>::fail_at(pos, state)
    }

    fn to_bnf() -> Expr {
        Expr::sequence(vec![T::to_bnf(), <Vec<(S, T)>>::to_bnf()])
    }

    fn visit_rules(rules: &mut Rules) {
        T::visit_rules(rules);
        S::visit_rules(rules);
    }
}

impl<T: Print, S: Print> Print for VecSep<T, S> {
    fn print_to(&self, buf: &mut String) {
        let mut items = self.items.iter();
        if let Some(first) = items.next() {
            first.print_to(buf);
        }
        for (sep, item) in self.seps.iter().zip(items) {
            sep.print_to(buf);
            item.print_to(buf);
        }
    }
}

/// A grammar read as `Source` and converted from it, so that a wrong shape is reported by what
/// the text must be. `NAME` is its rule; `REQUIREMENT` footnotes the EBNF, and `requirement`
/// words one failure, starting with `$` when it is a whole sentence about a function.
pub trait Converted: TryFrom<Self::Source> + 'static {
    type Source: Parse;
    const NAME: &'static str;
    const REQUIREMENT: &'static str;
    fn requirement(_error: <Self as TryFrom<Self::Source>>::Error) -> &'static str {
        Self::REQUIREMENT
    }
}

/// A message built while parsing, kept for the error to point at; the set stays small as long as
/// no message quotes the source.
pub fn intern(message: String) -> &'static str {
    static INTERNED: LazyLock<Mutex<HashSet<&'static str>>> = LazyLock::new(Default::default);
    let mut interned = INTERNED.lock().unwrap();
    if let Some(known) = interned.get(message.as_str()) {
        return known;
    }
    let message: &'static str = Box::leak(message.into_boxed_str());
    interned.insert(message);
    message
}

pub fn parse_at<T: Converted, M: Mode>(
    input: &str,
    pos: usize,
    mut state: State,
) -> Option<(M::Output<T>, usize)> {
    let mut state = state.node(T::NAME, pos);
    let (source, end) = T::Source::parse_at::<Build>(input, pos, state.reborrow())?;
    match T::try_from(source) {
        Ok(value) => Some((M::build(|_| value), end)),
        Err(error) => {
            state.reject(Some(T::NAME), pos..end, T::requirement(error));
            None
        }
    }
}

/// `parse_at` for a conversion that can't fail, which checking skips; no rule of its own is
/// entered, as the error can only be the source's.
pub fn parse_at_infallible<T: Converted + From<T::Source>, M: Mode>(
    input: &str,
    pos: usize,
    state: State,
) -> Option<(M::Output<T>, usize)> {
    let (source, end) = T::Source::parse_at::<M>(input, pos, state)?;
    Some((M::map(source, T::from), end))
}

pub fn fail_at<T: Converted>(pos: usize, mut state: State) -> bool {
    T::Source::fail_at(pos, state.node(T::NAME, pos))
}

pub fn fail_at_infallible<T: Converted>(pos: usize, state: State) -> bool {
    T::Source::fail_at(pos, state)
}

pub fn to_bnf<T: Converted>() -> Expr {
    Expr::RuleRef(T::NAME.to_string())
}

pub fn visit_rules<T: Converted>(rules: &mut Rules) {
    if rules.insert::<T>(T::NAME, || {
        Expr::side_condition(T::Source::to_bnf(), T::REQUIREMENT)
    }) {
        T::Source::visit_rules(rules);
    }
}

/// `converted!(Type)` implements `Parse` for a [`Converted`] type through its `TryFrom`;
/// `converted!(Type, infallible)` for one with a `From`, which checking needn't run; and
/// `converted!(Type<B>)` for a generic one, whose rule is spliced rather than named since every
/// instantiation would share the name.
macro_rules! converted {
    ($ty:ident) => {
        converted!(@impl $ty, parse_at, fail_at);
    };
    ($ty:ident, infallible) => {
        converted!(@impl $ty, parse_at_infallible, fail_at_infallible);
    };
    (@impl $ty:ident, $parse:ident, $fail:ident) => {
        impl Parse for $ty {
            type First = <<Self as $crate::convert::Converted>::Source as Parse>::First;
            fn parse_at<M: Mode>(input: &str, pos: usize, state: State) -> Option<(M::Output<Self>, usize)> {
                $crate::convert::$parse::<Self, M>(input, pos, state)
            }
            fn fail_at(pos: usize, state: State) -> bool {
                $crate::convert::$fail::<Self>(pos, state)
            }
            fn to_bnf() -> bnf::Expr {
                $crate::convert::to_bnf::<Self>()
            }
            fn visit_rules(rules: &mut bnf::Rules) {
                $crate::convert::visit_rules::<Self>(rules);
            }
        }
    };
    ($ty:ident<$p:ident>) => {
        impl<$p: Parse> Parse for $ty<$p> {
            type First = <<Self as $crate::convert::Converted>::Source as Parse>::First;
            fn parse_at<M: Mode>(input: &str, pos: usize, state: State) -> Option<(M::Output<Self>, usize)> {
                $crate::convert::parse_at_infallible::<Self, M>(input, pos, state)
            }
            fn fail_at(pos: usize, state: State) -> bool {
                $crate::convert::fail_at_infallible::<Self>(pos, state)
            }
            fn to_bnf() -> bnf::Expr {
                <<Self as $crate::convert::Converted>::Source as Parse>::to_bnf()
            }
            fn visit_rules(rules: &mut bnf::Rules) {
                <<Self as $crate::convert::Converted>::Source as Parse>::visit_rules(rules);
            }
        }
    };
}

/// `char_class!` for a predicate that isn't `const`: its `const` twin, equal to it on ASCII,
/// computes the first bytes, and every UTF-8 lead byte may start one.
macro_rules! unicode_class {
    ($vis:vis $class:ident, $name:literal, |$ch:ident| $pred:expr, |$ascii:ident| $const_pred:expr) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis struct $class(pub char);

        impl CharClass for $class {
            fn matches($ch: char) -> bool {
                $pred
            }
            fn name() -> &'static str {
                $name
            }
            fn new(ch: char) -> Self {
                $class(ch)
            }
            fn value(&self) -> char {
                self.0
            }
        }

        impl tygr::first::ByteSet for $class {
            const BYTE_SET: tygr::first::byte_set::Set = {
                const fn ascii($ascii: char) -> bool {
                    $const_pred
                }
                let mut bytes = <tygr::__private::NonAsciiStart as tygr::first::ByteSet>::BYTE_SET;
                let mut b = 0u8;
                while b < 0x80 {
                    if ascii(b as char) {
                        bytes = bytes.with(b);
                    }
                    b += 1;
                }
                bytes
            };
        }

        impl Parse for $class {
            type First = tygr::first::Required<$class>;
            fn parse_at<M: Mode>(input: &str, pos: usize, state: State) -> Option<(M::Output<Self>, usize)> {
                tygr::__private::class_parse_at::<Self, M>(input, pos, state)
            }
            fn fail_at(pos: usize, state: State) -> bool {
                tygr::__private::class_fail_at::<Self>(pos, state)
            }
            fn to_bnf() -> bnf::Expr {
                tygr::__private::class_to_bnf::<Self>()
            }
            fn visit_rules(_rules: &mut bnf::Rules) {}
        }

        impl Print for $class {
            fn print_to(&self, buf: &mut String) {
                buf.push(self.0)
            }
        }
    };
}
