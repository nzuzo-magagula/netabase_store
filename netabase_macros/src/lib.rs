// @review [ ]
// The derive entry points (`NetabaseModel`, `NetabaseBlob`) are PascalCase by
// convention — they mirror the trait/derive name — so the function-name lint
// does not apply to this crate's public macro surface.
#![allow(non_snake_case)]
extern crate proc_macro;

#[allow(unused_imports)]
use proc_macro::TokenStream;
use proc_macro_flow_core::{attribute_mut_pipeline, derive_mut_pipeline, derive_pipeline};
use proc_macro_flow_core::traits::structural::pipeline::PlannedPipeline;

mod pipeline;

derive_mut_pipeline!(
    NetabaseModel,
    crate::pipeline::NetabaseModelPipeline,
    attributes(netabase)
);

attribute_mut_pipeline!(
    netabase_model,
    crate::pipeline::NetabaseModelPipeline
);

derive_pipeline!(
    NetabaseBlob,
    crate::pipeline::NetabaseBlobPipeline,
    attributes(blob)
);

macro_rules! attribute_pipeline {
    ($name:ident, $pipeline:ty) => {
        #[proc_macro_attribute]
        pub fn $name(attr: TokenStream, item: TokenStream) -> TokenStream {
            let input = syn::parse_macro_input!(item as <$pipeline as PlannedPipeline>::Input);
            let attr = proc_macro2::TokenStream::from(attr);

            match <$pipeline as PlannedPipeline>::execute_attribute(attr, &input) {
                Ok(output) => output.into(),
                Err(err) => err.to_compile_error().into(),
            }
        }
    };
}

attribute_pipeline!(
    netabase_definition,
    crate::pipeline::NetabaseDefinitionPipeline
);
attribute_pipeline!(
    netabase_repository,
    crate::pipeline::NetabaseRepositoryPipeline
);

#[proc_macro_attribute]
pub fn netabase_internal_repo(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
