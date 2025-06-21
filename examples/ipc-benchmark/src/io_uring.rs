use tokio::task::spawn_local;
use tokio_uring::BufResult;
use tokio_uring::buf::BoundedBuf;
use tokio_uring::net::{UnixListener, UnixStream};

use super::*;

macro_rules! with {
    ($buf:ident = $expr:expr) => {{
        let r;
        (r, $buf) = $expr;
        r
    }};
}

macro_rules! tri {
    ($buf:ident = $expr:expr) => {{
        let r;
        (r, $buf) = $expr;
        match r {
            Ok(t) => t,
            Err(e) => return (Err(e.into()), $buf.into()),
        }
    }};
}

pub fn bench(id: &str, iters: usize, bufsize: usize) -> Duration {
    let sock = Path::new("/dev/shm").join(make_shmid(id));

    let mut elapsed = Duration::ZERO;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let (exited_tx, mut exited_rx) = tokio::sync::oneshot::channel::<()>();
    std::thread::scope(|cx| {
        // Server
        cx.spawn(|| {
            tokio_uring::start(async {
                let listener = UnixListener::bind(&sock).unwrap();
                started_tx.send(()).unwrap();
                let worker = |conn: UnixStream| async move {
                    let conn = &conn;
                    let mut wbuf = vec![0; bufsize];
                    loop {
                        let r;
                        (r, wbuf) = read_i32(conn, wbuf).await;
                        match r {
                            Ok(i) => {
                                assert_eq!(i, PING);
                                with!(wbuf = write_i32(conn, wbuf, PONG).await).unwrap();
                                wbuf.fill(BUFVAL);
                                with!(wbuf = conn.write_all(wbuf).await).unwrap();
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
                        r = listener.accept() => { spawn_local(worker(r.unwrap())); },
                        _ = &mut exited_rx =>  break,
                    }
                }
            });
        });
        // Client
        cx.spawn(|| {
            tokio_uring::start(async {
                started_rx.await.unwrap();
                let tasks = std::iter::repeat_with(|| {
                    let sock = sock.clone();
                    async move {
                        let conn = UnixStream::connect(sock).await.unwrap();
                        let mut rbuf = vec![0; bufsize];
                        for _ in 0..(iters / CONCURRENCY) {
                            with!(rbuf = write_i32(&conn, rbuf, PING).await).unwrap();
                            let i = with!(rbuf = read_i32(&conn, rbuf).await).unwrap();
                            assert_eq!(i, PONG);
                            with!(rbuf = read_exact(&conn, rbuf, bufsize).await).unwrap();
                            assert!(rbuf.iter().all(|b| *b == BUFVAL));
                        }
                    }
                })
                .map(tokio_uring::spawn)
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

async fn read_exact(conn: &UnixStream, buf: Vec<u8>, mut size: usize) -> BufResult<(), Vec<u8>> {
    assert!(buf.len() >= size);
    let mut sbuf = buf.slice(..size);
    let (r, sbuf) = async {
        loop {
            let n = tri!(sbuf = conn.read(sbuf).await);
            size -= n;
            if size == 0 {
                return (Ok(()), sbuf);
            }
            sbuf = sbuf.slice(n..);
        }
    }
    .await;
    (r, sbuf.into_inner())
}

async fn read_i32(conn: &UnixStream, mut buf: Vec<u8>) -> BufResult<i32, Vec<u8>> {
    tri!(buf = read_exact(conn, buf, 4).await);
    let i = i32::from_be_bytes(buf[..4].try_into().expect("buf too small"));
    (Ok(i), buf)
}

async fn write_i32(conn: &UnixStream, buf: Vec<u8>, i: i32) -> BufResult<(), Vec<u8>> {
    let mut sbuf = buf.slice(..4);
    sbuf.copy_from_slice(&i.to_be_bytes());
    let (r, sbuf) = conn.write_all(sbuf).await;
    (r, sbuf.into_inner())
}
