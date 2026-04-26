// @review [ ]
use proc_macro_flow_core::traits::structural::generator::FlowGeneratorInput;
use proc_macro_flow_core::traits::structural::planner::FlowPlanner;
use proc_macro_flow_core::traits::structural::validation::FlowValidate;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Ident, ItemMod, Result, Token};

use crate::pipeline::visitors::{NetabaseRepositoryAttr, NetabaseRepositoryVisitor};

pub struct NetabaseRepositoryPlan<'ast> {
    pub ident: Ident,
    pub mod_ident: &'ast Ident,
    pub subscriptions: Vec<Ident>,
    pub definitions: Vec<Ident>,
    /// Model struct names within each definition, in the same order as `definitions`.
    pub models_per_definition: Vec<Vec<Ident>>,
    pub storage_mode: String,
    pub item: &'ast ItemMod,
    pub trailing: proc_macro2::TokenStream,
}

impl<'ast> FlowGeneratorInput for NetabaseRepositoryPlan<'ast> {}

impl<'ast> FlowPlanner for NetabaseRepositoryPlan<'ast> {
    type Input = NetabaseRepositoryVisitor<'ast>;

    fn plan(input: Self::Input) -> syn::Result<Self> {
        let ident = input
            .attrs
            .iter()
            .find_map(|a| {
                if let NetabaseRepositoryAttr::name(id) = a {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| syn::Error::new(input.mod_ident.span(), "Missing repository name"))?;

        let subscriptions = input
            .attrs
            .iter()
            .filter_map(|a| {
                if let NetabaseRepositoryAttr::subscriptions(list) = a {
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
                if let NetabaseRepositoryAttr::storage(mode) = a {
                    Some(mode.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "sharded".to_string());

        let mut definitions = Vec::new();
        let mut models_per_definition: Vec<Vec<Ident>> = Vec::new();
        if let Some((_, items)) = &input.item.content {
            for item in items {
                if let syn::Item::Mod(m) = item {
                    for attr in &m.attrs {
                        if attr.path().is_ident("netabase_definition") {
                            let mut def_name = None;
                            let mut registers_to_us = false;

                            let _ = attr.parse_nested_meta(|meta| {
                                if def_name.is_none() {
                                    def_name = Some(meta.path.get_ident().unwrap().clone());
                                }
                                if meta.path.is_ident("repository") {
                                    if meta.input.peek(syn::token::Paren) {
                                        let content;
                                        syn::parenthesized!(content in meta.input);
                                        let list =
                                            Punctuated::<Ident, Token![,]>::parse_terminated(
                                                &content,
                                            )
                                            .ok();
                                        if let Some(list) = list
                                            && list.iter().any(|id| id == &ident) {
                                                registers_to_us = true;
                                            }
                                    } else {
                                        let target: Ident = meta.value()?.parse()?;
                                        if target == ident {
                                            registers_to_us = true;
                                        }
                                    }
                                }
                                Ok(())
                            });

                            if registers_to_us
                                && let Some(name) = def_name
                                    && !definitions.contains(&name) {
                                        // Collect model names from within this definition module.
                                        let mut model_names = Vec::new();
                                        if let Some((_, def_items)) = &m.content {
                                            for def_item in def_items {
                                                if let syn::Item::Struct(s) = def_item {
                                                    let has_model_attr = s.attrs.iter().any(|a| {
                                                        a.path().is_ident("netabase_model")
                                                    });
                                                    if has_model_attr {
                                                        model_names.push(s.ident.clone());
                                                    }
                                                }
                                            }
                                        }
                                        definitions.push(name);
                                        models_per_definition.push(model_names);
                                    }
                        }
                    }
                }
            }
        }

        Ok(Self {
            ident,
            mod_ident: input.mod_ident,
            subscriptions,
            definitions,
            models_per_definition,
            storage_mode,
            item: input.item,
            trailing: input.trailing,
        })
    }
}

impl<'ast> FlowValidate for NetabaseRepositoryPlan<'ast> {
    type Error = syn::Error;

    fn validate(&self) -> std::result::Result<(), Vec<Self::Error>> {
        let mut errors = Vec::new();

        // 1. Collect all repositories and their subscriptions from the parent module's attributes
        let mut available_repos = std::collections::HashMap::new();
        for attr in &self.item.attrs {
            let is_repo = attr.path().is_ident("netabase_repository");
            let is_internal = attr
                .path()
                .segments
                .last()
                .map(|s| s.ident == "netabase_internal_repo")
                .unwrap_or(false);

            if is_repo || is_internal {
                let list: syn::Result<crate::pipeline::visitors::NetabaseRepositoryAttrList> =
                    attr.parse_args();
                if let Ok(list) = list {
                    let mut name = None;
                    let mut subs = Vec::new();
                    for a in list.0 {
                        match a {
                            NetabaseRepositoryAttr::name(id) => name = Some(id),
                            NetabaseRepositoryAttr::subscriptions(l) => subs.extend(l.0),
                            NetabaseRepositoryAttr::storage(_) => {}
                        }
                    }
                    if let Some(name) = name {
                        available_repos.insert(name.to_string(), subs);
                    }
                }
            }
        }

        // Also add ourselves if we are not in the attributes (though we should be)
        available_repos.entry(self.ident.to_string()).or_insert_with(|| self.subscriptions.clone());

        // 2. Scan inner definitions and verify their declarations
        if let Some((_, items)) = &self.item.content {
            for item in items {
                if let syn::Item::Mod(m) = item {
                    for attr in &m.attrs {
                        if attr.path().is_ident("netabase_definition") {
                            let list: Result<
                                crate::pipeline::visitors::NetabaseDefinitionAttrList,
                            > = attr.parse_args();
                            match list {
                                Ok(list) => {
                                    let mut def_name = None;
                                    let mut def_repos = Vec::new();
                                    let mut def_subscribes = Vec::new();

                                    for a in list.0 {
                                        use crate::pipeline::visitors::NetabaseDefinitionAttr;
                                        match a {
                                            NetabaseDefinitionAttr::name(id) => def_name = Some(id),
                                            NetabaseDefinitionAttr::repository(l) => {
                                                def_repos.extend(l.0)
                                            }
                                            NetabaseDefinitionAttr::subscribe(l) => {
                                                def_subscribes.extend(l.0)
                                            }
                                            _ => {}
                                        }
                                    }

                                    let def_name_str = def_name
                                        .map(|id| id.to_string())
                                        .unwrap_or_else(|| "unknown".into());

                                    // Verify repositories
                                    for repo in def_repos {
                                        if !available_repos.contains_key(&repo.to_string()) {
                                            errors.push(syn::Error::new(
                                                repo.span(),
                                                format!(
                                                    "Definition '{}' registers to unknown repository '{}'. Available: {}",
                                                    def_name_str,
                                                    repo,
                                                    available_repos.keys().cloned().collect::<Vec<_>>().join(", ")
                                                )
                                            ));
                                        }
                                    }

                                    // Verify subscriptions
                                    for path in def_subscribes {
                                        if path.segments.len() == 2 {
                                            let repo = &path.segments[0].ident;
                                            let sub = &path.segments[1].ident;
                                            if let Some(repo_subs) =
                                                available_repos.get(&repo.to_string())
                                            {
                                                if !repo_subs.iter().any(|s| s == sub) {
                                                    errors.push(syn::Error::new(
                                                        sub.span(),
                                                        format!(
                                                            "Definition '{}' subscribes to unknown subscription '{}' on repository '{}'. Available: {}",
                                                            def_name_str,
                                                            sub,
                                                            repo,
                                                            repo_subs.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ")
                                                        )
                                                    ));
                                                }
                                            } else {
                                                errors.push(syn::Error::new(
                                                    repo.span(),
                                                    format!(
                                                        "Definition '{}' subscribes to repository '{}' which is not registered for this definition or doesn't exist in this scope.",
                                                        def_name_str,
                                                        repo
                                                    )
                                                ));
                                            }
                                        } else {
                                            errors.push(syn::Error::new(
                                                path.span(),
                                                "Subscription path must be in the form 'Repository::Subscription'"
                                            ));
                                        }
                                    }
                                }
                                Err(e) => errors.push(e),
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
