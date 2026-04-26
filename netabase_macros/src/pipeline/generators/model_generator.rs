// @review [x]
// TODO(#model_gen/id-g020): V[F(generate)], "The generator sets `type Tables = ModelTablesName<R, D, ()>` with TABLES = ModelTablesName(PhantomData). The const TABLES is a PhantomData singleton — it carries no actual table state. Verify this is by design (lazy table opening via transaction factory methods) and document it."
use crate::pipeline::generators::utils::{format_variant_ident, key_enum_ordered_encoding, key_newtype_impls};
use crate::pipeline::planners::{NetabaseModelDataPlan, NetabaseModelPlan};
use proc_macro_flow_core::traits::structural::generator::FlowMutationGenerator;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{GenericArgument, PathArguments, Type};

#[derive(Default)]
pub struct NetabaseModelGenerator;

impl FlowMutationGenerator for NetabaseModelGenerator {
    type Input = NetabaseModelPlan;
    type Item = syn::DeriveInput;
    type Output = TokenStream;
    type SynNode = syn::DeriveInput;

    fn generate(input: &Self::Input, item: &mut Self::Item) -> syn::Result<Self::Output> {
        let ident = &input.ident;
        let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

        let pk_struct_ident = format_ident!("{}PrimaryKey", ident);
        let address_ident = format_ident!("{}Address", ident);
        let keys_ident = format_ident!("{}Keys", ident);
        let secondary_keys_ident = format_ident!("{}SecondaryKeys", ident);
        let relational_keys_ident = format_ident!("{}RelationalKeys", ident);
        let relational_values_ident = format_ident!("{}RelationalValues", ident);
        let subscription_keys_ident = format_ident!("{}SubscriptionKeys", ident);
        let sub_discrim_ident = format_ident!("{}Subscriptions", ident);
        let blob_keys_ident = format_ident!("{}BlobKeys", ident);
        let model_key_ident = format_ident!("{}Key", ident);
        let tables_ident = format_ident!("{}Tables", ident);
        let primary_table_ident = format_ident!("{}PrimaryTable", ident);
        let secondary_table_ident = format_ident!("{}SecondaryTable", ident);
        let relational_table_ident = format_ident!("{}RelationalTable", ident);
        let table_name_ident = format_ident!("{}TableName", ident);

        let blob_table_name = format!("{}_Blob", ident);

        let mut internal_pk_field_ident = None;
        let mut pk_ty = None;
        let mut secondary_key_impls = Vec::new();
        let mut mutated_fields = Vec::new();

        if let NetabaseModelDataPlan::Struct { fields } = &input.data {
            // 1. Identify and Mutate Fields in AST
            if let syn::Data::Struct(s) = &mut item.data {
                for field_plan in fields {
                    let field_ident = field_plan.ident.as_ref().expect("NetabaseModel requires named fields");

                    // Reject dynamic/heap field types with a replacement hint.
                    // Relational fields are exempt: they reference model types,
                    // which the macro rewrites into fixed key/Relation forms.
                    if field_plan.relational_to.is_none() {
                        crate::pipeline::validators::type_policy::check_field_type(&field_plan.ty)?;
                    }

                    if field_plan.is_primary {
                        internal_pk_field_ident = Some(field_ident.clone());
                        pk_ty = Some(field_plan.ty.clone());

                        for field in &mut s.fields {
                            if field.ident.as_ref() == Some(field_ident) {
                                field.ty = syn::parse_quote!(#pk_struct_ident);
                            }
                        }
                        mutated_fields.push((field_ident.clone(), syn::parse_quote!(#pk_struct_ident)));
                    } else if field_plan.is_secondary {
                        let field_newtype_ident = format_ident!("{}{}", ident, format_variant_ident(field_ident));
                        let field_ty = &field_plan.ty;

                        for field in &mut s.fields {
                            if field.ident.as_ref() == Some(field_ident) {
                                field.ty = syn::parse_quote!(#field_newtype_ident);
                            }
                        }
                        mutated_fields.push((field_ident.clone(), syn::parse_quote!(#field_newtype_ident)));

                        // Generate Secondary Newtype Struct: an ordered key
                        // (delegating encoding) that is also a fixed-width value
                        // (delegating Archive) — both via the inner fixed type.
                        secondary_key_impls.push(key_newtype_impls(
                            &field_newtype_ident,
                            field_ty,
                        ));
                    } else if let Some(rel) = &field_plan.relational_to {
                        let target = &rel.to;
                        let repo = rel.repo.as_ref();
                        let def = rel.def.as_ref();
                        let mutated_ty = mutate_relational_type(&field_plan.ty, target, repo, def);

                        for field in &mut s.fields {
                            if field.ident.as_ref() == Some(field_ident) {
                                field.ty = mutated_ty.clone();
                            }
                        }
                        mutated_fields.push((field_ident.clone(), mutated_ty));
                    }
                }
            }
        }

        let pk_ty = if let Some(ty) = pk_ty {
            ty
        } else if let Some(ty) = &input.external_pk_ty {
            ty.clone()
        } else {
            return Err(syn::Error::new(
                ident.span(),
                "Model must have a primary key field or primary_key_type attribute",
            ));
        };

        let primary_key_fn_body = if let Some(field_ident) = &internal_pk_field_ident {
            quote! { #pk_struct_ident::from(self.#field_ident.clone()) }
        } else if let Some(hash_fn) = &input.hash_fn {
            quote! { #hash_fn(self) }
        } else if let Some(key_fn) = &input.key_fn {
            quote! { #key_fn(self) }
        } else {
            quote! { compile_error!("Missing primary key source") }
        };

        let fields = if let NetabaseModelDataPlan::Struct { fields } = &input.data {
            fields
        } else {
            &Vec::new()
        };

        let secondary_fields: Vec<_> = fields.iter().filter(|f| f.is_secondary).collect();
        let relational_fields: Vec<_> = fields
            .iter()
            .filter(|f| f.relational_to.is_some())
            .collect();
        let blob_fields: Vec<_> = fields.iter().filter(|f| f.is_blob).collect();
        let has_blob = !blob_fields.is_empty();

        let chunk_ident = format_ident!("{}Chunk", ident);
        let split_chunks_fn = format_ident!("__netabase_split_chunks_{}", ident);
        // "grouped" is accepted as a legacy alias for the "linear" single-table mode.
        let is_linear =
            input.storage_mode == "linear" || input.storage_mode == "grouped";
        let storage_mode_token = if is_linear {
            quote! { ::netabase_store::traits::structural::database::tables::core::TableStorageMode::Linear }
        } else {
            quote! { ::netabase_store::traits::structural::database::tables::core::TableStorageMode::Sharded }
        };
        // Pattern form of the selected mode, for compile-time drift assertions below.
        let storage_mode_pattern = if is_linear {
            quote! { ::netabase_store::traits::structural::database::tables::core::TableStorageMode::Linear }
        } else {
            quote! { ::netabase_store::traits::structural::database::tables::core::TableStorageMode::Sharded }
        };
        // The zero-sized dispatch marker that encodes this model's physical layout at the
        // type level. `{Model}Tables` is parameterised over it so the layout is visible in
        // the type, not just a generation-time bool. See `ModelTableDispatch` in core.rs.
        let dispatch_ty = if is_linear {
            quote! { ::netabase_store::traits::structural::database::tables::core::LinearDispatch }
        } else {
            quote! { ::netabase_store::traits::structural::database::tables::core::ShardedDispatch }
        };
        // Pure-shard dedup: strip blob + relational fields from the Primary record on insert and
        // rehydrate on read. Surfaced as the `BLOB_DEDUP`/`RELATIONAL_DEDUP` consts.
        let is_pure = input.pure;
        let pure_token = if is_pure { quote! { true } } else { quote! { false } };
        // Statements that blank out the dedup-able fields on a `__sk` skeleton clone (blob +
        // relational; both must be `Default`). The aux tables remain the source of truth and the
        // full in-memory `model` still feeds them.
        let pure_strip_stmts = {
            let blob_idents = blob_fields.iter().map(|f| f.ident.as_ref().unwrap());
            let rel_idents = relational_fields.iter().map(|f| f.ident.as_ref().unwrap());
            quote! {
                #( __sk.#blob_idents = ::std::default::Default::default(); )*
                #( __sk.#rel_idents = ::std::default::Default::default(); )*
            }
        };
        let primary_store_logic = if is_pure {
            quote! {
                let mut __sk = model.clone();
                #pure_strip_stmts
                {
                    let mut table = txn.open_write_table::<#ident #ty_generics, #pk_struct_ident, #ident #ty_generics>(stringify!(#ident))?;
                    table.insert(&primary_key, &__sk)?;
                }
            }
        } else {
            quote! {
                {
                    let mut table = txn.open_write_table::<#ident #ty_generics, #pk_struct_ident, #ident #ty_generics>(stringify!(#ident))?;
                    table.insert(&primary_key, &model)?;
                }
            }
        };
        // Compile-time guard tying the three places the layout mode is expressed
        // (NodeStorageMode::MODE and the selected ModelTableDispatch marker) so they can never
        // drift apart and silently target the wrong physical tables. Only emitted for
        // non-generic models, where the trait consts are referenceable without monomorphization.
        let drift_assertions = if input.generics.params.is_empty() {
            quote! {
                const _: () = {
                    assert!(matches!(
                        <#ident as ::netabase_store::traits::structural::database::tables::core::NodeStorageMode>::MODE,
                        #storage_mode_pattern
                    ));
                    assert!(matches!(
                        <#dispatch_ty as ::netabase_store::traits::structural::database::tables::core::ModelTableDispatch>::MODE,
                        #storage_mode_pattern
                    ));
                };
            }
        } else {
            quote! {}
        };

        // Prepare generics for trait impls
        let mut model_with_keys_generics = input.generics.clone();
        model_with_keys_generics.params.push(syn::parse_quote!(R: ::netabase_store::traits::structural::schema::repositories::NetabaseRepository));
        model_with_keys_generics.params.push(syn::parse_quote!(D: ::netabase_store::traits::structural::schema::definitions::NetabaseDefinition<R>));
        let (impl_model_with_keys, _, _) = model_with_keys_generics.split_for_impl();

        let mut subscription_generics = input.generics.clone();
        subscription_generics.params.push(syn::parse_quote!(R: ::netabase_store::traits::structural::schema::repositories::NetabaseRepository));
        let (impl_subscription, _, _) = subscription_generics.split_for_impl();

        let mut tables_generics = model_with_keys_generics.clone();
        tables_generics.params.push(syn::parse_quote!('db));
        tables_generics.params.push(
            syn::parse_quote!(DB: ::netabase_store::traits::structural::database::NetabaseStore<R>),
        );
        tables_generics.params.push(syn::parse_quote!(MDB));
        tables_generics.params.push(syn::parse_quote!(
            Mode: ::netabase_store::traits::structural::database::tables::core::ModelTableDispatch
        ));
        let (impl_tables, _, _) = tables_generics.split_for_impl();

        let pk_impls = key_newtype_impls(&pk_struct_ident, &pk_ty);

        // When the model declares `custom_table(...)`, suppress the default no-op
        // `CustomTableSideEffects` impl so the user can provide a real one (otherwise the two would
        // conflict). With no custom table declared, the default keeps custom side-effects a no-op.
        let custom_side_effects_default = if input.custom_tables.is_empty() {
            quote! {
                #[allow(unused_variables)]
                impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::custom::CustomTableSideEffects<R, D, #ident #ty_generics> for #ident #ty_generics #where_clause {
                    fn on_insert<'db_local, DB_LOCAL: ::netabase_store::traits::structural::database::NetabaseStore<R>>(
                        &self,
                        txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db_local, R, DB_LOCAL>,
                        model: &Self,
                    ) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { Ok(()) }

                    fn on_delete<'db_local, DB_LOCAL: ::netabase_store::traits::structural::database::NetabaseStore<R>>(
                        &self,
                        txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db_local, R, DB_LOCAL>,
                        key: &#pk_struct_ident,
                    ) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { Ok(()) }

                    fn on_get<'db_local, DB_LOCAL: ::netabase_store::traits::structural::database::NetabaseStore<R>>(
                        &self,
                        txn: &impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryReadTx<'db_local, R, DB_LOCAL>,
                        key: &#pk_struct_ident,
                    ) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { Ok(()) }
                }
            }
        } else {
            quote! {}
        };

        // SAFETY of NoUndef: under rkyv's `unaligned` layout every archived
        // primitive is align-1, so a `#[repr(C)]` archived struct of archived
        // fields has no inter-field or tail padding — every byte is defined.
        // Emitted for non-generic models, where the archived type name is
        // referenceable without monomorphization plumbing.
        // Per-model arena capacity. Provisional default until the
        // `#[netabase(capacity = N)]` attribute drives it through the plan.
        let arena_capacity = input
            .arena_capacity
            .unwrap_or(1024usize);
        let arena_capacity = proc_macro2::Literal::usize_unsuffixed(arena_capacity);

        let archived_ident = format_ident!("Archived{}", ident);
        let model_noundef_impl = if input.generics.params.is_empty() {
            quote! {
                // SAFETY: see the note above — `unaligned` archived layout has no padding.
                unsafe impl ::netabase_store::reexports::rkyv::traits::NoUndef for #archived_ident {}
            }
        } else {
            quote! {}
        };

        let model_impls = quote! {
            impl #impl_generics #ident #ty_generics #where_clause {
                pub fn primary_key(&self) -> #pk_struct_ident {
                    #primary_key_fn_body
                }
            }

            // Arena-manifest constants for this model: the per-model budget the
            // arena store uses to size its value slab and key index region. The
            // capacity default is provisional until a `#[netabase(capacity = N)]`
            // attribute drives it.
            impl #impl_generics #ident #ty_generics #where_clause {
                /// Maximum number of live records the arena reserves for this model.
                pub const ARENA_CAPACITY: usize = #arena_capacity;
                /// Bytes one archived record occupies in an arena slot.
                pub const SLOT_BYTES: usize =
                    ::core::mem::size_of::<#archived_ident>();
                /// Maximum encoded length of this model's primary key.
                pub const PK_MAX_ENCODED: usize =
                    <#pk_struct_ident as ::netabase_store::keys::ordered::OrderedKeyEncoding>::MAX_ENCODED_LEN;
            }

            // The model is a table value via the canonical rkyv codec (the
            // `#[derive(rkyv::Archive, Serialize, Deserialize)]` on the struct
            // gives `StoreValue` through the blanket impl). The archived form
            // is align-1/zero-padding under rkyv's `unaligned` layout, so the
            // crate-wide `NoUndef` requirement is satisfied here.
            #model_noundef_impl

            impl #impl_generics ::netabase_store::traits::behavioural::TransactionHooks for #ident #ty_generics #where_clause {}
            impl #impl_generics ::netabase_store::traits::structural::database::tables::core::TableOwner for #ident #ty_generics #where_clause {}

            impl #impl_generics ::netabase_store::traits::structural::database::tables::core::NodeStorageMode for #ident #ty_generics #where_clause {
                const MODE: ::netabase_store::traits::structural::database::tables::core::TableStorageMode = #storage_mode_token;
            }

            impl #impl_model_with_keys ::netabase_store::traits::structural::schema::models::NetabaseModelWithKeys<R, D> for #ident #ty_generics #where_clause {
                type Address = #address_ident;
                type Keys = #keys_ident;
                fn primary_key(&self) -> #pk_struct_ident {
                    self.primary_key()
                }
                fn get_primary_address() -> Self::Address {
                    #address_ident::Primary
                }
            }

            impl #impl_model_with_keys ::netabase_store::traits::structural::schema::models::NetabaseModel<R, D> for #ident #ty_generics #where_clause {
                // The dispatch marker (`ShardedDispatch`/`LinearDispatch`) makes this model's
                // physical layout visible in the type of its table owner.
                type Tables = #tables_ident<R, D, (), #dispatch_ty>;
                const TABLES: Self::Tables = #tables_ident(::std::marker::PhantomData);
            }

            // Per-category physical layout for this model's auxiliary tables, expressed at
            // the type level. Today `storage(...)` is model-wide so every category shares the
            // same mode, but the trait already supports per-category overrides for the future.
            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::AuxiliaryStorageMode<R, D, #ident #ty_generics> for #ident #ty_generics #where_clause {
                const SECONDARY_MODE: ::netabase_store::traits::structural::database::tables::core::TableStorageMode = #storage_mode_token;
                const RELATIONAL_MODE: ::netabase_store::traits::structural::database::tables::core::TableStorageMode = #storage_mode_token;
                const SUBSCRIPTION_MODE: ::netabase_store::traits::structural::database::tables::core::TableStorageMode = #storage_mode_token;
                const BLOB_MODE: ::netabase_store::traits::structural::database::tables::core::TableStorageMode = #storage_mode_token;
                const CUSTOM_MODE: ::netabase_store::traits::structural::database::tables::core::TableStorageMode = #storage_mode_token;
                const BLOB_DEDUP: bool = #pure_token;
                const RELATIONAL_DEDUP: bool = #pure_token;
            }

            #drift_assertions

            impl #impl_subscription ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionOwner<R> for #ident #ty_generics #where_clause {
                type SubscriptionsEnum = #sub_discrim_ident;
            }

            // Default no-op `CustomTableSideEffects`. Emitted only when the model does NOT declare a
            // `custom_table(...)`; declaring one suppresses this default so the user supplies their
            // own impl (the orchestrator calls `<Model as CustomTableSideEffects>::on_insert/...`).
            #custom_side_effects_default
        };
        // Blobbable (model level): chunking is the model's canonical byte form
        // split into fixed 1KB chunks; reassembly is concat + validated decode.
        // Field-strategy storage routes per-field bytes in blob_insert/get below
        // (the key variant carries the field; chunk values are flat).
        let blob_model_impl = if has_blob {
            quote! {
                impl #impl_generics ::netabase_store::traits::structural::schema::models::blob::Blobbable for #ident #ty_generics #where_clause {
                    type Chunk = #chunk_ident;

                    fn to_chunks(&self) -> ::std::vec::Vec<Self::Chunk> {
                        let bytes = ::netabase_store::traits::structural::database::tables::codec::serialize_value(self)
                            .expect("canonical serialization of a valid model cannot fail");
                        #split_chunks_fn(&bytes).map(#chunk_ident).collect()
                    }

                    fn from_chunks(chunks: ::std::vec::Vec<Self::Chunk>) -> Self {
                        let mut bytes = ::std::vec::Vec::new();
                        for chunk in chunks {
                            for b in chunk.0.as_slice() {
                                bytes.push(*b);
                            }
                        }
                        ::netabase_store::traits::structural::database::tables::codec::read_value(&bytes)
                            .expect("blob chunks must reassemble into a valid model")
                    }
                }
            }
        } else {
            quote! {}
        };

        // The flat chunk value type: 1KB of canonical bytes, itself a fixed-width
        // store value. One shape for every strategy — the blob KEY variant routes
        // fields; the value carries no variant. Emitted for every model so the
        // BlobKeys/ModelTables associated types are always satisfied.
        let archived_chunk_ident = format_ident!("Archived{}", chunk_ident);
        let blob_chunk_impl = quote! {
            // Derive rkyv directly (the inner NbVec now threads a non-() element
            // resolver, so a manual delegating Archive with `Resolver = ()` no
            // longer type-checks). The distinct archived type is also Deserialize-clean.
            #[derive(
                Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default,
                ::netabase_store::reexports::rkyv::Archive,
                ::netabase_store::reexports::rkyv::Serialize,
                ::netabase_store::reexports::rkyv::Deserialize,
            )]
            pub struct #chunk_ident(
                pub ::netabase_store::reexports::netabase_arena::fixed::NbVec<
                    u8,
                    { ::netabase_store::traits::structural::schema::models::blob::DEFAULT_BLOB_CHUNK_SIZE },
                >,
            );

            // SAFETY: repr(C) single-field wrapper over the NbVec archived form,
            // which is NoUndef (all-byte, zero-padding); no padding here either.
            unsafe impl ::netabase_store::reexports::rkyv::traits::NoUndef for #archived_chunk_ident {}

            impl #chunk_ident {
                /// The live chunk bytes.
                pub fn as_slice(&self) -> &[u8] {
                    self.0.as_slice()
                }
            }

            impl #archived_chunk_ident {
                /// The live chunk bytes of the archived form.
                pub fn as_slice(&self) -> &[u8] {
                    self.0.as_slice()
                }
            }

            impl ::netabase_store::traits::structural::schema::models::blob::BlobChunk for #chunk_ident {}

            /// Split canonical bytes into chunk-sized NbVec pieces.
            #[allow(non_snake_case)]
            fn #split_chunks_fn(
                bytes: &[u8],
            ) -> impl ::core::iter::Iterator<
                Item = ::netabase_store::reexports::netabase_arena::fixed::NbVec<
                    u8,
                    { ::netabase_store::traits::structural::schema::models::blob::DEFAULT_BLOB_CHUNK_SIZE },
                >,
            > + '_ {
                bytes
                    .chunks(::netabase_store::traits::structural::schema::models::blob::DEFAULT_BLOB_CHUNK_SIZE)
                    .map(|piece| {
                        ::netabase_store::reexports::netabase_arena::fixed::NbVec::try_from_slice(piece)
                            .expect("chunk piece is at most the chunk capacity")
                    })
            }
        };

        let secondary_variants = secondary_fields.iter().map(|f| {
            let field_ident = f.ident.as_ref().unwrap();
            let variant_name = format_variant_ident(field_ident);
            let field_newtype_ident = format_ident!("{}{}", ident, variant_name);
            quote! { #variant_name(#field_newtype_ident) }
        });

        let relational_key_variants = relational_fields.iter().map(|f| {
            let field_ident = f.ident.as_ref().unwrap();
            let variant_name = format_variant_ident(field_ident);
            quote! { #variant_name(#pk_struct_ident) }
        });


        let blob_key_variants = match input.blob_strategy {
            crate::pipeline::visitors::blob_visitor::BlobStrategy::whole => {
                quote! { Whole(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey<#pk_struct_ident>) }
            }
            crate::pipeline::visitors::blob_visitor::BlobStrategy::field => {
                let variants = blob_fields.iter().map(|f| {
                    let field_ident = f.ident.as_ref().unwrap();
                    let variant_ident = format_variant_ident(field_ident);
                    quote! { #variant_ident(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey<#pk_struct_ident>) }
                });
                quote! { #(#variants),* }
            }
        };

        let sub_keys = &input.subscription_keys;
        let subscription_key_variants = if sub_keys.is_empty() {
            quote! { __None }
        } else {
            quote! { #(#sub_keys,)* }
        };

        let subscription_table_names: Vec<String> = sub_keys
            .iter()
            .map(|k| format!("{}_{}", ident, k))
            .collect();

        // `{Model}Subscriptions` is a path enum (PathEnum) whose variants are ZST leaves carrying
        // the physical subscription table names. Being a path enum lets the *definition* level nest
        // it (`Model({Model}Subscriptions)`) so its discriminator reflects the full tree, and gives
        // `static_name()` (table name) + `to_path()/from_path()` (query string) for free.
        let sub_none_leaf_ident = format_ident!("{}NoneSubLeaf", ident);
        let sub_topic_leaf_idents: Vec<_> = sub_keys
            .iter()
            .map(|k| format_ident!("{}{}SubLeaf", ident, k))
            .collect();
        let sub_leaf_defs = if sub_keys.is_empty() {
            quote! {
                ::netabase_store::netabase_path_leaf! { pub struct #sub_none_leaf_ident => "" }
            }
        } else {
            let names = &subscription_table_names;
            quote! {
                #( ::netabase_store::netabase_path_leaf! { pub struct #sub_topic_leaf_idents => #names } )*
            }
        };
        let sub_discrim_path_variants = if sub_keys.is_empty() {
            quote! { __None(#sub_none_leaf_ident) }
        } else {
            quote! { #(#sub_keys(#sub_topic_leaf_idents),)* }
        };

        // --- {Model}SecondaryTable / {Model}RelationalTable / {Model}TableName ---
        let (secondary_table_variant_idents, secondary_table_name_strings): (Vec<syn::Ident>, Vec<String>) =
            if secondary_fields.is_empty() {
                (vec![format_ident!("__None")], vec!["".to_string()])
            } else if is_linear {
                (vec![format_ident!("All")], vec![format!("{}_Secondary", ident)])
            } else {
                secondary_fields.iter().map(|f| {
                    let fi = f.ident.as_ref().unwrap();
                    (format_ident!("{}", format_variant_ident(fi)), format!("{}_Secondary_{}", ident, fi))
                }).unzip()
            };

        let (relational_table_variant_idents, relational_table_name_strings): (Vec<syn::Ident>, Vec<String>) =
            if relational_fields.is_empty() {
                (vec![format_ident!("__None")], vec!["".to_string()])
            } else if is_linear {
                (vec![format_ident!("All")], vec![format!("{}_Relational", ident)])
            } else {
                relational_fields.iter().map(|f| {
                    let fi = f.ident.as_ref().unwrap();
                    (format_ident!("{}", format_variant_ident(fi)), format!("{}_Relational_{}", ident, fi))
                }).unzip()
            };

        let has_secondary = !secondary_fields.is_empty();
        let has_relational = !relational_fields.is_empty();
        let has_subscription = !sub_keys.is_empty();

        let primary_tn = ident.to_string();
        let blob_tn = blob_table_name.clone();

        let tname_sec_variant = if has_secondary { quote! { Secondary(#secondary_table_ident), } } else { quote! {} };
        let tname_rel_variant = if has_relational { quote! { Relational(#relational_table_ident), } } else { quote! {} };
        let tname_blob_variant = if has_blob { quote! { Blob, } } else { quote! {} };
        let tname_sub_variant = if has_subscription { quote! { Subscription(#sub_discrim_ident), } } else { quote! {} };

        let tname_sec_arm = if has_secondary { quote! { Self::Secondary(t) => t.table_name(), } } else { quote! {} };
        let tname_rel_arm = if has_relational { quote! { Self::Relational(t) => t.table_name(), } } else { quote! {} };
        let tname_blob_arm = if has_blob { quote! { Self::Blob => #blob_tn, } } else { quote! {} };
        let tname_sub_arm = if has_subscription { quote! { Self::Subscription(s) => s.table_name(), } } else { quote! {} };

        let subscription_insert_loop = if sub_keys.is_empty() {
            quote! {}
        } else {
            let table_names = &subscription_table_names;
            quote! {
                let sub_model_key = model.primary_key();
                for sub_key in keys {
                    let sub_table_name: &'static str = match sub_key {
                        #(#subscription_keys_ident::#sub_keys => #table_names,)*
                    };
                    let mut sub_table = txn.open_write_table::<#ident #ty_generics, #pk_struct_ident, ::netabase_store::traits::structural::database::tables::core::ModelHash>(sub_table_name)?;
                    sub_table.insert(&sub_model_key, &hash)?;
                }
            }
        };

        // NOTE: `subscribe_to` (the `subscribe(...)` attribute) is a declaration only. A write into
        // another scope's subscription is *gated* by the wrapping enum: a model can only register
        // into a definition's subscription by being inserted as `Definition::Model(model)`, at which
        // point the definition's orchestration performs the (wrapped-primary-key-keyed) write. We
        // therefore do not emit any child→parent subscription write here.

        let subscription_delete_loop = if sub_keys.is_empty() {
            quote! {}
        } else {
            let table_names = &subscription_table_names;
            quote! {
                #({
                    let mut sub_table = txn.open_write_table::<#ident #ty_generics, #pk_struct_ident, ::netabase_store::traits::structural::database::tables::core::ModelHash>(#table_names)?;
                    sub_table.remove(key)?;
                })*
            }
        };

        // ── Ordered-key encodings for the generated key enums ────────────
        // Variant lists are (ident, Option<payload type>) in declaration
        // order; the tag byte is the declaration index, __None = 0.
        let secondary_keys_encoding = {
            let mut variants: Vec<(syn::Ident, Option<proc_macro2::TokenStream>)> =
                vec![(format_ident!("__None"), None)];
            for f in &secondary_fields {
                let fi = f.ident.as_ref().unwrap();
                let variant = format_variant_ident(fi);
                let newtype = format_ident!("{}{}", ident, variant);
                variants.push((variant, Some(quote! { #newtype })));
            }
            key_enum_ordered_encoding(&secondary_keys_ident, &variants)
        };

        let relational_keys_encoding = {
            let mut variants: Vec<(syn::Ident, Option<proc_macro2::TokenStream>)> =
                vec![(format_ident!("__None"), None)];
            for f in &relational_fields {
                let fi = f.ident.as_ref().unwrap();
                variants.push((format_variant_ident(fi), Some(quote! { #pk_struct_ident })));
            }
            key_enum_ordered_encoding(&relational_keys_ident, &variants)
        };

        let subscription_keys_encoding = {
            let variants: Vec<(syn::Ident, Option<proc_macro2::TokenStream>)> =
                if sub_keys.is_empty() {
                    vec![(format_ident!("__None"), None)]
                } else {
                    sub_keys.iter().map(|k| (k.clone(), None)).collect()
                };
            key_enum_ordered_encoding(&subscription_keys_ident, &variants)
        };

        let blob_chunk_key_ty = quote! {
            ::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey<#pk_struct_ident>
        };
        let blob_keys_encoding = {
            let mut variants: Vec<(syn::Ident, Option<proc_macro2::TokenStream>)> =
                vec![(format_ident!("__None"), None)];
            if has_blob {
                match input.blob_strategy {
                    crate::pipeline::visitors::blob_visitor::BlobStrategy::whole => {
                        variants.push((format_ident!("Whole"), Some(blob_chunk_key_ty.clone())));
                    }
                    crate::pipeline::visitors::blob_visitor::BlobStrategy::field => {
                        for f in &blob_fields {
                            let fi = f.ident.as_ref().unwrap();
                            variants
                                .push((format_variant_ident(fi), Some(blob_chunk_key_ty.clone())));
                        }
                    }
                }
            } else {
                // Keep the historical Whole variant shape for blob-less models.
                variants.push((format_ident!("Whole"), Some(blob_chunk_key_ty.clone())));
            }
            key_enum_ordered_encoding(&blob_keys_ident, &variants)
        };

        // Max encoded length of any relational target's primary key: sizes the
        // NbVec<u8, N> used as the relational tables' value type.
        let relational_value_max = {
            let mut expr = quote! { 0usize };
            for f in &relational_fields {
                let rel = f.relational_to.as_ref().unwrap();
                let mut target_pk_path = rel.to.clone();
                let last = target_pk_path.segments.last_mut().unwrap();
                last.ident = format_ident!("{}PrimaryKey", last.ident);
                expr = quote! {
                    ::netabase_store::keys::ordered::const_max(
                        #expr,
                        <#target_pk_path as ::netabase_store::keys::ordered::OrderedKeyEncoding>::MAX_ENCODED_LEN,
                    )
                };
            }
            expr
        };

        let key_enums = quote! {
            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #address_ident { Primary }

            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::core::NetabaseModelAddress<R, D, #ident #ty_generics> for #address_ident #where_clause {}

            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub struct #keys_ident;
            impl ::netabase_store::reexports::strum::IntoDiscriminant for #keys_ident {
                type Discriminant = ::std::mem::Discriminant<Self>;
                fn discriminant(&self) -> Self::Discriminant { ::std::mem::discriminant(self) }
            }
            impl #impl_model_with_keys ::netabase_store::traits::structural::schema::models::keys::NetabaseModelKeys<R, D, #ident #ty_generics> for #keys_ident #where_clause {
                type PrimaryKey = #pk_struct_ident;
                type SecondaryKeys = #secondary_keys_ident;
                type RelationalKeys = #relational_keys_ident;
                type SubscriptionKeys = #subscription_keys_ident;
                type BlobKeys = #blob_keys_ident;
            }

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub enum #secondary_keys_ident {
                __None,
                #(#secondary_variants),*
            }
            impl ::netabase_store::reexports::strum::IntoDiscriminant for #secondary_keys_ident {
                type Discriminant = ::std::mem::Discriminant<Self>;
                fn discriminant(&self) -> Self::Discriminant { ::std::mem::discriminant(self) }
            }

            // Ordered encoding leads with the variant tag, so byte order groups entries by
            // field variant and a Linear `{Model}_Secondary` range bounded by one variant is
            // a contiguous scan.
            #secondary_keys_encoding

            impl #impl_model_with_keys ::netabase_store::traits::structural::schema::models::keys::secondary::SecondaryKeysEnum<R, D, #ident #ty_generics> for #secondary_keys_ident #where_clause {}

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub enum #relational_keys_ident {
                __None,
                #(#relational_key_variants),*
            }

            #relational_keys_encoding

            /// Relational table VALUE: the ordered-encoded target primary key, stored as
            /// fixed-capacity bytes (payload-carrying enums have no defined-bytes archived
            /// form, so the encoding-as-value is the canonical representation).
            pub type #relational_values_ident =
                ::netabase_store::reexports::netabase_arena::fixed::NbVec<u8, { #relational_value_max }>;

            impl #impl_model_with_keys ::netabase_store::traits::structural::schema::models::keys::relational::RelationalKeysEnum<R, D, #ident #ty_generics> for #relational_keys_ident #where_clause {}

            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub enum #subscription_keys_ident { #subscription_key_variants }
            #subscription_keys_encoding
            impl #impl_subscription ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionKeysEnum<R, #ident #ty_generics> for #subscription_keys_ident #where_clause {}

            #sub_leaf_defs
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
            impl #impl_subscription ::netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionKeysEnum<R, #ident #ty_generics> for #sub_discrim_ident #where_clause {}

            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
            pub enum #blob_keys_ident {
                __None,
                #blob_key_variants
            }

            // Ordered encoding: tag byte then BlobChunkKey = (pk, chunk index), so a per-key
            // range returns chunks contiguously and in order for reassembly.
            #blob_keys_encoding

            impl #impl_model_with_keys ::netabase_store::traits::structural::schema::models::keys::blob::BlobKeysEnum<R, D, #ident #ty_generics> for #blob_keys_ident #where_clause {}

            impl #impl_model_with_keys ::netabase_store::traits::structural::schema::models::keys::primary::PrimaryKey<R, D, #ident #ty_generics> for #pk_struct_ident #where_clause {
                const TABLE_NAME: &'static str = stringify!(#ident);
            }

            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #secondary_table_ident {
                #(#secondary_table_variant_idents,)*
            }
            impl #secondary_table_ident {
                pub fn table_name(&self) -> &'static str {
                    match self {
                        #(Self::#secondary_table_variant_idents => #secondary_table_name_strings,)*
                    }
                }
            }

            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #relational_table_ident {
                #(#relational_table_variant_idents,)*
            }
            impl #relational_table_ident {
                pub fn table_name(&self) -> &'static str {
                    match self {
                        #(Self::#relational_table_variant_idents => #relational_table_name_strings,)*
                    }
                }
            }

            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #table_name_ident {
                Primary,
                #tname_sec_variant
                #tname_rel_variant
                #tname_blob_variant
                #tname_sub_variant
            }
            impl #table_name_ident {
                pub fn table_name(&self) -> &'static str {
                    match self {
                        Self::Primary => #primary_tn,
                        #tname_sec_arm
                        #tname_rel_arm
                        #tname_blob_arm
                        #tname_sub_arm
                    }
                }
            }
        };

        // Physical table names are resolved through the generated `{Model}SecondaryTable` /
        // `{Model}RelationalTable` enums' `table_name()` rather than re-deriving the
        // `{Model}_Secondary_{field}` / linear `{Model}_Secondary` strings here. This keeps the
        // sharded-vs-linear name mapping defined in exactly one place (the enum), so the layout
        // toggle changes names in a single location.
        let secondary_insert_logic = secondary_fields.iter().map(|f| {
            let field_ident = f.ident.as_ref().unwrap();
            let variant_name = format_variant_ident(field_ident);
            let table_name = if is_linear {
                quote! { #secondary_table_ident::All.table_name() }
            } else {
                quote! { #secondary_table_ident::#variant_name.table_name() }
            };

            quote! {
                {
                    let mut table = txn.open_write_table::<#ident #ty_generics, #secondary_keys_ident, #pk_struct_ident>(#table_name)?;
                    table.insert(&#secondary_keys_ident::#variant_name(model.#field_ident.clone()), &model.primary_key())?;
                }
            }
        });

        let secondary_delete_logic = secondary_fields.iter().map(|f| {
            let field_ident = f.ident.as_ref().unwrap();
            let variant_name = format_variant_ident(field_ident);
            let table_name = if is_linear {
                quote! { #secondary_table_ident::All.table_name() }
            } else {
                quote! { #secondary_table_ident::#variant_name.table_name() }
            };

            quote! {
                {
                    let mut table = txn.open_write_table::<#ident #ty_generics, #secondary_keys_ident, #pk_struct_ident>(#table_name)?;
                    table.remove(&#secondary_keys_ident::#variant_name(model.#field_ident.clone()))?;
                }
            }
        });

        let relational_insert_logic = relational_fields.iter().map(|f| {
            let field_ident = f.ident.as_ref().unwrap();
            let variant_name = format_variant_ident(field_ident);
            let table_name = if is_linear {
                quote! { #relational_table_ident::All.table_name() }
            } else {
                quote! { #relational_table_ident::#variant_name.table_name() }
            };

            if relational_is_vec(&f.ty) {
                // One-to-many: each related element is a separate multimap entry under the
                // owning model's primary key. The value is the ordered-encoded target PK.
                quote! {
                    {
                        let mut table = txn.open_write_multimap_table::<#ident #ty_generics, #relational_keys_ident, #relational_values_ident>(#table_name)?;
                        for __elem in model.#field_ident.iter() {
                            let __val = ::netabase_store::keys::ordered::encode_rel_value::<_, { #relational_value_max }>(__elem)?;
                            table.insert(&#relational_keys_ident::#variant_name(model.primary_key()), &__val)?;
                        }
                    }
                }
            } else if relational_is_nboption(&f.ty) {
                // Optional one-to-one: write only when present.
                quote! {
                    {
                        if let ::core::option::Option::Some(__rel) = model.#field_ident.as_option() {
                            let mut table = txn.open_write_table::<#ident #ty_generics, #relational_keys_ident, #relational_values_ident>(#table_name)?;
                            let __val = ::netabase_store::keys::ordered::encode_rel_value::<_, { #relational_value_max }>(__rel)?;
                            table.insert(&#relational_keys_ident::#variant_name(model.primary_key()), &__val)?;
                        }
                    }
                }
            } else {
                // One-to-one: single key/value entry.
                quote! {
                    {
                        let mut table = txn.open_write_table::<#ident #ty_generics, #relational_keys_ident, #relational_values_ident>(#table_name)?;
                        let __val = ::netabase_store::keys::ordered::encode_rel_value::<_, { #relational_value_max }>(&model.#field_ident)?;
                        table.insert(&#relational_keys_ident::#variant_name(model.primary_key()), &__val)?;
                    }
                }
            }
        });

        let relational_delete_logic = relational_fields.iter().map(|f| {
            let field_ident = f.ident.as_ref().unwrap();
            let variant_name = format_variant_ident(field_ident);
            let table_name = if is_linear {
                quote! { #relational_table_ident::All.table_name() }
            } else {
                quote! { #relational_table_ident::#variant_name.table_name() }
            };

            if relational_is_vec(&f.ty) {
                // Multimap `remove(key)` removes all values for the owning model's key.
                quote! {
                    {
                        let mut table = txn.open_write_multimap_table::<#ident #ty_generics, #relational_keys_ident, #relational_values_ident>(#table_name)?;
                        table.remove(&#relational_keys_ident::#variant_name(model.primary_key()))?;
                    }
                }
            } else {
                quote! {
                    {
                        let mut table = txn.open_write_table::<#ident #ty_generics, #relational_keys_ident, #relational_values_ident>(#table_name)?;
                        table.remove(&#relational_keys_ident::#variant_name(model.primary_key()))?;
                    }
                }
            }
        });

        let blob_insert_logic = if has_blob {
            let blob_table_name = format!("{}_Blob", ident);
            match input.blob_strategy {
                crate::pipeline::visitors::blob_visitor::BlobStrategy::whole => {
                    quote! {
                       {
                           use ::netabase_store::traits::structural::schema::models::blob::Blobbable;
                           let chunks = <#ident #ty_generics as Blobbable>::to_chunks(model);
                           let mut blob_table = txn.open_write_table::<#ident #ty_generics, #blob_keys_ident, #chunk_ident>(#blob_table_name)?;
                           for (i, chunk) in chunks.into_iter().enumerate() {
                               blob_table.insert(&#blob_keys_ident::Whole(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(model.primary_key(), i as u64)), &chunk)?;
                           }
                       }
                    }
                }
                crate::pipeline::visitors::blob_visitor::BlobStrategy::field => {
                    let per_field = blob_fields.iter().map(|f| {
                        let field_ident = f.ident.as_ref().unwrap();
                        let variant_ident = format_variant_ident(field_ident);
                        quote! {
                            {
                                let __bytes = ::netabase_store::traits::structural::database::tables::codec::serialize_value(&model.#field_ident)?;
                                for (i, piece) in #split_chunks_fn(&__bytes).enumerate() {
                                    blob_table.insert(&#blob_keys_ident::#variant_ident(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(model.primary_key(), i as u64)), &#chunk_ident(piece))?;
                                }
                            }
                        }
                    });
                    quote! {
                        {
                            let mut blob_table = txn.open_write_table::<#ident #ty_generics, #blob_keys_ident, #chunk_ident>(#blob_table_name)?;
                            #(#per_field)*
                        }
                    }
                }
            }
        } else {
            quote! {}
        };

        let blob_delete_logic = if has_blob {
            let blob_table_name = format!("{}_Blob", ident);
            let key_ranges = match input.blob_strategy {
                crate::pipeline::visitors::blob_visitor::BlobStrategy::whole => {
                    vec![quote! { #blob_keys_ident::Whole(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(key.clone(), 0))..#blob_keys_ident::Whole(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(key.clone(), ::core::primitive::u64::MAX)) }]
                }
                crate::pipeline::visitors::blob_visitor::BlobStrategy::field => blob_fields
                    .iter()
                    .map(|f| {
                        let variant_ident = format_variant_ident(f.ident.as_ref().unwrap());
                        quote! { #blob_keys_ident::#variant_ident(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(key.clone(), 0))..#blob_keys_ident::#variant_ident(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(key.clone(), ::core::primitive::u64::MAX)) }
                    })
                    .collect::<Vec<_>>(),
            };
            quote! {
               {
                   let mut blob_table = txn.open_write_table::<#ident #ty_generics, #blob_keys_ident, #chunk_ident>(#blob_table_name)?;
                   let mut __doomed: ::std::vec::Vec<#blob_keys_ident> = ::std::vec::Vec::new();
                   #({
                       for __entry in blob_table.range(#key_ranges)? {
                           let (__k, _) = __entry?;
                           __doomed.push(__k);
                       }
                   })*
                   for __k in __doomed {
                       blob_table.remove(&__k)?;
                   }
               }
            }
        } else {
            quote! {}
        };

        let blob_get_logic = if has_blob {
            let blob_table_name = format!("{}_Blob", ident);
            match input.blob_strategy {
                crate::pipeline::visitors::blob_visitor::BlobStrategy::whole => {
                    quote! {
                       if let Some(ref mut model) = model_opt {
                           let blob_table = txn.open_read_table::<#ident #ty_generics, #blob_keys_ident, #chunk_ident>(#blob_table_name)?;
                           let mut __bytes = ::std::vec::Vec::new();
                           let mut __any = false;
                           for __entry in blob_table.range(
                               #blob_keys_ident::Whole(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(key.clone(), 0))..#blob_keys_ident::Whole(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(key.clone(), ::core::primitive::u64::MAX))
                           )? {
                               let (_, __chunk) = __entry?;
                               __any = true;
                               for __b in __chunk.as_slice() {
                                   __bytes.push(*__b);
                               }
                           }
                           if __any {
                               *model = ::netabase_store::traits::structural::database::tables::codec::read_value(&__bytes)?;
                           }
                       }
                    }
                }
                crate::pipeline::visitors::blob_visitor::BlobStrategy::field => {
                    let field_hydration = blob_fields.iter().map(|f| {
                        let field_ident = f.ident.as_ref().unwrap();
                        let variant_ident = format_variant_ident(field_ident);
                        quote! {
                            {
                                let mut __bytes = ::std::vec::Vec::new();
                                let mut __any = false;
                                for __entry in blob_table.range(
                                    #blob_keys_ident::#variant_ident(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(key.clone(), 0))..#blob_keys_ident::#variant_ident(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(key.clone(), ::core::primitive::u64::MAX))
                                )? {
                                    let (_, __chunk) = __entry?;
                                    __any = true;
                                    for __b in __chunk.as_slice() {
                                        __bytes.push(*__b);
                                    }
                                }
                                if __any {
                                    model.#field_ident = ::netabase_store::traits::structural::database::tables::codec::read_value(&__bytes)?;
                                }
                            }
                        }
                    });
                    quote! {
                       if let Some(ref mut model) = model_opt {
                           let blob_table = txn.open_read_table::<#ident #ty_generics, #blob_keys_ident, #chunk_ident>(#blob_table_name)?;
                           #(#field_hydration)*
                       }
                    }
                }
            }
        } else {
            quote! {}
        };

        // Pure-shard relational rehydration on `get`: relational fields were stripped from the
        // Primary record, so rebuild them from the relational table (keyed by the model's PK),
        // mirroring `relational_insert_logic` in reverse. Only emitted for pure models.
        let relational_get_logic = if is_pure && has_relational {
            let hydration = relational_fields.iter().map(|f| {
                let field_ident = f.ident.as_ref().unwrap();
                let variant_name = format_variant_ident(field_ident);
                let table_name = if is_linear {
                    quote! { #relational_table_ident::All.table_name() }
                } else {
                    quote! { #relational_table_ident::#variant_name.table_name() }
                };
                // Rebuild the mutated field value from a decoded target PK.
                let rel = f.relational_to.as_ref().unwrap();
                // The stored/decoded value is the target's primary key directly.
                let _ = rel;
                let elem_from_pk = quote! { __pk };
                if relational_is_vec(&f.ty) {
                    quote! {
                        {
                            let __table = txn.open_read_multimap_table::<#ident #ty_generics, #relational_keys_ident, #relational_values_ident>(#table_name)?;
                            // Rebuild in place so the collection type is the field's own.
                            model.#field_ident = ::core::default::Default::default();
                            for __entry in __table.get_all(&#relational_keys_ident::#variant_name(key.clone()))? {
                                let (_, __guard) = __entry?;
                                let __pk = ::netabase_store::keys::ordered::decode_rel_value::<_, { #relational_value_max }>(&*__guard)?;
                                model.#field_ident.try_push(#elem_from_pk).map_err(|_| {
                                    ::netabase_store::errors::NetabaseError::Capacity {
                                        table: stringify!(#ident),
                                        needed: 0,
                                        available: 0,
                                    }
                                })?;
                            }
                        }
                    }
                } else if relational_is_nboption(&f.ty) {
                    quote! {
                        {
                            let __table = txn.open_read_table::<#ident #ty_generics, #relational_keys_ident, #relational_values_ident>(#table_name)?;
                            if let ::core::option::Option::Some(__guard) = __table.get(&#relational_keys_ident::#variant_name(key.clone()))? {
                                let __pk = ::netabase_store::keys::ordered::decode_rel_value::<_, { #relational_value_max }>(&*__guard)?;
                                model.#field_ident = ::netabase_store::reexports::netabase_arena::fixed::NbOption::some(#elem_from_pk);
                            }
                        }
                    }
                } else {
                    quote! {
                        {
                            let __table = txn.open_read_table::<#ident #ty_generics, #relational_keys_ident, #relational_values_ident>(#table_name)?;
                            if let ::core::option::Option::Some(__guard) = __table.get(&#relational_keys_ident::#variant_name(key.clone()))? {
                                let __pk = ::netabase_store::keys::ordered::decode_rel_value::<_, { #relational_value_max }>(&*__guard)?;
                                model.#field_ident = #elem_from_pk;
                            }
                        }
                    }
                }
            });
            quote! {
                if let ::core::option::Option::Some(ref mut model) = model_opt {
                    use ::netabase_store::traits::structural::database::tables::core::TableReadOps;
                    #(#hydration)*
                }
            }
        } else {
            quote! {}
        };

        let fetch_indices_logic = match input.blob_strategy {
            crate::pipeline::visitors::blob_visitor::BlobStrategy::whole => {
                quote! {
                    for __entry in blob_table.range(
                        #blob_keys_ident::Whole(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(key.clone(), 0))..#blob_keys_ident::Whole(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(key.clone(), ::core::primitive::u64::MAX))
                    )? {
                        let (__k, _) = __entry?;
                        all_indices.push(__k);
                    }
                }
            }
            crate::pipeline::visitors::blob_visitor::BlobStrategy::field => {
                let variant_ranges = blob_fields.iter().map(|f| {
                    let variant_ident = format_variant_ident(f.ident.as_ref().unwrap());
                    quote! {
                        for __entry in blob_table.range(
                            #blob_keys_ident::#variant_ident(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(key.clone(), 0))..#blob_keys_ident::#variant_ident(::netabase_store::traits::structural::schema::models::blob::model_blob::BlobChunkKey(key.clone(), ::core::primitive::u64::MAX))
                        )? {
                            let (__k, _) = __entry?;
                            all_indices.push(__k);
                        }
                    }
                });
                quote! { #(#variant_ranges)* }
            }
        };

        // ---- Phase 1: concrete auxiliary-table ownership structs ----
        // Each model owns a typed struct per auxiliary abstraction. The model-table
        // orchestrator (below) dispatches operations to these, so the auxiliary trait
        // tree is live (no longer resolves to `()`), and each table's behaviour lives
        // in one place.
        let secondary_tables_ident = format_ident!("{}SecondaryTables", ident);
        let relational_tables_ident = format_ident!("{}RelationalTables", ident);
        let blob_tables_ident = format_ident!("{}BlobTables", ident);
        let subscription_tables_ident = format_ident!("{}SubscriptionTables", ident);
        let custom_tables_ident = format_ident!("{}CustomTables", ident);
        let auxiliary_tables_ident = format_ident!("{}AuxiliaryTables", ident);


        let aux_table_structs = quote! {
            // ===== Secondary =====
            pub struct #secondary_tables_ident;
            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::TableSideEffect<R, D, #ident #ty_generics> for #secondary_tables_ident #where_clause {
                fn on_insert(&mut self, _model: &#ident #ty_generics) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
                fn on_delete(&mut self, _key: &#pk_struct_ident) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
            }
            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::secondary::SecondaryTables<R, D, #ident #ty_generics> for #secondary_tables_ident #where_clause {
                #[allow(unused_variables)]
                fn orchestrate_insert<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<R>>(&mut self, txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>, model: &#ident #ty_generics) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                    use ::netabase_store::traits::structural::database::tables::core::{TableWriteOps, TableReadOps, NetabaseModelWithKeys};
                    #(#secondary_insert_logic)*
                    ::std::result::Result::Ok(())
                }
                #[allow(unused_variables)]
                fn orchestrate_delete<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<R>>(&mut self, txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>, key: &#pk_struct_ident, model: &#ident #ty_generics) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                    use ::netabase_store::traits::structural::database::tables::core::{TableWriteOps, TableReadOps, NetabaseModelWithKeys};
                    #(#secondary_delete_logic)*
                    ::std::result::Result::Ok(())
                }
            }

            // ===== Relational =====
            pub struct #relational_tables_ident;
            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::TableSideEffect<R, D, #ident #ty_generics> for #relational_tables_ident #where_clause {
                fn on_insert(&mut self, _model: &#ident #ty_generics) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
                fn on_delete(&mut self, _key: &#pk_struct_ident) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
            }
            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::relational::RelationalTables<R, D, #ident #ty_generics> for #relational_tables_ident #where_clause {
                #[allow(unused_variables)]
                fn orchestrate_insert<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<R>>(&mut self, txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>, model: &#ident #ty_generics) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                    use ::netabase_store::traits::structural::database::tables::core::{TableWriteOps, TableReadOps, NetabaseModelWithKeys};
                    #(#relational_insert_logic)*
                    ::std::result::Result::Ok(())
                }
                #[allow(unused_variables)]
                fn orchestrate_delete<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<R>>(&mut self, txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>, key: &#pk_struct_ident, model: &#ident #ty_generics) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                    use ::netabase_store::traits::structural::database::tables::core::{TableWriteOps, TableReadOps, NetabaseModelWithKeys};
                    #(#relational_delete_logic)*
                    ::std::result::Result::Ok(())
                }
            }

            // ===== Blob =====
            pub struct #blob_tables_ident;
            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::TableSideEffect<R, D, #ident #ty_generics> for #blob_tables_ident #where_clause {
                fn on_insert(&mut self, _model: &#ident #ty_generics) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
                fn on_delete(&mut self, _key: &#pk_struct_ident) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
            }
            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::blob::BlobTables<R, D, #ident #ty_generics> for #blob_tables_ident #where_clause {
                #[allow(unused_variables)]
                fn orchestrate_insert<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<R>>(&mut self, txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>, model: &#ident #ty_generics) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                    use ::netabase_store::traits::structural::database::tables::core::{TableWriteOps, TableReadOps, NetabaseModelWithKeys};
                    #blob_insert_logic
                    ::std::result::Result::Ok(())
                }
                #[allow(unused_variables)]
                fn orchestrate_delete<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<R>>(&mut self, txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>, key: &#pk_struct_ident) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                    use ::netabase_store::traits::structural::database::tables::core::{TableWriteOps, TableReadOps};
                    #blob_delete_logic
                    ::std::result::Result::Ok(())
                }
            }

            // ===== Subscription (model topics + compile-time parent subscriptions) =====
            pub struct #subscription_tables_ident;
            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::TableSideEffect<R, D, #ident #ty_generics> for #subscription_tables_ident #where_clause {
                fn on_insert(&mut self, _model: &#ident #ty_generics) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
                fn on_delete(&mut self, _key: &#pk_struct_ident) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
            }
            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::subscription::SubscriptionTables<R, D, #ident #ty_generics> for #subscription_tables_ident #where_clause {
                #[allow(unused_variables)]
                fn orchestrate_insert<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<R>>(&mut self, txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>, model: &#ident #ty_generics, hash: ::netabase_store::traits::structural::database::tables::core::ModelHash, keys: &[#subscription_keys_ident]) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                    use ::netabase_store::traits::structural::database::tables::core::{TableWriteOps, NetabaseModelWithKeys};
                    #subscription_insert_loop
                    ::std::result::Result::Ok(())
                }
                #[allow(unused_variables)]
                fn orchestrate_delete<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<R>>(&mut self, txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>, key: &#pk_struct_ident) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                    use ::netabase_store::traits::structural::database::tables::core::TableWriteOps;
                    #subscription_delete_loop
                    ::std::result::Result::Ok(())
                }
            }

            // ===== Custom =====
            // The model's `CustomTableSideEffects` impl carries the real custom behaviour
            // (it needs the model value, which the CustomTables trait's delete/get do not
            // provide), so it is invoked directly by the orchestrator. This struct exists
            // to satisfy the `ModelTables::Custom` associated type.
            pub struct #custom_tables_ident;
            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::TableSideEffect<R, D, #ident #ty_generics> for #custom_tables_ident #where_clause {
                fn on_insert(&mut self, _model: &#ident #ty_generics) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
                fn on_delete(&mut self, _key: &#pk_struct_ident) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
            }
            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::custom::CustomTables<R, D, #ident #ty_generics> for #custom_tables_ident #where_clause {
                fn orchestrate_insert<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<R>>(&mut self, _txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>, _model: &#ident #ty_generics) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
                fn orchestrate_delete<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<R>>(&mut self, _txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>, _key: &#pk_struct_ident) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
                fn orchestrate_get<'db, DB: ::netabase_store::traits::structural::database::NetabaseStore<R>>(&mut self, _txn: &impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryReadTx<'db, R, DB>, _key: &#pk_struct_ident) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
            }

            // ===== Auxiliary umbrella =====
            pub struct #auxiliary_tables_ident;
            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::TableSideEffect<R, D, #ident #ty_generics> for #auxiliary_tables_ident #where_clause {
                fn on_insert(&mut self, _model: &#ident #ty_generics) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
                fn on_delete(&mut self, _key: &#pk_struct_ident) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> { ::std::result::Result::Ok(()) }
            }
            impl #impl_model_with_keys ::netabase_store::traits::structural::database::tables::auxiliary::AuxiliaryTables<R, D, #ident #ty_generics> for #auxiliary_tables_ident #where_clause {
                type Secondary = #secondary_tables_ident;
                type Relational = #relational_tables_ident;
                type Subscription = #subscription_tables_ident;
                type Blob = #blob_tables_ident;
                type Custom = #custom_tables_ident;
            }
        };

        // Owning write-view: a struct that visibly holds the transaction borrow and this model's
        // table dispatcher, exposing the model's write operations as methods dispatched through it
        // (constructed via `open`). This gives the "one struct owns and dispatches the operations"
        // shape; it delegates to the `ModelTables` orchestration and pins the physical-layout `Mode`
        // to this model's storage mode. Emitted only for non-generic models (the common case),
        // avoiding model-generic plumbing in the view's own generics.
        let view_ident = format_ident!("{}WriteView", ident);
        let owning_view = if input.generics.params.is_empty() {
            quote! {
                pub struct #view_ident<'txn, 'db, R, D, DB, WTX>
                where
                    R: ::netabase_store::traits::structural::schema::repositories::NetabaseRepository + 'db,
                    D: ::netabase_store::traits::structural::schema::definitions::NetabaseDefinition<R>,
                    DB: ::netabase_store::traits::structural::database::NetabaseStore<R>,
                    WTX: ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>,
                {
                    txn: &'txn mut WTX,
                    tables: #tables_ident<R, D, (), #dispatch_ty>,
                    _p: ::std::marker::PhantomData<(&'db (), DB)>,
                }

                impl<'txn, 'db, R, D, DB, WTX> #view_ident<'txn, 'db, R, D, DB, WTX>
                where
                    R: ::netabase_store::traits::structural::schema::repositories::NetabaseRepository + 'db,
                    D: ::netabase_store::traits::structural::schema::definitions::NetabaseDefinition<R>,
                    DB: ::netabase_store::traits::structural::database::NetabaseStore<R>,
                    WTX: ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>,
                {
                    /// Open the view over a write transaction.
                    pub fn open(txn: &'txn mut WTX) -> Self {
                        Self { txn, tables: #tables_ident(::std::marker::PhantomData), _p: ::std::marker::PhantomData }
                    }

                    /// Insert a model, dispatching to all of its auxiliary tables.
                    /// The `InsertConfig` policy (`__P`) decides which categories are
                    /// written; dead branches fold at monomorphization.
                    pub fn insert<__P: ::netabase_store::traits::structural::database::tables::InsertPolicy>(
                        &mut self,
                        model: #ident,
                        config: &::netabase_store::traits::structural::database::tables::InsertConfig<'_, #subscription_keys_ident, __P>,
                    ) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                        use ::netabase_store::traits::structural::database::tables::core::ModelTables;
                        self.tables.orchestrate_insert(&mut *self.txn, model, config)
                    }

                    /// Delete a model by primary key, dispatching to all of its auxiliary tables.
                    pub fn delete(
                        &mut self,
                        key: #pk_struct_ident,
                    ) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                        use ::netabase_store::traits::structural::database::tables::core::ModelTables;
                        self.tables.orchestrate_delete(&mut *self.txn, key)
                    }
                }
            }
        } else {
            quote! {}
        };

        let tables_impls = quote! {
            #aux_table_structs
            #owning_view

            /// Zero-sized owner of this model's auxiliary tables. It owns the *dispatch
            /// logic and physical-layout decision* (via the `Mode` type param), not live
            /// table handles — redb handles are transaction-scoped and are opened per
            /// operation inside the orchestrate methods.
            pub struct #tables_ident<
                R,
                D,
                DB,
                Mode: ::netabase_store::traits::structural::database::tables::core::ModelTableDispatch
                    = ::netabase_store::traits::structural::database::tables::core::ShardedDispatch,
            >(pub ::std::marker::PhantomData<(R, D, DB, Mode)>);

            impl<
                R,
                D,
                DB,
                Mode: ::netabase_store::traits::structural::database::tables::core::ModelTableDispatch,
            > ::netabase_store::traits::structural::database::tables::core::TableConfig for #tables_ident<R, D, DB, Mode> {
                fn table_name(&self) -> &'static str { stringify!(#ident) }
            }

            impl #impl_tables ::netabase_store::traits::structural::database::tables::core::ModelTables<'db, R, D, #ident #ty_generics, DB> for #tables_ident<R, D, MDB, Mode> #where_clause {
                type Primary = #primary_table_ident;
                type Secondary = #secondary_tables_ident;
                type Relational = #relational_tables_ident;
                type Auxiliary = #auxiliary_tables_ident;
                type Subscription = #subscription_tables_ident;
                type Custom = #custom_tables_ident;
                type SubscriptionKey = #subscription_keys_ident;

                fn primary<'a>(txn: &impl ::netabase_store::traits::behavioural::database::transactions::NetabaseTransaction<'a, R, DB>) -> Self::Primary { #primary_table_ident }
                fn secondary<'a>(txn: &impl ::netabase_store::traits::behavioural::database::transactions::NetabaseTransaction<'a, R, DB>) -> Self::Secondary { #secondary_tables_ident }
                fn relational<'a>(txn: &impl ::netabase_store::traits::behavioural::database::transactions::NetabaseTransaction<'a, R, DB>) -> Self::Relational { #relational_tables_ident }
                fn all<'a>(txn: &impl ::netabase_store::traits::behavioural::database::transactions::NetabaseTransaction<'a, R, DB>) -> Self::Auxiliary { #auxiliary_tables_ident }
                fn subscription<'a>(txn: &impl ::netabase_store::traits::behavioural::database::transactions::NetabaseTransaction<'a, R, DB>) -> Self::Subscription { #subscription_tables_ident }
                fn custom<'a>(txn: &impl ::netabase_store::traits::behavioural::database::transactions::NetabaseTransaction<'a, R, DB>) -> Self::Custom { #custom_tables_ident }

                fn orchestrate_insert<__P: ::netabase_store::traits::structural::database::tables::InsertPolicy>(&mut self, txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>, model: #ident #ty_generics, config: &::netabase_store::traits::structural::database::tables::InsertConfig<'_, #subscription_keys_ident, __P>) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                    use ::netabase_store::traits::structural::database::tables::core::{TableWriteOps, TableReadOps, NetabaseModelWithKeys, NetabaseHasher, Blake3Hasher};
                    use ::netabase_store::traits::structural::database::tables::auxiliary::custom::CustomTableSideEffects;
                    use ::netabase_store::traits::structural::database::tables::auxiliary::secondary::SecondaryTables;
                    use ::netabase_store::traits::structural::database::tables::auxiliary::relational::RelationalTables;
                    use ::netabase_store::traits::structural::database::tables::auxiliary::blob::BlobTables;
                    use ::netabase_store::traits::structural::database::tables::auxiliary::subscription::SubscriptionTables;

                    // 1. Primary: insert the canonical byte form. Pure: insert a skeleton with
                    //    blob+relational fields stripped (the aux tables hold them); the full
                    //    in-memory `model` still feeds the aux orchestration below.
                    let primary_key = model.primary_key();
                    #primary_store_logic

                    // 2-5. Pass the operation through to the owned auxiliary-table structs.
                    //      Which categories are written is a compile-time property of the
                    //      InsertConfig policy — dead branches fold at monomorphization. The
                    //      trait calls are fully qualified to pin the R/D of this ModelTables
                    //      impl (a model may belong to several definitions).
                    if __P::SECONDARY {
                        let mut __t = #secondary_tables_ident;
                        <#secondary_tables_ident as SecondaryTables<R, D, #ident #ty_generics>>::orchestrate_insert(&mut __t, txn, &model)?;
                    }
                    if __P::RELATIONAL {
                        let mut __t = #relational_tables_ident;
                        <#relational_tables_ident as RelationalTables<R, D, #ident #ty_generics>>::orchestrate_insert(&mut __t, txn, &model)?;
                    }
                    if __P::SUBSCRIPTIONS {
                        // The content hash is only computed when subscriptions are active:
                        // hash of the model's canonical byte form.
                        let __bytes = ::netabase_store::traits::structural::database::tables::codec::serialize_value(&model)?;
                        let __model_hash = Blake3Hasher::hash(&__bytes);
                        let mut __t = #subscription_tables_ident;
                        <#subscription_tables_ident as SubscriptionTables<R, D, #ident #ty_generics>>::orchestrate_insert(&mut __t, txn, &model, __model_hash, config.subscriptions)?;
                    }
                    if __P::BLOB {
                        let mut __t = #blob_tables_ident;
                        <#blob_tables_ident as BlobTables<R, D, #ident #ty_generics>>::orchestrate_insert(&mut __t, txn, &model)?;
                    }
                    if __P::CUSTOM {
                        <#ident #ty_generics as CustomTableSideEffects<R, D, #ident #ty_generics>>::on_insert(&model, txn, &model)?;
                    }

                    Ok(())
                }
                fn orchestrate_delete(&mut self, txn: &mut impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx<'db, R, DB>, key: #pk_struct_ident) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                    use ::netabase_store::traits::structural::database::tables::core::{TableWriteOps, TableReadOps};
                    use ::netabase_store::traits::structural::database::tables::auxiliary::custom::CustomTableSideEffects;
                    use ::netabase_store::traits::structural::database::tables::auxiliary::secondary::SecondaryTables;
                    use ::netabase_store::traits::structural::database::tables::auxiliary::relational::RelationalTables;
                    use ::netabase_store::traits::structural::database::tables::auxiliary::blob::BlobTables;
                    use ::netabase_store::traits::structural::database::tables::auxiliary::subscription::SubscriptionTables;

                    // Fetch model first to resolve auxiliary keys
                    let model = {
                        let table = txn.open_write_table::<#ident #ty_generics, #pk_struct_ident, #ident #ty_generics>(stringify!(#ident))?;
                        table.get_value(&key)?.ok_or(::netabase_store::errors::NetabaseError::Routing(::netabase_store::errors::RoutingErrorKind::UnknownAddress))?
                    };

                    // 1. Primary
                    {
                        let mut table = txn.open_write_table::<#ident #ty_generics, #pk_struct_ident, #ident #ty_generics>(stringify!(#ident))?;
                        table.remove(&key)?;
                    }

                    // 2-5. Pass deletion through to the owned auxiliary-table structs
                    //      (trait calls fully qualified to pin this impl's R/D).
                    {
                        let mut __t = #secondary_tables_ident;
                        <#secondary_tables_ident as SecondaryTables<R, D, #ident #ty_generics>>::orchestrate_delete(&mut __t, txn, &key, &model)?;
                    }
                    {
                        let mut __t = #relational_tables_ident;
                        <#relational_tables_ident as RelationalTables<R, D, #ident #ty_generics>>::orchestrate_delete(&mut __t, txn, &key, &model)?;
                    }
                    {
                        let mut __t = #subscription_tables_ident;
                        <#subscription_tables_ident as SubscriptionTables<R, D, #ident #ty_generics>>::orchestrate_delete(&mut __t, txn, &key)?;
                    }
                    {
                        let mut __t = #blob_tables_ident;
                        <#blob_tables_ident as BlobTables<R, D, #ident #ty_generics>>::orchestrate_delete(&mut __t, txn, &key)?;
                    }

                    <#ident #ty_generics as CustomTableSideEffects<R, D, #ident #ty_generics>>::on_delete(&model, txn, &key)?;

                    Ok(())
                }
                fn orchestrate_get(&mut self, txn: &impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryReadTx<'db, R, DB>, key: #pk_struct_ident) -> ::std::result::Result<::std::option::Option<#ident #ty_generics>, ::netabase_store::errors::NetabaseError> {
                    use ::netabase_store::traits::structural::database::tables::core::TableReadOps;
                    use ::netabase_store::traits::structural::database::tables::auxiliary::custom::CustomTableSideEffects;

                    let table = txn.open_read_table::<#ident #ty_generics, #pk_struct_ident, #ident #ty_generics>(stringify!(#ident))?;
                    let mut model_opt = table.get_value(&key)?;

                    #blob_get_logic

                    #relational_get_logic

                    if let Some(ref mut model) = model_opt {
                         // Custom Hook
                         <#ident #ty_generics as CustomTableSideEffects<R, D, #ident #ty_generics>>::on_get(model, txn, &key)?;
                    }

                    Ok(model_opt)
                }

                fn orchestrate_fetch_blob_indices(
                    &mut self,
                    txn: &impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryReadTx<'db, R, DB>,
                    key: #pk_struct_ident,
                ) -> ::std::result::Result<::std::vec::Vec<#blob_keys_ident>, ::netabase_store::errors::NetabaseError> {
                    if #has_blob {
                        use ::netabase_store::traits::structural::database::tables::core::TableReadOps;
                        let blob_table = txn.open_read_table::<#ident #ty_generics, #blob_keys_ident, #chunk_ident>(#blob_table_name)?;
                        let mut all_indices = ::std::vec::Vec::new();
                        
                        #fetch_indices_logic
                        
                        Ok(all_indices)
                    } else {
                        Ok(::std::vec::Vec::new())
                    }
                }

                fn orchestrate_read_blob_chunks(
                    &mut self,
                    txn: &impl ::netabase_store::traits::structural::database::transactions::repository::RepositoryReadTx<'db, R, DB>,
                    indices: ::std::vec::Vec<#blob_keys_ident>,
                ) -> ::std::result::Result<::std::vec::Vec<::std::vec::Vec<u8>>, ::netabase_store::errors::NetabaseError> {
                    if #has_blob {
                        use ::netabase_store::traits::structural::database::tables::core::TableReadOps;
                        let blob_table = txn.open_read_table::<#ident #ty_generics, #blob_keys_ident, #chunk_ident>(#blob_table_name)?;
                        let mut chunks = ::std::vec::Vec::new();
                        for idx in indices {
                            if let Some(chunk) = blob_table.get(&idx)? {
                                chunks.push(chunk.as_slice().to_vec());
                            }
                        }
                        Ok(chunks)
                    } else {
                        Ok(::std::vec::Vec::new())
                    }
                }
            }

            impl #impl_tables ::netabase_store::traits::structural::database::transactions::model::ModelReadOps<'db, R, D, #ident #ty_generics, DB> for #tables_ident<R, D, MDB, Mode> #where_clause {
                fn get(&self, _key: #pk_struct_ident) -> ::std::result::Result<::std::option::Option<#ident #ty_generics>, ::netabase_store::errors::NetabaseError> {
                    // Direct table-config access is a routing misuse; use orchestrate_get.
                    ::std::result::Result::Err(::netabase_store::errors::NetabaseError::Routing(::netabase_store::errors::RoutingErrorKind::WrongVariant))
                }
            }

            impl #impl_tables ::netabase_store::traits::structural::database::transactions::model::ModelWriteOps<'db, R, D, #ident #ty_generics, DB> for #tables_ident<R, D, MDB, Mode> #where_clause {
                fn insert(&mut self, _model: #ident #ty_generics) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                    ::std::result::Result::Err(::netabase_store::errors::NetabaseError::Routing(::netabase_store::errors::RoutingErrorKind::WrongVariant))
                }
                fn delete(&mut self, _key: #pk_struct_ident) -> ::std::result::Result<(), ::netabase_store::errors::NetabaseError> {
                    ::std::result::Result::Err(::netabase_store::errors::NetabaseError::Routing(::netabase_store::errors::RoutingErrorKind::WrongVariant))
                }
            }

            pub struct #primary_table_ident;
            impl<R: ::netabase_store::traits::structural::schema::repositories::NetabaseRepository, D: ::netabase_store::traits::structural::schema::definitions::NetabaseDefinition<R>, M: ::netabase_store::traits::structural::schema::models::NetabaseModelWithKeys<R, D>>
                ::netabase_store::traits::structural::database::tables::core::ModelTable<R, D, M> for #primary_table_ident {
                type Key = #pk_struct_ident;
                type Value = #ident #ty_generics;
            }
            impl<R: ::netabase_store::traits::structural::schema::repositories::NetabaseRepository, D: ::netabase_store::traits::structural::schema::definitions::NetabaseDefinition<R>, M: ::netabase_store::traits::structural::schema::models::NetabaseModelWithKeys<R, D>>
                ::netabase_store::traits::structural::database::tables::core::PrimaryTable<R, D, M> for #primary_table_ident {
                type TableConfig = &'static str;
            }
        };

        let record_type_ident = format_ident!("{}Record", ident);
        let record_impl = quote! {
            pub struct #record_type_ident(pub #pk_struct_ident, pub #ident #ty_generics);
        };

        let versioned_impl = if let Some(prev_ty) = &input.prev_version {
            let versioned_trait_ident = format_ident!("{}Versioned", ident);
            quote! {
                pub trait #versioned_trait_ident {
                    fn to_current(self) -> #ident #ty_generics;
                }

                impl #versioned_trait_ident for #ident #ty_generics {
                    fn to_current(self) -> #ident #ty_generics { self }
                }

                impl #versioned_trait_ident for #prev_ty
                where
                    #ident #ty_generics: ::std::convert::From<#prev_ty>,
                {
                    fn to_current(self) -> #ident #ty_generics {
                        <#ident #ty_generics as ::std::convert::From<#prev_ty>>::from(self)
                    }
                }
            }
        } else {
            quote! {}
        };

        let item_output = if input.is_attribute_macro {
            // STRIP: Remove #[netabase] attributes before outputting the struct
            item.attrs.retain(|attr| !attr.path().is_ident("netabase"));
            if let syn::Data::Struct(s) = &mut item.data {
                for field in &mut s.fields {
                    field.attrs.retain(|attr| !attr.path().is_ident("netabase"));
                }
            } else if let syn::Data::Enum(e) = &mut item.data {
                for variant in &mut e.variants {
                    variant.attrs.retain(|attr| !attr.path().is_ident("netabase"));
                }
            }
            quote! { #item }
        } else {
            quote! {}
        };

        // Model-level aggregate key enum. Aggregates every key category behind a single
        // type whose serialized form is prefixed by a 1-byte category discriminant, so a
        // Linear-mode single table can prefix-prune / range-scan by category. Generated
        // now (addresses the model-key-enum gap); only Linear dispatch will consume it.
        let model_key_encoding = {
            let variants: Vec<(syn::Ident, Option<proc_macro2::TokenStream>)> = vec![
                (format_ident!("Primary"), Some(quote! { #pk_struct_ident })),
                (format_ident!("Secondary"), Some(quote! { #secondary_keys_ident })),
                (format_ident!("Relational"), Some(quote! { #relational_keys_ident })),
                (format_ident!("Blob"), Some(quote! { #blob_keys_ident })),
                (format_ident!("Subscription"), Some(quote! { #subscription_keys_ident })),
            ];
            key_enum_ordered_encoding(&model_key_ident, &variants)
        };

        let model_key_enum = quote! {
            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub enum #model_key_ident {
                Primary(#pk_struct_ident),
                Secondary(#secondary_keys_ident),
                Relational(#relational_keys_ident),
                Blob(#blob_keys_ident),
                Subscription(#subscription_keys_ident),
            }
            // Ordered encoding: a leading category tag byte (Primary=0, …) then the
            // category key's own encoding, so a Linear-mode range bounded by one
            // category variant is a contiguous prefix scan over that category.
            #model_key_encoding
        };

        Ok(quote! {
            #item_output
            #pk_impls
            #(#secondary_key_impls)*
            #model_impls
            #blob_model_impl
            #blob_chunk_impl
            #key_enums
            #model_key_enum
            #tables_impls
            #record_impl
            #versioned_impl
        })
    }
}

/// A relational field is stored as a one-to-many multimap when its (pre-mutation) type
/// is a sequence (`NbVec<_, N>`; legacy `Vec<_>` is rejected by the type policy but still
/// recognized here so the diagnostic comes from the policy, not a generation mismatch).
fn relational_is_vec(ty: &Type) -> bool {
    matches!(ty, Type::Path(tp) if tp.path.segments.last().is_some_and(|s| s.ident == "NbVec" || s.ident == "Vec"))
}

/// A relational field with optional one-to-one semantics (`NbOption<_>`).
fn relational_is_nboption(ty: &Type) -> bool {
    matches!(ty, Type::Path(tp) if tp.path.segments.last().is_some_and(|s| s.ident == "NbOption" || s.ident == "Option"))
}

/// Default inline capacity for a one-to-many (`NbVec`) relational field when
/// the user does not pin one. Multimap entries are stored individually in the
/// relational table; this only bounds the rehydrated in-memory collection.
const DEFAULT_RELATION_CAPACITY: usize = 16;

/// Rewrite a relational field's type into its fixed-width stored form:
/// `Vec<M>`/`NbVec<M, N>` → `NbVec<Inner, N>`, `Option<M>`/`NbOption<M>` →
/// `NbOption<Inner>`, bare `M` → `Inner`, where `Inner` is `Relation<R,D,M>`
/// for a cross-scope relation or `MPrimaryKey` otherwise.
fn mutate_relational_type(ty: &Type, target: &syn::Path, _repo: Option<&syn::Path>, _def: Option<&syn::Path>) -> Type {
    // A relational field stores the target's primary key (the related model
    // lives in its own table). Cross-scope `Relation<R,D,M>` routing is future
    // work — the stored form is always `{Target}PrimaryKey`.
    let inner_ty = {
        let mut target_pk_path = target.clone();
        let last_segment = target_pk_path.segments.last_mut().unwrap();
        last_segment.ident = format_ident!("{}PrimaryKey", last_segment.ident);
        quote! { #target_pk_path }
    };

    if let Type::Path(tp) = ty {
        let last_segment = tp.path.segments.last().unwrap();
        let ident = last_segment.ident.to_string();

        // Preserve an explicit capacity on NbVec<M, N>; otherwise default.
        let cap = if let PathArguments::AngleBracketed(args) = &last_segment.arguments {
            args.args.iter().find_map(|a| {
                if let GenericArgument::Const(expr) = a {
                    Some(quote! { #expr })
                } else {
                    None
                }
            })
        } else {
            None
        };

        match ident.as_str() {
            "Vec" | "NbVec" => {
                let cap = cap.unwrap_or_else(|| {
                    let n = DEFAULT_RELATION_CAPACITY;
                    quote! { #n }
                });
                return syn::parse_quote!(
                    ::netabase_store::reexports::netabase_arena::fixed::NbVec<#inner_ty, #cap>
                );
            }
            "Option" | "NbOption" => {
                return syn::parse_quote!(
                    ::netabase_store::reexports::netabase_arena::fixed::NbOption<#inner_ty>
                );
            }
            _ => {}
        }
    }
    syn::parse_quote!(#inner_ty)
}




