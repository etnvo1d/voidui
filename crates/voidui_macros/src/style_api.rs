//! Forward the canonical style setters without copying their implementation or
//! requiring callers to import a trait. Named component inputs can shadow setters.
use proc_macro2::{Group, TokenStream, TokenTree};
use quote::quote;
use std::collections::BTreeSet;
use syn::{
    Ident, ImplItem, ItemImpl, Pat, Path, Token, braced, bracketed,
    parse::{Parse, ParseStream},
    parse_quote,
    visit_mut::{self, VisitMut},
};

/// Signatures use the canonical builder's public type reexports. Qualifying them
/// here keeps generated component builders usable outside voidui and with renamed
/// Cargo dependencies, without leaking implementation imports into caller scope.
struct Qualify;
impl VisitMut for Qualify {
    fn visit_path_mut(&mut self, path: &mut Path) {
        visit_mut::visit_path_mut(self, path);
        let Some(first) = path.segments.first() else {
            return;
        };
        let name = first.ident.to_string();
        let prefix: Option<Path> = match name.as_str() {
            "crate" | "Self" => None,
            "std" | "core" => {
                path.leading_colon = Some(Default::default());
                None
            }
            "super" => Some(parse_quote!(crate::style)),
            "taffy" => Some(parse_quote!(crate::style::builder::taffy)),
            "voidui_gpui_wgpu" => Some(parse_quote!(crate::render)),
            "Option" => Some(parse_quote!(::core::option)),
            "Into" => Some(parse_quote!(::core::convert)),
            "Vec" => Some(parse_quote!(::std::vec)),
            "String" => Some(parse_quote!(::std::string)),
            "bool" | "f32" | "f64" | "usize" | "i32" | "u32" | "str" => None,
            _ => Some(parse_quote!(crate::style::builder)),
        };
        if let Some(mut prefix) = prefix {
            let skip = usize::from(matches!(
                name.as_str(),
                "super" | "taffy" | "voidui_gpui_wgpu"
            ));
            prefix
                .segments
                .extend(path.segments.iter().skip(skip).cloned());
            *path = prefix;
        }
    }
}

// $crate is a macro hygiene token, not a Rust path accepted by syn. Introduce it
// only after signature analysis, and retain each group's span for diagnostics.
fn macro_paths(tokens: TokenStream) -> TokenStream {
    tokens
        .into_iter()
        .flat_map(|token| match token {
            TokenTree::Ident(ref ident) if ident == "crate" => quote!($crate),
            TokenTree::Group(group) => {
                let mut next = Group::new(group.delimiter(), macro_paths(group.stream()));
                next.set_span(group.span());
                TokenStream::from(TokenTree::Group(next))
            }
            token => TokenStream::from(token),
        })
        .collect()
}

pub fn expand(name: Ident, mut implementation: ItemImpl) -> syn::Result<TokenStream> {
    let mut methods = Vec::new();
    for item in &mut implementation.items {
        let ImplItem::Fn(method) = item else {
            return Err(syn::Error::new_spanned(
                item,
                "expand setter macros before generating the shared style API",
            ));
        };
        // Export the tiny borrowed-editor bodies for cross-crate inlining so
        // sharing the API does not add a function-call layer to every setter.
        if !method
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("inline"))
        {
            method.attrs.push(parse_quote!(#[inline]));
        }
        let mut signature = method.sig.clone();
        Qualify.visit_signature_mut(&mut signature);
        if let Some(syn::FnArg::Receiver(receiver)) = signature.inputs.first_mut() {
            receiver.mutability = Some(Default::default());
        }
        let name = &signature.ident;
        let args = signature
            .inputs
            .iter()
            .filter_map(|arg| match arg {
                syn::FnArg::Typed(arg) => Some(match &*arg.pat {
                    Pat::Ident(pat) => Ok(&pat.ident),
                    _ => Err(syn::Error::new_spanned(
                        arg,
                        "style setters require named arguments",
                    )),
                }),
                _ => None,
            })
            .collect::<syn::Result<Vec<_>>>()?;
        let attrs = &method.attrs;
        // The borrowed style editor does not allocate, clone a style, or construct
        // a dummy widget. Cross-property helpers always call canonical setters.
        let forward = macro_paths(quote! {
            #(#attrs)*
            pub #signature {
                crate::style::builder::StyleBuilder::new(
                    crate::style::builder::StyleTarget::__style_mut(&mut self)
                ).#name(#(#args),*);
                self
            }
        });
        methods.push(quote!(#name { #forward }));
    }
    Ok(quote! {
        #implementation
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #name {
            ($($excluded:ident),* $(,)?) => {
                $crate::__voidui_filter_style_methods! {
                    [$($excluded),*] #(#methods)*
                }
            };
        }
    })
}

/// Read only the entry names. Bodies can contain hygienic $crate paths, so they
/// remain token streams rather than being reparsed as ordinary Rust functions.
pub struct Filter {
    methods: TokenStream,
}
impl Parse for Filter {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let excluded;
        bracketed!(excluded in input);
        let excluded: BTreeSet<_> = excluded
            .parse_terminated(Ident::parse, Token![,])?
            .into_iter()
            .map(|name| name.to_string())
            .collect();
        let mut methods = TokenStream::new();
        while !input.is_empty() {
            let name: Ident = input.parse()?;
            let body;
            braced!(body in input);
            let tokens: TokenStream = body.parse()?;
            if !excluded.contains(&name.to_string()) {
                methods.extend(tokens);
            }
        }
        Ok(Self { methods })
    }
}
impl Filter {
    pub fn expand(self) -> TokenStream {
        self.methods
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_use_public_crate_paths_and_preserve_type_arguments() {
        let mut signature: syn::Signature = parse_quote!(
            fn example(self, value: impl Into<CssValue<Option<FontFallbacks>>>,
                tracks: Vec<GridTemplateComponent<String>>, length: taffy::Dimension,
                overflow: super::scroll::Overflow) -> Self
        );
        Qualify.visit_signature_mut(&mut signature);
        let tokens = quote!(#signature).to_string();
        assert!(tokens.contains("crate :: style :: builder :: CssValue"));
        assert!(tokens.contains(":: core :: option :: Option"));
        assert!(tokens.contains("crate :: style :: builder :: FontFallbacks"));
        assert!(tokens.contains(":: std :: string :: String"));
        assert!(tokens.contains("crate :: style :: builder :: taffy :: Dimension"));
        assert!(tokens.contains("crate :: style :: scroll :: Overflow"));
        assert!(tokens.ends_with("-> Self"));
    }

    #[test]
    fn component_inputs_shadow_only_the_matching_inherent_methods() {
        let filtered: Filter = syn::parse2(quote! {
            [padding, w_full]
            padding { pub fn padding(self) {} }
            p_4 { pub fn p_4(self) { $crate::apply(); } }
            w_full { pub fn w_full(self) {} }
        })
        .unwrap();
        assert_eq!(
            filtered.expand().to_string(),
            quote! {
                pub fn p_4(self) { $crate::apply(); }
            }
            .to_string()
        );
    }
}
