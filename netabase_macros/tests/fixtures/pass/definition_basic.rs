// @review [ ]
use netabase_macros::netabase_definition;
use netabase_store::traits::structural::schema::repositories::NoRepository;

#[netabase_definition(UserDefinition, repository(NoRepository))]
pub mod definition {
}

fn main() {}
