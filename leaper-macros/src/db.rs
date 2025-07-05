use std::str::FromStr;

use darling::{
    FromDeriveInput, FromField,
    ast::{Data, Style},
    util::{Flag, Ignored},
};
use manyhow::Emitter;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use surrealdb_core::dbs::{Capabilities, capabilities::Targets};
use syn::{Attribute, Generics, Ident, LitStr, Type, Visibility, spanned::Spanned};

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

#[derive(FromDeriveInput)]
#[darling(supports(struct_named, struct_tuple), attributes(query))]
pub struct DBQuery {
    ident: Ident,
    generics: Generics,
    data: Data<Ignored, DBQueryField>,

    output: Option<LitStr>,
    check: Flag,
}

impl DBQuery {
    fn build_query(&self) -> TokenStream {
        let Self { data, check, .. } = self;

        let fields = match data {
            Data::Enum(_) => unreachable!(),
            Data::Struct(fields) => fields,
        };

        let binds = DBQueryField::build_query_binds(&fields.fields);

        let res = quote! {
            db.query(Self::QUERY_STR)
                #(#binds)*
                .await?
        };

        match check.is_present() {
            true => quote! {
                #res.check()?;
                Ok(())
            },
            false => quote!(Ok(#res.take::<Self::Output>(0)?)),
        }
    }
}

impl DeriveInputUtil for DBQuery {
    fn gen_(&self) -> manyhow::Result<TokenStream> {
        let Self {
            ident,
            generics,
            data,

            output,
            ..
        } = self;

        let output = output
            .as_ref()
            .map(|ty| {
                TokenStream::from_str(&ty.value())
                    .map_err(|err| manyhow::error_message!(ty.span(), "{err}"))
            })
            .transpose()?
            .unwrap_or_else(|| quote!(()));

        let (impl_gen, ty_gen, where_gen) = generics.split_for_impl();

        let fields = match data {
            Data::Enum(_) => unreachable!(),
            Data::Struct(fields) => fields,
        };

        let query_str = DBQueryField::build_query_str(&fields.fields);
        let query = self.build_query();

        let field_names = fields.fields.iter().enumerate().map(|(ind, field)| {
            field
                .ident
                .clone()
                .unwrap_or_else(|| Ident::new(&format!("var{ind}"), field.span()))
        });

        let self_unwrapped = {
            let fields = match fields.style {
                Style::Tuple => quote!((#(#field_names),*)),
                Style::Struct => quote!({#(#field_names),*}),
                Style::Unit => unreachable!(),
            };

            quote!(let Self #fields = self;)
        };

        Ok(quote! {
            impl #impl_gen crate::db::queries::DBQuery for #ident #ty_gen #where_gen {
                type Output = #output;
                const QUERY_STR: &'static str = macros::sql!(#query_str);

                async fn execute(self, db: Arc<DB>) -> crate::db::DBResult<Self::Output> {
                    #self_unwrapped
                    #query
                }
            }
        })
    }
}

#[derive(FromField)]
#[darling(attributes(var))]
struct DBQueryField {
    vis: Visibility,
    ident: Option<Ident>,
    ty: Type,

    sql: Option<LitStr>,
}

impl DBQueryField {
    fn span(&self) -> Span {
        match &self.sql {
            Some(str) => str.span(),
            None => match &self.vis {
                Visibility::Public(pub_) => pub_.span(),
                Visibility::Restricted(vis_restricted) => vis_restricted.span(),
                Visibility::Inherited => match &self.ident {
                    Some(ident) => ident.span(),
                    None => self.ty.span(),
                },
            },
        }
    }

    fn build_query_str(list: &[Self]) -> LitStr {
        let query_str = list.iter().enumerate().fold(
            String::new(),
            |str, (ind, DBQueryField { ident, sql, .. })| {
                let var_name = match ident {
                    Some(ident) => format!("${ident}"),
                    None => format!("$var{ind}"),
                };

                match sql {
                    Some(sql) => format!("{str}{}", sql.value().replace("{}", &var_name)),
                    None => format!("{str}{var_name}"),
                }
            },
        );

        LitStr::new(
            &query_str,
            match list.is_empty() {
                true => Span::mixed_site(),
                false => {
                    let fst_span = list[0].span();

                    match list.len() {
                        1 => fst_span,
                        _ => list[1..]
                            .iter()
                            .fold(fst_span, |span, field| span.join(field.span()).unwrap()),
                    }
                }
            },
        )
    }

    fn build_query_binds(list: &[Self]) -> impl Iterator<Item = TokenStream> {
        list.iter().enumerate().map(|(ind, Self { ident, .. })| {
            let ident_str = ident
                .as_ref()
                .map(|i| i.to_string())
                .unwrap_or_else(|| format!("var{ind}"));
            let ident_lit_str = LitStr::new(&ident_str, ident.span());

            quote!(.bind((#ident_lit_str, #ident)))
        })
    }
}
