// Pure-shard dedup: `#[netabase(pure)]` makes the Primary record omit blob + relational fields
// (the aux tables hold them), rehydrating on read. Secondary fields stay in the Primary.
use netabase_arena::fixed::{NbString, NbVec};
use netabase_macros::{NetabaseModel, netabase_definition, netabase_model, netabase_repository};
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_repository(PureRepository)]
pub mod repositories {
    use super::*;

    #[netabase_definition(PureDefinition, repository(PureRepository))]
    pub mod pure_definition {
        use super::*;

        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(PureDefinition), pure)]
        pub struct PureUser {
            #[netabase(PrimaryKey)]
            pub id: u32,
            #[netabase(secondary)]
            pub name: NbString<24>,
            #[netabase(relational(to = Note))]
            pub notes: Vec<Note>,
            #[netabase(blob)]
            pub avatar: NbVec<u8, 256>,
        }

        #[derive(
            Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default,
        )]
        #[rkyv(derive(Debug))]
        #[netabase(definition(PureDefinition))]
        pub struct Note {
            #[netabase(PrimaryKey)]
            pub id: u64,
            pub text: NbString<64>,
        }
    }
}

fn main() {}
