use std::str::FromStr;

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident, LitStr, parse_macro_input};

#[proc_macro_derive(ReplayScript, attributes(script))]
pub fn derive_replay_script(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    do_derive_replay_script(input)
        .unwrap_or_else(|err| err.to_compile_error().into())
        .into()
}

fn do_derive_replay_script(input: DeriveInput) -> Result<TokenStream, syn::Error> {
    let struct_name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let Data::Struct(data_struct) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input,
            "ReplayScript can only be derived for structs",
        ));
    };

    let Fields::Named(fields) = &data_struct.fields else {
        return Err(syn::Error::new_spanned(
            &input,
            "ReplayScript requires named fields",
        ));
    };

    let mut script_conditions = Vec::new();

    for field in &fields.named {
        let field_name = field.ident.as_ref().unwrap();

        for attr in &field.attrs {
            if attr.path().is_ident("script") {
                let rule_config = parse_tstl_rule_attr(attr)?;
                let condition = generate_condition_code(field_name, &rule_config);
                script_conditions.push(condition);
            }
        }
    }

    let expanded = quote! {
        impl #impl_generics ReplayScript for #struct_name #type_generics #where_clause {
            fn write_replay_script(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                #(#script_conditions)*
                Ok(())
            }
        }
    };

    Ok(TokenStream::from(expanded))
}

struct TstlRuleConfig {
    src: proc_macro2::TokenStream,
    enable: proc_macro2::TokenStream,
}

fn parse_tstl_rule_attr(attr: &syn::Attribute) -> Result<TstlRuleConfig, syn::Error> {
    let mut src = None;
    let mut enable = None;

    attr.parse_nested_meta(|meta| {
        let path = &meta.path;
        if path.is_ident("src") {
            let value: LitStr = meta.value()?.parse()?;
            src = Some(quote! { concat!(env!("OUT_DIR"), "/", #value) })
        } else if path.is_ident("src_rel") {
            let value: LitStr = meta.value()?.parse()?;
            src = Some(quote! { #value })
        } else if path.is_ident("enable") {
            let lit: LitStr = meta.value()?.parse()?;
            let token_stream = proc_macro2::TokenStream::from_str(&lit.value())
                .map_err(|e| syn::Error::new_spanned(&lit, format!("failed to parse: {e}")))?;
            enable = Some(token_stream);
        } else if path.is_ident("enable_when") {
            let value: proc_macro2::TokenStream = meta.value()?.parse()?;
            enable = Some(quote! { == #value });
        } else {
            return Err(meta.error("unknown attribute"));
        }
        Ok(())
    })?;

    let src = src.ok_or_else(|| syn::Error::new_spanned(attr, "missing src"))?;
    let enable = enable.ok_or_else(|| syn::Error::new_spanned(attr, "missing enable"))?;

    Ok(TstlRuleConfig { src, enable })
}

fn generate_condition_code(
    field_name: &Ident,
    rule_config: &TstlRuleConfig,
) -> proc_macro2::TokenStream {
    let src = &rule_config.src;
    let enable = &rule_config.enable;
    quote! {
        if self.#field_name #enable {
            fmt.write_str(include_str!(#src))?;
            fmt.write_str("\n")?;
        }
    }
}
