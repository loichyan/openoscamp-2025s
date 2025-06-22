extern crate tokio_uring;

use tokio_uring::BufResult;
use tokio_uring::buf::BoundedBuf;
use tokio_uring::net::{UnixListener, UnixStream};

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

            tokio_uring::start(async {
                let listener = UnixListener::bind(&sock).unwrap();
                started_tx.send(()).unwrap();
                let worker = |conn: UnixStream| {
                    // `pongdata` and `respdata` will never be written actually, but we
                    // need to transfer the ownship between this task and the
                    // io_uring driver.
                    let mut pongdata = PONGDATA;
                    let mut respdata = respdata.clone();
                    // TODO: use fixed buffer
                    let mut ping = vec![0; 4];
                    let mut req = vec![0; bufsize];
                    async move {
                        loop {
                            match with!(ping = read_i32(&conn, ping).await) {
                                Ok(ping) => {
                                    assert_eq!(ping, PING);
                                    with!(req = read_exact(&conn, req, bufsize).await).unwrap(); // read request
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
                    tokio::select! {
                        r = listener.accept() => { tokio_uring::spawn(worker(r.unwrap())); },
                        _ = &mut exited_rx =>  break,
                    }
                }
            });
        });
        // Client
        cx.spawn(|| {
            let reqdata = make_reqdata(bufsize);

            tokio_uring::start(async {
                started_rx.await.unwrap();
                let tasks = std::iter::repeat_with(|| {
                    let sock = sock.clone();
                    let mut pingdata = PINGDATA;
                    let mut reqdata = reqdata.clone();
                    let mut resp = vec![0; bufsize];
                    async move {
                        let conn = UnixStream::connect(sock).await.unwrap();
                        for _ in 0..(iters / CONCURRENCY) {
                            with!(pingdata = conn.write_all(pingdata).await).unwrap();
                            with!(reqdata = conn.write_all(reqdata).await).unwrap(); // write request

                            let pong = with!(resp = read_i32(&conn, resp).await).unwrap();
                            assert_eq!(pong, PONG);
                            with!(resp = read_exact(&conn, resp, bufsize).await).unwrap(); // read response
                            check_respdata(bufsize, &resp);
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
