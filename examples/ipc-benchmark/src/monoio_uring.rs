use monoio::io::{AsyncReadRentExt, AsyncWriteRentExt};
use monoio::net::{ListenerOpts, UnixListener, UnixStream};

use super::*;

pub fn bench(id: &str, iters: usize, bufsize: usize) -> Duration {
    let sock = Path::new("/dev/shm").join(make_shmid(id));

    let mut elapsed = Duration::ZERO;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let (exited_tx, mut exited_rx) = tokio::sync::oneshot::channel::<()>();
    std::thread::scope(|cx| {
        // Server
        cx.spawn(|| {
            let respdata = make_respdata(bufsize);

            monoio::start::<monoio::IoUringDriver, _>(async {
                let listener = UnixListener::bind_with_config(
                    &sock,
                    &ListenerOpts::default().reuse_port(false),
                )
                .unwrap();
                started_tx.send(()).unwrap();

                let worker = |mut conn: UnixStream| {
                    // `pongdata` and `respdata` will never be written actually, but we
                    // need to transfer the ownship between this task and the
                    // io_uring driver.
                    let mut pongdata = PONGDATA;
                    let mut respdata = respdata.clone();
                    let mut req = vec![0; bufsize];
                    async move {
                        loop {
                            match conn.read_i32().await {
                                Ok(ping) => {
                                    assert_eq!(ping, PING);
                                    with!(req = conn.read_exact(req).await).unwrap(); // read request
                                    check_reqdata(bufsize, &req);

                                    with!(pongdata = conn.write_all(pongdata).await).unwrap();
                                    with!(respdata = conn.write_all(respdata).await).unwrap(); // write response
                                },
                                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                                    return;
                                },
                                Err(e) => panic!("{e}"),
                            }
                        }
                    }
                };
                loop {
                    monoio::select! {
                        r = listener.accept() => { monoio::spawn(worker(r.unwrap().0)); },
                        _ = &mut exited_rx =>  break,
                    }
                }
            });
        });
        // Client
        cx.spawn(|| {
            let reqdata = make_reqdata(bufsize);

            monoio::start::<monoio::IoUringDriver, _>(async {
                started_rx.await.unwrap();
                let tasks = std::iter::repeat_with(|| {
                    let sock = sock.clone();
                    let mut pingdata = PINGDATA;
                    let mut reqdata = reqdata.clone();
                    let mut resp = vec![0; bufsize];
                    async move {
                        let mut conn = UnixStream::connect(sock).await.unwrap();
                        for _ in 0..(iters / CONCURRENCY) {
                            with!(pingdata = conn.write_all(pingdata).await).unwrap();
                            with!(reqdata = conn.write_all(reqdata).await).unwrap(); // write request

                            let pong = conn.read_i32().await.unwrap();
                            assert_eq!(pong, PONG);
                            with!(resp = conn.read_exact(resp).await).unwrap(); // read response
                            check_respdata(bufsize, &resp);
                        }
                    }
                })
                .map(monoio::spawn)
                .take(CONCURRENCY)
                .collect::<Vec<_>>();

                let now = Instant::now();
                for task in tasks {
                    task.await;
                }
                elapsed = now.elapsed();
                exited_tx.send(()).unwrap();
            });
        });
    });

    _ = std::fs::remove_file(sock);
    elapsed
}
