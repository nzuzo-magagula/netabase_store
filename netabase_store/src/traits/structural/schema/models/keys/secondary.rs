// @review [ ]
use crate::traits::structural::{
    database::tables::TableKey,
    schema::{
        definitions::NetabaseDefinition, models::NetabaseModelWithKeys,
        repositories::NetabaseRepository,
    },
};

pub trait SecondaryKeysEnum<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
> where
    Self: strum::IntoDiscriminant,
{
}

pub trait SecondaryKey<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>: TableKey
{
    type ParentEnum: SecondaryKeysEnum<R, D, M>;
    const TABLE_ID: &'static str;
}
