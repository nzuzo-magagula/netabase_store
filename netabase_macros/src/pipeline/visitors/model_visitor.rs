// @review [ ]
use crate::pipeline::visitors::common::{IdentList, NoopSynVisitor, PathList};
use proc_macro_flow_core::traits::structural::attribute::ParserBuilder;
use proc_macro_flow_core::traits::structural::input::FlowVisitorInput;
use proc_macro_flow_core::traits::structural::visitor::FlowVisitorMut;
use proc_macro_flow_macros::FlowVisitMut;
use syn::parse::ParseStream;
use syn::{Data, DeriveInput, Field, Ident, LitStr, Result, Token, Type, LitInt};

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum NetabaseContainerAttr {
    redb(LitStr),
    skip_redb,
    /// Pure-shard dedup: the Primary record omits data the auxiliary tables already hold
    /// (blob + relational fields), which are rehydrated on read.
    pure,
    subscribe(PathList),
    subscriptions(IdentList),
    definition(Ident),
    storage(syn::Ident),
    custom_table {
        name: Ident,
        key_ty: Type,
        value_ty: Type,
    },
    blob(crate::pipeline::visitors::blob_visitor::BlobStrategy),
    hash_fn(syn::ExprPath),
    key_fn(syn::ExprPath),
    primary_key_type(Type),
    internal_attribute_macro,
    version { prev: Option<Type>, number: Option<LitInt> },
    /// `#[netabase(capacity = N)]`: arena-store record budget for this model.
    capacity(LitInt),
}

impl ParserBuilder for NetabaseContainerAttr {
    fn parse_variant(input: ParseStream) -> Result<Self> {
        let ident: Ident = input.parse()?;
        let s = ident.to_string();
        if input.peek(Token![=]) {
            let _: Token![=] = input.parse()?;
            match s.as_str() {
                "redb" => Ok(Self::redb(input.parse()?)),
                "definition" => Ok(Self::definition(input.parse()?)),
                "storage" => Ok(Self::storage(input.parse()?)),
                "hash_fn" => Ok(Self::hash_fn(input.parse()?)),
                "key_fn" => Ok(Self::key_fn(input.parse()?)),
                "primary_key_type" => Ok(Self::primary_key_type(input.parse()?)),
                "capacity" => Ok(Self::capacity(input.parse()?)),
                _ => Err(syn::Error::new(
                    ident.span(),
                    format!("Unknown NetabaseContainerAttr with '=': {}", s),
                )),
            }
        } else if input.peek(syn::token::Paren) {
            let content;
            syn::parenthesized!(content in input);
            match s.as_str() {
                "redb" => Ok(Self::redb(content.parse()?)),
                "subscribe" => Ok(Self::subscribe(content.parse()?)),
                "subscriptions" => Ok(Self::subscriptions(content.parse()?)),
                "definition" => Ok(Self::definition(content.parse()?)),
                "storage" => Ok(Self::storage(content.parse()?)),
                "hash_fn" => Ok(Self::hash_fn(content.parse()?)),
                "key_fn" => Ok(Self::key_fn(content.parse()?)),
                "primary_key_type" => Ok(Self::primary_key_type(content.parse()?)),
                "blob" => {
                    let lookahead = content.lookahead1();
                    if lookahead.peek(Ident) {
                        let inner_ident: Ident = content.parse()?;
                        if inner_ident == "strategy" {
                            content.parse::<Token![=]>()?;
                            let strategy: crate::pipeline::visitors::blob_visitor::BlobStrategy = content.parse()?;
                            Ok(Self::blob(strategy))
                        } else {
                            Err(syn::Error::new(inner_ident.span(), "Expected `strategy`"))
                        }
                    } else {
                        Err(lookahead.error())
                    }
                }
                "custom_table" => {
                    let name: Ident = content.parse()?;
                    content.parse::<Token![,]>()?;
                    let key_ty: Type = content.parse()?;
                    content.parse::<Token![,]>()?;
                    let value_ty: Type = content.parse()?;
                    Ok(Self::custom_table {
                        name,
                        key_ty,
                        value_ty,
                    })
                }
                "version" => {
                    let mut prev: Option<Type> = None;
                    let mut number: Option<LitInt> = None;
                    while !content.is_empty() {
                        let key: Ident = content.parse()?;
                        content.parse::<Token![=]>()?;
                        match key.to_string().as_str() {
                            "prev" => prev = Some(content.parse()?),
                            "number" => number = Some(content.parse()?),
                            _ => return Err(syn::Error::new(key.span(), format!("Unknown version argument: {}", key))),
                        }
                        if content.peek(Token![,]) {
                            content.parse::<Token![,]>()?;
                        } else {
                            break;
                        }
                    }
                    Ok(Self::version { prev, number })
                }
                _ => Err(syn::Error::new(
                    ident.span(),
                    format!("Unknown NetabaseContainerAttr: {}", s),
                )),
            }
        } else {
            match s.as_str() {
                "skip_redb" => Ok(Self::skip_redb),
                "pure" => Ok(Self::pure),
                "internal_attribute_macro" => Ok(Self::internal_attribute_macro),
                _ => Err(syn::Error::new(
                    ident.span(),
                    format!("Unknown NetabaseContainerAttr: {}", s),
                )),
            }
        }
    }
}

impl syn::parse::Parse for NetabaseContainerAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Self::parse_variant(input)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum NetabaseFieldAttr {
    PrimaryKey,
    primary_key,
    secondary,
    secondary_key,
    relational { to: syn::Path, repo: Option<syn::Path>, def: Option<syn::Path> },
    relational_key { to: syn::Path, repo: Option<syn::Path>, def: Option<syn::Path> },
    blob,
}
// Note: subscriptions are a container-level concept (the `subscriptions(...)` attribute on a
// model/definition/repository), not a field-level one. Earlier `subscription` /
// `subscription_key` field attributes existed but were never wired into codegen; they have been
// removed so a field that uses them is a hard error rather than a silent no-op.

impl ParserBuilder for NetabaseFieldAttr {
    fn parse_variant(input: ParseStream) -> Result<Self> {
        let ident: Ident = input.parse()?;
        let s = ident.to_string();
        if input.peek(syn::token::Paren) {
            let content;
            syn::parenthesized!(content in input);
            match s.as_str() {
                "relational" | "relational_key" => {
                    let mut to = None;
                    let mut repo = None;
                    let mut def = None;

                    if content.peek(Ident) && content.peek2(Token![=]) {
                        while !content.is_empty() {
                            let arg_key: Ident = content.parse()?;
                            content.parse::<Token![=]>()?;
                            match arg_key.to_string().as_str() {
                                "to" => to = Some(content.parse()?),
                                "repo" | "repository" => repo = Some(content.parse()?),
                                "def" | "definition" => def = Some(content.parse()?),
                                _ => return Err(syn::Error::new(arg_key.span(), format!("Unknown relational argument: {}", arg_key))),
                            }
                            if content.peek(Token![,]) {
                                content.parse::<Token![,]>()?;
                            } else {
                                break;
                            }
                        }
                    } else if !content.is_empty() {
                        to = Some(content.parse()?);
                    }

                    let to = to.ok_or_else(|| syn::Error::new(ident.span(), "relational attribute requires at least a target model"))?;
                    if s == "relational" {
                        Ok(Self::relational { to, repo, def })
                    } else {
                        Ok(Self::relational_key { to, repo, def })
                    }
                }
                _ => Err(syn::Error::new(
                    ident.span(),
                    format!("Unknown NetabaseFieldAttr with (...): {}", s),
                )),
            }
        } else {
            match s.as_str() {
                "PrimaryKey" => Ok(Self::PrimaryKey),
                "primary_key" => Ok(Self::primary_key),
                "secondary" => Ok(Self::secondary),
                "secondary_key" => Ok(Self::secondary_key),
                "blob" => Ok(Self::blob),
                _ => Err(syn::Error::new(
                    ident.span(),
                    format!("Unknown NetabaseFieldAttr: {}", s),
                )),
            }
        }
    }
}

impl syn::parse::Parse for NetabaseFieldAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Self::parse_variant(input)
    }
}

#[derive(FlowVisitMut)]
#[flow(input = DeriveInput)]
pub struct NetabaseModelVisitor {
    #[flow(extract = input.ident.clone())]
    pub ident: Ident,

    #[flow(extract = input.generics.clone())]
    pub generics: syn::Generics,

    #[flow(parse_attr = "netabase", default)]
    pub container_attrs: Vec<NetabaseContainerAttr>,

    #[flow(visit(input = &mut input.data))]
    pub data: NetabaseModelData,
}


#[derive(Clone)]
pub enum NetabaseModelData {
    Struct {
        fields: Vec<NetabaseFieldVisitor>,
    },
    Enum {
        variants: Vec<NetabaseVariantVisitor>,
    },
    Union,
}

impl FlowVisitorInput for NetabaseModelData {}

impl FlowVisitorMut for NetabaseModelData {
    type Input = Data;
    type SynVisitor = NoopSynVisitor;

    fn build_mut(input: &mut Self::Input) -> Result<Self> {
        match input {
            Data::Struct(s) => {
                let fields = s
                    .fields
                    .iter_mut()
                    .map(NetabaseFieldVisitor::build_mut)
                    .collect::<Result<Vec<_>>>()?;
                Ok(Self::Struct { fields })
            }
            Data::Enum(e) => {
                let variants = e
                    .variants
                    .iter_mut()
                    .map(NetabaseVariantVisitor::build_mut)
                    .collect::<Result<Vec<_>>>()?;
                Ok(Self::Enum { variants })
            }
            Data::Union(_) => Ok(Self::Union),
        }
    }
}

#[derive(FlowVisitMut, Clone)]
#[flow(input = syn::Variant)]
pub struct NetabaseVariantVisitor {
    #[flow(extract = input.ident.clone())]
    pub ident: Ident,

    #[flow(parse_attr = "netabase", default)]
    pub attrs: Vec<NetabaseFieldAttr>,
}

#[derive(FlowVisitMut, Clone)]
#[flow(input = Field)]
pub struct NetabaseFieldVisitor {
    #[flow(extract = input.ident.as_ref().expect("NetabaseModel requires named fields").clone())]
    pub ident: Ident,

    #[flow(extract = input.ty.clone())]
    pub ty: Type,

    #[flow(parse_attr = "netabase", default)]
    pub attrs: Vec<NetabaseFieldAttr>,

    #[flow(extract = input.clone())]
    pub field: Field,
}
