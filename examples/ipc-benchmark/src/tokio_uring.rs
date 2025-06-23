extern crate tokio_uring;

use tokio_uring::BufResult;
use tokio_uring::buf::fixed::{FixedBuf, FixedBufPool};
use tokio_uring::buf::{BoundedBuf, IoBuf, Slice};
use tokio_uring::net::{UnixListener, UnixStream};

use super::*;

pub fn bench(id: &str, iters: usize, bufsize: usize) -> Duration {
    let sock = Path::new("/dev/shm").join(make_shmid(id));

    let make_pool = || {
        let pool = FixedBufPool::new(std::iter::repeat_with(|| vec![0; 4]).take(CONCURRENCY));
        pool.register().unwrap();
        pool
    };

    let mut elapsed = Duration::ZERO;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let (exited_tx, mut exited_rx) = tokio::sync::oneshot::channel::<()>();
    std::thread::scope(|cx| {
        // Server
        cx.spawn(|| {
            let respdata = make_respdata(bufsize);

            tokio_uring::start(async {
                let pool = make_pool();

                let listener = UnixListener::bind(&sock).unwrap();
                started_tx.send(()).unwrap();
                let worker = |conn: UnixStream| {
                    let pool = pool.clone();
                    // `pongdata` and `respdata` will never be written actually, but we
                    // need to transfer the ownship between this task and the
                    // io_uring driver.
                    let mut pongdata = PONGDATA;
                    let mut respdata = respdata.clone();
                    let mut req = vec![0; bufsize];
                    async move {
                        let mut ping = pool.next(4).await;
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

                pool.unregister().unwrap();
            });
        });
        // Client
        cx.spawn(|| {
            let reqdata = make_reqdata(bufsize);

            tokio_uring::start(async {
                let pool = make_pool();

                started_rx.await.unwrap();
                let tasks = std::iter::repeat_with(|| {
                    let sock = sock.clone();
                    let pool = pool.clone();

                    let mut pingdata = PINGDATA;
                    let mut reqdata = reqdata.clone();
                    let mut resp = vec![0; bufsize];
                    async move {
                        let mut pong = pool.next(4).await;

                        let conn = UnixStream::connect(sock).await.unwrap();
                        for _ in 0..(iters / CONCURRENCY) {
                            with!(pingdata = conn.write_all(pingdata).await).unwrap();
                            with!(reqdata = conn.write_all(reqdata).await).unwrap(); // write request

                            let pong = with!(pong = read_i32(&conn, pong).await).unwrap();
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

                pool.unregister().unwrap();
            });
        });
    });

    _ = std::fs::remove_file(sock);
    elapsed
}

async fn read_exact<B>(conn: &UnixStream, buf: B, mut size: usize) -> BufResult<(), B>
where
    B: IoBuf,
    Slice<B>: MaybeFixed<Inner = B>,
{
    let mut buf = buf.slice_full();
    buf = buf.slice(..size);
    loop {
        let n = tri!(buf = buf.read(conn).await, Slice::into_inner);
        size -= n;
        if size == 0 {
            return (Ok(()), buf.into_inner());
        }
        buf = buf.slice(n..);
    }
}

async fn read_i32(conn: &UnixStream, mut buf: FixedBuf) -> BufResult<i32, FixedBuf> {
    tri!(buf = read_exact(conn, buf, 4).await);
    let i = i32::from_be_bytes(buf[..4].try_into().expect("buf too small"));
    (Ok(i), buf)
}

trait MaybeFixed: From<Slice<Self::Inner>> + Into<Slice<Self::Inner>> {
    type Inner: IoBuf;

    async fn read(self, conn: &UnixStream) -> BufResult<usize, Self>;
}

impl MaybeFixed for Slice<FixedBuf> {
    type Inner = FixedBuf;

    async fn read(self, conn: &UnixStream) -> BufResult<usize, Self> {
        conn.read_fixed(self).await
    }
}

impl MaybeFixed for Slice<Vec<u8>> {
    type Inner = Vec<u8>;

    async fn read(self, conn: &UnixStream) -> BufResult<usize, Self> {
        conn.read(self).await
    }
}
