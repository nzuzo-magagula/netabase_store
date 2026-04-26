// @review [ ]
use netabase_macros::netabase_definition;
use netabase_store::traits::structural::schema::repositories::NoRepository;

pub mod Global {
    pub enum Events { All }
}

#[netabase_definition(FullUserDefinition, repository(NoRepository), subscriptions(UserCreated, UserDeleted), subscribe(Global::Events))]
pub mod definition {
}

fn main() {
    use FullUserDefinition;
}
