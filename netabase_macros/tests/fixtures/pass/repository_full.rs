// @review [ ]
use netabase_macros::{netabase_repository, netabase_definition};
use netabase_store::traits::structural::schema::repositories::NoRepository;

#[netabase_repository(FullRepository, subscriptions(All))]
pub mod repository {
    use super::*;
    
    #[netabase_definition(FullDefinition, repository(FullRepository))]
    pub mod definition {}
}

fn main() {}
