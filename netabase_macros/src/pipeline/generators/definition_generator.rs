// @review [x]
use crate::pipeline::planners::NetabaseDefinitionPlan;
use crate::pipeline::generators::utils::key_enum_ordered_encoding;
use proc_macro_flow_core::traits::structural::generator::FlowGenerator;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::marker::PhantomData;

pub struct NetabaseDefinitionGenerator<'ast>(pub PhantomData<&'ast ()>);

impl<'ast> FlowGenerator for NetabaseDefinitionGenerator<'ast> {
    type Input = NetabaseDefinitionPlan<'ast>;
    type Output = TokenStream;
    type SynNode = syn::ItemMod;

    fn generate(input: &Self::Input) -> syn::Result<Self::Output> {
        let item = input.item;
        let trailing = &input.trailing;
        let ident = &input.ident;
        let mod_ident = &input.mod_ident;
        let address_ident = format_ident!("{}Address", ident);
        let keys_ident = format_ident!("{}Keys", ident);
        let tables_ident = format_ident!("{}Tables", ident);

        let models = &input.models;
        let repositories = &input.repositories;
        let def_subscriptions = &input.subscriptions;

        // NOTE: `subscribe(...)` on a definition is a declaration only. Writing into another scope's
        // subscription is *gated* by the wrapping enum (a definition can only register into a
        // repository's subscription by being inserted as `Repository::Definition(def)`), so no
        // child→parent write is emitted here. Own-topic writes are handled below.

        let pk_enum_ident = format_ident!("{}PrimaryKey", ident);
        let secondary_keys_ident = format_ident!("{}SecondaryKeys", ident);
        let relational_keys_ident = format_ident!("{}RelationalKeys", ident);
        let blob_keys_ident = format_ident!("{}BlobKeys", ident);
        let sub_routing_ident = format_ident!("{}SubscriptionKeys", ident);
        let sub_discrim_ident = format_ident!("{}Subscriptions", ident);
        let sub_registry_ident = format_ident!("{}SubscriptionRegistry", ident);
        let table_name_ident = format_ident!("{}TableName", ident);
        let model_table_name_idents: Vec<_> = models.iter().map(|m| format_ident!("{}TableName", m)).collect();

        // `{Definition}Subscriptions` is a path enum: own-topic ZST leaves PLUS one nested
        // `Model({Model}Subscriptions)` variant per child model, so the discriminator reflects the
        // full subscription tree (fixing the prior own-topics-only asymmetry vs the routing enum).
        let def_sub_table_names: Vec<String> = def_subscriptions
            .iter()
            .map(|s| format!("{}_{}", ident, s))
            .collect();
        let def_sub_topic_leaf_idents: Vec<_> = def_subscriptions
            .iter()
            .map(|s| format_ident!("{}{}SubLeaf", ident, s))
            .collect();
        let model_sub_discrim_idents: Vec<_> = models
            .iter()
            .map(|m| format_ident!("{}Subscriptions", m))
            .collect();
        let def_sub_none_leaf_ident = format_ident!("{}NoneSubLeaf", ident);
        let (def_sub_leaf_defs, sub_discrim_path_variants) =
            if def_subscriptions.is_empty() && models.is_empty() {
                // No own topics and no child models: keep the discriminator inhabited with a single
                // ZST `__None` leaf so it is still a valid (non-empty) path enum.
                (
                    quote! { ::netabase_store::netabase_path_leaf! { pub struct #def_sub_none_leaf_ident => "" } },
                    quote! { __None(#def_sub_none_leaf_ident) },
                )
            } else {
                let names = &def_sub_table_names;
                (
                    quote! {
                        #( ::netabase_store::netabase_path_leaf! { pub struct #def_sub_topic_leaf_idents => #names } )*
                    },
                    quote! {
                        #(#def_subscriptions(#def_sub_topic_leaf_idents),)*
                        #(#models(#mod_ident::#model_sub_discrim_idents),)*
                    },
                )
            };

        // Body of the definition-level `primary_key()`: the wrapped model's primary key tagged by
        // model variant. For an uninhabited definition (no models) it is an empty match.
        let def_primary_key_body = if models.is_empty() {
            quote! { match *self {} }
        } else {
            quote! {
                match self {
                    #(Self::#models(m) => #pk_enum_ident::#models(m.primary_key()),)*
                }
            }
        };

        // Gated own-topic subscription write: a model registers into THIS definition's subscription
        // only by being inserted wrapped as `Definition::Model(model)` (i.e. `item`). Keyed by the
        // wrapped DefinitionPrimaryKey, valued by the wrapped model's content hash (merkle value).
        let def_own_sub_writes = if def_subscriptions.is_empty() || models.is_empty() {
            quote! {}
        } else {
            let table_names = &def_sub_table_names;
            quote! {
                if __P::SUBSCRIPTIONS {
                    use ::netabase_store::traits::structural::database::tables::core::TableWriteOps;
                    use ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionRegistry;
                    let __own_pk = item.primary_key();
                    let __own_hash = item.subscription_registry_entry()?.member_hash();
                    #(
                        if config.subscriptions.iter().any(|k| ::std::matches!(k, #sub_routing_ident::#def_subscriptions)) {
                            let mut __t = txn.open_write_table::<#ident, #pk_enum_ident, ::netabase_store::traits::structural::database::tables::core::ModelHash>(#table_names)?;
                            __t.insert(&__own_pk, &__own_hash)?;
                        }
                    )*
                }
            }
        };

        let model_pk_idents: Vec<_> = models
            .iter()
            .map(|m| format_ident!("{}PrimaryKey", m))
            .collect();
        let model_secondary_idents: Vec<_> = models
            .iter()
            .map(|m| format_ident!("{}SecondaryKeys", m))
            .collect();
        let model_relational_idents: Vec<_> = models
            .iter()
            .map(|m| format_ident!("{}RelationalValues", m))
            .collect();
        let model_blob_idents: Vec<_> = models
            .iter()
            .map(|m| format_ident!("{}BlobKeys", m))
            .collect();
        let model_sub_idents: Vec<_> = models
            .iter()
            .map(|m| format_ident!("{}SubscriptionKeys", m))
            .collect();
        // 0-based indices for pk/secondary/relational/blob enums and orchestrate routing (no own-subscription variants).
        let _model_variant_indices: Vec<u32> = (0..models.len() as u32).collect();
        // Indices for the subscription routing enum: own topics first, then child models.
        let _sub_own_variant_indices: Vec<u32> = (0..def_subscriptions.len() as u32).collect();
        let _sub_model_variant_indices: Vec<u32> =
            (def_subscriptions.len() as u32..(def_subscriptions.len() + models.len()) as u32)
                .collect();

        let storage_mode_token = match input.storage_mode.as_str() {
            // "grouped" is accepted as a legacy alias for the "linear" single-table mode.
            "linear" | "grouped" => {
                quote! { ::netabase_store::traits::structural::database::tables::core::TableStorageMode::Linear }
            }
            _ => {
                quote! { ::netabase_store::traits::structural::database::tables::core::TableStorageMode::Sharded }
            }
        };

        // Repository names in the netabase_internal_repo attribute use the base name
        // (e.g. "Repository1"). The actual R type param for NetabaseDefinition is the
        // item enum: "{Name}Item". NoRepository is a built-in type and stays unchanged.
        let repository_item_idents: Vec<syn::Ident> = repositories
            .iter()
            .map(|r| {
                if r == "NoRepository" {
                    r.clone()
                } else {
                    format_ident!("{}Item", r)
                }
            })
            .collect();

        let definition_impls = repository_item_idents.iter().map(|r| {
            quote! {
                impl ::netabase_store::traits::structural::schema::definitions::NetabaseDefinition<#r> for #ident {
                    type Address = #address_ident;
                    type Keys = #keys_ident;
                    type Tables = #tables_ident;
                    const TABLES: #tables_ident = #tables_ident;
                }

                impl ::netabase_store::traits::structural::database::tables::core::NetabaseDefinitionAddress<#r, #ident> for #address_ident {}
                impl ::netabase_store::traits::structural::schema::models::keys::NetabaseDefinitionKeys<#r, #ident> for #keys_ident {
                    type PrimaryKey = #pk_enum_ident;
                    type SecondaryKeys = #secondary_keys_ident;
                    type RelationalKeys = #relational_keys_ident;
                    type BlobKeys = #blob_keys_ident;
                    type SubscriptionKeys = #sub_routing_ident;
                }
                impl ::netabase_store::traits::structural::database::tables::core::DefinitionTables<#r, #ident> for #tables_ident {
                    type Config = ();

                    fn orchestrate_get<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<#r>>(
                        &self,
                        txn: &impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryReadTx<'db, #r, DB>,
                        key: #pk_enum_ident,
                    ) -> ::std::result::Result<::std::option::Option<#ident>, ::netabase_store::errors::NetabaseError>
                    where
                        #r: 'db
                    {
                        use ::netabase_store::traits::structural::database::tables::core::ModelTables;
                        use ::netabase_store::traits::structural::schema::models::NetabaseModel;
                        match key {
                            #( #pk_enum_ident::#models(inner_key) => {
                                let mut tables = <#mod_ident::#models as NetabaseModel<#r, #ident>>::TABLES;
                                tables.orchestrate_get(txn, inner_key).map(|res| res.map(#ident::#models))
                            }),*
                            #[allow(unreachable_patterns)]
                            _ => Err(::netabase_store::errors::NetabaseError::Routing(::netabase_store::errors::RoutingErrorKind::WrongVariant))
                        }
                    }

                    fn orchestrate_insert<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<#r>, __P: ::netabase_store::traits::structural::database::tables::InsertPolicy>(
                        &self,
                        txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, #r, DB>,
                        item: #ident,
                        config: &::netabase_store::traits::structural::database::tables::InsertConfig<'_, #sub_routing_ident, __P>,
                    ) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError>
                    where
                        #r: 'db
                    {
                        use ::netabase_store::traits::structural::database::tables::core::ModelTables;
                        use ::netabase_store::traits::structural::schema::models::NetabaseModel;

                        // Definition-own subscription topics: gated write keyed by the wrapped
                        // DefinitionPrimaryKey (must run before `item` is consumed by child routing).
                        #def_own_sub_writes

                        match item {
                            #(#ident::#models(m) => {
                                let model_subs: ::std::vec::Vec<#mod_ident::#model_sub_idents> = config.subscriptions
                                    .iter()
                                    .filter_map(|k| if let #sub_routing_ident::#models(sub) = k { ::std::option::Option::Some(sub.clone()) } else { ::std::option::Option::None })
                                    .collect();
                                // Same compile-time policy `__P`, child's own topic type.
                                let model_config = config.rekey(&model_subs);
                                let mut tables = <#mod_ident::#models as NetabaseModel<#r, #ident>>::TABLES;
                                tables.orchestrate_insert::<__P>(txn, m, &model_config)?;
                            }),*
                            #[allow(unreachable_patterns)]
                            _ => {}
                        }

                        Ok(())
                    }

                    fn orchestrate_delete<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<#r>>(
                        &self,
                        address: #address_ident,
                        key: #pk_enum_ident,
                        txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, #r, DB>,
                    ) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError>
                    where
                        #r: 'db
                    {
                        use ::netabase_store::traits::structural::database::tables::core::ModelTables;
                        use ::netabase_store::traits::structural::schema::models::NetabaseModel;
                        match address {
                            #(#address_ident::#models => {
                                if let #pk_enum_ident::#models(inner_key) = key {
                                    let mut tables = <#mod_ident::#models as NetabaseModel<#r, #ident>>::TABLES;
                                    tables.orchestrate_delete(txn, inner_key)
                                } else {
                                    Err(::netabase_store::errors::NetabaseError::Routing(::netabase_store::errors::RoutingErrorKind::WrongVariant))
                                }
                            }),*
                            #[allow(unreachable_patterns)]
                            _ => Ok(())
                        }
                    }

                    fn orchestrate_fetch_blob_indices<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<#r>>(
                        &self,
                        txn: &impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryReadTx<'db, #r, DB>,
                        key: #pk_enum_ident,
                    ) -> ::std::result::Result<::std::vec::Vec<#blob_keys_ident>, ::netabase_store::errors::NetabaseError>
                    where
                        #r: 'db
                    {
                        use ::netabase_store::traits::structural::database::tables::core::ModelTables;
                        use ::netabase_store::traits::structural::schema::models::NetabaseModel;
                        match key {
                            #( #pk_enum_ident::#models(inner_key) => {
                                let mut tables = <#mod_ident::#models as NetabaseModel<#r, #ident>>::TABLES;
                                let indices = tables.orchestrate_fetch_blob_indices(txn, inner_key)?;
                                Ok(indices.into_iter().map(#blob_keys_ident::#models).collect())
                            } ),*
                            #[allow(unreachable_patterns)]
                            _ => Ok(::std::vec::Vec::new())
                        }
                    }

                    fn orchestrate_read_blob_chunks<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<#r>>(
                        &self,
                        txn: &impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryReadTx<'db, #r, DB>,
                        indices: ::std::vec::Vec<#blob_keys_ident>,
                    ) -> ::std::result::Result<::std::vec::Vec<::std::vec::Vec<u8>>, ::netabase_store::errors::NetabaseError>
                    where
                        #r: 'db
                    {
                        use ::netabase_store::traits::structural::database::tables::core::ModelTables;
                        use ::netabase_store::traits::structural::schema::models::NetabaseModel;
                        if indices.is_empty() { return Ok(::std::vec::Vec::new()); }
                        match indices[0].clone() {
                            #( #blob_keys_ident::#models(_) => {
                                let model_indices = indices.into_iter().filter_map(|k| {
                                    if let #blob_keys_ident::#models(inner) = k { Some(inner) } else { None }
                                }).collect();
                                let mut tables = <#mod_ident::#models as NetabaseModel<#r, #ident>>::TABLES;
                                tables.orchestrate_read_blob_chunks(txn, model_indices)
                            } ),*
                            #[allow(unreachable_patterns)]
                            _ => Ok(::std::vec::Vec::new())
                        }
                    }
                }
                impl ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionOwner<#r> for #ident {
                    type SubscriptionsEnum = #sub_discrim_ident;
                }
                impl ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionKeysEnum<#r, #ident> for #sub_discrim_ident {}
                impl ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionKeysEnum<#r, #ident> for #sub_routing_ident {}
                impl ::netabase_store::traits::structural::schema::models::keys::DefinitionSecondaryKeys<#r, #ident> for #secondary_keys_ident {}
                impl ::netabase_store::traits::structural::schema::models::keys::DefinitionRelationalKeys<#r, #ident> for #relational_keys_ident {}
                impl ::netabase_store::traits::structural::schema::models::keys::DefinitionBlobKeys<#r, #ident> for #blob_keys_ident {}
                impl ::netabase_store::traits::structural::schema::models::keys::DefinitionSubscriptionKeys<#r, #ident> for #sub_routing_ident {}
            }
        });

        // Cascade storage mode to contained models that don't specify their own.
        let item_output: proc_macro2::TokenStream = if input.storage_mode == "sharded" {
            quote! { #item }
        } else {
            let storage_ident = format_ident!("{}", &input.storage_mode);
            let mut modified = item.clone();
            if let Some((_, items)) = &mut modified.content {
                for syn_item in items.iter_mut() {
                    if let syn::Item::Struct(s) = syn_item {
                        if !s.attrs.iter().any(|a| a.path().is_ident("netabase_model")) { continue; }
                        let has_storage = s.attrs.iter().any(|a| {
                            if !a.path().is_ident("netabase") { return false; }
                            let mut found = false;
                            let _ = a.parse_nested_meta(|m| { if m.path.is_ident("storage") { found = true; } Ok(()) });
                            found
                        });
                        if !has_storage {
                            for attr in s.attrs.iter_mut() {
                                if attr.path().is_ident("netabase") {
                                    if let syn::Meta::List(list) = &mut attr.meta {
                                        let existing = list.tokens.clone();
                                        list.tokens = quote! { #existing, storage(#storage_ident) };
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            quote! { #modified }
        };

        // Definition-level table name enum (subscription + child model table names)
        let def_sub_tname_variant = if !def_subscriptions.is_empty() {
            quote! { Subscription(#sub_discrim_ident), }
        } else { quote! {} };
        let def_sub_tname_arm = if !def_subscriptions.is_empty() {
            quote! { Self::Subscription(s) => s.table_name(), }
        } else { quote! {} };

        // Ordered-key encodings for the definition-level aggregation enums:
        // a leading variant tag (the child model index) then the child key's
        // own encoding, so a range bounded by one model is a contiguous scan.
        let def_secondary_encoding = key_enum_ordered_encoding(
            &secondary_keys_ident,
            &models.iter().zip(model_secondary_idents.iter())
                .map(|(m, k)| (m.clone(), Some(quote! { #mod_ident::#k })))
                .collect::<Vec<_>>(),
        );
        let def_relational_encoding = key_enum_ordered_encoding(
            &relational_keys_ident,
            &models.iter().zip(model_relational_idents.iter())
                .map(|(m, k)| (m.clone(), Some(quote! { #mod_ident::#k })))
                .collect::<Vec<_>>(),
        );
        let def_blob_encoding = key_enum_ordered_encoding(
            &blob_keys_ident,
            &models.iter().zip(model_blob_idents.iter())
                .map(|(m, k)| (m.clone(), Some(quote! { #mod_ident::#k })))
                .collect::<Vec<_>>(),
        );
        let def_sub_encoding = {
            let mut variants: Vec<(syn::Ident, Option<proc_macro2::TokenStream>)> =
                def_subscriptions.iter().map(|t| (t.clone(), None)).collect();
            for (m, k) in models.iter().zip(model_sub_idents.iter()) {
                variants.push((m.clone(), Some(quote! { #mod_ident::#k })));
            }
            key_enum_ordered_encoding(&sub_routing_ident, &variants)
        };
        let def_pk_encoding = key_enum_ordered_encoding(
            &pk_enum_ident,
            &models.iter().zip(model_pk_idents.iter())
                .map(|(m, k)| (m.clone(), Some(quote! { #mod_ident::#k })))
                .collect::<Vec<_>>(),
        );

        let output = quote! {
            #item_output
            #trailing

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #ident {
                #(#models(#mod_ident::#models),)*
            }

            impl ::netabase_store::traits::behavioural::TransactionHooks for #ident {}
            impl ::netabase_store::traits::structural::database::tables::core::TableOwner for #ident {}
            impl ::netabase_store::traits::structural::database::tables::core::NodeStorageMode for #ident {
                const MODE: ::netabase_store::traits::structural::database::tables::core::TableStorageMode = #storage_mode_token;
            }

            // --- Definition-level key aggregation enums ---

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub enum #secondary_keys_ident {
                #(#models(#mod_ident::#model_secondary_idents),)*
            }
            #def_secondary_encoding


            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub enum #relational_keys_ident {
                #(#models(#mod_ident::#model_relational_idents),)*
            }
            #def_relational_encoding


            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub enum #blob_keys_ident {
                #(#models(#mod_ident::#model_blob_idents),)*
            }
            #def_blob_encoding


            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub enum #sub_routing_ident {
                #(#def_subscriptions,)*
                #(#models(#mod_ident::#model_sub_idents),)*
            }
            #def_sub_encoding


            // --- End key aggregation enums ---

            #def_sub_leaf_defs
            ::netabase_store::netabase_path_enum! {
                #[derive(Copy)]
                pub enum #sub_discrim_ident { #sub_discrim_path_variants }
            }
            impl #sub_discrim_ident {
                /// Physical subscription table name (delegates to the path-enum leaf).
                pub fn table_name(&self) -> &'static str {
                    ::netabase_store::traits::structural::addressing::PathEnum::static_name(self)
                }
            }

            // --- Definition subscription registry: enum of model hashes, enumerated
            //     by model. A model "subscribes" by being inserted through the
            //     definition-wrapped enum, which produces its registry entry; the set
            //     of entries is compared between nodes via merkle_root for sync. ---
            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #sub_registry_ident {
                #(#models(::netabase_store::traits::structural::database::tables::core::ModelHash),)*
            }
            impl ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionRegistry for #sub_registry_ident {
                fn member_hash(&self) -> ::netabase_store::traits::structural::database::tables::core::ModelHash {
                    match self {
                        #(Self::#models(h) => h.clone(),)*
                        #[allow(unreachable_patterns)]
                        _ => ::netabase_store::traits::structural::database::tables::core::ModelHash([0u8; 32]),
                    }
                }
            }
            impl #ident {
                /// The definition-level primary key for this value: the wrapped model's primary
                /// key tagged by model variant. This is the key used for the definition's own
                /// subscription writes (gated by the wrapping enum).
                pub fn primary_key(&self) -> #pk_enum_ident {
                    #def_primary_key_body
                }

                /// The subscription registry entry for this definition value: the content
                /// hash of the wrapped model, tagged by model. Inserting a model through
                /// the definition variant registers it into the definition's subscription.
                pub fn subscription_registry_entry(&self) -> ::std::result::Result<#sub_registry_ident, ::netabase_store::errors::NetabaseError> {
                    use ::netabase_store::traits::structural::database::tables::core::{NetabaseHasher, Blake3Hasher};
                    use ::netabase_store::traits::structural::database::tables::codec::serialize_value;
                    match self {
                        #(Self::#models(m) => {
                            let bytes = serialize_value(m)?;
                            Ok(#sub_registry_ident::#models(Blake3Hasher::hash(&bytes)))
                        }),*
                        #[allow(unreachable_patterns)]
                        _ => Err(::netabase_store::errors::NetabaseError::Routing(::netabase_store::errors::RoutingErrorKind::WrongVariant)),
                    }
                }
            }

            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #table_name_ident {
                #def_sub_tname_variant
                #(#models(#mod_ident::#model_table_name_idents),)*
            }
            impl #table_name_ident {
                pub fn table_name(&self) -> &'static str {
                    match *self {
                        #def_sub_tname_arm
                        #(Self::#models(t) => t.table_name(),)*
                    }
                }
            }

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #address_ident {
                #(#models,)*
            }

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #pk_enum_ident {
                #(#models(#mod_ident::#model_pk_idents),)*
            }

            // Ordered encoding so the DefinitionPrimaryKey can serve as a
            // subscription-table key (leading model tag + child PK encoding).
            #def_pk_encoding


#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub struct #keys_ident;

            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub struct #tables_ident;

            #(#definition_impls)*
            #(impl ::netabase_store::traits::structural::database::tables::core::ModelStorageMode<#repository_item_idents, #ident> for #ident {})*
        };

        Ok(output)
    }
}
