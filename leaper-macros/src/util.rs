use darling::FromDeriveInput;
use proc_macro2::TokenStream;
use syn::DeriveInput;

pub trait DeriveInputUtil: FromDeriveInput {
    fn parse(input: TokenStream) -> manyhow::Result<Self> {
        let derive_input: DeriveInput = syn::parse2(input)?;
        let res = Self::from_derive_input(&derive_input)?;
        Ok(res)
    }

    fn gen_(&self) -> manyhow::Result<TokenStream>;
}
