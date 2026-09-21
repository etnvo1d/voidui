//! Expand one utility catalog into native methods and CSS metadata.
//! This macro only expands names and tables; the host CSS engine owns semantics.
use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Ident, LitStr, Token, braced, bracketed,
    parse::{Parse, ParseStream},
};

mod docs;

type Values = Vec<(String, String)>;

struct Utility {
    method: Option<Ident>,
    class: String,
    properties: Values,
}

pub struct Catalog(Vec<Utility>);

fn pairs(input: ParseStream<'_>) -> syn::Result<Values> {
    let body;
    braced!(body in input);
    let mut values = Vec::new();
    while !body.is_empty() {
        let key: LitStr = body.parse()?;
        body.parse::<Token![=>]>()?;
        let value: LitStr = body.parse()?;
        body.parse::<Token![;]>()?;
        values.push((key.value(), value.value()));
    }
    Ok(values)
}

impl Parse for Catalog {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut scales = BTreeMap::<String, Values>::new();
        let mut utilities = Vec::new();
        let mut classes = BTreeSet::new();
        let mut methods = BTreeSet::new();
        while !input.is_empty() {
            let kind: Ident = input.parse()?;
            match kind.to_string().as_str() {
                "scale" => {
                    let name: Ident = input.parse()?;
                    if scales.insert(name.to_string(), pairs(input)?).is_some() {
                        return Err(syn::Error::new(name.span(), "duplicate scale"));
                    }
                }
                "family" => {
                    let prefix: LitStr = input.parse()?;
                    let scale: Ident = input.parse()?;
                    let properties;
                    bracketed!(properties in input);
                    let properties =
                        properties.parse_terminated(|p| p.parse::<LitStr>(), Token![,])?;
                    input.parse::<Token![;]>()?;
                    let values = scales.get(&scale.to_string()).ok_or_else(|| {
                        syn::Error::new(scale.span(), "define the scale before its families")
                    })?;
                    for (suffix, value) in values {
                        let class = format!("{}-{suffix}", prefix.value());
                        let method = class.replace(['-', '/'], "_").replace('.', "p");
                        let method = syn::parse_str::<Ident>(&method).map_err(|_| {
                            syn::Error::new(prefix.span(), "invalid generated method name")
                        })?;
                        utilities.push(Utility {
                            method: Some(method),
                            class,
                            properties: properties
                                .iter()
                                .map(|p| (p.value(), value.clone()))
                                .collect(),
                        });
                    }
                }
                "utility" | "existing" => {
                    let method: Ident = input.parse()?;
                    let class: LitStr = input.parse()?;
                    utilities.push(Utility {
                        method: (kind == "utility").then_some(method),
                        class: class.value(),
                        properties: pairs(input)?,
                    });
                }
                _ => {
                    return Err(syn::Error::new(
                        kind.span(),
                        "expected scale, family, utility, or existing",
                    ));
                }
            }
        }
        for utility in &utilities {
            if !classes.insert(&utility.class) {
                return Err(input.error(format!("duplicate utility class: {}", utility.class)));
            }
            if let Some(method) = &utility.method {
                if !methods.insert(method.to_string()) {
                    return Err(syn::Error::new(
                        method.span(),
                        format!("duplicate utility method: {method}"),
                    ));
                }
            }
        }
        Ok(Self(utilities))
    }
}

pub fn expand(catalog: Catalog) -> TokenStream {
    let entries = catalog.0.iter().map(|utility| {
        let class = &utility.class;
        let properties = utility
            .properties
            .iter()
            .map(|(name, value)| quote!((#name, #value)));
        quote!(Utility { class: #class, properties: &[#(#properties),*] })
    });
    let methods = catalog.0.iter().enumerate().filter_map(|(index, utility)| {
        let method = utility.method.as_ref()?;
        let doc = docs::method(utility);
        Some(quote! {
            #[doc = #doc]
            pub fn #method(self) -> Self {
                apply_utility(self.style, #index);
                self
            }
        })
    });
    // A generated test exercises every generated method against its catalog
    // entry, catching index drift without hand-maintaining thousands of calls.
    // Function pointers keep each builder in its own small debug stack frame.
    let checks = catalog.0.iter().enumerate().filter_map(|(index, utility)| {
        let method = utility.method.as_ref()?;
        Some(quote! { (#index, || crate::IntoElement::into_element(crate::div().#method())) })
    });
    quote! {
        pub(super) const UTILITIES: &[Utility] = &[#(#entries),*];
        #[voidui_macros::style_methods(__voidui_tailwind_styles)]
        impl<'a> crate::style::builder::StyleBuilder<'a> { #(#methods)* }
        #[cfg(test)]
        #[test]
        fn generated_methods_match_catalog() {
            let checks: &[(usize, fn() -> crate::Element)] = &[#(#checks),*];
            for &(index, build) in checks {
                let mut expected = crate::style::style::Style::default();
                apply_utility(&mut expected, index);
                assert_eq!(build().props.style, expected, "{}", UTILITIES[index].class);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_duplicate_and_unknown_entries() {
        for source in [
            r#"scale s { "1" => "1px"; } family "w" s ["width"]; family "w" s ["width"];"#,
            r#"family "w" missing ["width"];"#,
            r#"utility a "a" {} utility a "b" {}"#,
        ] {
            assert!(syn::parse_str::<Catalog>(source).is_err());
        }
    }
}
