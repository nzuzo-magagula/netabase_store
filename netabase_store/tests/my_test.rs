//! Multi-repository / multi-definition routing syntax with the fixed-width
//! model vocabulary. (The former `NetabaseBlob`-as-model-field design is gone:
//! model fields must be fixed-width, so blob data uses `NbVec<u8, N>` inline.)
use netabase_arena::fixed::{NbString, NbVec};
use netabase_macros::{netabase_definition, netabase_repository};

#[netabase_repository(Work, subscriptions(Employment, SideBusiness))]
#[netabase_repository(Projects, subscriptions(Code, Home, Life))]
#[netabase_repository(Hobbies, subscriptions(Code, Ouid))]
pub mod repo {
    use super::*;

    #[netabase_definition(
        Routine,
        subscriptions(Daily, Weekly, Monthly),
        repository(Work, Projects, Hobbies)
    )]
    #[netabase_definition(
        Goals,
        repository(Hobbies, Projects),
        subscriptions(Daily, Weekly, Monthly),
        subscribe(Hobbies::Code, Projects::Code)
    )]
    #[netabase_definition(
        HardwareHealth,
        subscriptions(Bedroom, Lounge, Office),
        subscribe(Projects::Code, Projects::Home, Hobbies::Code)
    )]
    pub mod defs {
        use super::*;
        use netabase_macros::NetabaseModel;
        use rkyv::{Archive, Deserialize, Serialize};

        #[derive(Archive, Serialize, Deserialize, NetabaseModel, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
        #[rkyv(derive(Debug))]
        pub struct Activity {
            #[netabase(primary_key)]
            id: u64,
            name: NbString<32>,
        }

        #[derive(Archive, Serialize, Deserialize, NetabaseModel, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
        #[rkyv(derive(Debug))]
        pub struct Machine {
            #[netabase(primary_key)]
            id: u64,
            #[netabase(blob)]
            data: NbVec<u8, 512>,
        }
    }
}

#[test]
fn test_syntax() {}
