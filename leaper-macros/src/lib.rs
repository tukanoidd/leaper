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
#[proc_macro_derive(DBQuery, attributes(query, var))]
pub fn query(input: TokenStream) -> manyhow::Result<TokenStream> {
    let query = DBQuery::parse(input)?;
    let res = query.gen_()?;

    Ok(res)
}
