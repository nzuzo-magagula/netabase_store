// @review [x]
use proc_macro_flow_core::traits::structural::generator::FlowGeneratorInput;
use proc_macro_flow_core::traits::structural::planner::FlowPlanner;
use proc_macro_flow_core::traits::structural::validation::FlowValidate;
use syn::{Generics, Ident, Type};

use crate::pipeline::visitors::{
    NetabaseContainerAttr, NetabaseFieldAttr, NetabaseFieldVisitor, NetabaseModelData,
    NetabaseModelVisitor,
};

pub struct NetabaseModelPlan {
    pub ident: Ident,
    pub generics: Generics,
    pub definition: Option<Ident>,
    pub redb_mode: String,
    pub storage_mode: String,
    /// Pure-shard dedup: strip blob + relational fields from the Primary record (rehydrated on read).
    pub pure: bool,
    pub subscription_keys: Vec<syn::Ident>,
    pub subscribe_to: Vec<syn::Path>,
    pub custom_tables: Vec<CustomTablePlan>,
    pub is_attribute_macro: bool,
    pub blob_strategy: crate::pipeline::visitors::blob_visitor::BlobStrategy,
    pub hash_fn: Option<syn::ExprPath>,
    pub key_fn: Option<syn::ExprPath>,
    pub external_pk_ty: Option<syn::Type>,
    pub data: NetabaseModelDataPlan,
    pub prev_version: Option<syn::Type>,
    pub version_number: Option<u8>,
    /// Arena-store record budget from `#[netabase(capacity = N)]`.
    pub arena_capacity: Option<usize>,
}

pub struct CustomTablePlan {
    pub name: Ident,
    pub key_ty: Type,
    pub value_ty: Type,
}

impl FlowGeneratorInput for NetabaseModelPlan {}

impl FlowPlanner for NetabaseModelPlan {
    type Input = NetabaseModelVisitor;

    fn plan(input: Self::Input) -> syn::Result<Self> {
        let skip_redb_attr_count = input
            .container_attrs
            .iter()
            .filter(|a| matches!(a, NetabaseContainerAttr::skip_redb))
            .count();

        let pure = input
            .container_attrs
            .iter()
            .any(|a| matches!(a, NetabaseContainerAttr::pure));

        let data = match input.data {
            NetabaseModelData::Struct { fields } => NetabaseModelDataPlan::Struct {
                fields: fields.into_iter().map(field_to_plan).collect(),
            },
            NetabaseModelData::Enum { .. } => NetabaseModelDataPlan::Enum,
            NetabaseModelData::Union => NetabaseModelDataPlan::Union,
        };

        let has_any_blob_field = if let NetabaseModelDataPlan::Struct { fields } = &data {
            fields.iter().any(|f| f.is_blob)
        } else {
            false
        };

        let redb_mode = if skip_redb_attr_count > 0 {
            "skip".to_string()
        } else {
            input
                .container_attrs
                .iter()
                .find_map(|a| {
                    if let NetabaseContainerAttr::redb(mode) = a {
                        Some(mode.value())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "auto".to_string())
        };

        let storage_mode = input
            .container_attrs
            .iter()
            .find_map(|a| {
                if let NetabaseContainerAttr::storage(mode) = a {
                    Some(mode.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "sharded".to_string());

        let subscription_keys = input
            .container_attrs
            .iter()
            .find_map(|a| {
                if let NetabaseContainerAttr::subscriptions(list) = a {
                    Some(list.0.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let subscribe_to = input
            .container_attrs
            .iter()
            .filter_map(|a| {
                if let NetabaseContainerAttr::subscribe(list) = a {
                    Some(list.0.clone())
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        let custom_tables = input
            .container_attrs
            .iter()
            .filter_map(|a| {
                if let NetabaseContainerAttr::custom_table {
                    name,
                    key_ty,
                    value_ty,
                } = a
                {
                    Some(CustomTablePlan {
                        name: name.clone(),
                        key_ty: key_ty.clone(),
                        value_ty: value_ty.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        let blob_strategy = input
            .container_attrs
            .iter()
            .find_map(|a| {
                if let NetabaseContainerAttr::blob(strategy) = a {
                    Some(strategy.clone())
                } else {
                    None
                }
            })
            .unwrap_or({
                if has_any_blob_field {
                    crate::pipeline::visitors::blob_visitor::BlobStrategy::field
                } else {
                    crate::pipeline::visitors::blob_visitor::BlobStrategy::whole
                }
            });

        let definition = input.container_attrs.iter().find_map(|a| {
            if let NetabaseContainerAttr::definition(id) = a {
                Some(id.clone())
            } else {
                None
            }
        });

        let hash_fn = input.container_attrs.iter().find_map(|a| {
            if let NetabaseContainerAttr::hash_fn(path) = a {
                Some(path.clone())
            } else {
                None
            }
        });

        let key_fn = input.container_attrs.iter().find_map(|a| {
            if let NetabaseContainerAttr::key_fn(path) = a {
                Some(path.clone())
            } else {
                None
            }
        });

        let external_pk_ty = input.container_attrs.iter().find_map(|a| {
            if let NetabaseContainerAttr::primary_key_type(ty) = a {
                Some(ty.clone())
            } else {
                None
            }
        });

        let is_attribute_macro = input
            .container_attrs
            .iter()
            .any(|a| matches!(a, NetabaseContainerAttr::internal_attribute_macro));

        let prev_version = input.container_attrs.iter().find_map(|a| {
            if let NetabaseContainerAttr::version { prev, .. } = a {
                prev.clone()
            } else {
                None
            }
        });

        let version_number = input.container_attrs.iter().find_map(|a| {
            if let NetabaseContainerAttr::version { number, .. } = a {
                number.as_ref().and_then(|n| n.base10_parse::<u8>().ok())
            } else {
                None
            }
        });

        let arena_capacity = input.container_attrs.iter().find_map(|a| {
            if let NetabaseContainerAttr::capacity(n) = a {
                n.base10_parse::<usize>().ok()
            } else {
                None
            }
        });

        Ok(Self {
            ident: input.ident,
            generics: input.generics,
            definition,
            redb_mode,
            storage_mode,
            pure,
            subscription_keys,
            subscribe_to,
            custom_tables,
            is_attribute_macro,
            blob_strategy,
            hash_fn,
            key_fn,
            external_pk_ty,
            data,
            prev_version,
            version_number,
            arena_capacity,
        })
    }
}

impl FlowValidate for NetabaseModelPlan {
    type Error = syn::Error;

    fn validate(&self) -> std::result::Result<(), Vec<Self::Error>> {
        let mut errors = Vec::new();

        let has_internal_pk = if let NetabaseModelDataPlan::Struct { fields } = &self.data {
            fields.iter().any(|f| f.is_primary)
        } else {
            false
        };

        if !has_internal_pk {
            if self.external_pk_ty.is_none() {
                errors.push(syn::Error::new(
                    self.ident.span(),
                    "Model must have either a primary key field or a primary_key_type attribute",
                ));
            }
            if self.hash_fn.is_none() && self.key_fn.is_none() {
                errors.push(syn::Error::new(
                    self.ident.span(),
                    "External key models require either hash_fn or key_fn to extract the key",
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

pub enum NetabaseModelDataPlan {
    Struct {
        fields: Vec<NetabaseFieldPlan>,
    },
    Enum,
    Union,
}

impl FlowGeneratorInput for NetabaseModelDataPlan {}

pub struct RelationalPlan {
    pub to: syn::Path,
    pub repo: Option<syn::Path>,
    pub def: Option<syn::Path>,
}

pub struct NetabaseFieldPlan {
    pub ident: Option<Ident>,
    pub ty: Type,
    pub is_primary: bool,
    pub is_secondary: bool,
    pub relational_to: Option<RelationalPlan>,
    pub is_blob: bool,
}

impl FlowGeneratorInput for NetabaseFieldPlan {}

fn field_to_plan(field: NetabaseFieldVisitor) -> NetabaseFieldPlan {
    NetabaseFieldPlan {
        ident: Some(field.ident.clone()),
        ty: field.ty,
        is_primary: field.attrs.iter().any(|a| {
            matches!(
                a,
                NetabaseFieldAttr::PrimaryKey | NetabaseFieldAttr::primary_key
            )
        }),
        is_secondary: field.attrs.iter().any(|a| {
            matches!(
                a,
                NetabaseFieldAttr::secondary | NetabaseFieldAttr::secondary_key
            )
        }),
        relational_to: field.attrs.iter().find_map(|a| match a {
            NetabaseFieldAttr::relational { to, repo, def } => Some(RelationalPlan {
                to: to.clone(),
                repo: repo.clone(),
                def: def.clone(),
            }),
            NetabaseFieldAttr::relational_key { to, repo, def } => Some(RelationalPlan {
                to: to.clone(),
                repo: repo.clone(),
                def: def.clone(),
            }),
            _ => None,
        }),
        is_blob: field
            .attrs
            .iter()
            .any(|a| matches!(a, NetabaseFieldAttr::blob)),
    }
}
