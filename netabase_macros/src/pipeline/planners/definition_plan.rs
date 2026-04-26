// @review [x]
use proc_macro_flow_core::traits::structural::generator::FlowGeneratorInput;
use proc_macro_flow_core::traits::structural::planner::FlowPlanner;
use proc_macro_flow_core::traits::structural::validation::FlowValidate;
use syn::punctuated::Punctuated;
use syn::{Ident, ItemMod, Token};

use crate::pipeline::visitors::{NetabaseDefinitionAttr, NetabaseDefinitionVisitor};

pub struct NetabaseDefinitionPlan<'ast> {
    pub ident: Ident,
    pub mod_ident: &'ast Ident,
    pub repositories: Vec<Ident>,
    pub storage_mode: String,
    pub subscribe_to: Vec<syn::Path>,
    pub subscriptions: Vec<Ident>,
    pub models: Vec<Ident>,
    pub item: &'ast ItemMod,
    pub trailing: proc_macro2::TokenStream,
}

impl<'ast> FlowGeneratorInput for NetabaseDefinitionPlan<'ast> {}

impl<'ast> FlowPlanner for NetabaseDefinitionPlan<'ast> {
    type Input = NetabaseDefinitionVisitor<'ast>;

    fn plan(input: Self::Input) -> syn::Result<Self> {
        let ident = input
            .attrs
            .iter()
            .find_map(|a| {
                if let NetabaseDefinitionAttr::name(id) = a {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| syn::Error::new(input.mod_ident.span(), "Missing definition name"))?;

        let repositories = input
            .attrs
            .iter()
            .filter_map(|a| {
                if let NetabaseDefinitionAttr::repository(list) = a {
                    Some(list.0.clone())
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        let storage_mode = input
            .attrs
            .iter()
            .find_map(|a| {
                if let NetabaseDefinitionAttr::storage(mode) = a {
                    Some(mode.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "sharded".to_string());

        let subscribe_to = input
            .attrs
            .iter()
            .filter_map(|a| {
                if let NetabaseDefinitionAttr::subscribe(list) = a {
                    Some(list.0.clone())
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        let subscriptions = input
            .attrs
            .iter()
            .filter_map(|a| {
                if let NetabaseDefinitionAttr::subscriptions(list) = a {
                    Some(list.0.clone())
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        let mut models = Vec::new();
        if let Some((_, items)) = &input.item.content {
            for item in items {
                let (item_ident, item_attrs) = match item {
                    syn::Item::Struct(s) => (&s.ident, &s.attrs),
                    syn::Item::Enum(e) => (&e.ident, &e.attrs),
                    _ => continue,
                };

                let registers_to_us = item_attrs.iter().any(|attr| {
                    if attr.path().is_ident("netabase") {
                        let mut found = false;
                        let _ = attr.parse_nested_meta(|meta| {
                            if meta.path.is_ident("definition") {
                                if meta.input.peek(syn::token::Paren) {
                                    let content;
                                    syn::parenthesized!(content in meta.input);
                                    let list =
                                        Punctuated::<Ident, Token![,]>::parse_terminated(&content)
                                            .ok();
                                    if let Some(list) = list
                                        && list.iter().any(|id| id == &ident) {
                                            found = true;
                                        }
                                } else {
                                    let target: Ident = meta.value()?.parse()?;
                                    if target == ident {
                                        found = true;
                                    }
                                }
                            }
                            Ok(())
                        });
                        found
                    } else {
                        false
                    }
                });

                if registers_to_us {
                    models.push(item_ident.clone());
                }
            }
        }

        Ok(Self {
            ident,
            mod_ident: input.mod_ident,
            repositories,
            storage_mode,
            subscribe_to,
            subscriptions,
            models,
            item: input.item,
            trailing: input.trailing,
        })
    }
}

impl<'ast> FlowValidate for NetabaseDefinitionPlan<'ast> {
    type Error = syn::Error;

    fn validate(&self) -> std::result::Result<(), Vec<Self::Error>> {
        let errors = Vec::new();

        if let Some((_, items)) = &self.item.content {
            for item in items {
                let (_item_ident, item_attrs) = match item {
                    syn::Item::Struct(s) => (&s.ident, &s.attrs),
                    syn::Item::Enum(e) => (&e.ident, &e.attrs),
                    _ => continue,
                };

                for attr in item_attrs {
                    if attr.path().is_ident("netabase") {
                        let mut registered_definitions = Vec::new();
                        let _ = attr.parse_nested_meta(|meta| {
                            if meta.path.is_ident("definition") {
                                if meta.input.peek(syn::token::Paren) {
                                    let content;
                                    syn::parenthesized!(content in meta.input);
                                    let list =
                                        Punctuated::<Ident, Token![,]>::parse_terminated(&content)
                                            .ok();
                                    if let Some(list) = list {
                                        registered_definitions.extend(list);
                                    }
                                } else {
                                    let target: Ident = meta.value()?.parse()?;
                                    registered_definitions.push(target);
                                }
                            }
                            Ok(())
                        });

                        for def in registered_definitions {
                            if def != self.ident {
                                // Handled by other definitions
                            }
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}
