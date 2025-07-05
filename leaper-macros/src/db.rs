use darling::{FromDeriveInput, FromField, ast::Data, util::Ignored};
use manyhow::Emitter;
use proc_macro2::TokenStream;
use quote::quote;
use surrealdb_core::dbs::{Capabilities, capabilities::Targets};
use syn::{Attribute, Generics, Ident, LitStr, Type, Visibility};

use crate::DeriveInputUtil;

#[derive(FromDeriveInput)]
#[darling(supports(struct_named), attributes(table), forward_attrs(serde, opt))]
pub struct DBTable {
    vis: Visibility,
    ident: Ident,
    generics: Generics,
    data: Data<Ignored, DBTableField>,
    attrs: Vec<Attribute>,

    sql: Option<Vec<LitStr>>,
    db: Ident,
}

impl DeriveInputUtil for DBTable {
    fn gen_(&self) -> manyhow::Result<proc_macro2::TokenStream> {
        let Self {
            vis,
            ident,
            generics,
            data,
            attrs,

            sql,
            db,
        } = self;

        let (_impl_gen, ty_gen, where_gen) = generics.split_for_impl();

        let fields = match data {
            Data::Enum(_) => unreachable!(),
            Data::Struct(fields) => fields,
        };

        let struct_fields = fields.iter().map(DBTableField::gen_struct_field);

        let db_str = db.to_string();
        let db = LitStr::new(&db_str, db.span());

        let mut emitter = Emitter::new();

        let mut capabilities = Capabilities::all();
        *capabilities.allowed_experimental_features_mut() = Targets::All;

        let sql = sql.as_ref().map(|sql| {
            let list = sql
                .iter()
                .flat_map(|sql_lit_str| {
                    let sql_str = sql_lit_str.value();
                    let sql_parse_result =
                        surrealdb_core::syn::parse_with_capabilities(&sql_str, &capabilities);

                    match sql_parse_result {
                        Ok(_) => Some(sql_lit_str),
                        Err(err) => {
                            emitter.emit(manyhow::error_message!(sql_lit_str.span(), "{err}"));
                            None
                        }
                    }
                })
                .collect::<Vec<_>>();

            quote!(#[sql([#(#list),*])])
        });

        emitter.into_result().map(|_| {
            quote! {
                #[derive(Debug, Clone, surrealdb_extras::SurrealTable, serde::Serialize, serde::Deserialize)]
                #(#attrs)*
                #[db(#db)]
                #sql
                #vis struct #ident #ty_gen #where_gen {
                    #(#struct_fields),*
                }
            }
        })
    }
}

#[derive(FromField)]
#[darling(forward_attrs(opt, serde))]
struct DBTableField {
    vis: Visibility,
    ident: Option<Ident>,
    ty: Type,
    attrs: Vec<Attribute>,
}

impl DBTableField {
    fn gen_struct_field(&self) -> TokenStream {
        let Self {
            vis,
            ident,
            ty,
            attrs,
        } = self;

        quote! {
            #(#attrs)*
            #vis #ident: #ty
        }
    }
}
