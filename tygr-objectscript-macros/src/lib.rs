//! `#[levels]` on the inline module of the SQL grammar emits it twice: `full`, every spelling IRIS
//! accepts, and `core`, the same minus its syntactic sugar, with an impl of `Desugar` per unmarked
//! struct and enum and per literal, a keyword's core copy being its declared, upper-case spelling
//! and a `Spellings!(A, …)` or `Kw!("A", …)` type's its canonical `A`, and an impl of `Default` per struct and enum
//! that builds from defaults, a struct's fields or an enum's first variant, unless written by hand;
//! fns, traits, macros, impls of `Desugar` and `#[sugar]` items go to `full` alone.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, format_ident, quote};
use syn::punctuated::Punctuated;
use syn::visit::Visit;
use syn::visit_mut::VisitMut;
use syn::{
    Attribute, Error, Field, Fields, GenericArgument, Generics, Ident, Index, Item, ItemEnum,
    ItemImpl, ItemMod, ItemStruct, LitStr, Macro, Meta, Path, PathArguments, Token, Type,
    TypeMacro, Variant, WherePredicate, parse_quote,
};

/// `#[sugar(f)]` on a variant leaves it out of the core, `f` taking the variant's fields by value to the
/// enclosing core type; with `#[sugar]` fields inside, the core keeps the variant without them and `f`
/// takes those fields and the converted variant. `#[sugar]` on a field drops it; on a struct or enum,
/// both levels keep it and the author writes the conversion, its variants' marks then bare.
/// `Kw!("READ ONLY", "READONLY")`: a keyword, its words a blank apart and none the start of a longer
/// word, in its canonical spelling or others, a phrase one `Phrase` and the spellings one `Spellings`.
#[proc_macro]
#[allow(non_snake_case)]
pub fn Kw(input: TokenStream) -> TokenStream {
    let parser = Punctuated::<LitStr, Token![,]>::parse_terminated;
    let spellings = match syn::parse::Parser::parse(parser, input) {
        Ok(spellings) => spellings,
        Err(error) => return error.into_compile_error().into(),
    };
    let mut keywords = spellings.iter().map(keyword).rev();
    let last = keywords
        .next()
        .unwrap_or_else(|| quote!(::core::compile_error!("`Kw!` takes one spelling or more")));
    keywords
        .fold(
            last,
            |others, canonical| quote!(Spellings<#canonical, #others>),
        )
        .into()
}

/// `(StringEqCI!("READ"), NotFollowedBy<WordEnd>)`, a phrase's words a `Space1` apart in a `Phrase`.
fn keyword(spelling: &LitStr) -> TokenStream2 {
    let words: Vec<TokenStream2> = (spelling.value().split(' '))
        .map(|word| {
            let word = LitStr::new(word, spelling.span());
            quote!((StringEqCI!(#word), NotFollowedBy<WordEnd>))
        })
        .collect();
    match words.as_slice() {
        [word] => word.clone(),
        _ => {
            let parts = words.iter().enumerate().map(|(i, word)| match i {
                0 => word.clone(),
                _ => quote!(Space1, #word),
            });
            quote!(Phrase<(#(#parts),*)>)
        }
    }
}

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
    let (full_defaults, core_defaults) = (out.full.defaults(&out), out.core.defaults(&out));
    let Output {
        full, core, impls, ..
    } = out;
    let (full, core) = (full.items, core.items);
    Ok(quote! {
        #vis mod full {
            use super::core;
            use super::desugar::*;
            #(#full)* #(#impls)* #(#full_defaults)*
        }
        #vis mod core { #(#core)* #(#core_defaults)* }
    })
}

fn desugar() -> TokenStream2 {
    quote!(crate::sql::desugar::Desugar)
}

/// `Some(None)` for `#[sugar]`, `Some(Some(f))` for `#[sugar(f)]`.
fn take_mark(attrs: &mut Vec<Attribute>) -> syn::Result<Option<Option<Path>>> {
    let Some(i) = attrs.iter().position(|attr| attr.path().is_ident("sugar")) else {
        return Ok(None);
    };
    let attr = attrs.remove(i);
    match &attr.meta {
        Meta::Path(_) => Ok(Some(None)),
        Meta::List(list) => Ok(Some(Some(list.parse_args()?))),
        _ => Err(Error::new_spanned(
            attr,
            "expected `#[sugar]` or `#[sugar(f)]`",
        )),
    }
}

/// `#[sugar]` alone, where no conversion is named.
fn plain_mark(attrs: &mut Vec<Attribute>) -> syn::Result<bool> {
    match take_mark(attrs)? {
        Some(Some(path)) => Err(Error::new_spanned(path, "`#[sugar(f)]` goes on a variant")),
        mark => Ok(mark.is_some()),
    }
}

fn implements(item: &ItemImpl, name: &str) -> bool {
    (item.trait_.as_ref())
        .is_some_and(|(_, path, _)| path.segments.last().is_some_and(|s| s.ident == name))
}

fn spellings(mac: &Macro) -> Option<Punctuated<Type, Token![,]>> {
    (mac.path.is_ident("Spellings"))
        .then(|| mac.parse_body_with(Punctuated::parse_terminated).ok())
        .flatten()
}

/// Whether `ty` builds from defaults: a literal, a blank, a type parameter, a type of the level found
/// so, or tuples, options, vectors, boxes and tygr's wrappers of such.
fn defaultable(ty: &Type, known: &BTreeSet<String>, params: &BTreeSet<String>) -> bool {
    let each = |args: &PathArguments| match args {
        PathArguments::None => true,
        PathArguments::AngleBracketed(args) => args.args.iter().all(|arg| match arg {
            GenericArgument::Type(ty) => defaultable(ty, known, params),
            _ => true,
        }),
        PathArguments::Parenthesized(_) => false,
    };
    match ty {
        Type::Tuple(tuple) => (tuple.elems.iter()).all(|ty| defaultable(ty, known, params)),
        Type::Paren(paren) => defaultable(&paren.elem, known, params),
        Type::Macro(TypeMacro { mac }) => match spellings(mac) {
            Some(spellings) => (spellings.first()).is_some_and(|ty| defaultable(ty, known, params)),
            None => (mac.path.get_ident()).is_some_and(|name| {
                matches!(name.to_string().as_str(), "Kw" | "StringEq" | "StringEqCI")
            }),
        },
        Type::Path(path) => {
            let Some(last) = path.path.segments.last() else {
                return false;
            };
            match last.ident.to_string() {
                name if matches!(name.as_str(), "Option" | "Vec" | "NotFollowedBy") => true,
                name if matches!(
                    name.as_str(),
                    "Box" | "Raw" | "Hidden" | "Wrap" | "Prefix" | "Suffix"
                ) || params.contains(&name)
                    || known.contains(&name) =>
                {
                    each(&last.arguments)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// `Spellings!(A, …)` in `ty` is `A` and `Kw!("A", …)` is `Kw!("A")`, the canonical spelling, in the core.
fn canonical(ty: &mut Type) {
    struct Canonical;
    impl VisitMut for Canonical {
        fn visit_type_mut(&mut self, ty: &mut Type) {
            if let Type::Macro(TypeMacro { mac }) = ty {
                if let Some(first) = spellings(mac).and_then(|s| s.into_iter().next()) {
                    *ty = first;
                } else if let Some(first) =
                    keyword_spellings(mac).and_then(|s| s.into_iter().next())
                {
                    *ty = parse_quote!(Kw!(#first));
                }
            }
            syn::visit_mut::visit_type_mut(self, ty);
        }
    }
    Canonical.visit_type_mut(ty);
}

/// The spellings of a `Kw!("A", "B", …)` with more than one.
fn keyword_spellings(mac: &Macro) -> Option<Punctuated<LitStr, Token![,]>> {
    (mac.path.is_ident("Kw"))
        .then(|| mac.parse_body_with(Punctuated::parse_terminated).ok())
        .flatten()
        .filter(|spellings| spellings.len() > 1)
}

/// The fields kept in the core, each type in its canonical spelling.
fn core_fields(fields: &mut Fields, dropped: &[usize]) {
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
    for field in fields.iter_mut() {
        canonical(&mut field.ty);
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

/// `Self::V(a, n, b) => f(n, core::E::V(a, b))`, `n` the dropped field, for a variant the core keeps.
fn hook_arm(f: &Path, enum_name: &Ident, variant: &Variant, dropped: &[usize]) -> TokenStream2 {
    let name = &variant.ident;
    let (pattern, binds) = variant_pattern(variant, &[]);
    let body = fields_desugar(&variant.fields, dropped, |i, _| binds[i].to_token_stream());
    let passed = dropped.iter().map(|&i| &binds[i]);
    quote!(#pattern => #f(#(#passed,)* super::core::#enum_name::#name #body),)
}

/// `Self::V(a, b) => f(a, b)`.
fn sugar_arm(f: &Path, variant: &Variant) -> TokenStream2 {
    let (pattern, binds) = variant_pattern(variant, &[]);
    quote!(#pattern => #f(#(#binds),*),)
}

/// `impl Clone` written out, as the derive refuses a generic item with a `Kw!` field.
fn clone_impl(name: &Ident, generics: &Generics, body: TokenStream2) -> TokenStream2 {
    let mut generics = generics.clone();
    for param in generics.type_params_mut() {
        param.bounds.push(parse_quote!(::core::clone::Clone));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    quote! {
        impl #impl_generics ::core::clone::Clone for #name #ty_generics #where_clause {
            fn clone(&self) -> Self { #body }
        }
    }
}

/// `Path { f: f.clone(), … }`, `Path(a.clone(), …)` or `Path`, each field's value by `access`.
fn clone_fields(
    path: TokenStream2,
    fields: &Fields,
    access: impl Fn(usize, &Field) -> TokenStream2,
) -> TokenStream2 {
    let clone = quote!(::core::clone::Clone::clone);
    let values = fields.iter().enumerate().map(|(i, field)| {
        let value = access(i, field);
        match &field.ident {
            Some(name) => quote!(#name: #clone(#value)),
            None => quote!(#clone(#value)),
        }
    });
    match fields {
        Fields::Named(_) => quote!(#path { #(#values),* }),
        Fields::Unnamed(_) => quote!(#path(#(#values),*)),
        Fields::Unit => path,
    }
}

fn clone_struct(item: &ItemStruct) -> TokenStream2 {
    let body = clone_fields(quote!(Self), &item.fields, |i, field| match &field.ident {
        Some(name) => quote!(&self.#name),
        None => {
            let index = Index::from(i);
            quote!(&self.#index)
        }
    });
    clone_impl(&item.ident, &item.generics, body)
}

fn clone_enum(item: &ItemEnum) -> TokenStream2 {
    let arms = item.variants.iter().map(|variant| {
        let name = &variant.ident;
        let binds: Vec<Ident> = (0..variant.fields.len())
            .map(|i| format_ident!("f{i}"))
            .collect();
        let pattern = match &variant.fields {
            Fields::Named(fields) => {
                let fields = fields.named.iter().zip(&binds).map(|(field, bind)| {
                    let name = &field.ident;
                    quote!(#name: #bind)
                });
                quote!(Self::#name { #(#fields),* })
            }
            Fields::Unnamed(_) => quote!(Self::#name(#(#binds),*)),
            Fields::Unit => quote!(Self::#name),
        };
        let value = clone_fields(quote!(Self::#name), &variant.fields, |i, _| {
            binds[i].to_token_stream()
        });
        quote!(#pattern => #value,)
    });
    clone_impl(
        &item.ident,
        &item.generics,
        quote!(match self { #(#arms)* }),
    )
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
            fn desugar(self) -> Self::Core { #body }
        }
    }
}

/// What a struct's or an enum's `Default` builds: its fields, or its first variant's.
struct Shape {
    name: Ident,
    generics: Generics,
    variant: Option<Ident>,
    fields: Fields,
}

#[derive(Default)]
struct Level {
    items: Vec<TokenStream2>,
    shapes: Vec<Shape>,
    aliases: Vec<(Ident, Generics, Type)>,
}

impl Level {
    fn shape(&mut self, item: &ItemStruct) {
        self.shapes.push(Shape {
            name: item.ident.clone(),
            generics: item.generics.clone(),
            variant: None,
            fields: item.fields.clone(),
        });
    }

    fn first_variant(&mut self, item: &ItemEnum) {
        if let Some(variant) = item.variants.first() {
            self.shapes.push(Shape {
                name: item.ident.clone(),
                generics: item.generics.clone(),
                variant: Some(variant.ident.clone()),
                fields: variant.fields.clone(),
            });
        }
    }

    /// `impl Default` for each shape that builds from defaults, found as a least fixpoint over the
    /// level's shapes and aliases, the hand-written impls given.
    fn defaults(&self, out: &Output) -> Vec<TokenStream2> {
        let params = |generics: &Generics| -> BTreeSet<String> {
            (generics.type_params())
                .map(|param| param.ident.to_string())
                .collect()
        };
        let mut known: BTreeSet<String> = out.defaults.clone();
        known.extend(["Space", "Space1"].map(String::from));
        loop {
            let before = known.len();
            for shape in &self.shapes {
                let params = params(&shape.generics);
                if (shape.fields.iter()).all(|field| defaultable(&field.ty, &known, &params)) {
                    known.insert(shape.name.to_string());
                }
            }
            for (name, generics, ty) in &self.aliases {
                if defaultable(ty, &known, &params(generics)) {
                    known.insert(name.to_string());
                }
            }
            if known.len() == before {
                break;
            }
        }
        let derived = (self.shapes.iter())
            .filter(|shape| known.contains(&shape.name.to_string()))
            .filter(|shape| !out.defaults.contains(&shape.name.to_string()));
        derived.map(Shape::default_impl).collect()
    }
}

impl Shape {
    fn default_impl(&self) -> TokenStream2 {
        let default = quote!(::core::default::Default::default());
        let fields = self.fields.iter().map(|field| match &field.ident {
            Some(name) => quote!(#name: #default),
            None => default.clone(),
        });
        let fields = match &self.fields {
            Fields::Named(_) => quote!({ #(#fields,)* }),
            Fields::Unnamed(_) => quote!(( #(#fields,)* )),
            Fields::Unit => quote!(),
        };
        let variant = self.variant.iter();
        let mut generics = self.generics.clone();
        for param in generics.type_params_mut() {
            param.eq_token = None;
            param.default = None;
            param.bounds.push(parse_quote!(::core::default::Default));
        }
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let name = &self.name;
        quote! {
            impl #impl_generics ::core::default::Default for #name #ty_generics #where_clause {
                fn default() -> Self { Self #(::#variant)* #fields }
            }
        }
    }
}

#[derive(Default)]
struct Output {
    full: Level,
    core: Level,
    impls: Vec<TokenStream2>,
    /// The literals given an impl, as `(has a cased char, text)`.
    literals: BTreeSet<(bool, String)>,
    /// The types whose `Default` the author writes.
    defaults: BTreeSet<String>,
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
            Item::Impl(item) => implements(item, "Desugar"),
            _ => false,
        };
        let attrs = match &mut item {
            Item::Type(alias) => {
                self.literals(&alias.ty);
                Some(&mut alias.attrs)
            }
            Item::Impl(item) => {
                if implements(item, "Default")
                    && let Type::Path(path) = &*item.self_ty
                    && let Some(name) = path.path.segments.last()
                {
                    self.defaults.insert(name.ident.to_string());
                }
                Some(&mut item.attrs)
            }
            Item::Use(item) => Some(&mut item.attrs),
            _ => None,
        };
        let marked = attrs.map_or(Ok(false), plain_mark)?;
        self.full.items.push(item.to_token_stream());
        if let Item::Type(alias) = &mut item {
            let (name, generics) = (alias.ident.clone(), alias.generics.clone());
            self.full
                .aliases
                .push((name.clone(), generics.clone(), (*alias.ty).clone()));
            canonical(&mut alias.ty);
            self.core
                .aliases
                .push((name, generics, (*alias.ty).clone()));
        }
        if !marked && !full_only {
            self.core.items.push(item.to_token_stream());
        }
        Ok(())
    }

    fn structure(&mut self, mut item: ItemStruct) -> syn::Result<()> {
        let marked = plain_mark(&mut item.attrs)?;
        let dropped = self.fields(&mut item.fields)?;
        self.full.items.push(item.to_token_stream());
        self.full.items.push(clone_struct(&item));
        self.full.shape(&item);
        let body = fields_desugar(&item.fields, &dropped, |i, field| match &field.ident {
            Some(name) => quote!(self.#name),
            None => {
                let index = Index::from(i);
                quote!(self.#index)
            }
        });
        core_fields(&mut item.fields, &dropped);
        self.core.items.push(item.to_token_stream());
        self.core.items.push(clone_struct(&item));
        self.core.shape(&item);
        if !marked {
            let (name, generics) = (&item.ident, &item.generics);
            let body = quote!(super::core::#name #body);
            self.impls.push(impl_of(name, generics, body));
        }
        Ok(())
    }

    fn enumeration(&mut self, mut item: ItemEnum) -> syn::Result<()> {
        let marked = plain_mark(&mut item.attrs)?;
        let mut core_variants = Punctuated::new();
        let mut arms = Vec::new();
        for variant in &mut item.variants {
            let mark = take_mark(&mut variant.attrs)?;
            let dropped = self.fields(&mut variant.fields)?;
            match mark {
                Some(Some(f)) if !dropped.is_empty() => {
                    let mut core_variant = variant.clone();
                    core_fields(&mut core_variant.fields, &dropped);
                    core_variants.push(core_variant);
                    arms.push(hook_arm(&f, &item.ident, variant, &dropped));
                }
                Some(Some(f)) => arms.push(sugar_arm(&f, variant)),
                Some(None) if marked => {}
                Some(None) => {
                    let message = "a sugar variant names its conversion, `#[sugar(f)]`";
                    return Err(Error::new_spanned(&variant.ident, message));
                }
                None => {
                    let mut core_variant = variant.clone();
                    core_fields(&mut core_variant.fields, &dropped);
                    core_variants.push(core_variant);
                    arms.push(plain_arm(&item.ident, variant, &dropped));
                }
            }
        }
        self.full.items.push(item.to_token_stream());
        self.full.items.push(clone_enum(&item));
        self.full.first_variant(&item);
        item.variants = core_variants;
        self.core.items.push(item.to_token_stream());
        self.core.items.push(clone_enum(&item));
        self.core.first_variant(&item);
        if marked {
            return Ok(());
        }
        let body = quote!(match self { #(#arms)* });
        self.impls.push(impl_of(&item.ident, &item.generics, body));
        Ok(())
    }

    /// Strips the field marks, returning the indices of the sugar fields.
    fn fields(&mut self, fields: &mut Fields) -> syn::Result<Vec<usize>> {
        let mut dropped = Vec::new();
        for (i, field) in fields.iter_mut().enumerate() {
            self.literals(&field.ty);
            if plain_mark(&mut field.attrs)? {
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
        if let Some(spellings) = spellings(mac) {
            for ty in &spellings {
                self.literals(ty);
            }
            return;
        }
        let Some(name) = mac.path.segments.last() else {
            return;
        };
        let case_insensitive = match name.ident.to_string().as_str() {
            "StringEq" => false,
            "StringEqCI" | "Kw" => true,
            _ => return,
        };
        let Ok(words) = mac.parse_body_with(Punctuated::<LitStr, Token![,]>::parse_terminated)
        else {
            return;
        };
        for word in words.iter().flat_map(|phrase| {
            (phrase
                .value()
                .split(' ')
                .map(|word| LitStr::new(word, phrase.span())))
            .collect::<Vec<_>>()
        }) {
            let text = word.value();
            // `StringEqCI!` of a text without cased chars is the `StringEq!` type.
            let cased = case_insensitive
                && text.chars().any(|ch| {
                    let ch = ch.to_string();
                    ch.to_lowercase() != ch || ch.to_uppercase() != ch
                });
            if self.literals.insert((cased, text)) {
                let (desugar, path) = (desugar(), &mac.path);
                self.impls.push(quote! {
                    impl #desugar for #path!(#word) {
                        type Core = Self;
                        fn desugar(self) -> Self { ::core::default::Default::default() }
                    }
                });
            }
        }
    }
}
