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
    pub struct Select {
        pub kw: StringEqCI!("select"),
        #[sugar]
        pub all: Option<StringEqCI!("all")>,
    }
}

fn main() {}
