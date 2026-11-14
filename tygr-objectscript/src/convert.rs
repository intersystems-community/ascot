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

impl<T: Parse, S: Parse> Parse for VecSep<T, S> {
    type First = <T::First as First>::Seq<Vec<(S, T)>>;

    #[inline]
    fn parse_at<M: Mode>(
        input: &str,
        pos: usize,
        mut state: State<M>,
    ) -> Option<(M::Output<Self>, usize)> {
        let (first, mut pos) = T::parse_at(input, pos, state.reborrow())?;
        let mut vec_sep = M::build(|_| VecSep {
            items: Vec::new(),
            seps: Vec::new(),
        });
        if let Some(mode) = M::building() {
            mode.get_mut(&mut vec_sep).items.push(mode.get(first));
        }
        loop {
            let Some((sep, after_sep)) = S::parse_at(input, pos, state.reborrow()) else {
                break;
            };
            let Some((item, end)) = T::parse_at(input, after_sep, state.reborrow()) else {
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

    fn fail_at<M: Mode>(pos: usize, mut state: State<M>) -> bool {
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
