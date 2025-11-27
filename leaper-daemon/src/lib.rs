pub mod client;

pub mod fs;

use std::{
    net::{Ipv4Addr, SocketAddrV4},
    path::PathBuf,
    sync::OnceLock,
};

use db::DB;

pub const ADDRESS: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 9876);

pub static DB_REF: OnceLock<DB> = OnceLock::new();

#[tarpc::service]
pub trait LeaperDaemon {
    async fn search_apps();
    async fn index(root: PathBuf, parents: bool);
}
