use darling::{
    FromDeriveInput, FromField, FromVariant,
    ast::{Data, Fields, Style},
    util::{Flag, Ignored},
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{ExprArray, Ident, LitStr, Path, Type, Visibility};

use crate::util::DeriveInputUtil;

#[derive(FromDeriveInput)]
#[darling(supports(enum_any), forward_attrs, attributes(lerr))]
pub struct LError {
    vis: Visibility,
    ident: Ident,
    data: Data<LErrorVariant, Ignored>,
    result_name: Option<Ident>,
    prefix: Option<LitStr>,
}

impl DeriveInputUtil for LError {
    fn gen_(&self) -> manyhow::Result<TokenStream> {
        let Self {
            vis,
            ident,
            data,
            result_name,
            prefix,
        } = self;

        let variants = match data {
            Data::Enum(items) => items,
            Data::Struct(_) => unreachable!(),
        };
        let enum_vars = variants.iter().map(|var| var.gen_ty_var(prefix));

        let froms = variants.iter().filter_map(|var| var.gen_from(ident));

        let result_ty = result_name
            .as_ref()
            .map(|ty| quote! { #vis type #ty<T> = Result<T, #ident>; });

        Ok(quote! {
            #[derive(Debug, Clone, thiserror::Error)]
            #vis enum #ident {
                #(#enum_vars),*
            }

            #(#froms)*

            #result_ty
        })
    }
}

#[derive(FromVariant)]
#[darling(attributes(lerr))]
struct LErrorVariant {
    ident: Ident,
    fields: Fields<LErrorField>,

    str: LitStr,
    args: Option<ExprArray>,
    profile: Flag,
}

impl LErrorVariant {
    fn gen_ty_var(&self, prefix: &Option<LitStr>) -> TokenStream {
        let Self {
            ident,
            fields,

            str,
            args,
            profile,
        } = self;

        if !cfg!(feature = "profile") && profile.is_present() {
            return quote!();
        }

        let fields = match fields.style {
            darling::ast::Style::Tuple => {
                let fields = fields.iter().map(LErrorField::gen_ty_var_field);
                Some(quote! { (#(#fields),*) })
            }
            darling::ast::Style::Struct => {
                let fields = fields.iter().map(LErrorField::gen_ty_var_field);
                Some(quote!({ #(#fields),* }))
            }
            darling::ast::Style::Unit => None,
        };

        let str = match prefix {
            Some(prefix) => LitStr::new(
                &format!("{} {}", prefix.value(), str.value()),
                prefix.span(),
            ),
            None => str.clone(),
        };
        let args = args.as_ref().map(|args| {
            let args = args.elems.iter();
            quote! { , #(#args),* }
        });

        quote! {
            #[error(#str #args)]
            #ident #fields
        }
    }

    fn gen_from(&self, err: &Ident) -> Option<TokenStream> {
        if !cfg!(feature = "profile") && self.profile.is_present() {
            return None;
        }

        self.fields.fields.iter().find_map(|f| {
            f.from
                .is_present()
                .then(|| f.gen_from(err, &self.ident, &self.fields.style))
        })
    }
}

#[derive(FromField)]
#[darling(attributes(lerr))]
struct LErrorField {
    vis: Visibility,
    ident: Option<Ident>,
    ty: Type,
    from: Flag,
    backtrace: Flag,
    wrap: Option<Path>,
}

impl LErrorField {
    fn gen_ty_var_field(&self) -> TokenStream {
        let Self {
            vis,
            ident,
            ty,
            wrap,
            backtrace,
            ..
        } = self;

        let name = ident.as_ref().map(|i| quote!(#i:));
        let ty = match wrap {
            Some(wrap) => quote!(#wrap<#ty>),
            None => quote!(#ty),
        };
        let backtrace = backtrace.is_present().then(|| quote!(#[backtrace]));

        quote! {
            #backtrace
            #vis #name #ty
        }
    }

    fn gen_from(&self, err: &Ident, var: &Ident, style: &Style) -> TokenStream {
        let Self {
            ident, ty, wrap, ..
        } = self;
        let impl_ = {
            let res_val = wrap
                .as_ref()
                .map(|wrap| quote! { #wrap::new(val) })
                .unwrap_or(quote!(val));
            let val = match style {
                Style::Tuple => quote! { (#res_val) },
                Style::Struct => quote!({ #ident: #res_val }),
                Style::Unit => unreachable!(),
            };

            quote! {
                impl From<#ty> for #err {
                    fn from(val: #ty) -> Self {
                        Self::#var #val
                    }
                }
            }
        };
        let wrapped_impl = wrap.as_ref().map(|wrap| {
            let from_ty = quote!(#wrap<#ty>);
            let val = match style {
                Style::Tuple => quote! { (val) },
                Style::Struct => quote!({ #ident: val }),
                Style::Unit => unreachable!(),
            };

            quote! {
                impl From<#from_ty> for #err {
                    fn from(val: #from_ty) -> Self {
                        Self::#var #val
                    }
                }
            }
        });

        quote! {
            #impl_
            #wrapped_impl
        }
    }
}
