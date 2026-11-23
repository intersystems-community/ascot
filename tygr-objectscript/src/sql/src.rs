//! `Src<T>`: a node with the byte range of its source text, and the recorder that maps where
//! each node prints to where it came from.

use std::cell::RefCell;
use std::ops::{Deref, Range};

use tygr::{Parse, Print, Ranged};

/// `T` with the range of the text it comes from: a parsed node's own range, or, after a rewrite,
/// the range of the text it stands for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Parse)]
#[tygr(inline, try_from_infallible = Ranged<T>)]
pub struct Src<T> {
    pub range: Range<usize>,
    pub node: T,
}

impl<T> Src<T> {
    pub fn new(range: Range<usize>, node: T) -> Self {
        Src { range, node }
    }

    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Apply `f` to the node, keeping the same range.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Src<U> {
        Src {
            range: self.range,
            node: f(self.node),
        }
    }
}

impl<T> From<Ranged<T>> for Src<T> {
    fn from(ranged: Ranged<T>) -> Self {
        Src {
            range: ranged.range(),
            node: ranged.ranged,
        }
    }
}

impl<T> Deref for Src<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.node
    }
}

/// Where a node printed, and the range of the text it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    pub printed: Range<usize>,
    pub origin: Range<usize>,
}

thread_local! {
    static RECORDER: RefCell<Option<Vec<Mapping>>> = const { RefCell::new(None) };
}

impl<T: Print> Print for Src<T> {
    fn print_to(&self, buf: &mut String) {
        let start = buf.len();
        self.node.print_to(buf);
        RECORDER.with_borrow_mut(|recorder| {
            if let Some(mappings) = recorder {
                mappings.push(Mapping {
                    printed: start..buf.len(),
                    origin: self.range.clone(),
                });
            }
        });
    }
}

/// `t`'s text, with where each `Src` node in it printed, innermost first.
pub fn print_mapped<T: Print>(t: &T) -> (String, Vec<Mapping>) {
    let outer = RECORDER.replace(Some(Vec::new()));
    let text = t.print();
    let mappings = RECORDER.replace(outer).unwrap_or_default();
    (text, mappings)
}
