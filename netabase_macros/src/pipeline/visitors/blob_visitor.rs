// @review [x]
#![allow(non_camel_case_types)]
use proc_macro_flow_core::traits::structural::attribute::ParserBuilder;
use proc_macro_flow_core::traits::structural::input::FlowVisitorInput;
use proc_macro_flow_core::traits::structural::visitor::FlowVisitor;
use proc_macro_flow_macros::{AttributeParser, FlowVisitor};
use syn::{Data, DeriveInput, Field, Generics, Ident, Result, Type};

use crate::pipeline::visitors::common::NoopSynVisitor;

#[derive(AttributeParser, Clone, Debug, PartialEq, Eq)]
pub enum BlobStrategy {
    field,
    whole,
}

impl syn::parse::Parse for BlobStrategy {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        use proc_macro_flow_core::traits::structural::attribute::ArgumentParser;
        Self::parse_tokens(input.parse()?)
    }
}

#[derive(AttributeParser, Clone, Debug, PartialEq, Eq)]
pub enum NetabaseBlobContainerAttr {
    strategy(BlobStrategy),
}

impl syn::parse::Parse for NetabaseBlobContainerAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Self::parse_variant(input)
    }
}

#[derive(AttributeParser, Clone, Debug, PartialEq, Eq)]
pub enum NetabaseBlobFieldAttr {
    blobbable,
}

impl syn::parse::Parse for NetabaseBlobFieldAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Self::parse_variant(input)
    }
}


#[derive(FlowVisitor)]
#[flow(input = DeriveInput)]
pub struct NetabaseBlobVisitor<'ast> {
    #[flow(extract = &input.ident)]
    pub ident: &'ast Ident,

    #[flow(extract = &input.generics)]
    pub generics: &'ast Generics,

    #[flow(parse_attr = "blob", default)]
    pub container_attrs: Vec<NetabaseBlobContainerAttr>,

    #[flow(visit(input = &input.data))]
    pub data: NetabaseBlobData<'ast>,
}

pub enum NetabaseBlobData<'ast> {
    Struct {
        fields: Vec<NetabaseBlobFieldVisitor<'ast>>,
    },
    Enum {
        variant_names: Vec<&'ast Ident>,
    },
}

impl<'ast> Clone for NetabaseBlobData<'ast> {
    fn clone(&self) -> Self {
        match self {
            Self::Struct { fields } => Self::Struct {
                fields: fields.clone(),
            },
            Self::Enum { variant_names } => Self::Enum {
                variant_names: variant_names.clone(),
            },
        }
    }
}

impl<'ast> FlowVisitor<'ast> for NetabaseBlobData<'ast> {
    type Input = Data;
    type SynVisitor = NoopSynVisitor;

    fn build(input: &'ast Self::Input) -> Result<Self> {
        match input {
            Data::Struct(s) => {
                let fields = s
                    .fields
                    .iter()
                    .map(NetabaseBlobFieldVisitor::build)
                    .collect::<Result<Vec<_>>>()?;
                Ok(Self::Struct { fields })
            }
            Data::Enum(e) => Ok(Self::Enum {
                variant_names: e.variants.iter().map(|v| &v.ident).collect(),
            }),
            _ => Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "Only structs and enums are supported",
            )),
        }
    }
}

impl<'ast> FlowVisitorInput for NetabaseBlobData<'ast> {}

#[derive(FlowVisitor, Clone)]
#[flow(input = Field)]
pub struct NetabaseBlobFieldVisitor<'ast> {
    #[flow(extract = input.ident.as_ref().expect("NetabaseBlob requires named fields"))]
    pub ident: &'ast Ident,

    #[flow(extract = &input.ty)]
    pub ty: &'ast Type,

    #[flow(parse_attr = "blob", default)]
    pub attrs: Vec<NetabaseBlobFieldAttr>,

    #[flow(extract = input)]
    pub field: &'ast Field,
}
