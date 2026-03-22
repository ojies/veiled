pub mod db;
pub mod service;
pub mod store;
pub mod wallet;

pub mod pb {
    tonic::include_proto!("registry");
}
