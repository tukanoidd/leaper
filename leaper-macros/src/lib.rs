mod db;
mod errors;
mod util;

use proc_macro2::TokenStream;

use crate::{db::DBEntry, errors::LError, util::DeriveInputUtil};

#[manyhow::manyhow]
#[proc_macro_attribute]
pub fn lerror(_attr: TokenStream, input: TokenStream) -> manyhow::Result<TokenStream> {
    let err = LError::parse(input)?;
    let res = err.gen_();

    Ok(res)
}

#[manyhow::manyhow]
#[proc_macro_attribute]
pub fn db_entry(_attr: TokenStream, input: TokenStream) -> manyhow::Result<TokenStream> {
    let err = DBEntry::parse(input)?;
    let res = err.gen_();

    Ok(res)
}
