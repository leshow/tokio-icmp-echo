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
