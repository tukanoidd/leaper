mod db;
mod errors;
mod util;

use proc_macro2::TokenStream;
use quote::ToTokens;
use surrealdb_core::dbs::{Capabilities, capabilities::Targets};
use syn::LitStr;

use crate::{
    db::{DBQuery, DBTable},
    errors::LError,
    util::DeriveInputUtil,
};

#[manyhow::manyhow]
#[proc_macro_attribute]
pub fn lerror(_attr: TokenStream, input: TokenStream) -> manyhow::Result<TokenStream> {
    let err = LError::parse(input)?;
    let res = err.gen_()?;

    Ok(res)
}

#[manyhow::manyhow]
#[proc_macro_attribute]
pub fn db_table(_attr: TokenStream, input: TokenStream) -> manyhow::Result<TokenStream> {
    let table = DBTable::parse(input)?;
    let res = table.gen_()?;

    Ok(res)
}

#[manyhow::manyhow]
#[proc_macro_derive(DBQuery, attributes(query, var))]
pub fn query(input: TokenStream) -> manyhow::Result<TokenStream> {
    let query = DBQuery::parse(input)?;
    let res = query.gen_()?;

    Ok(res)
}

#[manyhow::manyhow]
#[proc_macro]
pub fn sql(input: TokenStream) -> manyhow::Result<TokenStream> {
    let sql_lit_str = syn::parse2::<LitStr>(input)?;
    let sql_str = sql_lit_str.value();

    let mut capabilities = Capabilities::all();
    *capabilities.allowed_experimental_features_mut() = Targets::All;

    match surrealdb_core::syn::parse_with_capabilities(&sql_str, &capabilities) {
        Ok(_) => Ok(sql_lit_str.to_token_stream()),
        Err(err) => manyhow::bail!(sql_lit_str.span(), "{err}"),
    }
}
