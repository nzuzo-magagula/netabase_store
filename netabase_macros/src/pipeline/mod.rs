// @review [ ]
pub mod generators;
pub mod validators;
// The visitor/planner structs capture the *complete* parsed schema grammar;
// the current generators consume a subset (e.g. `version_number`, `redb_mode`,
// and custom-table key/value types are parsed ahead of the features that will
// emit them), so some fields are intentionally not yet read.
#[allow(dead_code)]
pub mod planners;
#[allow(dead_code)]
pub mod visitors;

use crate::pipeline::generators::NetabaseBlobGenerator;
use crate::pipeline::generators::NetabaseModelGenerator;
use crate::pipeline::planners::NetabaseBlobPlan;
use crate::pipeline::planners::NetabaseModelPlan;
use crate::pipeline::visitors::NetabaseBlobVisitor;
use crate::pipeline::visitors::NetabaseModelVisitor;
use proc_macro_flow_core::traits::structural::pipeline::{PlannedMutationPipeline, PlannedPipeline};
use syn::DeriveInput;

use proc_macro_flow_core::traits::structural::generator::FlowGenerator;
use proc_macro_flow_core::traits::structural::validation::aggregate_errors;
use proc_macro_flow_core::traits::structural::visitor::FlowVisitor;

pub struct NetabaseModelPipeline;

impl PlannedMutationPipeline for NetabaseModelPipeline {
    type Input = DeriveInput;
    type Visitor = NetabaseModelVisitor;
    type Planner = NetabaseModelPlan;
    type Generator = NetabaseModelGenerator;

    fn execute_attribute_mut(
        attr: proc_macro2::TokenStream,
        input: &mut Self::Input,
    ) -> syn::Result<<Self::Generator as proc_macro_flow_core::traits::structural::generator::FlowMutationGenerator>::Output> {
        input.attrs.push(syn::parse_quote!(#[netabase(internal_attribute_macro)]));
        if !attr.is_empty() {
            let synthetic_attr: syn::Attribute = syn::parse_quote!(#[netabase(#attr)]);
            input.attrs.push(synthetic_attr);
        }
        Self::execute_mut(input)
    }
}

pub struct NetabaseBlobPipeline;

impl<'ast> PlannedPipeline<'ast> for NetabaseBlobPipeline {
    type Input = DeriveInput;
    type Visitor = NetabaseBlobVisitor<'ast>;
    type Planner = NetabaseBlobPlan<'ast>;
    type Generator = NetabaseBlobGenerator<'ast>;
}

pub struct NetabaseDefinitionPipeline;

impl<'ast> PlannedPipeline<'ast> for NetabaseDefinitionPipeline {
    type Input = visitors::ModWithTrailing;
    type Visitor = visitors::NetabaseDefinitionVisitor<'ast>;
    type Planner = planners::NetabaseDefinitionPlan<'ast>;
    type Generator = generators::NetabaseDefinitionGenerator<'ast>;

    fn execute_attribute(
        attr: proc_macro2::TokenStream,
        input: &'ast Self::Input,
    ) -> syn::Result<<Self::Generator as FlowGenerator>::Output> {
        let mut visitor = Self::Visitor::build(input)?;
        let macro_attrs: visitors::NetabaseDefinitionAttrList = syn::parse2(attr)?;
        visitor.attrs.extend(macro_attrs.0);

        use proc_macro_flow_core::traits::structural::validation::FlowValidate;
        if let Err(errors) = visitor.validate()
            && let Some(err) = aggregate_errors(errors) {
                return Err(err);
            }

        use proc_macro_flow_core::traits::structural::planner::FlowPlanner;
        let plan = Self::Planner::plan(visitor)?;
        if let Err(errors) = plan.validate()
            && let Some(err) = aggregate_errors(errors) {
                return Err(err);
            }

        Self::Generator::generate(&plan)
    }
}

pub struct NetabaseRepositoryPipeline;

impl<'ast> PlannedPipeline<'ast> for NetabaseRepositoryPipeline {
    type Input = visitors::ModWithTrailing;
    type Visitor = visitors::NetabaseRepositoryVisitor<'ast>;
    type Planner = planners::NetabaseRepositoryPlan<'ast>;
    type Generator = generators::NetabaseRepositoryGenerator<'ast>;

    fn execute_attribute(
        attr: proc_macro2::TokenStream,
        input: &'ast Self::Input,
    ) -> syn::Result<<Self::Generator as FlowGenerator>::Output> {
        let mut visitor = Self::Visitor::build(input)?;
        let macro_attrs: visitors::NetabaseRepositoryAttrList = syn::parse2(attr)?;
        visitor.attrs.extend(macro_attrs.0);

        use proc_macro_flow_core::traits::structural::validation::FlowValidate;
        if let Err(errors) = visitor.validate()
            && let Some(err) = aggregate_errors(errors) {
                return Err(err);
            }

        use proc_macro_flow_core::traits::structural::planner::FlowPlanner;
        let plan = Self::Planner::plan(visitor)?;
        if let Err(errors) = plan.validate()
            && let Some(err) = aggregate_errors(errors) {
                return Err(err);
            }

        Self::Generator::generate(&plan)
    }
}
