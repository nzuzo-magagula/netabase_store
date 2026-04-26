// Should fail: From<UserV1> for UserV2 is NOT implemented.
// The macro enforces this via a `where UserV2: From<UserV1>` bound.
use netabase_arena::fixed::NbString;
use netabase_macros::{NetabaseModel, netabase_definition};
use netabase_store::traits::structural::schema::repositories::NoRepository;
use rkyv::{Archive, Deserialize, Serialize};

#[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[rkyv(derive(Debug))]
pub struct UserV1 {
    pub id: u32,
}

#[netabase_definition(UserDefinition, repository(NoRepository))]
pub mod definition {
    use super::*;

    #[derive(Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    #[rkyv(derive(Debug))]
    #[netabase(definition(UserDefinition), version(prev = super::UserV1))]
    pub struct UserV2 {
        #[netabase(PrimaryKey)]
        pub id: u32,
        pub email: NbString<48>,
    }
}

// No From<UserV1> for UserV2 impl — should fail when UserV2Versioned for UserV1 instantiates.
fn force_error(v1: UserV1) -> definition::UserV2 {
    use definition::UserV2Versioned;
    v1.to_current()
}

fn main() {}
