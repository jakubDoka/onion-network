use {
    quote::{format_ident, ToTokens},
    syn::{punctuated::Punctuated, Expr, Meta, Token},
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

    let crate_name = syn::Ident::new("codec", proc_macro2::Span::call_site());
    let is_packed = input.attrs.iter().any(|a| {
        a.path().is_ident("repr") && &a.meta.require_list().unwrap().tokens.to_string() == "packed"
    });
    let [encode, decode] = match input.data {
        syn::Data::Struct(s) => derive_codec_struct(s, is_packed, &crate_name),
        syn::Data::Enum(e) => derive_codec_enum(e, &crate_name),
        syn::Data::Union(_) => unimplemented!("Unions are not supported"),
    };
    let (encode_generics, _) =
        modify_generics(input.generics.clone(), false, syn::parse_quote!(#crate_name::Encode));
    let (decode_generics, lt) =
        modify_generics(input.generics, true, syn::parse_quote!(#crate_name::Decode<'a>));

    let (impl_encode_generics, ty_generics, where_clause) = encode_generics.split_for_impl();
    let lt = lt.unwrap();
    let (impl_decode_generics, ..) = decode_generics.split_for_impl();
    let ident = &input.ident;

    quote! {
        impl #impl_decode_generics #crate_name::Decode<#lt> for #ident #ty_generics #where_clause {
            fn decode(buffer: &mut &#lt [u8]) -> Option<Self> {
                #decode
            }
        }

        impl #impl_encode_generics #crate_name::Encode for #ident #ty_generics #where_clause {
            fn encode(&self, buffer: &mut impl #crate_name::Buffer) -> Option<()> {
                #encode
                Some(())
            }
        }
    }
    .into()
}

fn modify_generics(
    mut generics: syn::Generics,
    add_lifetime: bool,
    trait_path: syn::TypeParamBound,
) -> (syn::Generics, Option<syn::Lifetime>) {
    if generics.lifetimes().next().is_none() && add_lifetime {
        let default_lt =
            syn::LifetimeParam::new(syn::Lifetime::new("'a", proc_macro2::Span::call_site()));
        generics.params.insert(0, syn::GenericParam::Lifetime(default_lt));
    }

    let mut lts = generics.lifetimes();
    let lt = lts.next().cloned().map(|lt| lt.lifetime);

    for gen in generics.type_params_mut() {
        gen.bounds.push(trait_path.clone());
    }

    (generics, lt)
}

fn derive_codec_enum(e: syn::DataEnum, crate_name: &syn::Ident) -> [proc_macro2::TokenStream; 2] {
    let expand = |(i, v): (usize, syn::Variant)| {
        let name = v.ident;
        let i = i as u8;
        match v.fields {
            syn::Fields::Named(n) => {
                let fields = FieldMeta::from_fields(n.named);
                let (bindings, encodes, decodes) = FieldMeta::full_set(&fields, false, crate_name);
                (
                    quote! {
                        Self::#name { #(#bindings,)* } => {
                            buffer.push(#i)?;
                            #(#encodes;)*
                        }
                    },
                    quote! { #i => Some(Self::#name { #(#decodes,)* }) },
                )
            }
            syn::Fields::Unnamed(u) => {
                let fields = FieldMeta::from_fields(u.unnamed);
                let (bindings, encodes, decodes) = FieldMeta::full_set(&fields, false, crate_name);
                (
                    quote! {
                        Self::#name(#(#bindings,)*) => {
                            buffer.push(#i)?;
                            #(#encodes;)*
                        }
                    },
                    quote! { #i => Some(Self::#name(#(#decodes,)*)) },
                )
            }
            syn::Fields::Unit => {
                (quote! { Self::#name => buffer.push(#i)? }, quote! { #i => Some(Self::#name) })
            }
        }
    };
    let (encodes, decodes): (Vec<_>, Vec<_>) =
        e.variants.into_iter().enumerate().map(expand).unzip();

    [
        quote! {
            match self { #(#encodes,)* }
        },
        quote! {
            match buffer.take_first()? {
                #(#decodes,)*
                _ => None,
            }
        },
    ]
}

fn derive_codec_struct(
    s: syn::DataStruct,
    is_packed: bool,
    crate_name: &syn::Ident,
) -> [proc_macro2::TokenStream; 2] {
    let slf = match is_packed {
        true => quote! { *self },
        false => quote! { self },
    };

    match s.fields {
        syn::Fields::Named(n) => {
            let fields = FieldMeta::from_fields(n.named);
            let (bindings, encodes, decodes) = FieldMeta::full_set(&fields, is_packed, crate_name);
            [
                quote! {
                    let Self { #(#bindings,)* } = #slf;
                    #(#encodes;)*
                },
                quote! { Some(Self { #(#decodes,)* }) },
            ]
        }
        syn::Fields::Unnamed(u) => {
            let fields = FieldMeta::from_fields(u.unnamed);
            let (bindings, encodes, decodes) = FieldMeta::full_set(&fields, is_packed, crate_name);
            [
                quote! {
                    let Self(#(#bindings,)*) = #slf;
                    #(#encodes;)*
                },
                quote! { Some(Self(#(#decodes,)*)) },
            ]
        }
        syn::Fields::Unit => [quote! {}, quote! { Some(Self) }],
    }
}

struct FieldMeta {
    ignore: bool,
    with_wrapper: Option<Expr>,
    name: Result<syn::Ident, usize>,
}

impl FieldMeta {
    fn from_fields(fields: impl IntoIterator<Item = syn::Field>) -> Vec<Self> {
        fields.into_iter().enumerate().map(|(i, f)| Self::new(&f.attrs, f.ident.ok_or(i))).collect()
    }

    fn full_set<'a>(
        fields: &'a [Self],
        is_packed: bool,
        crate_name: &'a syn::Ident,
    ) -> (
        impl Iterator<Item = proc_macro2::TokenStream> + 'a,
        impl Iterator<Item = proc_macro2::TokenStream> + 'a,
        impl Iterator<Item = proc_macro2::TokenStream> + 'a,
    ) {
        (
            fields.iter().map(Self::binding),
            fields.iter().map(move |f| f.encode_field(crate_name, is_packed)),
            fields.iter().map(|f| f.decode_field(crate_name)),
        )
    }

    fn new(attributes: &[syn::Attribute], name: Result<syn::Ident, usize>) -> Self {
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
            .fold(Self { name, ignore: false, with_wrapper: None }, |s, m| Self {
                ignore: s.ignore || m.path().is_ident("skip"),
                with_wrapper: m
                    .path()
                    .is_ident("with")
                    .then(|| m.require_name_value().ok())
                    .flatten()
                    .map(|mnv| mnv.value.clone()),
                ..s
            })
    }

    fn decode_field(&self, crate_name: &syn::Ident) -> proc_macro2::TokenStream {
        let value = if self.ignore {
            quote! { Default::default() }
        } else if let Some(with) = &self.with_wrapper {
            quote! { #with::decode(buffer)? }
        } else {
            quote! { #crate_name::Decode::<'a>::decode(buffer)? }
        };

        match &self.name {
            Err(_) => value,
            Ok(n) => quote! { #n: #value },
        }
    }

    fn encode_field(&self, crate_name: &syn::Ident, is_packed: bool) -> proc_macro2::TokenStream {
        if self.ignore {
            return quote! {};
        }

        let name = match &self.name {
            Err(n) => format_ident!("f{}", n),
            Ok(n) => n.clone(),
        };

        let name = match is_packed {
            true => quote! { &#name },
            false => quote! { #name },
        };

        if let Some(with) = &self.with_wrapper {
            quote! { #with::encode(#name, buffer)?; }
        } else {
            quote! { #crate_name::Encode::encode(#name, buffer)?; }
        }
    }

    fn binding(&self) -> proc_macro2::TokenStream {
        match &self.name {
            Err(n) => {
                if self.ignore {
                    format_ident!("_").to_token_stream()
                } else {
                    format_ident!("f{}", n).to_token_stream()
                }
            }
            Ok(n) => {
                if self.ignore {
                    quote! { #n: _ }
                } else {
                    n.clone().to_token_stream()
                }
            }
        }
    }
}
