// @review [x]
use crate::pipeline::planners::NetabaseRepositoryPlan;
use crate::pipeline::generators::utils::key_enum_ordered_encoding;
use proc_macro_flow_core::traits::structural::generator::FlowGenerator;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::marker::PhantomData;

pub struct NetabaseRepositoryGenerator<'ast>(pub PhantomData<&'ast ()>);

impl<'ast> FlowGenerator for NetabaseRepositoryGenerator<'ast> {
    type Input = NetabaseRepositoryPlan<'ast>;
    type Output = TokenStream;
    type SynNode = syn::ItemMod;

    fn generate(input: &Self::Input) -> syn::Result<Self::Output> {
        let item = input.item;
        let trailing = &input.trailing;
        let ident = &input.ident;
        let mod_ident = input.mod_ident;
        // The data-carrier enum is named {Name}Item; the physical struct keeps {Name}.
        let item_ident = format_ident!("{}Item", ident);
        let address_ident = format_ident!("{}Address", ident);
        let keys_ident = format_ident!("{}Keys", ident);
        let tables_ident = format_ident!("{}Tables", ident);

        let definitions = &input.definitions;
        let subscriptions = &input.subscriptions;

        // Use the base name in the internal-repo attribute so validation in nested
        // definition macros sees the same name as the user writes in repository(...).
        // The definition generator appends "Item" when generating trait bounds.
        let mut item_with_internal = item.clone();
        item_with_internal.attrs.push(syn::parse_quote! {
            #[::netabase_macros::netabase_internal_repo(#ident, subscriptions(#(#subscriptions),*))]
        });

        let pk_enum_ident = format_ident!("{}PrimaryKey", ident);
        let secondary_keys_ident = format_ident!("{}SecondaryKeys", ident);
        let relational_keys_ident = format_ident!("{}RelationalKeys", ident);
        let blob_keys_ident = format_ident!("{}BlobKeys", ident);
        let sub_routing_ident = format_ident!("{}SubscriptionKeys", ident);
        let sub_discrim_ident = format_ident!("{}Subscriptions", ident);
        let sub_registry_ident = format_ident!("{}SubscriptionRegistry", ident);
        let table_name_ident = format_ident!("{}TableName", ident);
        let def_table_name_idents: Vec<_> = definitions.iter().map(|d| format_ident!("{}TableName", d)).collect();

        // `{Repository}Subscriptions` is a path enum: own-topic ZST leaves PLUS one nested
        // `Definition({Definition}Subscriptions)` variant per child definition, so the discriminator
        // reflects the full subscription tree (fixing the prior own-topics-only asymmetry).
        let repo_sub_table_names: Vec<String> = subscriptions
            .iter()
            .map(|s| format!("{}_{}", ident, s))
            .collect();
        let repo_sub_topic_leaf_idents: Vec<_> = subscriptions
            .iter()
            .map(|s| format_ident!("{}{}SubLeaf", ident, s))
            .collect();
        let def_sub_discrim_idents: Vec<_> = definitions
            .iter()
            .map(|d| format_ident!("{}Subscriptions", d))
            .collect();
        let repo_sub_none_leaf_ident = format_ident!("{}NoneSubLeaf", ident);
        let (repo_sub_leaf_defs, sub_discrim_path_variants) =
            if subscriptions.is_empty() && definitions.is_empty() {
                // No own topics and no child definitions: keep the discriminator inhabited with a
                // single ZST `__None` leaf so it is still a valid (non-empty) path enum.
                (
                    quote! { ::netabase_store::netabase_path_leaf! { pub struct #repo_sub_none_leaf_ident => "" } },
                    quote! { __None(#repo_sub_none_leaf_ident) },
                )
            } else {
                let names = &repo_sub_table_names;
                (
                    quote! {
                        #( ::netabase_store::netabase_path_leaf! { pub struct #repo_sub_topic_leaf_idents => #names } )*
                    },
                    quote! {
                        #(#subscriptions(#repo_sub_topic_leaf_idents),)*
                        #(#definitions(#mod_ident::#def_sub_discrim_idents),)*
                    },
                )
            };

        let def_pk_idents: Vec<_> = definitions
            .iter()
            .map(|d| format_ident!("{}PrimaryKey", d))
            .collect();
        let def_secondary_idents: Vec<_> = definitions
            .iter()
            .map(|d| format_ident!("{}SecondaryKeys", d))
            .collect();
        let def_relational_idents: Vec<_> = definitions
            .iter()
            .map(|d| format_ident!("{}RelationalKeys", d))
            .collect();
        let def_blob_idents: Vec<_> = definitions
            .iter()
            .map(|d| format_ident!("{}BlobKeys", d))
            .collect();
        let def_sub_idents: Vec<_> = definitions
            .iter()
            .map(|d| format_ident!("{}SubscriptionKeys", d))
            .collect();
        // 0-based indices for pk/secondary/relational/blob enums (no own-subscription variants there).
        let _definition_variant_indices: Vec<u32> = (0..definitions.len() as u32).collect();
        // Indices for the subscription routing enum: own topics first, then child definitions.
        let _sub_own_variant_indices: Vec<u32> = (0..subscriptions.len() as u32).collect();
        let _sub_def_variant_indices: Vec<u32> =
            (subscriptions.len() as u32..(subscriptions.len() + definitions.len()) as u32)
                .collect();
        let address_idents: Vec<_> = definitions
            .iter()
            .map(|d| format_ident!("{}Address", d))
            .collect();

        // Cascade storage mode to contained definitions (which in turn cascade to models).
        let item_with_internal_output: proc_macro2::TokenStream = if input.storage_mode == "sharded" {
            quote! { #item_with_internal }
        } else {
            let storage_ident = format_ident!("{}", &input.storage_mode);
            let mut modified = item_with_internal.clone();
            if let Some((_, items)) = &mut modified.content {
                for syn_item in items.iter_mut() {
                    if let syn::Item::Mod(m) = syn_item {
                        let has_def = m.attrs.iter().any(|a| a.path().is_ident("netabase_definition"));
                        if !has_def { continue; }
                        for attr in m.attrs.iter_mut() {
                            if attr.path().is_ident("netabase_definition") {
                                let has_storage = {
                                    let mut found = false;
                                    let _ = attr.parse_nested_meta(|meta| { if meta.path.is_ident("storage") { found = true; } Ok(()) });
                                    found
                                };
                                if !has_storage
                                    && let syn::Meta::List(list) = &mut attr.meta {
                                        let existing = list.tokens.clone();
                                        list.tokens = quote! { #existing, storage(#storage_ident) };
                                    }
                                break;
                            }
                        }
                    }
                }
            }
            quote! { #modified }
        };

        // Repository-level table name enum (subscription + child definition table names)
        let repo_sub_tname_variant = if !subscriptions.is_empty() {
            quote! { Subscription(#sub_discrim_ident), }
        } else { quote! {} };
        let repo_sub_tname_arm = if !subscriptions.is_empty() {
            quote! { Self::Subscription(s) => s.table_name(), }
        } else { quote! {} };

        // Per-definition database fields: _db_0, _db_1, ...
        let _def_field_idents: Vec<_> = (0..definitions.len())
            .map(|i| format_ident!("_db_{}", i))
            .collect();
        // File names for each definition database.
        let _def_file_names: Vec<String> = definitions
            .iter()
            .map(|d| format!("{}.redb", d))
            .collect();

        // Router function: maps a table config string to a definition index.
        // Emit checks sorted by model name length descending so that longer names
        // (e.g. "Model1Definition2") are matched before shorter prefixes ("Model1").
        // Each check uses exact equality OR a starts_with("{name}_") test to avoid
        // treating "Model1Definition2" as "Model1" with suffix.
        let _router_fn_ident = format_ident!("_netabase_{}_table_router", ident);
        let mut all_router_checks: Vec<(usize, String)> = input
            .models_per_definition
            .iter()
            .enumerate()
            .flat_map(|(i, models)| {
                models.iter().map(move |m| (i, m.to_string()))
            })
            .collect();
        // Sort descending by model name length so longer names are checked first.
        all_router_checks.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        let _router_checks: Vec<TokenStream> = all_router_checks
            .iter()
            .map(|(i, name)| {
                let prefix = format!("{}_", name);
                quote! {
                    if config == #name || config.starts_with(#prefix) { return #i; }
                }
            })
            .collect();

        // Body of the repository-level `primary_key()`: the wrapped definition's primary key tagged
        // by definition variant. For an uninhabited repository (no definitions) it is an empty match.
        let repo_primary_key_body = if definitions.is_empty() {
            quote! { match *self {} }
        } else {
            quote! {
                match self {
                    #(Self::#definitions(d) => #pk_enum_ident::#definitions(d.primary_key()),)*
                }
            }
        };

        // Gated own-topic subscription write: a definition registers into THIS repository's
        // subscription only by being inserted wrapped as `Repository::Definition(def)` (i.e.
        // `item`). Keyed by the wrapped RepositoryPrimaryKey, valued by the wrapped value's
        // content hash (the merkle value).
        let repo_own_sub_writes = if subscriptions.is_empty() || definitions.is_empty() {
            quote! {}
        } else {
            let table_names = &repo_sub_table_names;
            quote! {
                if __P::SUBSCRIPTIONS {
                    use ::netabase_store::traits::structural::database::tables::core::TableWriteOps;
                    use ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionRegistry;
                    let __own_pk = item.primary_key();
                    let __own_hash = item.subscription_registry_entry()?.member_hash();
                    #(
                        if config.subscriptions.iter().any(|k| ::std::matches!(k, #sub_routing_ident::#subscriptions)) {
                            let mut __t = txn.open_write_table::<#item_ident, #pk_enum_ident, ::netabase_store::traits::structural::database::tables::core::ModelHash>(#table_names)?;
                            __t.insert(&__own_pk, &__own_hash)?;
                        }
                    )*
                }
            }
        };

        let repo_secondary_encoding = key_enum_ordered_encoding(
            &secondary_keys_ident,
            &definitions.iter().zip(def_secondary_idents.iter())
                .map(|(d, k)| (d.clone(), Some(quote! { #mod_ident::#k }))).collect::<Vec<_>>(),
        );
        let repo_relational_encoding = key_enum_ordered_encoding(
            &relational_keys_ident,
            &definitions.iter().zip(def_relational_idents.iter())
                .map(|(d, k)| (d.clone(), Some(quote! { #mod_ident::#k }))).collect::<Vec<_>>(),
        );
        let repo_blob_encoding = key_enum_ordered_encoding(
            &blob_keys_ident,
            &definitions.iter().zip(def_blob_idents.iter())
                .map(|(d, k)| (d.clone(), Some(quote! { #mod_ident::#k }))).collect::<Vec<_>>(),
        );
        let repo_sub_encoding = {
            let mut variants: Vec<(syn::Ident, Option<proc_macro2::TokenStream>)> =
                subscriptions.iter().map(|t| (t.clone(), None)).collect();
            for (d, k) in definitions.iter().zip(def_sub_idents.iter()) {
                variants.push((d.clone(), Some(quote! { #mod_ident::#k })));
            }
            key_enum_ordered_encoding(&sub_routing_ident, &variants)
        };
        let repo_pk_encoding = key_enum_ordered_encoding(
            &pk_enum_ident,
            &definitions.iter().zip(def_pk_idents.iter())
                .map(|(d, k)| (d.clone(), Some(quote! { #mod_ident::#k }))).collect::<Vec<_>>(),
        );

        let output = quote! {
            #item_with_internal_output
            #trailing

            // ── Item enum (data carrier) ────────────────────────────────────────

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #item_ident {
                #(#definitions(#mod_ident::#definitions),)*
            }

            impl ::netabase_store::traits::behavioural::TransactionHooks for #item_ident {}
            impl ::netabase_store::traits::structural::database::tables::core::TableOwner for #item_ident {}



            impl ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionOwner<Self> for #item_ident {
                type SubscriptionsEnum = #sub_discrim_ident;
            }

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #address_ident {
                #(#definitions(#mod_ident::#address_idents),)*
            }

            impl ::netabase_store::traits::structural::database::tables::core::NetabaseRepositoryAddress<#item_ident> for #address_ident {}

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #pk_enum_ident {
                #(#definitions(#mod_ident::#def_pk_idents),)*
            }

            // Ordered encoding so RepositoryPrimaryKey serves as a subscription-table key.
            #repo_pk_encoding


            // --- Repository-level key aggregation enums ---

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub enum #secondary_keys_ident {
                #(#definitions(#mod_ident::#def_secondary_idents),)*
            }
            #repo_secondary_encoding

            impl ::netabase_store::traits::structural::schema::models::keys::RepositorySecondaryKeys<#item_ident> for #secondary_keys_ident {}

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub enum #relational_keys_ident {
                #(#definitions(#mod_ident::#def_relational_idents),)*
            }
            #repo_relational_encoding

            impl ::netabase_store::traits::structural::schema::models::keys::RepositoryRelationalKeys<#item_ident> for #relational_keys_ident {}

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub enum #blob_keys_ident {
                #(#definitions(#mod_ident::#def_blob_idents),)*
            }
            #repo_blob_encoding

            impl ::netabase_store::traits::structural::schema::models::keys::RepositoryBlobKeys<#item_ident> for #blob_keys_ident {}

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub enum #sub_routing_ident {
                #(#subscriptions,)*
                #(#definitions(#mod_ident::#def_sub_idents),)*
            }
            #repo_sub_encoding

            impl ::netabase_store::traits::structural::schema::models::keys::RepositorySubscriptionKeys<#item_ident> for #sub_routing_ident {}
            impl ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionKeysEnum<#item_ident, #item_ident> for #sub_routing_ident {}

            // --- End repository key aggregation enums ---

            #repo_sub_leaf_defs
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
            impl ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionKeysEnum<#item_ident, #item_ident> for #sub_discrim_ident {}

            // --- Repository subscription registry: enum of definition merkle hashes,
            //     enumerated by definition. Built by recursing into each definition's
            //     own subscription registry entry; compared between nodes via merkle_root. ---
            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #sub_registry_ident {
                #(#definitions(::netabase_store::traits::structural::database::tables::core::ModelHash),)*
            }
            impl ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionRegistry for #sub_registry_ident {
                fn member_hash(&self) -> ::netabase_store::traits::structural::database::tables::core::ModelHash {
                    match self {
                        #(Self::#definitions(h) => h.clone(),)*
                        #[allow(unreachable_patterns)]
                        _ => ::netabase_store::traits::structural::database::tables::core::ModelHash([0u8; 32]),
                    }
                }
            }
            impl #item_ident {
                /// The repository-level primary key for this value: the wrapped definition's
                /// primary key tagged by definition variant. This is the key used for the
                /// repository's own subscription writes (gated by the wrapping enum).
                pub fn primary_key(&self) -> #pk_enum_ident {
                    #repo_primary_key_body
                }

                /// The subscription registry entry for this repository value: dispatches
                /// downward to the wrapped definition's registry entry and tags it by
                /// definition, so the repository sees which definitions are registered.
                pub fn subscription_registry_entry(&self) -> ::std::result::Result<#sub_registry_ident, ::netabase_store::errors::NetabaseError> {
                    use ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionRegistry;
                    match self {
                        #(Self::#definitions(d) => {
                            let entry = d.subscription_registry_entry()?;
                            Ok(#sub_registry_ident::#definitions(entry.member_hash()))
                        }),*
                        #[allow(unreachable_patterns)]
                        _ => Err(::netabase_store::errors::NetabaseError::Routing(::netabase_store::errors::RoutingErrorKind::WrongVariant)),
                    }
                }
            }

            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #table_name_ident {
                #repo_sub_tname_variant
                #(#definitions(#mod_ident::#def_table_name_idents),)*
            }
            impl #table_name_ident {
                pub fn table_name(&self) -> &'static str {
                    match *self {
                        #repo_sub_tname_arm
                        #(Self::#definitions(t) => t.table_name(),)*
                    }
                }
            }

            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub struct #keys_ident;
            impl ::netabase_store::traits::structural::schema::models::keys::NetabaseRepositoryKeys<#item_ident> for #keys_ident {
                type PrimaryKey = #pk_enum_ident;
                type SecondaryKeys = #secondary_keys_ident;
                type RelationalKeys = #relational_keys_ident;
                type BlobKeys = #blob_keys_ident;
                type SubscriptionKeys = #sub_routing_ident;
            }

            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub struct #tables_ident;
            impl ::netabase_store::traits::structural::database::tables::core::RepositoryTables<#item_ident> for #tables_ident {
                type Config = ();

                fn orchestrate_get<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<#item_ident>>(
                    &self,
                    txn: &impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryReadTx<'db, #item_ident, DB>,
                    key: #pk_enum_ident,
                ) -> ::std::result::Result<::std::option::Option<#item_ident>, ::netabase_store::errors::NetabaseError>
                where
                    #item_ident: 'db
                {
                    use ::netabase_store::traits::structural::database::tables::core::DefinitionTables;
                    use ::netabase_store::traits::structural::schema::definitions::NetabaseDefinition;
                    match key {
                        #( #pk_enum_ident::#definitions(inner_key) => {
                            <#mod_ident::#definitions as NetabaseDefinition<#item_ident>>::TABLES
                                .orchestrate_get(txn, inner_key)
                                .map(|res| res.map(#item_ident::#definitions))
                        }),*
                        #[allow(unreachable_patterns)]
                        _ => Err(::netabase_store::errors::NetabaseError::Routing(::netabase_store::errors::RoutingErrorKind::WrongVariant))
                    }
                }

                fn orchestrate_insert<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<#item_ident>, __P: ::netabase_store::traits::structural::database::tables::InsertPolicy>(
                    &self,
                    txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, #item_ident, DB>,
                    item: #item_ident,
                    config: &::netabase_store::traits::structural::database::tables::InsertConfig<'_, #sub_routing_ident, __P>,
                ) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError>
                where
                    #item_ident: 'db
                {
                    use ::netabase_store::traits::structural::database::tables::core::DefinitionTables;
                    use ::netabase_store::traits::structural::schema::definitions::NetabaseDefinition;

                    // Repository-own subscription topics: gated write keyed by the wrapped
                    // RepositoryPrimaryKey (must run before `item` is consumed by child routing).
                    #repo_own_sub_writes

                    match item {
                        #(#item_ident::#definitions(d) => {
                            let def_subs: ::std::vec::Vec<#mod_ident::#def_sub_idents> = config.subscriptions
                                .iter()
                                .filter_map(|k| if let #sub_routing_ident::#definitions(sub) = k { ::std::option::Option::Some(sub.clone()) } else { ::std::option::Option::None })
                                .collect();
                            let def_config = config.rekey(&def_subs);
                            <#mod_ident::#definitions as NetabaseDefinition<#item_ident>>::TABLES
                                .orchestrate_insert::<DB, __P>(txn, d, &def_config)?;
                        }),*
                        #[allow(unreachable_patterns)]
                        _ => {}
                    }

                    Ok(())
                }

                fn orchestrate_delete<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<#item_ident>>(
                    &self,
                    address: #address_ident,
                    key: #pk_enum_ident,
                    txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, #item_ident, DB>,
                ) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError>
                where
                    #item_ident: 'db
                {
                    use ::netabase_store::traits::structural::database::tables::core::DefinitionTables;
                    use ::netabase_store::traits::structural::schema::definitions::NetabaseDefinition;
                    match address {
                        #(#address_ident::#definitions(inner_addr) => {
                            if let #pk_enum_ident::#definitions(inner_key) = key {
                                <#mod_ident::#definitions as NetabaseDefinition<#item_ident>>::TABLES
                                    .orchestrate_delete(inner_addr, inner_key, txn)
                            } else {
                                Err(::netabase_store::errors::NetabaseError::Routing(::netabase_store::errors::RoutingErrorKind::WrongVariant))
                            }
                        }),*
                        #[allow(unreachable_patterns)]
                        _ => Ok(())
                    }
                }
            }

            impl ::netabase_store::traits::structural::schema::repositories::NetabaseRepository for #item_ident {
                type Address = #address_ident;
                type Keys = #keys_ident;
                type Tables = #tables_ident;
                const TABLES: #tables_ident = #tables_ident;
            }

            // NOTE: the repository does not generate its own physical store. Stores are
            // generic over the repository type — open `RedbStore::<{Repo}Item>::open(path)`
            // or `MemoryStore::<{Repo}Item>::open(())`. (The former per-definition multi-DB
            // store was removed with the multi_db backend.)
        };

        Ok(output)
    }
}
