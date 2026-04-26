// @review [ ]
use proc_macro_flow_core::traits::structural::input::FlowVisitorInput;
use syn::parse::{Parse, ParseStream};
use syn::visit::Visit;
use syn::visit_mut::VisitMut;
use syn::{Ident, ItemMod, Result, Token, punctuated::Punctuated};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentList(pub Vec<Ident>);

impl Parse for IdentList {
    fn parse(input: ParseStream) -> Result<Self> {
        let idents = Punctuated::<Ident, Token![,]>::parse_terminated(input)?
            .into_iter()
            .collect();
        Ok(Self(idents))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathList(pub Vec<syn::Path>);

impl Parse for PathList {
    fn parse(input: ParseStream) -> Result<Self> {
        let paths = Punctuated::<syn::Path, Token![,]>::parse_terminated(input)?
            .into_iter()
            .collect();
        Ok(Self(paths))
    }
}

pub struct NoopSynVisitor;
impl<'ast> Visit<'ast> for NoopSynVisitor {}
impl VisitMut for NoopSynVisitor {}

pub struct ModWithTrailing {
    pub item: ItemMod,
    pub trailing: proc_macro2::TokenStream,
}

impl FlowVisitorInput for ModWithTrailing {}

impl Parse for ModWithTrailing {
    fn parse(input: ParseStream) -> Result<Self> {
        let item = input.parse()?;
        let trailing = input.parse()?;
        Ok(Self { item, trailing })
    }
}
