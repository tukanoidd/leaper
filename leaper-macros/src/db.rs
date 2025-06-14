use darling::{
    FromDeriveInput, FromField, FromVariant,
    ast::{Data, Fields, Style},
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Attribute, Ident, LitStr, Type, Visibility};

use crate::util::DeriveInputUtil;

#[derive(FromDeriveInput)]
#[darling(supports(any), forward_attrs, attributes(db))]
pub struct DBEntry {
    vis: Visibility,
    ident: Ident,
    data: Data<DBEntryVariant, DBEntryField>,
    attrs: Vec<Attribute>,
    db_name: LitStr,
    table_name: LitStr,
}

impl DeriveInputUtil for DBEntry {
    fn gen_(&self) -> proc_macro2::TokenStream {
        let Self {
            vis,
            ident,
            data,
            attrs,
            ..
        } = &self;

        let table = self.gen_table();

        let derives = quote!(#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]);
        let entry = self.gen_entry();

        match data {
            Data::Enum(items) => {
                let vars = items.iter().map(DBEntryVariant::gen_enum_var);

                quote! {
                    #table

                    #derives
                    #(#attrs)*
                    #vis enum #ident {
                        #(#vars),*
                    }

                    #entry
                }
            }
            Data::Struct(fields) => {
                let fields = fields.iter().map(DBEntryField::gen_field);

                quote! {
                    #derives
                    #vis struct #ident {
                        #(#fields),*
                    }

                    #table

                    #entry
                }
            }
        }
    }
}

impl DBEntry {
    fn gen_table(&self) -> TokenStream {
        let Self {
            vis,
            db_name,
            table_name,
            ..
        } = self;

        let name = self.table_name();

        quote! {
            #vis struct #name;

            impl leaper_db::DBTable for #name {
                const DB_NAME: &'static str = #db_name;
                const NAME: &'static str = #table_name;
            }
        }
    }

    fn table_name(&self) -> Ident {
        format_ident!("{}Table", self.ident)
    }

    fn gen_entry(&self) -> TokenStream {
        let Self { ident, .. } = self;

        let table = self.table_name();

        quote! {
            impl leaper_db::TDBTableEntry for #ident {
                type Table = #table;
            }
        }
    }
}

#[derive(FromVariant)]
#[darling(forward_attrs)]
struct DBEntryVariant {
    ident: Ident,
    fields: Fields<DBEntryField>,
    attrs: Vec<Attribute>,
}

impl DBEntryVariant {
    fn gen_enum_var(&self) -> TokenStream {
        let Self {
            ident,
            fields,
            attrs,
        } = self;

        let fields = match fields.style {
            Style::Tuple => {
                let fields = fields.fields.iter().map(DBEntryField::gen_field);
                Some(quote! { (#(#fields),*) })
            }
            Style::Struct => {
                let fields = fields.fields.iter().map(DBEntryField::gen_field);
                Some(quote!({ #(#fields),* }))
            }
            Style::Unit => None,
        };

        quote! {
            #(#attrs)*
            #ident #fields
        }
    }
}

#[derive(FromField)]
#[darling(forward_attrs)]
struct DBEntryField {
    vis: Visibility,
    ident: Option<Ident>,
    ty: Type,
    attrs: Vec<Attribute>,
}

impl DBEntryField {
    fn gen_field(&self) -> TokenStream {
        let Self {
            vis,
            ident,
            ty,
            attrs,
        } = self;
        let name = ident.as_ref().map(|i| quote!(#i:));

        quote!(#(#attrs)* #vis #name #ty)
    }
}
