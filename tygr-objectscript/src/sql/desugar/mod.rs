//! `Desugar`: a node of the full SQL grammar into its copy in the core grammar, the sugar rewritten
//! (see `grammar`). The grammar's own types get their impls from the `levels` attribute, each
//! `#[sugar]` mark naming its rule in `query` or `stat`; the leaves both levels share and tygr's
//! containers get theirs here.

pub mod query;

use either::Either;
use tygr::{Hidden, NotFollowedBy, Prefix, Ranged, Raw, Suffix, Vec1, Wrap};

use super::leaf::*;
use super::src::Src;
use crate::convert::VecSep;
use crate::cos::{MacroArgUnit, OSText};

pub trait Desugar {
    type Core;
    fn desugar(&self) -> Self::Core;
}

/// The types both levels share, each its own core.
macro_rules! identity {
    ($($ty:ty),* $(,)?) => {
        $(impl Desugar for $ty {
            type Core = Self;
            fn desugar(&self) -> Self {
                self.clone()
            }
        })*
    };
}

identity!(
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
    IsDbIdentStart,
    IsDbIdentPart,
    IsNotParen,
    SpaceUnit,
    Space,
    Ident,
    SQLIdentNonKeyword,
    IdentifierStart,
    IdentifierPart,
    IdentifierStruct,
    Identifier,
    SQLDelimitedIdentStruct,
    DelimitedIdentifierPart,
    DelimitedIdentifier,
    DottedIdent,
    StdCharacterRepresentation,
    StringLiteral,
    Integer,
    SQLExponent,
    ExpoSymbol,
    Number,
    NumberBase,
    Literal,
    ParameterMarker,
    HostVar,
    InstanceVariableHostRef,
    PropertyHostRef,
    Var,
    HostRef,
    HostIdentifier,
    HostIdentifierPart,
    Param,
    Sub,
    MacroArgUnit,
    OSText,
);

macro_rules! tuples {
    ($(($($t:ident $i:tt),+))*) => {
        $(impl<$($t: Desugar),+> Desugar for ($($t,)+) {
            type Core = ($($t::Core,)+);
            fn desugar(&self) -> Self::Core {
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
    fn desugar(&self) -> Self::Core {
        self.as_ref().map(Desugar::desugar)
    }
}

impl<T: Desugar> Desugar for Vec<T> {
    type Core = Vec<T::Core>;
    fn desugar(&self) -> Self::Core {
        self.iter().map(Desugar::desugar).collect()
    }
}

impl<T: Desugar> Desugar for Vec1<T> {
    type Core = Vec1<T::Core>;
    fn desugar(&self) -> Self::Core {
        Vec1::try_from((**self).desugar())
            .ok()
            .expect("one item or more")
    }
}

impl<T: Desugar> Desugar for Box<T> {
    type Core = Box<T::Core>;
    fn desugar(&self) -> Self::Core {
        Box::new((**self).desugar())
    }
}

impl<L: Desugar, R: Desugar> Desugar for Either<L, R> {
    type Core = Either<L::Core, R::Core>;
    fn desugar(&self) -> Self::Core {
        match self {
            Either::Left(left) => Either::Left(left.desugar()),
            Either::Right(right) => Either::Right(right.desugar()),
        }
    }
}

impl<T: Desugar, S: Desugar> Desugar for VecSep<T, S> {
    type Core = VecSep<T::Core, S::Core>;
    fn desugar(&self) -> Self::Core {
        VecSep {
            items: self.items.desugar(),
            seps: self.seps.desugar(),
        }
    }
}

impl<P: Desugar, T: Desugar, S: Desugar> Desugar for Wrap<P, T, S> {
    type Core = Wrap<P::Core, T::Core, S::Core>;
    fn desugar(&self) -> Self::Core {
        Wrap::new(
            self.prefix.desugar(),
            self.wrapped.desugar(),
            self.suffix.desugar(),
        )
    }
}

impl<P: Desugar, T: Desugar> Desugar for Prefix<P, T> {
    type Core = Prefix<P::Core, T::Core>;
    fn desugar(&self) -> Self::Core {
        Prefix::new(self.prefix.desugar(), self.prefixed.desugar())
    }
}

impl<T: Desugar, S: Desugar> Desugar for Suffix<T, S> {
    type Core = Suffix<T::Core, S::Core>;
    fn desugar(&self) -> Self::Core {
        Suffix::new(self.suffixed.desugar(), self.suffix.desugar())
    }
}

/// Covers `StringOf` and `StringOf1`, aliases of `Raw`.
impl<T: Desugar> Desugar for Raw<T> {
    type Core = Raw<T::Core>;
    fn desugar(&self) -> Self::Core {
        Raw::new(self.as_str())
    }
}

impl<T: Desugar> Desugar for Hidden<T> {
    type Core = Hidden<T::Core>;
    fn desugar(&self) -> Self::Core {
        Hidden(self.0.desugar())
    }
}

impl<T: Desugar> Desugar for Ranged<T> {
    type Core = Ranged<T::Core>;
    fn desugar(&self) -> Self::Core {
        Ranged::new(self.start, self.ranged.desugar(), self.end)
    }
}

impl<T: Desugar> Desugar for Src<T> {
    type Core = Src<T::Core>;
    fn desugar(&self) -> Self::Core {
        Src::new(self.range(), self.rewriting(|| self.node.desugar()))
    }
}

impl<G: Desugar> Desugar for NotFollowedBy<G> {
    type Core = NotFollowedBy<G::Core>;
    fn desugar(&self) -> Self::Core {
        NotFollowedBy::default()
    }
}
