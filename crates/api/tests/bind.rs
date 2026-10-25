use hermes_dash_api::BIND_ADDR;
use std::net::{Ipv4Addr, SocketAddr};

#[test]
fn production_address_is_fixed_ipv4_loopback() {
    assert_eq!(BIND_ADDR, SocketAddr::from((Ipv4Addr::LOCALHOST, 8787)));
}
