//! `Desugar`: a full SQL node into its core copy, the sugar rewritten by the rules beside their
//! marks in `sql`. Here the trait, the impls of the leaves both levels share and of tygr's
//! containers, and the argument gathering the function rules share.

use std::cell::Cell;

use either::Either;
use tygr::{Grammar, Hidden, IntoInner, NotFollowedBy, Prefix, Ranged, Raw, Suffix, Vec1, Wrap};

use super::{core, full, sugar, *};
use crate::convert::VecSep;
use crate::cos::{MacroArgUnit, OSText};

pub trait Desugar {
    type Core;
    fn desugar(self) -> Self::Core;
}

/// The types both levels share, each its own core.
macro_rules! identity {
    ($($ty:ty),* $(,)?) => {
        $(impl Desugar for $ty {
            type Core = Self;
            fn desugar(self) -> Self {
                self
            }
        })*
    };
}

identity!(
    (),
    IsSpace,
    IsSimpleLatinLetter,
    IsNonAsciiLetter,
    IsNotDQ,
    IsNotSQ,
    IsDigit,
    IsNotNL,
    IsNotStar,
    IsAnyChar,
    IsExtFuncChar,
    IsNotParen,
    SpaceUnit,
    Space,
    Space1,
    Ident,
    SQLIdentNonKeyword,
    IdentifierStart,
    IdentifierPart,
    WordEnd,
    Identifier,
    DelimitedIdentifierPart,
    DelimitedIdentifier,
    DottedIdent,
    StdCharacterRepresentation,
    StringLiteral,
    Integer,
    SQLExponent,
    Number,
    NumberBase,
    Literal,
    HostVar,
    Var,
    HostRef,
    Host,
    HostIdentifier,
    HostIdentifierStart,
    HostIdentifierPart,
    Param,
    Sign,
    Sub,
    MacroArgUnit,
    OSText,
);

macro_rules! tuples {
    ($(($($t:ident $i:tt),+))*) => {
        $(impl<$($t: Desugar),+> Desugar for ($($t,)+) {
            type Core = ($($t::Core,)+);
            fn desugar(self) -> Self::Core {
                ($(self.$i.desugar(),)+)
            }
        })*
    };
}

tuples! {
    (A 0, B 1)
    (A 0, B 1, C 2)
    (A 0, B 1, C 2, D 3)
    (A 0, B 1, C 2, D 3, E 4)
    (A 0, B 1, C 2, D 3, E 4, F 5)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10)
}

impl<T: Desugar> Desugar for Option<T> {
    type Core = Option<T::Core>;
    fn desugar(self) -> Self::Core {
        self.map(Desugar::desugar)
    }
}

impl<T: Desugar> Desugar for Vec<T> {
    type Core = Vec<T::Core>;
    fn desugar(self) -> Self::Core {
        self.into_iter().map(Desugar::desugar).collect()
    }
}

impl<T: Desugar> Desugar for Vec1<T> {
    type Core = Vec1<T::Core>;
    fn desugar(self) -> Self::Core {
        Vec1::try_from(self.into_inner().desugar())
            .ok()
            .expect("one item or more")
    }
}

impl<T: Desugar> Desugar for Box<T> {
    type Core = Box<T::Core>;
    fn desugar(self) -> Self::Core {
        Box::new((*self).desugar())
    }
}

impl<Canonical: Grammar + Default + Desugar, Other: Grammar> Desugar
    for Spellings<Canonical, Other>
{
    type Core = Canonical::Core;
    fn desugar(self) -> Self::Core {
        match self {
            Spellings::Canonical(canonical) => canonical,
            Spellings::Other(_) => Default::default(),
        }
        .desugar()
    }
}

impl<L: Desugar, R: Desugar> Desugar for Either<L, R> {
    type Core = Either<L::Core, R::Core>;
    fn desugar(self) -> Self::Core {
        match self {
            Either::Left(left) => Either::Left(left.desugar()),
            Either::Right(right) => Either::Right(right.desugar()),
        }
    }
}

impl<T: Desugar, S: Desugar> Desugar for VecSep<T, S> {
    type Core = VecSep<T::Core, S::Core>;
    fn desugar(self) -> Self::Core {
        VecSep {
            items: self.items.desugar(),
            seps: self.seps.desugar(),
        }
    }
}

impl<P: Desugar, T: Desugar, S: Desugar> Desugar for Wrap<P, T, S> {
    type Core = Wrap<P::Core, T::Core, S::Core>;
    fn desugar(self) -> Self::Core {
        Wrap::new(
            self.prefix.desugar(),
            self.wrapped.desugar(),
            self.suffix.desugar(),
        )
    }
}

impl<P: Desugar, T: Desugar> Desugar for Prefix<P, T> {
    type Core = Prefix<P::Core, T::Core>;
    fn desugar(self) -> Self::Core {
        Prefix::new(self.prefix.desugar(), self.prefixed.desugar())
    }
}

impl<T: Desugar, S: Desugar> Desugar for Suffix<T, S> {
    type Core = Suffix<T::Core, S::Core>;
    fn desugar(self) -> Self::Core {
        Suffix::new(self.suffixed.desugar(), self.suffix.desugar())
    }
}

/// Covers `StringOf` and `StringOf1`, aliases of `Raw`.
impl<T: Desugar> Desugar for Raw<T> {
    type Core = Raw<T::Core>;
    fn desugar(self) -> Self::Core {
        Raw::new(self.into_inner())
    }
}

impl<T: Desugar> Desugar for Phrase<T> {
    type Core = Phrase<T::Core>;
    fn desugar(self) -> Self::Core {
        Phrase(self.0.desugar())
    }
}

impl<T: Desugar> Desugar for Hidden<T> {
    type Core = Hidden<T::Core>;
    fn desugar(self) -> Self::Core {
        Hidden(self.0.desugar())
    }
}

thread_local! {
    /// The range of the `Ranged` node being desugared, which the nodes a rule synthesizes stand for.
    static REWRITING: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

impl<T: Desugar> Desugar for Ranged<T> {
    type Core = Ranged<T::Core>;
    fn desugar(self) -> Self::Core {
        let outer = REWRITING.replace((self.start, self.end));
        let ranged = self.ranged.desugar();
        REWRITING.set(outer);
        Ranged::new(self.start, ranged, self.end)
    }
}

/// A node a rule synthesizes, at the range of the node being rewritten.
pub fn synthesized<T>(node: T) -> Ranged<T> {
    let (start, end) = REWRITING.get();
    Ranged::new(start, node, end)
}

impl<G: Desugar> Desugar for NotFollowedBy<G> {
    type Core = NotFollowedBy<G::Core>;
    fn desugar(self) -> Self::Core {
        NotFollowedBy::default()
    }
}

pub(super) fn commas<T>(items: Vec<T>) -> VecSep<T, ArgSep> {
    let sep = (Space::default(), Default::default(), Space::blank());
    let seps = vec![sep; items.len().saturating_sub(1)];
    VecSep { items, seps }
}

pub(super) fn parenthesized<T>(wrapped: T) -> Parenthesized<T> {
    let open = (Default::default(), Space::default());
    Wrap::new(open, wrapped, (Space::default(), Default::default()))
}

pub(super) fn list(args: Vec<core::Scalx>) -> core::ArgsN {
    let items = (!args.is_empty()).then(|| commas(args));
    (Space::default(), parenthesized(items))
}

/// A term as the expression standing for the node being rewritten.
pub(super) fn term(term: core::Term) -> core::Scalx {
    core::Scalx(VecSep::one(synthesized(term)))
}

pub(super) fn string(text: &str) -> core::Scalx {
    let literal = StringLiteral(Raw::new(format!("'{text}'")));
    term(core::Term::Literal(Literal::String(literal)))
}

pub(super) fn call(name: &str, args: Vec<core::Scalx>) -> core::Term {
    let (dollar, name) = match name.strip_prefix('$') {
        Some(name) => (Some(Default::default()), name),
        None => (None, name),
    };
    let name = DottedIdent {
        head: Ident::simple(name),
        rest: Vec::new(),
    };
    core::Term::Call(core::Call {
        name: core::SQLFunctionName {
            dollar,
            name: synthesized(name),
        },
        args: list(args),
    })
}

/// The arguments a function's argument list gives the call, in order.
pub(super) trait Arg {
    fn args(self, out: &mut Vec<core::Scalx>);
}

pub(super) fn args(shape: impl Arg) -> Vec<core::Scalx> {
    let mut out = Vec::new();
    shape.args(&mut out);
    out
}

impl Arg for full::Scalx {
    fn args(self, out: &mut Vec<core::Scalx>) {
        out.push(self.desugar())
    }
}

impl Arg for Ranged<full::Term> {
    fn args(self, out: &mut Vec<core::Scalx>) {
        out.push(core::Scalx(VecSep::one(self.desugar())))
    }
}

impl Arg for VecSep<full::Scalx, ArgSep> {
    fn args(self, out: &mut Vec<core::Scalx>) {
        out.extend(self.items.into_iter().map(Desugar::desugar))
    }
}

impl Arg for () {
    fn args(self, _: &mut Vec<core::Scalx>) {}
}

impl<T: Arg> Arg for Option<T> {
    fn args(self, out: &mut Vec<core::Scalx>) {
        if let Some(arg) = self {
            arg.args(out)
        }
    }
}

impl<K, T: Arg> Arg for (K, T) {
    fn args(self, out: &mut Vec<core::Scalx>) {
        self.1.args(out)
    }
}

impl<K, T: Arg, U: Arg> Arg for (K, T, U) {
    fn args(self, out: &mut Vec<core::Scalx>) {
        self.1.args(out);
        self.2.args(out)
    }
}

impl<T: Arg> Arg for Parenthesized<T> {
    fn args(self, out: &mut Vec<core::Scalx>) {
        self.wrapped.args(out)
    }
}

/// The argument shapes of the functions called by name, each its arguments in order.
macro_rules! shapes {
    ($($Shape:ident<$($T:ident),*> { $($field:ident)* })*) => {$(
        impl<$($T: Arg + Grammar),*> Arg for full::$Shape<$($T),*> {
            fn args(self, out: &mut Vec<core::Scalx>) {
                $(self.$field.args(out);)*
            }
        }
    )*};
}

shapes! {
    RR<T1, T2> { arg1 arg2 }
    RRR<T1, T2, T3> { arg1 arg2 arg3 }
    RRRR<T1, T2, T3, T4> { arg1 arg2 arg3 arg4 }
    RO<T1, T2> { arg1 arg2 }
    ROO<T1, T2, T3> { arg1 rest }
    RRO<T1, T2, T3> { arg1 arg2 arg3 }
    RROO<T1, T2, T3, T4> { arg1 arg2 rest }
}

/// `POSITION(a IN b)` has the arguments of `CHARINDEX(a, b)`.
impl Arg for sugar::SQLPositionArgs {
    fn args(self, out: &mut Vec<core::Scalx>) {
        self.needle.args(out);
        self.haystack.args(out)
    }
}

impl Arg for sugar::SubstringArgsInternal {
    fn args(self, out: &mut Vec<core::Scalx>) {
        self.expr.args(out);
        self.rest.args(out)
    }
}

impl<L: Arg, R: Arg> Arg for Either<L, R> {
    fn args(self, out: &mut Vec<core::Scalx>) {
        match self {
            Either::Left(left) => left.args(out),
            Either::Right(right) => right.args(out),
        }
    }
}
