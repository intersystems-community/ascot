//! A grammar tygr no longer provides: a list with its separators kept apart.

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

impl<T: Parse, Sep: Parse> Parse for VecSep<T, Sep> {
    type First = <T::First as First>::Seq<Vec<(Sep, T)>>;

    #[inline]
    fn parse_at<S: Success, L: Level>(
        input: &str,
        pos: usize,
        mut state: State<L>,
    ) -> Option<(S::Output<Self>, usize)> {
        let (first, mut pos) = T::parse_at::<S, L>(input, pos, state.reborrow())?;
        let mut vec_sep = S::and_then(first, |first| {
            S::output(|| VecSep {
                items: vec![first],
                seps: Vec::new(),
            })
        });
        loop {
            let Some((sep, after_sep)) = Sep::parse_at::<S, L>(input, pos, state.reborrow()) else {
                break;
            };
            let Some((item, end)) = T::parse_at::<S, L>(input, after_sep, state.reborrow()) else {
                break;
            };
            if end == pos {
                break; // a pair matching empty would loop forever
            }
            vec_sep = S::and_then(vec_sep, |mut vec_sep| {
                S::and_then(sep, |sep| {
                    S::and_then(item, |item| {
                        vec_sep.seps.push(sep);
                        vec_sep.items.push(item);
                        S::output(|| vec_sep)
                    })
                })
            });
            pos = end;
        }
        Some((vec_sep, pos))
    }

    fn to_bnf() -> Expr {
        Expr::sequence(vec![T::to_bnf(), <Vec<(Sep, T)>>::to_bnf()])
    }

    fn visit_rules(rules: &mut Rules) {
        T::visit_rules(rules);
        Sep::visit_rules(rules);
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
