//! Function components with retained, replayable inputs.
use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use quote::{format_ident, quote};
mod builder;
mod style_api;
mod utilities;

use syn::{FnArg, ItemFn, Pat, Type, parse_macro_input, parse_quote};

/// Defer a component body until mount, then replay it with cloned inputs on updates.
#[proc_macro_attribute]
pub fn component(args: TokenStream, input: TokenStream) -> TokenStream {
    let memo = args.to_string() == "memo";
    if !args.is_empty() && !memo {
        return syn::Error::new(
            proc_macro::Span::call_site().into(),
            "expected #[component] or #[component(memo)]",
        )
        .to_compile_error()
        .into();
    }
    let mut item = parse_macro_input!(input as ItemFn);
    if builder::requested(&item) {
        return builder::expand(item, memo)
            .unwrap_or_else(syn::Error::into_compile_error)
            .into();
    }
    match expand(&mut item, memo) {
        Ok(()) => quote!(#item).into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand(item: &mut ItemFn, memo: bool) -> syn::Result<()> {
    if item.sig.asyncness.is_some()
        || item.sig.constness.is_some()
        || item.sig.unsafety.is_some()
        || item.sig.abi.is_some()
        || item.sig.variadic.is_some()
    {
        return Err(syn::Error::new_spanned(
            &item.sig,
            "components must be ordinary synchronous functions",
        ));
    }
    let path =
        match crate_name("voidui").map_err(|error| syn::Error::new_spanned(&item.sig, error))? {
            FoundCrate::Itself => quote!(::voidui),
            FoundCrate::Name(name) => {
                let name = format_ident!("{}", name);
                quote!(::#name)
            }
        };
    let mut implementation = item.clone();
    implementation
        .attrs
        .retain(|attr| attr.path().is_ident("allow"));
    implementation.vis = syn::Visibility::Inherited;
    let helper = syn::Ident::new("__voidui_render", proc_macro::Span::mixed_site().into());
    implementation.sig.ident = helper.clone();
    if matches!(implementation.sig.output, syn::ReturnType::Default) {
        implementation.sig.output = parse_quote!(-> impl #path::core::element::IntoElement);
    }
    let generic_args: Vec<_> = item
        .sig
        .generics
        .params
        .iter()
        .filter_map(|param| match param {
            syn::GenericParam::Type(param) => {
                let name = &param.ident;
                Some(quote!(#name))
            }
            syn::GenericParam::Const(param) => {
                let name = &param.ident;
                Some(quote!(#name))
            }
            syn::GenericParam::Lifetime(_) => None,
        })
        .collect();
    let mut conversions = Vec::new();
    let mut names = Vec::new();
    let mut converted = Vec::new();
    for arg in &mut item.sig.inputs {
        let FnArg::Typed(arg) = arg else {
            return Err(syn::Error::new_spanned(
                arg,
                "components cannot have a self parameter",
            ));
        };
        let Pat::Ident(pattern) = &mut *arg.pat else {
            return Err(syn::Error::new_spanned(
                &arg.pat,
                "use a named component parameter",
            ));
        };
        if pattern.by_ref.is_some() || pattern.subpat.is_some() {
            return Err(syn::Error::new_spanned(
                pattern,
                "use a named component parameter",
            ));
        }
        let name = &pattern.ident;
        names.push(name.clone());
        pattern.mutability = None;
        match &mut *arg.ty {
            Type::ImplTrait(ty) => {
                ty.bounds.push(parse_quote!(::core::clone::Clone));
                ty.bounds.push(parse_quote!('static));
                if memo {
                    ty.bounds.push(parse_quote!(::core::cmp::PartialEq));
                }
            }
            ty => {
                if let Type::Reference(reference) = ty
                    && reference.lifetime.is_none()
                {
                    reference.lifetime = Some(parse_quote!('static));
                }
                // Retained descriptions cannot borrow a caller's stack frame.
                // Rc/Arc inputs make cloning large models independent of model size.
                item.sig
                    .generics
                    .make_where_clause()
                    .predicates
                    .push(parse_quote!(#ty: ::core::clone::Clone + 'static));
                if memo {
                    item.sig
                        .generics
                        .make_where_clause()
                        .predicates
                        .push(parse_quote!(#ty: ::core::cmp::PartialEq));
                }
            }
        }
        // Normalize explicit framework input types once at the call boundary.
        // The render body keeps its declared type; plain Rust parameters keep
        // ordinary value semantics, including their existing Clone requirement.
        let convert = if let Type::Path(ty) = &*arg.ty {
            ty.path.segments.last().is_some_and(|s| {
                matches!(
                    s.ident.to_string().as_str(),
                    "Read" | "List" | "Callback" | "AsyncCallback"
                )
            })
        } else {
            false
        };
        converted.push(convert);
        if convert {
            let ty = &arg.ty;
            conversions.push(quote!(let #name: #ty = ::core::convert::Into::into(#name);));
        }
    }
    // Keep added bounds in the same location as existing generic bounds so the
    // generated public signature does not introduce multiple-bound-location lints.
    let mut existing_bounds: Vec<syn::WherePredicate> = Vec::new();
    for parameter in &mut item.sig.generics.params {
        if let syn::GenericParam::Type(parameter) = parameter
            && !parameter.bounds.is_empty()
        {
            let name = &parameter.ident;
            let bounds = std::mem::take(&mut parameter.bounds);
            existing_bounds.push(parse_quote!(#name: #bounds));
        }
    }
    if !existing_bounds.is_empty() {
        item.sig
            .generics
            .make_where_clause()
            .predicates
            .extend(existing_bounds);
    }
    // The body uses the same retained-input contract as the public wrapper.
    // In particular, generic inputs must also be eligible for state<T: 'static>.
    implementation.sig.generics = item.sig.generics.clone();
    for (body, wrapper) in implementation.sig.inputs.iter_mut().zip(&item.sig.inputs) {
        if let (FnArg::Typed(body), FnArg::Typed(wrapper)) = (body, wrapper) {
            body.ty = wrapper.ty.clone();
        }
    }
    for (arg, convert) in item.sig.inputs.iter_mut().zip(converted) {
        if convert && let FnArg::Typed(arg) = arg {
            let ty = &arg.ty;
            *arg.ty = parse_quote!(impl ::core::convert::Into<#ty>);
        }
    }
    let marker = syn::Ident::new("__VoiduiComponent", proc_macro::Span::mixed_site().into());
    let constructor = if memo {
        quote!(with_memo_inputs)
    } else {
        quote!(with_inputs)
    };
    let clones = names
        .iter()
        .map(|name| quote!(::core::clone::Clone::clone(#name)));
    item.block = parse_quote!({
        #implementation
        struct #marker;
        #(#conversions)*
        #path::core::component::ComponentElement::#constructor::<#marker, _, _, _>(
            (#(#names,)*), move |(#(#names,)*)| {
                #path::core::element::IntoElement::into_element(#helper::<#(#generic_args),*>(#(#clones),*))
            }
        )
    });
    item.sig.output = parse_quote!(-> #path::core::component::ComponentElement);
    Ok(())
}

/// Generate voidui's utility methods and stylesheet catalog from shared scales.
#[doc(hidden)]
#[proc_macro]
pub fn utility_catalog(input: TokenStream) -> TokenStream {
    utilities::expand(parse_macro_input!(input as utilities::Catalog)).into()
}

/// Share canonical style setters with widgets and deferred component builders.
#[doc(hidden)]
#[proc_macro_attribute]
pub fn style_methods(args: TokenStream, input: TokenStream) -> TokenStream {
    style_api::expand(
        parse_macro_input!(args as syn::Ident),
        parse_macro_input!(input as syn::ItemImpl),
    )
    .unwrap_or_else(syn::Error::into_compile_error)
    .into()
}

/// Omit style methods shadowed by named component inputs.
#[doc(hidden)]
#[proc_macro]
pub fn filter_style_methods(input: TokenStream) -> TokenStream {
    parse_macro_input!(input as style_api::Filter)
        .expand()
        .into()
}
