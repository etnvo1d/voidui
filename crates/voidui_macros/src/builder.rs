//! Named properties are ordinary typed builders over the existing component
//! constructor. Normalization and retained identity still have one implementation.
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, ItemFn, Pat, Type, parse_quote};

fn is_children(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == "Children"))
}
pub(super) fn requested(item: &ItemFn) -> bool {
    item.sig.inputs.iter().any(|arg| {
        matches!(arg, FnArg::Typed(arg)
        if is_children(&arg.ty) || arg.attrs.iter().any(|a| a.path().is_ident("prop")))
    })
}

pub(super) fn expand(mut item: ItemFn, memo: bool) -> syn::Result<TokenStream> {
    let path = match proc_macro_crate::crate_name("voidui")
        .map_err(|e| syn::Error::new_spanned(&item.sig, e))?
    {
        proc_macro_crate::FoundCrate::Itself => quote!(::voidui),
        proc_macro_crate::FoundCrate::Name(name) => {
            let name = format_ident!("{name}");
            quote!(::#name)
        }
    };
    let name = item.sig.ident.clone();
    let visibility = item.vis.clone();
    let attrs = item.attrs.clone();
    let cfg: Vec<_> = attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg") || a.path().is_ident("cfg_attr"))
        .collect();
    let builder = format_ident!("__Voidui_{}_Builder", name);
    let mount = format_ident!("__voidui_mount_{}", name);
    let mut names = Vec::new();
    let mut types = Vec::new();
    let mut defaults = Vec::new();
    let mut child_index = None;
    for (index, arg) in item.sig.inputs.iter_mut().enumerate() {
        let FnArg::Typed(arg) = arg else {
            return Err(syn::Error::new_spanned(
                arg,
                "components cannot have a self parameter",
            ));
        };
        let Pat::Ident(pattern) = &*arg.pat else {
            return Err(syn::Error::new_spanned(
                arg,
                "use a named component parameter",
            ));
        };
        let mut default = None;
        for attr in arg.attrs.iter().filter(|a| a.path().is_ident("prop")) {
            attr.parse_nested_meta(|meta| {
                if !meta.path.is_ident("default") || default.is_some() {
                    return Err(meta.error("expected one default or default = expression"));
                }
                default = Some(if meta.input.peek(syn::Token![=]) {
                    meta.value()?.parse::<syn::Expr>()?
                } else {
                    parse_quote!(::core::default::Default::default())
                });
                Ok(())
            })?;
            if default.is_none() {
                return Err(syn::Error::new_spanned(
                    attr,
                    "expected #[prop(default)] or #[prop(default = expression)]",
                ));
            }
        }
        arg.attrs.retain(|a| !a.path().is_ident("prop"));
        if is_children(&arg.ty) {
            if child_index.replace(index).is_some() {
                return Err(syn::Error::new_spanned(
                    arg,
                    "only one Children parameter is supported",
                ));
            }
            default.get_or_insert_with(|| parse_quote!(::core::default::Default::default()));
        } else if default.is_some() && reserved(&pattern.ident.to_string()) {
            return Err(syn::Error::new_spanned(
                &pattern.ident,
                "property name conflicts with a component builder method; use a domain-specific name",
            ));
        }
        // Anonymous input types need names when retained in a public builder.
        if let Type::ImplTrait(ty) = &*arg.ty {
            let generic = format_ident!("__VoiduiInput{index}");
            let bounds = &ty.bounds;
            item.sig
                .generics
                .params
                .push(parse_quote!(#generic: #bounds));
            arg.ty = Box::new(parse_quote!(#generic));
        }
        if let Type::Reference(reference) = &mut *arg.ty
            && reference.lifetime.is_none()
        {
            reference.lifetime = Some(parse_quote!('static));
        }
        names.push(pattern.ident.clone());
        types.push((*arg.ty).clone());
        defaults.push(default);
    }
    item.sig.ident = mount.clone();
    item.vis = syn::Visibility::Inherited;
    // The legacy expansion supplies exactly the same ownership bounds and input
    // conversions for positional and named-property components.
    super::expand(&mut item, memo)?;
    let generics = &item.sig.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let indices: Vec<_> = (0..names.len()).map(syn::Index::from).collect();
    let mut required = Vec::new();
    let mut initialize = Vec::new();
    let mut methods = Vec::new();
    for (index, (((name, ty), default), arg)) in names
        .iter()
        .zip(&types)
        .zip(&defaults)
        .zip(&item.sig.inputs)
        .enumerate()
    {
        let field = &indices[index];
        if let Some(default) = default {
            initialize.push(quote!(let #name: #ty = #default;));
            if Some(index) != child_index {
                let FnArg::Typed(arg) = arg else {
                    unreachable!()
                };
                let setter_type = &arg.ty;
                methods.push(quote! {
                    #[doc = concat!("Set the `", stringify!(#name), "` component input.")]
                    pub fn #name(mut self, value: #setter_type) -> Self {
                        self.inputs.#field = value.into(); self
                    }
                });
            }
        } else {
            required.push(arg.clone());
            initialize.push(quote!(let #name: #ty = ::core::convert::Into::into(#name);));
        }
    }
    if let Some(index) = child_index {
        let field = &indices[index];
        methods.push(quote! {
            /// Append a component, text, or a repeatable widget factory.
            pub fn child(mut self, child: impl #path::IntoChild) -> Self {
                self.inputs.#field = self.inputs.#field.child(child); self
            }
            /// Append keyed content in iterator order.
            pub fn children(mut self, children: impl IntoIterator<Item = impl #path::IntoChild>) -> Self {
                self.inputs.#field = self.inputs.#field.children(children); self
            }
        });
    }
    let generic_args: Vec<_> = generics
        .params
        .iter()
        .filter_map(|p| match p {
            syn::GenericParam::Type(p) => {
                let name = &p.ident;
                Some(quote!(#name))
            }
            syn::GenericParam::Const(p) => {
                let name = &p.ident;
                Some(quote!(#name))
            }
            syn::GenericParam::Lifetime(_) => None,
        })
        .collect();
    let style_shadows: Vec<_> = names
        .iter()
        .enumerate()
        .filter(|(index, _)| defaults[*index].is_some() && Some(*index) != child_index)
        .map(|(_, name)| name)
        .collect();
    Ok(quote! {
        #item
        #(#cfg)*
        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        #visibility struct #builder #impl_generics #where_clause {
            inputs: (#(#types,)*),
            options: #path::core::component::ComponentOptions,
            events: #path::EventBindings,
        }
        #(#cfg)*
        impl #impl_generics ::core::clone::Clone for #builder #type_generics #where_clause {
            fn clone(&self) -> Self {
                Self { inputs: self.inputs.clone(), options: self.options.clone(), events: self.events.clone() }
            }
        }
        #(#cfg)*
        impl #impl_generics #builder #type_generics #where_clause {
            #(#methods)*
            #path::__voidui_style_methods!(#(#style_shadows),*);
            #path::__voidui_component_modifiers!();
            /// Finish configuring the component without mounting it.
            pub fn build(self) -> #path::ComponentElement {
                let (#(#names,)*) = self.inputs;
                self.options.apply(#mount::<#(#generic_args),*>(#(#names),*), self.events)
            }
        }
        #(#cfg)*
        impl #impl_generics #path::style::builder::StyleTarget for #builder #type_generics #where_clause {
            fn __style_mut(&mut self) -> &mut #path::style::style::Style {
                #path::style::builder::StyleTarget::__style_mut(&mut self.options)
            }
        }
        #(#cfg)*
        impl #impl_generics #path::IntoElement for #builder #type_generics #where_clause {
            fn into_element(self) -> #path::Element {
                #path::IntoElement::into_element(self.build())
            }
        }
        #(#cfg)*
        impl #impl_generics #path::IntoChild for #builder #type_generics #where_clause {
            fn into_child(self) -> #path::ComponentElement { self.build() }
        }
        #(#attrs)*
        #visibility fn #name #impl_generics (#(#required),*) -> #builder #type_generics #where_clause {
            #(#initialize)*
            #builder { inputs: (#(#names,)*), options: Default::default(), events: Default::default() }
        }
    })
}
fn reserved(name: &str) -> bool {
    matches!(
        name,
        "key"
            | "id"
            | "class"
            | "add_class"
            | "child"
            | "children"
            | "build"
            | "into_element"
            | "into_child"
            | "on_click"
            | "on_mouse_enter"
            | "on_mouse_leave"
            | "on_mouse_down"
            | "on_mouse_up"
            | "on_mouse_move"
            | "on_mouse_scroll"
            | "on_drag"
            | "on_key_down"
            | "on_key_up"
    )
}
