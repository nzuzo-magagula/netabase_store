use netabase_arena::fixed::NbString;
use netabase_macros::{NetabaseModel, netabase_definition, netabase_repository};
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_repository(MainRepository)]
pub mod repositories {
    use super::*;

    #[netabase_definition(UserDefinition, repository(MainRepository))]
    pub mod definition {
        use super::*;

        #[derive(Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
    use repositories::definition::User;
    use repositories::UserDefinition;

    type R = MainRepositoryItem;
    type D = UserDefinition;
    type M = User;

    fn assert_is_model<R, D, M>()
    where
        R: netabase_store::traits::structural::schema::repositories::NetabaseRepository,
        D: netabase_store::traits::structural::schema::definitions::NetabaseDefinition<R>,
        M: netabase_store::traits::structural::schema::models::NetabaseModel<R, D>,
    {
    }

    assert_is_model::<R, D, M>();
}
