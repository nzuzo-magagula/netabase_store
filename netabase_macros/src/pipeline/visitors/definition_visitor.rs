// @review [x]
#![allow(non_camel_case_types)]
use crate::pipeline::visitors::common::{IdentList, ModWithTrailing, NoopSynVisitor, PathList};
use proc_macro_flow_core::traits::structural::input::FlowVisitorInput;
use proc_macro_flow_core::traits::structural::validation::FlowValidate;
use proc_macro_flow_core::traits::structural::visitor::FlowVisitor;
use proc_macro_flow_macros::AttributeParser;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, ItemMod, Result, Token};

#[derive(AttributeParser, Clone, Debug, PartialEq, Eq)]
pub enum NetabaseDefinitionAttr {
    name(Ident),
    repository(IdentList),
    subscriptions(IdentList),
    subscribe(PathList),
    storage(Ident),
}

impl syn::parse::Parse for NetabaseDefinitionAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let ident: Ident = input.parse()?;
        let s = ident.to_string();
        if input.peek(syn::token::Paren) {
            let content;
            syn::parenthesized!(content in input);
            match s.as_str() {
                "repository" => Ok(Self::repository(content.parse()?)),
                "subscriptions" => Ok(Self::subscriptions(content.parse()?)),
                "subscribe" => Ok(Self::subscribe(content.parse()?)),
                "storage" => Ok(Self::storage(content.parse()?)),
                _ => Err(syn::Error::new(ident.span(), format!("Unknown NetabaseDefinitionAttr: {}", s))),
            }
        } else {
            Ok(Self::name(ident))
        }
    }
}

pub struct NetabaseDefinitionAttrList(pub Vec<NetabaseDefinitionAttr>);

impl Parse for NetabaseDefinitionAttrList {
    fn parse(input: ParseStream) -> Result<Self> {
        let punctuated = Punctuated::<NetabaseDefinitionAttr, Token![,]>::parse_terminated(input)?;
        Ok(Self(punctuated.into_iter().collect()))
    }
}

pub struct NetabaseDefinitionVisitor<'ast> {
    pub mod_ident: &'ast Ident,
    pub attrs: Vec<NetabaseDefinitionAttr>,
    pub item: &'ast ItemMod,
    pub trailing: proc_macro2::TokenStream,
}

impl<'ast> FlowVisitor<'ast> for NetabaseDefinitionVisitor<'ast> {
    type Input = ModWithTrailing;
    type SynVisitor = NoopSynVisitor;

    fn build(input: &'ast Self::Input) -> Result<Self> {
        Ok(Self {
            mod_ident: &input.item.ident,
            attrs: Vec::new(), // Do not collect from the item itself
            item: &input.item,
            trailing: input.trailing.clone(),
        })
    }
}

impl<'ast> FlowVisitorInput for NetabaseDefinitionVisitor<'ast> {}

impl<'ast> FlowValidate for NetabaseDefinitionVisitor<'ast> {
    type Error = syn::Error;

    fn validate(&self) -> std::result::Result<(), Vec<Self::Error>> {
        Ok(())
    }
}
