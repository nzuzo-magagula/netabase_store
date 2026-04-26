// @review [x]
use crate::traits::structural::{
    database::tables::TableKey,
    schema::{
        definitions::NetabaseDefinition, models::NetabaseModelWithKeys,
        repositories::NetabaseRepository,
    },
};

// Query(#id-3e8): Q[Tr(BlobKeysEnum)], Is this Still used?
pub trait BlobKeysEnum<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>
{
}

// Query(#blob/id-708): Q[Tr(BlobKey)], Is this still necessary?
#[cfg(feature = "std")]
pub trait BlobKey<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
    B: crate::traits::structural::schema::models::blob::Blobbable,
>: TableKey
{
}
