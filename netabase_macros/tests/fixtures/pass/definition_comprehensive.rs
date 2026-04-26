use netabase_arena::fixed::NbString;
use netabase_macros::{NetabaseModel, netabase_definition, netabase_repository};
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_repository(MainRepository)]
pub mod repositories {
    use super::*;

    #[netabase_definition(MainDefinition, repository(MainRepository))]
    pub mod main_definition {
        use super::*;

        #[derive(Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(MainDefinition))]
        pub struct User {
            #[netabase(PrimaryKey)]
            pub id: u32,
            pub name: NbString<32>,
        }

        #[derive(Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(MainDefinition))]
        pub struct Post {
            #[netabase(PrimaryKey)]
            pub id: u64,
            pub title: NbString<48>,
        }
    }
}

fn main() {}
