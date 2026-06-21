// @review [~]
pub mod definition;
pub mod model;
pub mod repository;

pub trait Addressable {
    type Address: Address;
}

pub trait Address {}
