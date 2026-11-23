//! The `#[levels]` attribute on a toy grammar: the full level parses the sugar, the core level
//! lacks it, and desugaring one into the other prints the core text and round-trips through it.

use tygr::*;
use tygr_objectscript::convert::VecSep;
use tygr_objectscript::sql::desugar::Desugar;
use tygr_objectscript_macros::levels;

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

// The literals as leaves of the toy's own: the identity impl the attribute emits for a literal
// spelled in the module would be an orphan here, the trait and tygr's literal types both foreign.
macro_rules! literals {
    ($($name:ident = $text:literal),* $(,)?) => {
        $(
            #[derive(Grammar, Clone, Debug, PartialEq, Default)]
            pub struct $name(StringEqCI!($text));
        )*
        identity!($($name),*);
    };
}

char_class!(pub Digit, "digit", |ch| ch.is_ascii_digit());
char_class!(pub Blank, "blank", |ch| ch == ' ');

/// Blanks: a leaf both levels share, the same type at each.
#[derive(Grammar, Clone, Debug, PartialEq)]
#[tygr(hidden)]
pub struct Sp(StringOf1<Blank>);

identity!(Digit, Blank, Sp);
literals!(
    Comma = ",",
    Open = "(",
    Close = ")",
    Semi = ";",
    SelectKw = "select",
    AllKw = "all",
    Plus = "+",
    Minus = "-",
    TwiceKw = "twice",
);

#[levels(full, core, desugar = super::Desugar)]
pub mod grammar {
    use super::*;

    type Sep = (Option<Sp>, Comma, Option<Sp>);

    #[derive(Grammar, Debug, PartialEq)]
    pub struct Query {
        pub select: Ranged<Select>,
        pub semicolon: Option<Semi>,
    }

    /// `select all` reads as `select`: the field goes, the type's rule rebuilds the rest.
    #[derive(Grammar, Debug, PartialEq)]
    #[sugar(into = |select| super::core::Select {
        kw: select.kw.desugar(),
        sp: select.sp.desugar(),
        items: select.items.desugar(),
    })]
    pub struct Select {
        pub kw: SelectKw,
        pub sp: Sp,
        #[sugar]
        pub all: Option<Suffix<AllKw, Sp>>,
        pub items: VecSep<Expr, Sep>,
    }

    #[derive(Grammar, Debug, PartialEq)]
    pub enum Expr {
        Add {
            plus: Plus,
            sp: Sp,
            lhs: Box<Expr>,
            sp2: Sp,
            rhs: Box<Expr>,
        },
        Neg(Prefix<Minus, Box<Expr>>),
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
        pub kw: TwiceKw,
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
    pub struct Num(pub Raw<(Option<Minus>, StringOf1<Digit>)>);
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
