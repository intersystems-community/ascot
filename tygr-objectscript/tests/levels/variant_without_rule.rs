#![allow(dead_code, unused_imports)]
use tygr::*;

pub trait Desugar {
    type Core;
    fn desugar(&self) -> Self::Core;
}

#[tygr_objectscript_macros::levels(full, core, desugar = super::Desugar)]
mod grammar {
    use super::*;

    #[derive(Grammar)]
    pub enum Expr {
        #[sugar]
        Twice(StringEqCI!("twice"), Box<Expr>),
        Num(StringEq!("1")),
    }
}

fn main() {}
