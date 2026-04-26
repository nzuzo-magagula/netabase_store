// The type policy rejects `String` model fields with a replacement hint.
use netabase_macros::{NetabaseModel, netabase_definition};
use netabase_store::traits::structural::schema::repositories::NoRepository;
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_definition(D, repository(NoRepository))]
pub mod definition {
    use super::*;

    #[derive(Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    #[rkyv(derive(Debug))]
    #[netabase(definition(D))]
    pub struct M {
        #[netabase(PrimaryKey)]
        pub id: u32,
        pub name: String,
    }
}

fn main() {}
