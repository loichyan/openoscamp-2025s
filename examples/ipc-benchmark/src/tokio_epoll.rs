use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::spawn_local;

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

            tokio_block_on_current(async {
                let listener = UnixListener::bind(&sock).unwrap();
                started_tx.send(()).unwrap();
                let worker = |mut conn: UnixStream| {
                    let respdata = respdata.clone();
                    let mut req = vec![0; bufsize];
                    async move {
                        loop {
                            match conn.read_i32().await {
                                Ok(ping) => {
                                    assert_eq!(ping, PING);
                                    conn.read_exact(&mut req).await.unwrap(); // read request
                                    check_reqdata(bufsize, &req);

                                    conn.write_i32(PONG).await.unwrap();
                                    conn.write_all(&respdata).await.unwrap(); // write response
                                    conn.flush().await.unwrap();
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
                    tokio::select! {
                        r = listener.accept() => {
                            let (conn, _) = r.unwrap();
                            spawn_local(worker(conn));
                        },
                        _ = &mut exited_rx =>  break,
                    }
                }
            });
        });
        // Client
        cx.spawn(|| {
            let reqdata = make_reqdata(bufsize);

            tokio_block_on_current(async {
                started_rx.await.unwrap();
                let tasks = std::iter::repeat_with(|| {
                    let sock = sock.clone();
                    let reqdata = reqdata.clone();
                    let mut resp = vec![0; bufsize];
                    async move {
                        let mut conn = UnixStream::connect(sock).await.unwrap();
                        for _ in 0..(iters / CONCURRENCY) {
                            conn.write_i32(PING).await.unwrap();
                            conn.write_all(&reqdata).await.unwrap(); // write request
                            conn.flush().await.unwrap();

                            let pong = conn.read_i32().await.unwrap();
                            assert_eq!(pong, PONG);
                            conn.read_exact(&mut resp).await.unwrap(); // read response
                            check_respdata(bufsize, &resp);
                        }
                    }
                })
                .map(spawn_local)
                .take(CONCURRENCY)
                .collect::<Vec<_>>();

                let now = Instant::now();
                for task in tasks {
                    task.await.unwrap();
                }
                elapsed = now.elapsed();
                exited_tx.send(()).unwrap();
            });
        });
    });

    _ = std::fs::remove_file(sock);
    elapsed
}
