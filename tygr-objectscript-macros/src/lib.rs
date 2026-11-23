//! One grammar module produced at two levels: `full`, every spelling the language accepts, and
//! `core`, the same minus its syntactic sugar, with the rewrite from one to the other written at
//! the sugar's mark.
//!
//! ```ignore
//! #[levels(full, core, desugar = super::desugar::Desugar)]
//! pub mod grammar {
//!     use super::*;
//!
//!     #[derive(Grammar)]
//!     pub enum Expr {
//!         Add { plus: StringEq!("+"), sp: Sp, lhs: Box<Expr>, sp2: Sp, rhs: Box<Expr> },
//!         /// `twice x` is `+ x x`.
//!         #[sugar(into = |twice, expr| {
//!             let expr = expr.desugar();
//!             super::core::Expr::Add { plus: Default::default(), sp: twice.sp.desugar(), lhs: expr.clone(), sp2: twice.sp.desugar(), rhs: expr }
//!         })]
//!         Twice(Twice, Box<Expr>),
//!         Num(Num),
//!     }
//! }
//! ```
//!
//! `#[levels(full, core, desugar = PATH)]` on an inline module emits two modules named by its
//! first two arguments, the sugar-ful one and the one without it, and in the first an impl of
//! `PATH` per type converting it into its core copy. `PATH` names the trait, which the crate using
//! the attribute defines:
//!
//! ```ignore
//! pub trait Desugar {
//!     type Core;
//!     fn desugar(&self) -> Self::Core;
//! }
//! ```
//!
//! # Marks
//!
//! - `#[sugar(into = EXPR)]` on an enum variant: the variant is absent from the core; the enum's
//!   impl converts it with `EXPR`, a function or closure taking the variant's fields by reference
//!   in declaration order and returning the enum's core type.
//! - `#[sugar]` on a field: the field is absent from the core, and the type carries the rule.
//! - `#[sugar(into = EXPR)]` on a struct or enum: present in the core minus its marked fields and
//!   variants; `EXPR`, taking `&full_value` and returning the core type, is its whole conversion.
//! - `#[sugar]` on a struct or enum: absent from the core, with no impl. On any other item (an
//!   impl block, a type alias), likewise left out of the core.
//!
//! # What is generated
//!
//! Every item is copied into both modules: `use` declarations, type aliases, impl blocks, macro
//! invocations, functions and nested modules verbatim; structs and enums with their attributes,
//! `#[derive(Clone)]` added where `Clone` is not derived (the impls take `&self`), the marks
//! stripped, and in the core copy the marked fields, variants and types left out. The impls go at
//! the end of the full module, so the attribute's `PATH`, a rule and a carried bound resolve as
//! the module's own items do: `super::` is the module's parent, `super::core::T` a core type. An
//! unmarked struct or enum converts field by field and variant by variant through `.desugar()`,
//! a unit variant into the same unit variant. A generic type's impl bounds each type parameter
//! `T` by the trait, repeats `T`'s bounds for `T::Core`, and has `Core = core::Foo<T::Core>`.
//!
//! Types from outside the module need impls of their own: blanket ones for tygr's containers,
//! identity ones for the leaves both levels share. A literal (`StringEq!`, `StringEqCI!`) cannot
//! have a blanket impl, as `tygr::__private::Token<G>` bounds `G` by a trait tygr does not
//! export, so the attribute emits an identity impl for every literal spelled in the module's
//! fields and type aliases, once each (`StringEqCI!` of a text without cased chars is the
//! `StringEq!` type). A literal the module names only through an outside alias gets its impl by
//! hand; one leveled module per trait, as a second would emit the same impls again.
//!
//! # Errors
//!
//! Each a compile error at the mark:
//! - a `#[sugar]` variant without `into`, in a type without its own `into`;
//! - `into` on a field, which goes on a variant or on the type;
//! - a `#[sugar]` field in a type without its own `into`;
//! - a key other than `into`;
//! - a variant's `into` in a type whose own `into` converts the whole value, which would go
//!   unused.
//!
//! A `derive` rejects a type macro as a field of a generic type, the added `#[derive(Clone)]`
//! included: alias the literal first, `type Open = StringEq!("(");`.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::visit_mut::{self, VisitMut};
use syn::{
    AttrStyle, Attribute, Error, Expr, Field, Fields, GenericParam, Generics, Ident, Index, Item,
    ItemEnum, ItemMod, ItemStruct, LitStr, Macro, Meta, Path, Token, TraitBoundModifier, Type,
    TypeMacro, TypeParamBound, Variant, WherePredicate, parse_quote,
};

const USAGE: &str = "expected `#[levels(full, core, desugar = PATH)]`";
const FORMS: &str = "expected `#[sugar]` or `#[sugar(into = EXPR)]`";
const ABSENT: &str = "the type is absent from the core: its fields and variants need no mark";

/// `full, core, desugar = PATH`.
struct Args {
    full: Ident,
    core: Ident,
    desugar: Path,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let full = input.parse()?;
        input.parse::<Token![,]>()?;
        let core = input.parse()?;
        input.parse::<Token![,]>()?;
        let key: Ident = input.parse()?;
        if key != "desugar" {
            return Err(Error::new(key.span(), USAGE));
        }
        input.parse::<Token![=]>()?;
        let desugar = input.parse()?;
        Ok(Args {
            full,
            core,
            desugar,
        })
    }
}

/// See the [crate docs](crate).
#[proc_macro_attribute]
pub fn levels(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = match syn::parse::<Args>(attr) {
        Ok(args) => args,
        Err(err) => return Error::new(err.span(), USAGE).to_compile_error().into(),
    };
    let module = syn::parse_macro_input!(item as ItemMod);
    expand(&args, module)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn expand(args: &Args, module: ItemMod) -> syn::Result<TokenStream2> {
    let Some((_, items)) = module.content else {
        let message = "`#[levels]` goes on an inline module, `mod name { … }`";
        return Err(Error::new(module.mod_token.span, message));
    };
    let mut out = Output {
        args,
        full: Vec::new(),
        core: Vec::new(),
        impls: Vec::new(),
        literals: BTreeSet::new(),
    };
    for item in items {
        out.item(item)?;
    }
    let (inner, outer): (Vec<_>, Vec<_>) = module
        .attrs
        .into_iter()
        .partition(|attr| matches!(attr.style, AttrStyle::Inner(_)));
    let Args { full, core, .. } = args;
    let vis = &module.vis;
    let Output {
        full: full_items,
        core: core_items,
        impls,
        ..
    } = out;
    Ok(quote! {
        #(#outer)* #vis mod #full { #(#inner)* #(#full_items)* #(#impls)* }
        #(#outer)* #vis mod #core { #(#inner)* #(#core_items)* }
    })
}

enum Mark {
    Bare,
    Into(Expr),
}

struct Marked {
    mark: Mark,
    span: Span,
}

fn take_mark(attrs: &mut Vec<Attribute>) -> syn::Result<Option<Marked>> {
    let mut marked = None;
    for attr in std::mem::take(attrs) {
        if !attr.path().is_ident("sugar") {
            attrs.push(attr);
            continue;
        }
        if marked.is_some() {
            return Err(Error::new_spanned(attr, "one `#[sugar]` per item"));
        }
        marked = Some(parse_mark(&attr)?);
    }
    Ok(marked)
}

fn parse_mark(attr: &Attribute) -> syn::Result<Marked> {
    let span = attr.span();
    let mark = match &attr.meta {
        Meta::Path(_) => Mark::Bare,
        Meta::List(_) => {
            let mut into = None;
            attr.parse_nested_meta(|meta| {
                if !meta.path.is_ident("into") {
                    let message = "unknown key: `#[sugar]` takes `into = EXPR` or nothing";
                    return Err(meta.error(message));
                }
                if into.is_some() {
                    return Err(meta.error("`into` given twice"));
                }
                into = Some(meta.value()?.parse()?);
                Ok(())
            })?;
            into.map(Mark::Into)
                .ok_or_else(|| Error::new(span, FORMS))?
        }
        Meta::NameValue(_) => return Err(Error::new(span, FORMS)),
    };
    Ok(Marked { mark, span })
}

fn ensure_clone(attrs: &mut Vec<Attribute>) {
    let derived = attrs
        .iter()
        .filter(|attr| attr.path().is_ident("derive"))
        .any(|attr| {
            attr.parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated)
                .is_ok_and(|paths| {
                    paths.iter().any(|path| {
                        path.segments
                            .last()
                            .is_some_and(|segment| segment.ident == "Clone")
                    })
                })
        });
    if !derived {
        attrs.push(parse_quote!(#[derive(::core::clone::Clone)]));
    }
}

fn remove_fields(fields: &mut Fields, dropped: &[usize]) {
    let keep = |fields: &mut Punctuated<Field, Token![,]>| {
        *fields = std::mem::take(fields)
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !dropped.contains(i))
            .map(|(_, field)| field)
            .collect();
    };
    match fields {
        Fields::Named(named) => keep(&mut named.named),
        Fields::Unnamed(unnamed) => keep(&mut unnamed.unnamed),
        Fields::Unit => {}
    }
}

fn item_attrs(item: &mut Item) -> Option<&mut Vec<Attribute>> {
    Some(match item {
        Item::Const(item) => &mut item.attrs,
        Item::Enum(item) => &mut item.attrs,
        Item::ExternCrate(item) => &mut item.attrs,
        Item::Fn(item) => &mut item.attrs,
        Item::ForeignMod(item) => &mut item.attrs,
        Item::Impl(item) => &mut item.attrs,
        Item::Macro(item) => &mut item.attrs,
        Item::Mod(item) => &mut item.attrs,
        Item::Static(item) => &mut item.attrs,
        Item::Struct(item) => &mut item.attrs,
        Item::Trait(item) => &mut item.attrs,
        Item::TraitAlias(item) => &mut item.attrs,
        Item::Type(item) => &mut item.attrs,
        Item::Union(item) => &mut item.attrs,
        Item::Use(item) => &mut item.attrs,
        _ => return None,
    })
}

/// `{ f: desugar(value), … }`, `(desugar(value), …)` or nothing, `value` the full field.
fn fields_desugar(
    desugar: &Path,
    fields: &Fields,
    value: impl Fn(usize, &Field) -> TokenStream2,
) -> TokenStream2 {
    match fields {
        Fields::Named(named) => {
            let parts = named.named.iter().enumerate().map(|(i, field)| {
                let (name, value) = (&field.ident, value(i, field));
                quote!(#name: #desugar::desugar(#value))
            });
            quote!({ #(#parts,)* })
        }
        Fields::Unnamed(unnamed) => {
            let parts = unnamed.unnamed.iter().enumerate().map(|(i, field)| {
                let value = value(i, field);
                quote!(#desugar::desugar(#value))
            });
            quote!(( #(#parts,)* ))
        }
        Fields::Unit => quote!(),
    }
}

fn variant_pattern(variant: &Variant) -> (TokenStream2, Vec<Ident>) {
    let name = &variant.ident;
    match &variant.fields {
        Fields::Named(named) => {
            let binds: Vec<Ident> = named
                .named
                .iter()
                .map(|field| field.ident.clone().expect("named"))
                .collect();
            (quote!(Self::#name { #(#binds),* }), binds)
        }
        Fields::Unnamed(unnamed) => {
            let binds: Vec<Ident> = (0..unnamed.unnamed.len())
                .map(|i| format_ident!("__f{i}"))
                .collect();
            (quote!(Self::#name(#(#binds),*)), binds)
        }
        Fields::Unit => (quote!(Self::#name), Vec::new()),
    }
}

/// Replaces each type parameter `T` with `<T as Desugar>::Core`.
struct Subst<'a> {
    params: &'a [Ident],
    desugar: &'a Path,
}

impl VisitMut for Subst<'_> {
    fn visit_type_mut(&mut self, ty: &mut Type) {
        if let Type::Path(path) = ty
            && path.qself.is_none()
            && path.path.leading_colon.is_none()
            && let Some(first) = path.path.segments.first()
            && first.arguments.is_none()
            && self.params.contains(&first.ident)
        {
            let (param, desugar) = (&first.ident, self.desugar);
            let rest = path.path.segments.iter().skip(1);
            let core: Type = if path.path.segments.len() == 1 {
                parse_quote!(<#param as #desugar>::Core)
            } else {
                parse_quote!(<<#param as #desugar>::Core>::#(#rest)::*)
            };
            *ty = core;
            return;
        }
        visit_mut::visit_type_mut(self, ty);
    }
}

struct Output<'a> {
    args: &'a Args,
    full: Vec<TokenStream2>,
    core: Vec<TokenStream2>,
    impls: Vec<TokenStream2>,
    /// The literals given an impl, as `(has a cased char, text)`.
    literals: BTreeSet<(bool, String)>,
}

impl Output<'_> {
    fn item(&mut self, item: Item) -> syn::Result<()> {
        match item {
            Item::Struct(item) => self.structure(item),
            Item::Enum(item) => self.enumeration(item),
            mut item => {
                if let Item::Type(alias) = &item {
                    self.literals(&alias.ty);
                }
                let mark = match item_attrs(&mut item) {
                    Some(attrs) => take_mark(attrs)?,
                    None => None,
                };
                self.full.push(item.to_token_stream());
                match mark {
                    None => self.core.push(item.to_token_stream()),
                    Some(Marked {
                        mark: Mark::Bare, ..
                    }) => {}
                    Some(Marked {
                        mark: Mark::Into(_),
                        span,
                    }) => {
                        let message = "`into` goes on a struct, an enum or a variant";
                        return Err(Error::new(span, message));
                    }
                }
                Ok(())
            }
        }
    }

    fn structure(&mut self, mut item: ItemStruct) -> syn::Result<()> {
        let type_mark = take_mark(&mut item.attrs)?;
        ensure_clone(&mut item.attrs);
        let dropped = self.fields(&mut item.fields, &type_mark)?;
        self.full.push(item.to_token_stream());
        match type_mark.map(|marked| marked.mark) {
            Some(Mark::Bare) => {}
            Some(Mark::Into(rule)) => {
                remove_fields(&mut item.fields, &dropped);
                self.core.push(item.to_token_stream());
                self.impls
                    .push(self.whole_impl(&item.ident, &item.generics, &rule));
            }
            None => {
                self.core.push(item.to_token_stream());
                let Args { core, desugar, .. } = self.args;
                let name = &item.ident;
                let body = fields_desugar(desugar, &item.fields, |i, field| match &field.ident {
                    Some(name) => quote!(&self.#name),
                    None => {
                        let index = Index::from(i);
                        quote!(&self.#index)
                    }
                });
                let (header, core_type) = self.header(name, &item.generics);
                self.impls.push(quote! {
                    #header {
                        type Core = #core_type;
                        fn desugar(&self) -> Self::Core { super::#core::#name #body }
                    }
                });
            }
        }
        Ok(())
    }

    fn enumeration(&mut self, mut item: ItemEnum) -> syn::Result<()> {
        let type_mark = take_mark(&mut item.attrs)?;
        ensure_clone(&mut item.attrs);
        let mut core_variants = Punctuated::new();
        let mut arms = Vec::new();
        for variant in &mut item.variants {
            let mark = take_mark(&mut variant.attrs)?;
            let dropped = self.fields(&mut variant.fields, &type_mark)?;
            match (mark, &type_mark) {
                (None, _) => {
                    let mut core_variant = variant.clone();
                    remove_fields(&mut core_variant.fields, &dropped);
                    core_variants.push(core_variant);
                    if type_mark.is_none() {
                        arms.push(self.plain_arm(&item.ident, variant));
                    }
                }
                (
                    Some(Marked { span, .. }),
                    Some(Marked {
                        mark: Mark::Bare, ..
                    }),
                ) => return Err(Error::new(span, ABSENT)),
                (
                    Some(Marked {
                        mark: Mark::Bare,
                        span,
                    }),
                    None,
                ) => {
                    let message = "a sugar variant needs a rule: `#[sugar(into = EXPR)]` on the variant, or on the type";
                    return Err(Error::new(span, message));
                }
                (
                    Some(Marked {
                        mark: Mark::Bare, ..
                    }),
                    Some(_),
                ) => {}
                (
                    Some(Marked {
                        mark: Mark::Into(_),
                        span,
                    }),
                    Some(_),
                ) => {
                    let message = "unused rule: the type's own `into` converts the whole value";
                    return Err(Error::new(span, message));
                }
                (
                    Some(Marked {
                        mark: Mark::Into(rule),
                        ..
                    }),
                    None,
                ) => arms.push(self.rule_arm(variant, &rule)),
            }
        }
        self.full.push(item.to_token_stream());
        let (name, generics) = (item.ident.clone(), item.generics.clone());
        match type_mark.map(|marked| marked.mark) {
            Some(Mark::Bare) => {}
            Some(Mark::Into(rule)) => {
                item.variants = core_variants;
                self.core.push(item.to_token_stream());
                self.impls.push(self.whole_impl(&name, &generics, &rule));
            }
            None => {
                item.variants = core_variants;
                self.core.push(item.to_token_stream());
                let (header, core_type) = self.header(&name, &generics);
                let body = if arms.is_empty() {
                    quote!(match *self {})
                } else {
                    quote!(match self { #(#arms)* })
                };
                self.impls.push(quote! {
                    #header {
                        type Core = #core_type;
                        fn desugar(&self) -> Self::Core { #body }
                    }
                });
            }
        }
        Ok(())
    }

    /// Strips the field marks, returning the indices of the sugar fields.
    fn fields(
        &mut self,
        fields: &mut Fields,
        type_mark: &Option<Marked>,
    ) -> syn::Result<Vec<usize>> {
        let mut dropped = Vec::new();
        for (i, field) in fields.iter_mut().enumerate() {
            self.literals(&field.ty);
            match take_mark(&mut field.attrs)? {
                None => {}
                Some(Marked {
                    mark: Mark::Into(_),
                    span,
                }) => {
                    let message = "`into` goes on a variant or on the type, not on a field";
                    return Err(Error::new(span, message));
                }
                Some(Marked {
                    mark: Mark::Bare,
                    span,
                }) => match type_mark {
                    Some(Marked {
                        mark: Mark::Into(_),
                        ..
                    }) => dropped.push(i),
                    Some(_) => return Err(Error::new(span, ABSENT)),
                    None => {
                        let message =
                            "a sugar field needs a rule on the type: `#[sugar(into = EXPR)]`";
                        return Err(Error::new(span, message));
                    }
                },
            }
        }
        Ok(dropped)
    }

    fn plain_arm(&self, enum_name: &Ident, variant: &Variant) -> TokenStream2 {
        let Args { core, desugar, .. } = self.args;
        let name = &variant.ident;
        let (pattern, binds) = variant_pattern(variant);
        let body = fields_desugar(desugar, &variant.fields, |i, _| binds[i].to_token_stream());
        quote!(#pattern => super::#core::#enum_name::#name #body,)
    }

    fn rule_arm(&self, variant: &Variant, rule: &Expr) -> TokenStream2 {
        let (pattern, binds) = variant_pattern(variant);
        let types = variant.fields.iter().map(|field| &field.ty);
        quote! {
            #pattern => {
                let __rule: fn(#(&#types),*) -> Self::Core = #rule;
                __rule(#(#binds),*)
            },
        }
    }

    fn whole_impl(&self, name: &Ident, generics: &Generics, rule: &Expr) -> TokenStream2 {
        let (header, core_type) = self.header(name, generics);
        quote! {
            #header {
                type Core = #core_type;
                fn desugar(&self) -> Self::Core {
                    let __rule: fn(&Self) -> Self::Core = #rule;
                    __rule(self)
                }
            }
        }
    }

    /// `impl<…> Desugar for Name<…> where …`, and the core type `super::core::Name<T::Core, …>`.
    fn header(&self, name: &Ident, generics: &Generics) -> (TokenStream2, TokenStream2) {
        let Args { core, desugar, .. } = self.args;
        let mut generics = generics.clone();
        let params: Vec<Ident> = generics
            .type_params()
            .map(|param| param.ident.clone())
            .collect();
        let mut carried: Vec<WherePredicate> = generics
            .where_clause
            .iter()
            .flat_map(|clause| clause.predicates.iter().cloned())
            .collect();
        let mut core_args = Vec::new();
        for param in &mut generics.params {
            match param {
                GenericParam::Type(param) => {
                    param.eq_token = None;
                    param.default = None;
                    let ident = &param.ident;
                    let bounds: Vec<&TypeParamBound> = param
                        .bounds
                        .iter()
                        .filter(|bound| {
                            !matches!(bound, TypeParamBound::Trait(bound)
                                if matches!(bound.modifier, TraitBoundModifier::Maybe(_)))
                        })
                        .collect();
                    if !bounds.is_empty() {
                        carried.push(parse_quote!(#ident: #(#bounds)+*));
                    }
                    core_args.push(quote!(<#ident as #desugar>::Core));
                    param.bounds.push(parse_quote!(#desugar));
                }
                GenericParam::Lifetime(param) => core_args.push(param.lifetime.to_token_stream()),
                GenericParam::Const(param) => {
                    param.eq_token = None;
                    param.default = None;
                    core_args.push(param.ident.to_token_stream());
                }
            }
        }
        let mut subst = Subst {
            params: &params,
            desugar,
        };
        for mut predicate in carried {
            subst.visit_where_predicate_mut(&mut predicate);
            generics.make_where_clause().predicates.push(predicate);
        }
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let core_args = (!core_args.is_empty()).then(|| quote!(<#(#core_args),*>));
        (
            quote!(impl #impl_generics #desugar for #name #ty_generics #where_clause),
            quote!(super::#core::#name #core_args),
        )
    }

    /// Gives each literal in `ty` not seen before an identity impl.
    fn literals(&mut self, ty: &Type) {
        struct Scan<'a, 'b>(&'a mut Output<'b>);
        impl<'ast> Visit<'ast> for Scan<'_, '_> {
            fn visit_type_macro(&mut self, ty: &'ast TypeMacro) {
                self.0.literal(&ty.mac);
            }
        }
        Scan(self).visit_type(ty);
    }

    fn literal(&mut self, mac: &Macro) {
        let Some(name) = mac.path.segments.last() else {
            return;
        };
        let case_insensitive = match name.ident.to_string().as_str() {
            "StringEq" => false,
            "StringEqCI" => true,
            _ => return,
        };
        let Ok(text) = mac.parse_body::<LitStr>() else {
            return;
        };
        let text = text.value();
        // `StringEqCI!` wraps a char in its case variants only if it has any.
        let cased = case_insensitive
            && text.chars().any(|ch| {
                let ch = ch.to_string();
                ch.to_lowercase() != ch || ch.to_uppercase() != ch
            });
        if self.literals.insert((cased, text)) {
            let desugar = &self.args.desugar;
            self.impls.push(quote! {
                impl #desugar for #mac {
                    type Core = Self;
                    fn desugar(&self) -> Self { ::core::clone::Clone::clone(self) }
                }
            });
        }
    }
}
