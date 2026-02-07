use proc_macro::TokenStream;
use quote::quote;
use std::collections::HashSet;
use syn::{
    Expr, ExprLit, Fields, Ident, ItemStruct, Lit, Meta, Path, Token, parse_macro_input, parse_str,
    punctuated::Punctuated,
};

struct BaseEntityConfig {
    traits_path: Path,
    active_model_ident: Ident,
    id_field: Ident,
    created_at_field: Ident,
    updated_at_field: Ident,
}

impl Default for BaseEntityConfig {
    fn default() -> Self {
        Self {
            traits_path: parse_str("crate::db::dao::base_traits")
                .expect("default traits path should parse"),
            active_model_ident: Ident::new("ActiveModel", proc_macro2::Span::call_site()),
            id_field: Ident::new("id", proc_macro2::Span::call_site()),
            created_at_field: Ident::new("created_at", proc_macro2::Span::call_site()),
            updated_at_field: Ident::new("updated_at", proc_macro2::Span::call_site()),
        }
    }
}

#[proc_macro_attribute]
pub fn base_entity(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr with Punctuated<Meta, Token![,]>::parse_terminated);
    let mut config = BaseEntityConfig::default();
    if let Err(err) = apply_args(&mut config, args) {
        return err.to_compile_error().into();
    }

    let mut input = parse_macro_input!(item as ItemStruct);
    let fields = match &mut input.fields {
        Fields::Named(fields) => fields,
        _ => {
            return syn::Error::new_spanned(
                input,
                "base_entity requires a struct with named fields",
            )
            .to_compile_error()
            .into();
        }
    };

    let existing: HashSet<String> = fields
        .named
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| ident.to_string()))
        .collect();

    let mut new_fields = Punctuated::new();

    if !existing.contains(&config.id_field.to_string()) {
        let id_ident = config.id_field.clone();
        let id_field: syn::Field = syn::parse_quote! {
            #[sea_orm(primary_key, auto_increment = false)]
            pub #id_ident: uuid::Uuid
        };
        new_fields.push(id_field);
    }

    if !existing.contains(&config.created_at_field.to_string()) {
        let created_ident = config.created_at_field.clone();
        let created_field: syn::Field = syn::parse_quote! {
            #[sea_orm(default_expr = "Expr::current_timestamp()")]
            pub #created_ident: sea_orm::entity::prelude::DateTimeWithTimeZone
        };
        new_fields.push(created_field);
    }

    if !existing.contains(&config.updated_at_field.to_string()) {
        let updated_ident = config.updated_at_field.clone();
        let updated_field: syn::Field = syn::parse_quote! {
            #[sea_orm(default_expr = "Expr::current_timestamp()")]
            pub #updated_ident: sea_orm::entity::prelude::DateTimeWithTimeZone
        };
        new_fields.push(updated_field);
    }

    for field in fields.named.iter().cloned() {
        new_fields.push(field);
    }

    fields.named = new_fields;

    let traits_path = config.traits_path;
    let active_model = config.active_model_ident;
    let id_field = config.id_field;
    let created_at_field = config.created_at_field;
    let updated_at_field = config.updated_at_field;

    let expanded = quote! {
        #input

        impl #traits_path::HasIdActiveModel for #active_model {
            fn set_id(&mut self, id: uuid::Uuid) {
                self.#id_field = sea_orm::ActiveValue::Set(id);
            }
        }

        impl #traits_path::TimestampedActiveModel for #active_model {
            fn set_created_at(
                &mut self,
                ts: sea_orm::entity::prelude::DateTimeWithTimeZone,
            ) {
                self.#created_at_field = sea_orm::ActiveValue::Set(ts);
            }

            fn set_updated_at(
                &mut self,
                ts: sea_orm::entity::prelude::DateTimeWithTimeZone,
            ) {
                self.#updated_at_field = sea_orm::ActiveValue::Set(ts);
            }
        }

        impl #traits_path::HasCreatedAtColumn for Entity {
            fn created_at_column() -> Column {
                Column::CreatedAt
            }
        }
    };

    expanded.into()
}

fn apply_args(
    config: &mut BaseEntityConfig,
    args: Punctuated<Meta, Token![,]>,
) -> Result<(), syn::Error> {
    for meta in args {
        let Meta::NameValue(name_value) = meta else {
            return Err(syn::Error::new_spanned(
                meta,
                "expected name-value pair, e.g. traits = \"path::to::traits\"",
            ));
        };

        let Some(ident) = name_value.path.get_ident() else {
            return Err(syn::Error::new_spanned(
                name_value.path,
                "expected simple identifier for attribute key",
            ));
        };

        let value = match name_value.value {
            Expr::Lit(ExprLit {
                lit: Lit::Str(lit_str),
                ..
            }) => lit_str,
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "expected string literal for attribute value",
                ));
            }
        };

        match ident.to_string().as_str() {
            "traits" => {
                config.traits_path = value.parse::<Path>().map_err(|err| {
                    syn::Error::new(value.span(), format!("invalid traits path: {err}"))
                })?;
            }
            "active_model" => {
                config.active_model_ident = Ident::new(&value.value(), value.span());
            }
            "id" => {
                config.id_field = Ident::new(&value.value(), value.span());
            }
            "created_at" => {
                config.created_at_field = Ident::new(&value.value(), value.span());
            }
            "updated_at" => {
                config.updated_at_field = Ident::new(&value.value(), value.span());
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    ident,
                    "unknown base_entity attribute key",
                ));
            }
        }
    }

    Ok(())
}
