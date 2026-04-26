// Locks the Sharded<->Linear toggle: a model can opt into Linear (single per-category table)
// layout via `storage(linear)`. Both `linear` and the legacy `grouped` alias map to LinearDispatch.
use netabase_arena::fixed::NbString;
use netabase_macros::{netabase_definition, netabase_model, netabase_repository};
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_repository(LinearRepository)]
pub mod repositories {
    use super::*;

    #[netabase_definition(LinearDefinition, repository(LinearRepository))]
    pub mod linear_definition {
        use super::*;

        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(LinearDefinition), storage(linear))]
        pub struct LinearModel {
            #[netabase(primary_key)]
            pub id: u64,
            #[netabase(secondary_key)]
            pub tag: NbString<16>,
            #[netabase(secondary_key)]
            pub score: u32,
        }
    }
}

fn main() {}
