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
            static PONG_BYTES: &[u8] = PONG.to_be_bytes().as_slice();
            let resp = make_resp(bufsize);

            monoio::start::<monoio::IoUringDriver, _>(async {
                let listener = UnixListener::bind_with_config(
                    &sock,
                    &ListenerOpts::default().reuse_port(false),
                )
                .unwrap();
                started_tx.send(()).unwrap();

                let worker = |mut conn: UnixStream| {
                    // `pong` and `resp` will never be written actually, but we
                    // need to transfer the ownship between this task and the
                    // io_uring driver.
                    let mut pong = PONG_BYTES;
                    let mut resp = resp.clone();
                    async move {
                        loop {
                            match conn.read_i32().await {
                                Ok(ping) => {
                                    assert_eq!(ping, PING);
                                    with!(pong = conn.write_all(pong).await).unwrap();
                                    with!(resp = conn.write_all(resp).await).unwrap();
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
            static PING_BYTES: &[u8] = PING.to_be_bytes().as_slice();

            monoio::start::<monoio::IoUringDriver, _>(async {
                started_rx.await.unwrap();
                let tasks = std::iter::repeat_with(|| {
                    let sock = sock.clone();
                    let mut ping = PING_BYTES;
                    let mut resp = vec![0; bufsize];
                    async move {
                        let mut conn = UnixStream::connect(sock).await.unwrap();
                        for _ in 0..(iters / CONCURRENCY) {
                            with!(ping = conn.write_all(ping).await).unwrap();

                            let pong = conn.read_i32().await.unwrap();
                            assert_eq!(pong, PONG);
                            with!(resp = conn.read_exact(resp).await).unwrap();
                            check_resp(bufsize, &resp);
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
