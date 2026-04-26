use netabase_macros::{NetabaseModel, netabase_definition, netabase_repository};
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_repository(MainRepository, subscriptions(SystemEvent))]
pub mod my_repository {
    use super::*;

    #[netabase_definition(UserDefinition, repository(MainRepository))]
    pub mod user_def {
        use super::*;

        #[derive(Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(UserDefinition))]
        pub struct User {
            #[netabase(PrimaryKey)]
            pub id: u32,
        }
    }

    #[netabase_definition(PostDefinition, repository(MainRepository))]
    pub mod post_def {
        use super::*;

        #[derive(Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(PostDefinition))]
        pub struct Post {
            #[netabase(PrimaryKey)]
            pub id: u64,
        }
    }
}

fn main() {}
