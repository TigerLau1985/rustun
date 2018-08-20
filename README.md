rustun
======

[![Crates.io: rustun](http://meritbadge.herokuapp.com/rustun)](https://crates.io/crates/rustun)
[![Documentation](https://docs.rs/rustun/badge.svg)](https://docs.rs/rustun)
[![Build Status](https://travis-ci.org/sile/rustun.svg?branch=master)](https://travis-ci.org/sile/rustun)
[![Code Coverage](https://codecov.io/gh/sile/rustun/branch/master/graph/badge.svg)](https://codecov.io/gh/sile/rustun/branch/master)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A Rust library for implementing STUN server and client asynchronously.

[Documentation](https://docs.rs/rustun)

The STUN protocol is defined in [RFC 5389](https://tools.ietf.org/html/rfc5389).

Installation
------------

Add following lines to your `Cargo.toml`:

```toml
[dependencies]
rustun = "0.1"
```

# Examples

A client-side example that issues a Binding request:

```rust
extern crate fibers;
extern crate rustun;

use fibers::{Executor, InPlaceExecutor, Spawn};
use rustun::{Method, Client};
use rustun::client::UdpClient;
use rustun::rfc5389;

fn main() {
    let server_addr = "127.0.0.1:3478".parse().unwrap();
    let mut executor = InPlaceExecutor::new().unwrap();

    let mut client = UdpClient::new(&executor.handle(), server_addr);
    let request = rfc5389::methods::Binding.request::<rfc5389::Attribute>();
    let future = client.call(request);

    let monitor = executor.spawn_monitor(future);
    match executor.run_fiber(monitor).unwrap() {
        Ok(v) => println!("OK: {:?}", v),
        Err(e) => println!("ERROR: {}", e),
    }
}
```

You can run example server and client which handle `Binding` method as follows:

```bash
# Starts the STUN server in a shell.
$ cargo run --example binding_srv

# Executes a STUN client in another shell.
$ cargo run --example binding_cli -- 127.0.0.1
OK: Ok(SuccessResponse {
           method: Binding,
           transaction_id: [246, 217, 191, 180, 118, 246, 250, 168, 86, 124, 126, 130],
           attributes: [XorMappedAddress(XorMappedAddress(V4(127.0.0.1:61991)))]
      })
```
