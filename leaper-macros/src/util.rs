use darling::FromDeriveInput;
use proc_macro2::TokenStream;
use syn::DeriveInput;

pub trait DeriveInputUtil: FromDeriveInput {
    fn parse(input: TokenStream) -> manyhow::Result<Self> {
        let derive_input: DeriveInput = syn::parse2(input)?;
        let res =
            Self::from_derive_input(&derive_input).map_err(|e| manyhow::error_message!("{e}"))?;
        Ok(res)
    }

    fn gen_(&self) -> manyhow::Result<TokenStream>;
}
