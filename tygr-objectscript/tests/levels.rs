//! The `#[levels]` attribute on a toy grammar: the full level parses the sugar, the core level
//! lacks it, and desugaring one into the other prints the core text and round-trips through it.
//! The trait and the container impls are the test's own until the SQL grammar brings the real ones.

use either::Either;
use tygr::*;
use tygr_objectscript::convert::VecSep;
use tygr_objectscript_macros::levels;

pub trait Desugar {
    type Core;
    fn desugar(&self) -> Self::Core;
}

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
    (A 0)
    (A 0, B 1)
    (A 0, B 1, C 2)
    (A 0, B 1, C 2, D 3)
    (A 0, B 1, C 2, D 3, E 4)
    (A 0, B 1, C 2, D 3, E 4, F 5)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7)
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

/// The range stays the origin's in the full text.
impl<T: Desugar> Desugar for Ranged<T> {
    type Core = Ranged<T::Core>;
    fn desugar(&self) -> Self::Core {
        Ranged::new(self.start, self.ranged.desugar(), self.end)
    }
}

char_class!(pub Digit, "digit", |ch| ch.is_ascii_digit());
char_class!(pub Blank, "blank", |ch| ch == ' ');

/// Blanks: a leaf both levels share, the same type at each.
#[derive(Grammar, Clone, Debug, PartialEq)]
#[tygr(hidden)]
pub struct Sp(StringOf1<Blank>);

identity!(Digit, Blank, Sp);

#[levels(full, core, desugar = super::Desugar)]
pub mod grammar {
    use super::*;

    type Comma = (Option<Sp>, StringEq!(","), Option<Sp>);
    type Open = StringEq!("(");
    type Close = StringEq!(")");

    #[derive(Grammar, Debug, PartialEq)]
    pub struct Query {
        pub select: Ranged<Select>,
        pub semicolon: Option<StringEq!(";")>,
    }

    /// `select all` reads as `select`: the field goes, the type's rule rebuilds the rest.
    #[derive(Grammar, Debug, PartialEq)]
    #[sugar(into = |select| super::core::Select {
        kw: select.kw.desugar(),
        sp: select.sp.desugar(),
        items: select.items.desugar(),
    })]
    pub struct Select {
        pub kw: StringEqCI!("select"),
        pub sp: Sp,
        #[sugar]
        pub all: Option<Suffix<StringEqCI!("all"), Sp>>,
        pub items: VecSep<Expr, Comma>,
    }

    #[derive(Grammar, Debug, PartialEq)]
    pub enum Expr {
        Add {
            plus: StringEq!("+"),
            sp: Sp,
            lhs: Box<Expr>,
            sp2: Sp,
            rhs: Box<Expr>,
        },
        Neg(Prefix<StringEq!("-"), Box<Expr>>),
        Paren(Parened<Box<Expr>>),
        /// `twice x` is `+ x x`.
        #[sugar(into = |twice, expr| {
            let expr = expr.desugar();
            super::core::Expr::Add {
                plus: Default::default(),
                sp: twice.sp.desugar(),
                lhs: expr.clone(),
                sp2: twice.sp.desugar(),
                rhs: expr,
            }
        })]
        Twice(Twice, Box<Expr>),
        Num(Num),
    }

    /// The sugar's keyword and the blank after it: absent from the core.
    #[sugar]
    #[derive(Grammar, Debug, PartialEq)]
    pub struct Twice {
        pub kw: StringEqCI!("twice"),
        pub sp: Sp,
    }

    #[sugar]
    impl Twice {
        pub fn keyword(&self) -> String {
            self.kw.print()
        }
    }

    #[derive(Grammar, Debug, PartialEq)]
    pub struct Parened<T: Grammar = Num> {
        pub open: Open,
        pub inner: T,
        pub close: Close,
    }

    #[derive(Grammar, Debug, PartialEq)]
    pub struct Num(pub Raw<(Option<StringEq!("-")>, StringOf1<Digit>)>);
}

#[test]
fn the_core_lacks_the_sugar() {
    assert!(full::Query::parse("select all twice 3").is_ok());
    assert!(core::Query::parse("select all 3").is_err());
    assert!(core::Query::parse("select twice 3").is_err());
    assert!(core::Query::parse("select + 3 3, 2;").is_ok());

    // Exhaustive without a `Twice` arm.
    fn arity(expr: &core::Expr) -> usize {
        match expr {
            core::Expr::Add { .. } => 2,
            core::Expr::Neg(_) | core::Expr::Paren(_) => 1,
            core::Expr::Num(_) => 0,
        }
    }
    assert_eq!(arity(&core::Expr::parse("+ 1 2").unwrap()), 2);
}

#[test]
fn desugaring_prints_the_core_text_and_round_trips() {
    for (full_text, core_text) in [
        ("select all twice 3, 2;", "select + 3 3, 2;"),
        ("SELECT ALL -(Twice 1)", "SELECT -(+ 1 1)"),
        ("select 1 ,2", "select 1 ,2"),
    ] {
        let desugared = full::Query::parse(full_text).unwrap().desugar();
        assert_eq!(desugared.print(), core_text);
        let reparsed = core::Query::parse(core_text).unwrap();
        assert_eq!(reparsed.select.ranged, desugared.select.ranged);
        assert_eq!(reparsed.semicolon, desugared.semicolon);
    }
}

#[test]
fn a_desugared_node_keeps_its_origin_range() {
    let desugared = full::Query::parse("select all twice 3;").unwrap().desugar();
    assert_eq!(desugared.select.range(), 0..18);
    assert_eq!(desugared.select.print(), "select + 3 3");
}

#[test]
fn the_full_level_keeps_what_the_core_drops() {
    let full::Expr::Twice(twice, _) = full::Expr::parse("TWICE 1").unwrap() else {
        panic!("not the sugar")
    };
    assert_eq!(twice.keyword(), "TWICE");
    let parened = full::Parened::<Box<full::Expr>>::parse("(twice 2)").unwrap();
    assert_eq!(parened.desugar().print(), "(+ 2 2)");
    assert_eq!(
        <full::Parened>::parse("(-1)").unwrap().desugar().print(),
        "(-1)"
    );
}

#[test]
fn a_mark_out_of_place_is_a_compile_error() {
    trybuild::TestCases::new().compile_fail("tests/levels/*.rs");
}
