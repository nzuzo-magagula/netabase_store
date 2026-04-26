use netabase_arena::fixed::{NbString, NbVec};
use netabase_macros::{NetabaseModel, netabase_definition, netabase_model, netabase_repository};
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_repository(MainRepository)]
pub mod repositories {
    use super::*;

    #[netabase_definition(MainDefinition, repository(MainRepository))]
    pub mod main_definition {
        use super::*;

        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(MainDefinition), subscriptions(OnActiveChange))]
        pub struct ComprehensiveUser {
            #[netabase(PrimaryKey)]
            pub id: u32,

            #[netabase(secondary)]
            pub username: NbString<24>,

            #[netabase(relational(to = Post, repo = MainRepositoryItem, def = MainDefinition))]
            pub posts: Vec<Post>,

            pub active: bool,

            #[netabase(blob)]
            pub profile_picture: NbVec<u8, 256>,
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
