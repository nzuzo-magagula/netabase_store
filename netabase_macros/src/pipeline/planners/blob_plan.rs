// @review [ ]
use crate::pipeline::visitors::{
    BlobStrategy, NetabaseBlobContainerAttr, NetabaseBlobData, NetabaseBlobFieldAttr,
    NetabaseBlobVisitor,
};
use proc_macro_flow_core::traits::structural::generator::FlowGeneratorInput;
use proc_macro_flow_core::traits::structural::planner::FlowPlanner;
use proc_macro_flow_core::traits::structural::validation::FlowValidate;
use syn::{Generics, Ident, Type};

pub struct NetabaseBlobPlan<'ast> {
    pub ident: &'ast Ident,
    pub generics: &'ast Generics,
    pub strategy: BlobStrategy,
    pub blobbable_fields: Vec<NetabaseBlobFieldPlan<'ast>>,
}

pub struct NetabaseBlobFieldPlan<'ast> {
    pub ident: &'ast Ident,
    pub ty: &'ast Type,
    pub index: usize,
}

impl<'ast> FlowGeneratorInput for NetabaseBlobPlan<'ast> {}
impl<'ast> FlowGeneratorInput for NetabaseBlobFieldPlan<'ast> {}

impl<'ast> FlowPlanner for NetabaseBlobPlan<'ast> {
    type Input = NetabaseBlobVisitor<'ast>;

    fn plan(visitor: Self::Input) -> syn::Result<Self> {
        let mut strategy = BlobStrategy::whole;
        for attr in &visitor.container_attrs {
            match attr {
                NetabaseBlobContainerAttr::strategy(s) => strategy = s.clone(),
            }
        }

        let mut blobbable_fields = Vec::new();
        match visitor.data {
            NetabaseBlobData::Struct { fields } => {
                for (i, field_visitor) in fields.into_iter().enumerate() {
                    let mut is_blobbable = false;
                    for attr in &field_visitor.attrs {
                        match attr {
                            NetabaseBlobFieldAttr::blobbable => is_blobbable = true,
                        }
                    }
                    if is_blobbable {
                        blobbable_fields.push(NetabaseBlobFieldPlan {
                            ident: field_visitor.ident,
                            ty: field_visitor.ty,
                            index: i,
                        });
                    }
                }
            }
            NetabaseBlobData::Enum { .. } => {
                // Enums are not supported for blobbable fields in this version
            }
        }

        Ok(Self {
            ident: visitor.ident,
            generics: visitor.generics,
            strategy,
            blobbable_fields,
        })
    }
}

impl<'ast> FlowValidate for NetabaseBlobPlan<'ast> {
    type Error = syn::Error;

    fn validate(&self) -> std::result::Result<(), Vec<Self::Error>> {
        if self.strategy == BlobStrategy::field && self.blobbable_fields.is_empty() {
            return Err(vec![syn::Error::new(
                self.ident.span(),
                "Field strategy requires at least one blobbable field",
            )]);
        }
        Ok(())
    }
}
