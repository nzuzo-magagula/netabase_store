// @review [~]
pub mod backend;
pub mod contract;
pub mod definition;
pub mod model;
pub mod repository;

pub trait Addressable {
    type Address: Address;
}

pub trait Address {}
