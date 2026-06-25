#[cfg(unix)]
pub mod daemon_socket;
#[cfg(unix)]
pub mod socket;
pub mod stdio;

#[cfg(unix)]
pub use socket::SocketCoreClient;
pub use stdio::StdioCoreClient;
