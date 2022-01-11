# tokio-icmp-echo

[![Latest Version](https://img.shields.io/crates/v/tokio-icmp-echo.svg)](https://crates.io/crates/tokio-icmp-echo/)
[![docs](https://docs.rs/tokio-icmp-echo/badge.svg)](https://docs.rs/tokio-icmp-echo)

tokio-icmp-echo is an asynchronous ICMP pinging library. It was originally written by Fedor Gogolev, a.k.a. knsd, and distributed under the name tokio-ping. This here is a fork that includes mostly maintenance work, to make sure it works in the current state of the async rust ecosystem.

## Updated to use DGRAM

Forked from tokio-icmp-echo, updated to use the `DGRAM` type so that you no longer need root/raw socket capabilities to send ICMP echo reply/request. This was originally merged in the kernel in [here](https://lwn.net/Articles/420800/) with ipv6 support coming later.

This fork has minor changes to the repo besides this, but I plan to make larger changes later on

## Usage example

You can use `Pinger::dgram()` to use unprivileged ICMP, or `Pinger::raw()` will require privileges.

```rust
use futures::{future, StreamExt};

#[tokio::main]
async fn main() {
    let addr = std::env::args().nth(1).unwrap().parse().unwrap();

    let pinger = tokio_icmp_echo::Pinger::dgram().unwrap();
    let stream = pinger.chain(addr).stream();
    stream
        .take(3)
        .for_each(|mb_time| {
            match mb_time {
                Ok(Some(time)) => println!("time={:?}", time),
                Ok(None) => println!("timeout"),
                Err(err) => println!("error: {:?}", err),
            }
            future::ready(())
        })
        .await;
}

```

## License

This project is licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you shall be dual licensed as above, without any additional terms or conditions.
