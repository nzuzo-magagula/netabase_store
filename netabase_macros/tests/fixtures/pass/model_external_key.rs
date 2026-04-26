use netabase_arena::fixed::NbString;
use netabase_macros::{netabase_definition, netabase_model, netabase_repository};
use rkyv::{Archive, Deserialize, Serialize};

pub fn my_hash_fn(user: &repositories::definition::User) -> repositories::definition::UserPrimaryKey {
    repositories::definition::UserPrimaryKey(user.name.len() as u32)
}

#[netabase_repository(MainRepository)]
pub mod repositories {
    use super::*;

    #[netabase_definition(UserDefinition, repository(MainRepository))]
    pub mod definition {
        use super::*;

        #[netabase_model(primary_key_type = u32, hash_fn = crate::my_hash_fn)]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(UserDefinition))]
        pub struct User {
            pub name: NbString<32>,
        }
    }
}

fn main() {
    use repositories::definition::{User, UserRecord};

    let user = User {
        name: NbString::try_from_str("Alice").unwrap(),
    };

    // primary_key() calls hash_fn.
    let pk = user.primary_key();
    assert_eq!(pk.0, 5); // "Alice".len() == 5

    let _record = UserRecord(pk, user);
}
