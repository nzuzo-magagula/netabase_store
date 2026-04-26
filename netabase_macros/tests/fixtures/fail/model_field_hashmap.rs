// The type policy rejects `HashMap` model fields with a replacement hint.
use netabase_macros::{NetabaseModel, netabase_definition};
use netabase_store::traits::structural::schema::repositories::NoRepository;
use rkyv::{Archive, Deserialize, Serialize};
use std::collections::HashMap;

#[netabase_definition(D, repository(NoRepository))]
pub mod definition {
    use super::*;

    #[derive(Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    #[rkyv(derive(Debug))]
    #[netabase(definition(D))]
    pub struct M {
        #[netabase(PrimaryKey)]
        pub id: u32,
        pub meta: HashMap<u32, u32>,
    }
}

fn main() {}
