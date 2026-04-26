// @review [ ]
use netabase_macros::NetabaseModel;

#[derive(NetabaseModel)]
pub enum UserVariant {
    Created { id: u32 },
    Updated { id: u32 },
    Deleted { id: u32 },
}

fn main() {}
