//! Grammar crate: parsers for InterSystems ObjectScript (`cos`) and embedded IRIS SQL (`sql`),
//! written as `tygr` grammar types. Parsing only; type checking lives in the `ascot` crate that
//! depends on this one. Published separately to crates.io so other tools can reuse the grammar.

pub mod cos;
pub mod sql;
