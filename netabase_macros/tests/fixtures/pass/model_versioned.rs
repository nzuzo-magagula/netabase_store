use netabase_arena::fixed::NbString;
use netabase_macros::{NetabaseModel, netabase_definition};
use netabase_store::traits::structural::schema::repositories::NoRepository;
use rkyv::{Archive, Deserialize, Serialize};

// V1 — original model, no version attribute.
#[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[rkyv(derive(Debug))]
pub struct UserV1 {
    pub id: u32,
    pub name: NbString<32>,
}

#[netabase_definition(UserDefinition, repository(NoRepository))]
pub mod definition {
    use super::*;

    // V2 — adds email; declares V1 as previous version.
    #[derive(Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    #[rkyv(derive(Debug))]
    #[netabase(definition(UserDefinition), version(prev = super::UserV1))]
    pub struct UserV2 {
        #[netabase(PrimaryKey)]
        pub id: u32,
        pub name: NbString<32>,
        pub email: NbString<48>,
    }
}

// User supplies the migration; the macro enforces the bound at compile time.
impl From<UserV1> for definition::UserV2 {
    fn from(v1: UserV1) -> Self {
        definition::UserV2 {
            id: v1.id,
            name: v1.name,
            email: NbString::new(),
        }
    }
}

fn upgrade<V: definition::UserV2Versioned>(v: V) -> definition::UserV2 {
    v.to_current()
}

fn main() {
    let v1 = UserV1 {
        id: 1,
        name: NbString::try_from_str("Alice").unwrap(),
    };
    let v2: definition::UserV2 = upgrade(v1);
    assert_eq!(v2.id, 1);
    assert_eq!(v2.name.as_str(), "Alice");
    assert_eq!(v2.email.as_str(), "");
}
