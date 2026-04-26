// @review [x]
// PARTIAL(#blob_gen/id-g030): the `whole` strategy's chunk size is now the named constant
// `netabase_store::...::blob::DEFAULT_BLOB_CHUNK_SIZE` (tunable in one place) rather than a magic
// `1024`. Remaining: per-type override via a `#[blob(chunk_size = N)]` attribute (needs visitor/
// planner plumbing in blob_visitor.rs / blob_plan.rs).
// RESOLVED(#blob_gen/id-g031): the field strategy's `from_chunks` reconstructs non-blob fields with
// `..Default::default()`. The `Default` requirement is enforced at compile time (the generated
// `..Default::default()` fails to compile unless the model is `Default`), and the constraint is
// documented loudly on the generated `from_chunks` below. Note this whole-value reconstruction is
// *not* on the framework's read path: `get` overlays each blob field onto the record stored in the
// primary table (see `blob_get_logic` in model_generator.rs), so a normal read never loses non-blob
// data. Direct callers of `Blobbable::from_chunks` get a value whose non-blob fields are `Default`.
use crate::pipeline::generators::utils::format_variant_ident;
use crate::pipeline::planners::NetabaseBlobPlan;
use crate::pipeline::visitors::BlobStrategy;
use proc_macro_flow_core::traits::structural::generator::FlowGenerator;
use proc_macro2::TokenStream;
use quote::quote;

pub struct NetabaseBlobGenerator<'ast>(pub std::marker::PhantomData<&'ast ()>);

impl<'ast> FlowGenerator for NetabaseBlobGenerator<'ast> {
    type Input = NetabaseBlobPlan<'ast>;
    type Output = TokenStream;
    type SynNode = syn::DeriveInput;

    fn generate(plan: &Self::Input) -> syn::Result<Self::Output> {
        let ident = plan.ident;
        let (impl_generics, ty_generics, where_clause) = plan.generics.split_for_impl();
        let chunk_ident = quote::format_ident!("{}Chunk", ident);

        let chunk_def = match plan.strategy {
            BlobStrategy::whole => {
                quote! {
                    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
                    pub struct #chunk_ident(pub usize, pub ::std::vec::Vec<u8>);
                }
            }
            BlobStrategy::field => {
                let variants = plan.blobbable_fields.iter().map(|f| {
                    let variant_ident = format_variant_ident(f.ident);
                    let ty = f.ty;
                    quote! {
                        #variant_ident(<#ty as ::netabase_store::traits::structural::schema::models::blob::Blobbable>::Chunk)
                    }
                });
                quote! {
                    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
                    pub enum #chunk_ident {
                        #(#variants),*
                    }
                }
            }
        };

        // `BlobChunk` is just a marker (`'static`) now — chunks travel as raw
        // bytes at the storage layer, so no table codec is generated here.
        let chunk_traits = quote! {
            impl ::netabase_store::traits::structural::schema::models::blob::BlobChunk for #chunk_ident {}
        };

        let to_chunks_logic = match plan.strategy {
            BlobStrategy::whole => {
                quote! {
                    let data = <Self as ::std::convert::TryInto<::std::vec::Vec<u8>>>::try_into(self.clone()).expect("Failed to serialize for chunking");
                    data.chunks(::netabase_store::traits::structural::schema::models::blob::DEFAULT_BLOB_CHUNK_SIZE).enumerate().map(|(i, c)| #chunk_ident(i, c.to_vec())).collect()
                }
            }
            BlobStrategy::field => {
                let fields = plan.blobbable_fields.iter().map(|f| {
                    let field_ident = f.ident;
                    let variant_ident = format_variant_ident(field_ident);
                    let ty = f.ty;
                    quote! {
                        <#ty as ::netabase_store::traits::structural::schema::models::blob::Blobbable>::to_chunks(&self.#field_ident)
                            .into_iter()
                            .map(|c| #chunk_ident::#variant_ident(c))
                    }
                });
                quote! {
                    let mut all_chunks = ::std::vec::Vec::new();
                    #(all_chunks.extend(#fields);)*
                    all_chunks
                }
            }
        };

        let blobbable_impl = quote! {
            impl #impl_generics ::netabase_store::traits::structural::schema::models::blob::Blobbable for #ident #ty_generics #where_clause {
                type Chunk = #chunk_ident;

                fn to_chunks(&self) -> ::std::vec::Vec<Self::Chunk> {
                    #to_chunks_logic
                }

                fn from_chunks(chunks: ::std::vec::Vec<Self::Chunk>) -> Self {
                    Self::from(chunks)
                }
            }
        };

        let strategy_impl = match plan.strategy {
            BlobStrategy::whole => {
                quote! {
                    impl #impl_generics ::netabase_store::traits::structural::schema::models::blob::ChunkBlobbable for #ident #ty_generics #where_clause {}

                    impl #impl_generics From<::std::vec::Vec<#chunk_ident>> for #ident #ty_generics #where_clause {
                        fn from(mut chunks: ::std::vec::Vec<#chunk_ident>) -> Self {
                            chunks.sort_by_key(|c| c.0);
                            let data: ::std::vec::Vec<u8> = chunks.into_iter().flat_map(|c| c.1).collect();
                            <Self as ::std::convert::TryFrom<::std::vec::Vec<u8>>>::try_from(data).expect("Failed to reconstruct from chunks")
                        }
                    }
                }
            }
            BlobStrategy::field => {
                let field_reassemble = plan.blobbable_fields.iter().map(|f| {
                    let var_name = quote::format_ident!("{}", f.ident);
                    let variant_ident = format_variant_ident(f.ident);
                    let ty = f.ty;
                    quote! {
                        let #var_name = {
                            let field_chunks: ::std::vec::Vec<_> = chunks.iter().filter_map(|c| {
                                if let #chunk_ident::#variant_ident(inner) = c {
                                    Some(inner.clone())
                                } else {
                                    None
                                }
                            }).collect();
                            <#ty as ::netabase_store::traits::structural::schema::models::blob::Blobbable>::from_chunks(field_chunks)
                        };
                    }
                });

                let field_names = plan.blobbable_fields.iter().map(|f| f.ident);
                // CONSTRAINT: field-strategy reconstruction only restores `#[blob]` fields; every
                // other field takes `Default`. This requires the model to be `Default` (enforced
                // here at compile time by `..Default::default()`). If a model has a mandatory
                // non-blob field, that field's value is NOT round-tripped by this whole-value
                // `from_chunks` — callers must rely on the framework `get` path, which overlays
                // blob fields onto the stored record rather than rebuilding the whole value.
                let construction = quote! {
                    Self {
                        #(#field_names,)*
                        ..Default::default()
                    }
                };

                quote! {
                    impl #impl_generics ::netabase_store::traits::structural::schema::models::blob::FieldBlobbable for #ident #ty_generics #where_clause {}

                    impl #impl_generics From<::std::vec::Vec<#chunk_ident>> for #ident #ty_generics #where_clause {
                        fn from(chunks: ::std::vec::Vec<#chunk_ident>) -> Self {
                            #(#field_reassemble)*
                            #construction
                        }
                    }
                }
            }
        };

        Ok(quote! {
            #chunk_def
            #chunk_traits
            #blobbable_impl
            #strategy_impl
        })
    }
}

