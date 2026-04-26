use netabase_arena::fixed::NbString;
use netabase_macros::{netabase_definition, netabase_model, netabase_repository};
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_repository(MainRepository)]
pub mod repositories {
    use super::*;

    #[netabase_definition(UserDefinition, repository(MainRepository))]
    pub mod definition {
        use super::*;

        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(UserDefinition))]
        pub struct User {
            #[netabase(PrimaryKey)]
            pub id: u32,
            pub name: NbString<32>,
        }
    }
}

fn main() {
    use repositories::definition::{User, UserPrimaryKey};

    // The 'id' field was mutated to the 'UserPrimaryKey' newtype.
    let user = User {
        id: UserPrimaryKey(1),
        name: NbString::try_from_str("Alice").unwrap(),
    };

    assert_eq!(user.id.0, 1);

    let pk = user.primary_key();
    assert_eq!(pk.0, 1);
}
