// @review [x]
use crate::errors::NetabaseError;

pub trait TransactionHooks {
    fn pre_transaction(&self) -> Result<(), NetabaseError> {
        Ok(())
    }

    fn post_transaction(&self) -> Result<(), NetabaseError> {
        Ok(())
    }
}
