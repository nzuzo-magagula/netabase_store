// @review [ ]
use crate::traits::structural::contract::Scope;

pub mod repo_keys;
pub mod repo_tables;
pub mod repo_values;

pub trait Repository: Scope {}

pub struct NoRepository;

impl Scope for NoRepository {
    type Tables;
}

impl Repository for NoRepository {}
