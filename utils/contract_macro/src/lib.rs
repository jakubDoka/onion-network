extern crate proc_macro;

use {
    contract_metadata::ContractMetadata,
    heck::ToUpperCamelCase as _,
    ink_metadata::{InkProject, MetadataVersion, Selector},
    proc_macro::TokenStream,
    proc_macro_error::{abort_call_site, proc_macro_error},
    scale_typegen::{TypeGenerator, TypeGeneratorSettings},
};

#[proc_macro]
#[proc_macro_error]
pub fn contract(input: TokenStream) -> TokenStream {
    let contract_path = syn::parse_macro_input!(input as syn::LitStr);

    let root = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let metadata_path: std::path::PathBuf = [&root, &contract_path.value()].iter().collect();

    let reader = std::fs::File::open(metadata_path)
        .unwrap_or_else(|e| abort_call_site!("Failed to read metadata file: {}", e));
    let metadata: ContractMetadata = serde_json::from_reader(reader)
        .unwrap_or_else(|e| abort_call_site!("Failed to deserialize contract metadata: {}", e));
    let contract_name = metadata.contract.name;
    let ink_metadata: InkProject = serde_json::from_value(serde_json::Value::Object(metadata.abi))
        .unwrap_or_else(|e| abort_call_site!("Failed to deserialize ink metadata: {}", e));
    if &MetadataVersion::V4 == ink_metadata.version() {
        let contract_mod = generate_contract_mod(&contract_name, &ink_metadata);
        contract_mod.into()
    } else {
        abort_call_site!("Invalid contract metadata version")
    }
}

fn generate_contract_mod(contract_name: &str, metadata: &InkProject) -> proc_macro2::TokenStream {
    let crate_path: syn::Path = syn::parse_quote!(::subxt);

    let settings = TypeGeneratorSettings::new()
        .type_mod_name("contract_types")
        .add_derives_for_all(
            [
                quote::quote!(::parity_scale_codec::Encode),
                quote::quote!(::parity_scale_codec::Decode),
            ]
            .map(syn::parse2)
            .map(Result::unwrap),
        )
        .substitute(
            syn::parse_quote!(ink_primitives::types::AccountId),
            syn::parse_quote!(#crate_path::utils::AccountId32),
        )
        .substitute(
            syn::parse_quote!(ink_env::types::U256),
            syn::parse_quote!(::primitive_types::U256),
        );
    let type_generator = TypeGenerator::new(
        metadata.registry(),
        &settings,
        // "contract_types",
        // type_substitutes,
        // derives_registry,
        // crate_path,
        // false,
    );
    let types_mod = type_generator.generate_types_mod().expect("Error in type generation");
    let types_mod_ident = types_mod.ident();

    let contract_name = quote::format_ident!("{}", contract_name);
    let constructors = generate_constructors(metadata, &type_generator);
    let messages = generate_messages(metadata, &type_generator);
    let events = generate_events(metadata, &type_generator);
    let root_event = generate_root_event(metadata);
    let glob = types_mod.children().any(|child| *child.0 == contract_name).then(|| {
        quote::quote! { pub use #types_mod_ident::#contract_name::#contract_name::*; }
    });

    quote::quote!(
        #[allow(clippy::all)]
        pub mod #contract_name {
            #glob
            #types_mod

            pub mod constructors {
                use super::#types_mod_ident;
                #( #constructors )*
            }

            pub mod messages {
                use super::#types_mod_ident;
                #( #messages )*
            }

            #root_event

            pub mod events {
                use super::#types_mod_ident;

                #( #events )*
            }
        }
    )
}

fn generate_events(
    metadata: &ink_metadata::InkProject,
    type_gen: &TypeGenerator,
) -> Vec<proc_macro2::TokenStream> {
    metadata
        .spec()
        .events()
        .iter()
        .map(|event| {
            let name = event.label();
            let args = event
                .args()
                .iter()
                .map(|arg| (arg.label().as_str(), arg.ty().ty().id))
                .collect::<Vec<_>>();
            generate_event_impl(type_gen, name, &args)
        })
        .collect()
}

fn generate_root_event(metadata: &ink_metadata::InkProject) -> proc_macro2::TokenStream {
    let events = metadata
        .spec()
        .events()
        .iter()
        .map(|event| quote::format_ident!("{}", event.label()))
        .collect::<Vec<_>>();

    quote::quote! (
        #[derive(::parity_scale_codec::Decode)]
        pub enum Event {
            #( #events ( events::#events ) ),*
        }
    )
}

fn generate_event_impl(
    type_gen: &TypeGenerator<'_>,
    name: &str,
    args: &[(&str, u32)],
) -> proc_macro2::TokenStream {
    let name_ident = quote::format_ident!("{}", name);
    let (fields, _field_names): (Vec<_>, Vec<_>) = args
        .iter()
        .enumerate()
        .map(|(i, &(name, type_id))| {
            // In Solidity, event arguments may not have names.
            // If an argument without a name is included in the metadata, a name is generated for it
            let name = if name.is_empty() { format!("arg{i}") } else { name.to_string() };
            let name = quote::format_ident!("{}", name.as_str());
            let ty = type_gen.resolve_type_path(type_id).unwrap();
            (quote::quote!( pub #name: #ty ), name)
        })
        .unzip();

    quote::quote! (
        #[derive(::parity_scale_codec::Decode)]
        pub struct #name_ident {
            #( #fields ),*
        }
    )
}

fn generate_constructors(
    metadata: &ink_metadata::InkProject,
    type_gen: &TypeGenerator,
) -> Vec<proc_macro2::TokenStream> {
    let trait_path = syn::parse_quote!(crate::InkConstructor);
    metadata
        .spec()
        .constructors()
        .iter()
        .map(|constructor| {
            let name = constructor.label();
            let args = constructor
                .args()
                .iter()
                .map(|arg| (arg.label().as_str(), arg.ty().ty().id))
                .collect::<Vec<_>>();
            generate_message_impl(type_gen, name, args, constructor.selector(), &trait_path)
        })
        .collect()
}

fn generate_messages(
    metadata: &ink_metadata::InkProject,
    type_gen: &TypeGenerator,
) -> Vec<proc_macro2::TokenStream> {
    let trait_path = syn::parse_quote!(crate::InkMessage);
    metadata
        .spec()
        .messages()
        .iter()
        .map(|message| {
            // strip trait prefix from trait message labels
            let name =
                message.label().split("::").last().unwrap_or_else(|| {
                    abort_call_site!("Invalid message label: {}", message.label())
                });
            let args = message
                .args()
                .iter()
                .map(|arg| (arg.label().as_str(), arg.ty().ty().id))
                .collect::<Vec<_>>();

            generate_message_impl(type_gen, name, args, message.selector(), &trait_path)
        })
        .collect()
}

fn generate_message_impl(
    type_gen: &TypeGenerator,
    name: &str,
    args: Vec<(&str, u32)>,
    selector: &Selector,
    impl_trait: &syn::Path,
) -> proc_macro2::TokenStream {
    let struct_ident = quote::format_ident!("{}", name.to_upper_camel_case());
    let fn_ident = quote::format_ident!("{}", name);
    let (args, arg_names): (Vec<_>, Vec<_>) = args
        .iter()
        .enumerate()
        .map(|(i, (name, type_id))| {
            // In Solidity, function arguments may not have names.
            // If an argument without a name is included in the metadata, a name is generated for it
            let name = if name.is_empty() { format!("arg{i}") } else { (*name).to_string() };
            let name = quote::format_ident!("{}", name.as_str());
            let ty = type_gen.resolve_type_path(*type_id).unwrap();
            (quote::quote!( #name: #ty ), name)
        })
        .unzip();
    let selector_bytes = hex_lits(selector);
    quote::quote! (
        #[derive(::parity_scale_codec::Encode)]
        pub struct #struct_ident {
            #( #args ), *
        }

        impl #impl_trait for #struct_ident {
            const SELECTOR: [u8; 4] = [ #( #selector_bytes ),* ];
        }

        pub fn #fn_ident(#( #args ), *) -> #struct_ident {
            #struct_ident {
                #( #arg_names ), *
            }
        }
    )
}

/// Returns the 4 bytes that make up the selector as hex encoded bytes.
fn hex_lits(selector: &ink_metadata::Selector) -> [syn::LitInt; 4] {
    let hex_lits = selector
        .to_bytes()
        .iter()
        .map(|byte| syn::LitInt::new(&format!("0x{byte:02X}_u8"), proc_macro2::Span::call_site()))
        .collect::<Vec<_>>();
    hex_lits.try_into().expect("Invalid selector bytes length")
}
