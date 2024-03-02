use {
    quote::format_ident,
    std::ops::Not,
    syn::{punctuated::Punctuated, Meta, Token},
};

extern crate proc_macro;
use {
    proc_macro::TokenStream,
    quote::quote,
    syn::{parse_macro_input, DeriveInput},
};

#[proc_macro_derive(Codec, attributes(codec))]
pub fn derive_codec(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let crate_name = syn::Ident::new("component_utils", proc_macro2::Span::call_site());
    let [encode, decode] = match input.data {
        syn::Data::Struct(s) => derive_codec_struct(s, crate_name.clone()),
        syn::Data::Enum(e) => derive_codec_enum(e, crate_name.clone()),
        syn::Data::Union(_) => unimplemented!("Unions are not supported"),
    };
    let (generics, lt) = modify_generics(crate_name.clone(), input.generics.clone());

    let ident = input.ident;
    let (impl_generics, ..) = generics.split_for_impl();
    let (.., ty_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics #crate_name::Codec<#lt> for #ident #ty_generics #where_clause {
            fn encode(&self, buffer: &mut impl #crate_name::Buffer) -> Option<()> {
                #encode
                Some(())
            }

            fn decode(buffer: &mut &#lt [u8]) -> Option<Self> {
                #decode
            }
        }
    }
    .into()
}

fn modify_generics(
    crate_name: syn::Ident,
    mut generics: syn::Generics,
) -> (syn::Generics, syn::Lifetime) {
    if generics.lifetimes().next().is_none() {
        let default_lt =
            syn::LifetimeParam::new(syn::Lifetime::new("'a", proc_macro2::Span::call_site()));
        generics.params.insert(0, syn::GenericParam::Lifetime(default_lt));
    }
    let mut lts = generics.lifetimes();
    let lt = lts.next().unwrap().clone();
    assert!(lts.next().is_none(), "Only one lifetime is supported");

    for gen in generics.type_params_mut() {
        gen.bounds.push(syn::parse_quote!(#crate_name::Codec<#lt>));
    }

    (generics, lt.lifetime)
}

fn derive_codec_enum(e: syn::DataEnum, crate_name: syn::Ident) -> [proc_macro2::TokenStream; 2] {
    let variant_index = 0..e.variants.len() as u8;
    let variant_index2 = 0..e.variants.len() as u8;

    let destructure = e.variants.iter().map(|v| {
        let name = &v.ident;
        match &v.fields {
            syn::Fields::Named(n) => {
                let field_names = n.named.iter().map(|f| {
                    let name = &f.ident;
                    if FieldAttrFlags::new(&f.attrs).ignore {
                        quote! { #name: _ }
                    } else {
                        quote! { #name }
                    }
                });
                quote! { #name {#(#field_names),*} }
            }
            syn::Fields::Unnamed(u) => {
                let field_names = u.unnamed.iter().enumerate().map(|(i, f)| {
                    if FieldAttrFlags::new(&f.attrs).ignore {
                        format_ident!("_")
                    } else {
                        format_ident!("f{}", i)
                    }
                });
                quote! { #name (#(#field_names),*) }
            }
            syn::Fields::Unit => quote! { #name },
        }
    });

    let encode_variant = e.variants.iter().map(|v| match &v.fields {
        syn::Fields::Named(n) => {
            let field_names = n
                .named
                .iter()
                .filter_map(|f| FieldAttrFlags::new(&f.attrs).ignore.not().then_some(&f.ident));
            quote! { #(#crate_name::Codec::<'a>::encode(#field_names, buffer)?;)* }
        }
        syn::Fields::Unnamed(u) => {
            let field_names = u
                .unnamed
                .iter()
                .map(|f| FieldAttrFlags::new(&f.attrs))
                .enumerate()
                .filter_map(|(i, f)| f.ignore.not().then(|| format_ident!("f{}", i)));
            quote! { #(#crate_name::Codec::<'a>::encode(#field_names, buffer)?;)* }
        }
        syn::Fields::Unit => quote! {},
    });

    let decode_variant = e.variants.iter().map(|v| {
        let name = &v.ident;
        match &v.fields {
            syn::Fields::Named(n) => {
                let fields = n.named.iter().map(|f| {
                    let name = &f.ident;
                    if FieldAttrFlags::new(&f.attrs).ignore {
                        quote! { #name: Default::default() }
                    } else {
                        quote! { #name: #crate_name::Codec::decode(buffer)? }
                    }
                });
                quote! { #name {#(#fields),*} }
            }
            syn::Fields::Unnamed(u) => {
                let fields = u.unnamed.iter().map(|f| {
                    if FieldAttrFlags::new(&f.attrs).ignore {
                        quote! { Default::default() }
                    } else {
                        quote! { #crate_name::Codec::decode(buffer)? }
                    }
                });
                quote! { #name (#(#fields),*) }
            }
            syn::Fields::Unit => quote! { #name },
        }
    });

    [
        quote! {
            match self {
                #(Self::#destructure => {
                    buffer.push(#variant_index)?;
                    #encode_variant
                },)*
            }
        },
        quote! {
            let index = buffer.get(0)?;
            *buffer = &buffer[1..];

            match index {
                #(#variant_index2 => Some(Self::#decode_variant),)*
                _ => None,
            }
        },
    ]
}

fn derive_codec_struct(
    s: syn::DataStruct,
    crate_name: syn::Ident,
) -> [proc_macro2::TokenStream; 2] {
    match s.fields {
        syn::Fields::Named(n) => derive_codec_named_struct(n, crate_name),
        syn::Fields::Unnamed(u) => derive_codec_unnamed_struct(u, crate_name),
        syn::Fields::Unit => derive_codec_unit_struct(),
    }
}

fn derive_codec_unnamed_struct(
    u: syn::FieldsUnnamed,
    crate_name: syn::Ident,
) -> [proc_macro2::TokenStream; 2] {
    let flags = u.unnamed.iter().map(|f| FieldAttrFlags::new(&f.attrs)).collect::<Vec<_>>();

    let field_names = flags.iter().enumerate().map(|(i, f)| {
        if f.ignore {
            format_ident!("_")
        } else {
            format_ident!("f{}", i)
        }
    });
    let used_fields = flags
        .iter()
        .enumerate()
        .filter_map(|(i, f)| f.ignore.not().then(|| format_ident!("f{}", i)));

    let decode_fields = flags.iter().map(|f| {
        if f.ignore {
            quote! { Default::default() }
        } else {
            quote! { #crate_name::Codec::decode(buffer)? }
        }
    });

    [
        quote! {
            let Self(#(#field_names,)*) = self;
            #(#crate_name::Codec::<'a>::encode(#used_fields, buffer)?;)*
        },
        quote! {
            Some(Self(#(#decode_fields,)*))
        },
    ]
}

fn derive_codec_named_struct(
    n: syn::FieldsNamed,
    crate_name: syn::Ident,
) -> [proc_macro2::TokenStream; 2] {
    let flags = n.named.iter().map(|f| FieldAttrFlags::new(&f.attrs)).collect::<Vec<_>>();

    let field_names = flags.iter().zip(&n.named).map(|(f, nf)| {
        let name = &nf.ident;
        if f.ignore {
            quote! { #name: _ }
        } else {
            quote! { #name }
        }
    });
    let used_fields =
        flags.iter().zip(&n.named).filter_map(|(f, nf)| f.ignore.not().then_some(&nf.ident));

    let decode_fields = flags.iter().zip(&n.named).map(|(f, nf)| {
        let name = &nf.ident;
        if f.ignore {
            quote! { #name: Default::default() }
        } else {
            quote! { #name: #crate_name::Codec::decode(buffer)? }
        }
    });

    [
        quote! {
            let Self { #(#field_names,)* } = self;
            #(#crate_name::Codec::<'a>::encode(#used_fields, buffer)?;)*
        },
        quote! {
            Some(Self { #(#decode_fields,)* })
        },
    ]
}

fn derive_codec_unit_struct() -> [proc_macro2::TokenStream; 2] {
    [quote! {}, quote! { Some(Self) }]
}

#[derive(Default)]
struct FieldAttrFlags {
    ignore: bool,
}

impl FieldAttrFlags {
    fn new(attributes: &[syn::Attribute]) -> Self {
        attributes
            .iter()
            .filter(|a| a.path().is_ident("codec"))
            .filter_map(|a| match &a.meta {
                Meta::List(ml) => Some(ml),
                _ => None,
            })
            .flat_map(|ml| {
                ml.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated).unwrap()
            })
            .fold(Self::default(), |s, m| Self { ignore: s.ignore || m.path().is_ident("skip") })
    }
}
