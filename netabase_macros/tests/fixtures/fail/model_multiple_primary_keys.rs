// @review [ ]
use netabase_macros::NetabaseModel;

#[derive(NetabaseModel)]
pub struct DoublePk {
    #[netabase(primary_key)]
    id1: u64,
    #[netabase(primary_key)]
    id2: u64,
}

fn main() {}
