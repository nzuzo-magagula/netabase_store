// @review [ ]
use crate::traits::structural::{
    database::tables::TableKey,
    schema::{
        definitions::NetabaseDefinition, models::NetabaseModelWithKeys,
        repositories::NetabaseRepository,
    },
};

pub trait PrimaryKey<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>: TableKey
{
    const TABLE_NAME: &'static str;
}
