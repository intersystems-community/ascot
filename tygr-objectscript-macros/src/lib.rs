//! `#[levels]` on the inline module of the SQL grammar emits it twice: `full`, every spelling IRIS
//! accepts, and `core`, the same minus its syntactic sugar, with an impl of `Desugar` per unmarked
//! struct and enum; fns, traits, macros, impls of `Desugar` and `#[sugar]` items go to `full` alone.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, format_ident, quote};
use syn::punctuated::Punctuated;
use syn::visit::Visit;
use syn::{
    Attribute, Error, Field, Fields, Generics, Ident, Index, Item, ItemEnum, ItemImpl, ItemMod,
    ItemStruct, LitStr, Macro, Meta, Token, Type, TypeMacro, Variant, WherePredicate, parse_quote,
};

/// `#[sugar]` on a variant leaves it out of the core, converted by the author's `impl Desugar` for
/// `XSugar<'a>`, the emitted enum of the marked variants holding their fields by reference; on a
/// field, drops it; on a struct or enum, both levels keep it and the author writes the conversion.
#[proc_macro_attribute]
pub fn levels(_: TokenStream, item: TokenStream) -> TokenStream {
    let module = syn::parse_macro_input!(item as ItemMod);
    expand(module)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn expand(module: ItemMod) -> syn::Result<TokenStream2> {
    let Some((_, items)) = module.content else {
        let message = "`#[levels]` goes on an inline module, `mod name { … }`";
        return Err(Error::new(module.mod_token.span, message));
    };
    let mut out = Output::default();
    for item in items {
        out.item(item)?;
    }
    let vis = &module.vis;
    let Output {
        full, core, impls, ..
    } = out;
    Ok(quote! {
        #vis mod full {
            use super::core;
            use super::desugar::*;
            #(#full)* #(#impls)*
        }
        #vis mod core { #(#core)* }
    })
}

fn desugar() -> TokenStream2 {
    quote!(crate::sql::desugar::Desugar)
}

fn take_mark(attrs: &mut Vec<Attribute>) -> syn::Result<bool> {
    let Some(i) = attrs.iter().position(|attr| attr.path().is_ident("sugar")) else {
        return Ok(false);
    };
    let attr = attrs.remove(i);
    match attr.meta {
        Meta::Path(_) => Ok(true),
        _ => Err(Error::new_spanned(attr, "expected `#[sugar]`")),
    }
}

fn is_desugar_impl(item: &ItemImpl) -> bool {
    (item.trait_.as_ref())
        .is_some_and(|(_, path, _)| path.segments.last().is_some_and(|s| s.ident == "Desugar"))
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

/// `{ f: desugar(value), … }`, `(desugar(value), …)` or nothing over the fields kept, `value` the
/// full field.
fn fields_desugar(
    fields: &Fields,
    dropped: &[usize],
    value: impl Fn(usize, &Field) -> TokenStream2,
) -> TokenStream2 {
    let desugar = desugar();
    let kept = (fields.iter().enumerate()).filter(|(i, _)| !dropped.contains(i));
    let parts = kept.map(|(i, field)| {
        let value = value(i, field);
        match &field.ident {
            Some(name) => quote!(#name: #desugar::desugar(#value)),
            None => quote!(#desugar::desugar(#value)),
        }
    });
    match fields {
        Fields::Named(_) => quote!({ #(#parts,)* }),
        Fields::Unnamed(_) => quote!(( #(#parts,)* )),
        Fields::Unit => quote!(),
    }
}

/// `Self::Name { a, b: _ }` or `Self::Name(__f0, _)`, a `_` for a dropped field, and the names bound.
fn variant_pattern(variant: &Variant, dropped: &[usize]) -> (TokenStream2, Vec<Ident>) {
    let name = &variant.ident;
    let binds: Vec<Ident> = (variant.fields.iter().enumerate())
        .map(|(i, field)| {
            field
                .ident
                .clone()
                .unwrap_or_else(|| format_ident!("__f{i}"))
        })
        .collect();
    let fields =
        binds
            .iter()
            .enumerate()
            .map(|(i, bind)| match (dropped.contains(&i), &variant.fields) {
                (true, Fields::Named(_)) => quote!(#bind: _),
                (true, _) => quote!(_),
                (false, _) => quote!(#bind),
            });
    let pattern = match &variant.fields {
        Fields::Named(_) => quote!(Self::#name { #(#fields),* }),
        Fields::Unnamed(_) => quote!(Self::#name(#(#fields),*)),
        Fields::Unit => quote!(Self::#name),
    };
    (pattern, binds)
}

fn plain_arm(enum_name: &Ident, variant: &Variant, dropped: &[usize]) -> TokenStream2 {
    let name = &variant.ident;
    let (pattern, binds) = variant_pattern(variant, dropped);
    let body = fields_desugar(&variant.fields, dropped, |i, _| binds[i].to_token_stream());
    quote!(#pattern => super::core::#enum_name::#name #body,)
}

/// `Self::V(a, b) => XSugar::V(a, b).desugar()`, the fields of the marked variant by reference.
fn sugar_arm(sugar: &Ident, variant: &Variant) -> TokenStream2 {
    let desugar = desugar();
    let name = &variant.ident;
    let (pattern, binds) = variant_pattern(variant, &[]);
    let value = match &variant.fields {
        Fields::Named(_) => quote!(#sugar::#name { #(#binds),* }),
        _ => quote!(#sugar::#name(#(#binds),*)),
    };
    quote!(#pattern => #desugar::desugar(&#value),)
}

/// The marked variant with every field a reference, for the `XSugar<'a>` enum.
fn sugar_variant(variant: &Variant) -> Variant {
    let mut variant = variant.clone();
    variant.attrs.clear();
    for field in variant.fields.iter_mut() {
        let ty = &field.ty;
        field.ty = parse_quote!(&'a #ty);
    }
    variant
}

/// `impl<T: Desugar> Desugar for Name<T>`, `T`'s bounds carried to `T::Core`, converting by `body`.
fn impl_of(name: &Ident, generics: &Generics, body: TokenStream2) -> TokenStream2 {
    let desugar = desugar();
    let mut generics = generics.clone();
    let (mut core_args, mut carried): (Vec<TokenStream2>, Vec<WherePredicate>) = (vec![], vec![]);
    for param in generics.type_params_mut() {
        param.eq_token = None;
        param.default = None;
        let (ident, bounds) = (&param.ident, &param.bounds);
        if !bounds.is_empty() {
            carried.push(parse_quote!(<#ident as #desugar>::Core: #bounds));
        }
        core_args.push(quote!(<#ident as #desugar>::Core));
        param.bounds.push(parse_quote!(#desugar));
    }
    generics.make_where_clause().predicates.extend(carried);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let core_args = (!core_args.is_empty()).then(|| quote!(<#(#core_args),*>));
    quote! {
        impl #impl_generics #desugar for #name #ty_generics #where_clause {
            type Core = super::core::#name #core_args;
            fn desugar(&self) -> Self::Core { #body }
        }
    }
}

#[derive(Default)]
struct Output {
    full: Vec<TokenStream2>,
    core: Vec<TokenStream2>,
    impls: Vec<TokenStream2>,
    /// The literals given an impl, as `(has a cased char, text)`.
    literals: BTreeSet<(bool, String)>,
}

impl Output {
    fn item(&mut self, item: Item) -> syn::Result<()> {
        let mut item = match item {
            Item::Struct(item) => return self.structure(item),
            Item::Enum(item) => return self.enumeration(item),
            item => item,
        };
        let full_only = match &item {
            Item::Fn(_) | Item::Const(_) | Item::Static(_) | Item::Trait(_) | Item::Macro(_) => {
                true
            }
            Item::Impl(item) => is_desugar_impl(item),
            _ => false,
        };
        let attrs = match &mut item {
            Item::Type(alias) => {
                self.literals(&alias.ty);
                Some(&mut alias.attrs)
            }
            Item::Impl(item) => Some(&mut item.attrs),
            Item::Use(item) => Some(&mut item.attrs),
            _ => None,
        };
        let marked = attrs.map_or(Ok(false), take_mark)?;
        self.full.push(item.to_token_stream());
        if !marked && !full_only {
            self.core.push(item.to_token_stream());
        }
        Ok(())
    }

    fn structure(&mut self, mut item: ItemStruct) -> syn::Result<()> {
        let marked = take_mark(&mut item.attrs)?;
        item.attrs
            .push(parse_quote!(#[derive(::core::clone::Clone)]));
        let dropped = self.fields(&mut item.fields)?;
        self.full.push(item.to_token_stream());
        let body = fields_desugar(&item.fields, &dropped, |i, field| match &field.ident {
            Some(name) => quote!(&self.#name),
            None => {
                let index = Index::from(i);
                quote!(&self.#index)
            }
        });
        remove_fields(&mut item.fields, &dropped);
        self.core.push(item.to_token_stream());
        if !marked {
            let (name, generics) = (&item.ident, &item.generics);
            let body = quote!(super::core::#name #body);
            self.impls.push(impl_of(name, generics, body));
        }
        Ok(())
    }

    fn enumeration(&mut self, mut item: ItemEnum) -> syn::Result<()> {
        let marked = take_mark(&mut item.attrs)?;
        item.attrs
            .push(parse_quote!(#[derive(::core::clone::Clone)]));
        let sugar = format_ident!("{}Sugar", item.ident);
        let mut core_variants = Punctuated::new();
        let mut sugar_variants: Punctuated<Variant, Token![,]> = Punctuated::new();
        let mut arms = Vec::new();
        for variant in &mut item.variants {
            let variant_marked = take_mark(&mut variant.attrs)?;
            let dropped = self.fields(&mut variant.fields)?;
            if variant_marked {
                sugar_variants.push(sugar_variant(variant));
                arms.push(sugar_arm(&sugar, variant));
            } else {
                let mut core_variant = variant.clone();
                remove_fields(&mut core_variant.fields, &dropped);
                core_variants.push(core_variant);
                arms.push(plain_arm(&item.ident, variant, &dropped));
            }
        }
        self.full.push(item.to_token_stream());
        item.variants = core_variants;
        self.core.push(item.to_token_stream());
        if marked {
            return Ok(());
        }
        let (name, generics, vis) = (&item.ident, &item.generics, &item.vis);
        if !sugar_variants.is_empty() {
            let mut generics = generics.clone();
            for param in generics.type_params_mut() {
                param.eq_token = None;
                param.default = None;
            }
            let params = generics.params.iter();
            self.full.push(quote! {
                #vis enum #sugar<'a, #(#params),*> { #sugar_variants }
            });
        }
        let body = quote!(match self { #(#arms)* });
        self.impls.push(impl_of(name, generics, body));
        Ok(())
    }

    /// Strips the field marks, returning the indices of the sugar fields.
    fn fields(&mut self, fields: &mut Fields) -> syn::Result<Vec<usize>> {
        let mut dropped = Vec::new();
        for (i, field) in fields.iter_mut().enumerate() {
            self.literals(&field.ty);
            if take_mark(&mut field.attrs)? {
                dropped.push(i);
            }
        }
        Ok(dropped)
    }

    /// Gives each literal in `ty` not seen before an identity impl.
    fn literals(&mut self, ty: &Type) {
        struct Scan<'a>(&'a mut Output);
        impl<'ast> Visit<'ast> for Scan<'_> {
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
            "StringEqCI" | "Kw" => true,
            _ => return,
        };
        let Ok(text) = mac.parse_body::<LitStr>() else {
            return;
        };
        let text = text.value();
        // `StringEqCI!` of a text without cased chars is the `StringEq!` type.
        let cased = case_insensitive
            && text.chars().any(|ch| {
                let ch = ch.to_string();
                ch.to_lowercase() != ch || ch.to_uppercase() != ch
            });
        if self.literals.insert((cased, text)) {
            let desugar = desugar();
            self.impls.push(quote! {
                impl #desugar for #mac {
                    type Core = Self;
                    fn desugar(&self) -> Self { ::core::clone::Clone::clone(self) }
                }
            });
        }
    }
}
