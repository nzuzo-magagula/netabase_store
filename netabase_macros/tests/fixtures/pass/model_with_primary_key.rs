use netabase_macros::{NetabaseModel, netabase_definition};
use netabase_store::traits::structural::schema::repositories::NoRepository;
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_definition(UserDefinition, repository(NoRepository))]
pub mod definition {
    use super::*;

    #[derive(Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    #[rkyv(derive(Debug))]
    #[netabase(definition(UserDefinition))]
    pub struct User {
        #[netabase(PrimaryKey)]
        pub id: u32,
    }
}

fn main() {}
