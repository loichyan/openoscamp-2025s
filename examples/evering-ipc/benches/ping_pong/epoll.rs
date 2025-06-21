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
            block_on(async {
                let listener = UnixListener::bind(&sock).unwrap();
                started_tx.send(()).unwrap();
                let worker = |mut conn: UnixStream| async move {
                    let wbuf = vec![BUFVAL; bufsize];
                    loop {
                        match conn.read_i32().await {
                            Ok(i) => {
                                assert_eq!(i, PING);
                                conn.write_i32(PONG).await.unwrap();
                                conn.write_all(&wbuf).await.unwrap();
                                conn.flush().await.unwrap();
                            },
                            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                                return;
                            },
                            Err(e) => panic!("{e}"),
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
            block_on(async {
                started_rx.await.unwrap();
                let tasks = std::iter::repeat_with(|| {
                    let sock = sock.clone();
                    async move {
                        let mut conn = UnixStream::connect(sock).await.unwrap();
                        let mut rbuf = vec![0; bufsize];
                        for _ in 0..(iters / CONCURRENCY) {
                            conn.write_i32(PING).await.unwrap();
                            conn.flush().await.unwrap();
                            assert_eq!(conn.read_i32().await.unwrap(), PONG);
                            conn.read_exact(&mut rbuf).await.unwrap();
                            assert!(rbuf.iter().all(|b| *b == BUFVAL));
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
