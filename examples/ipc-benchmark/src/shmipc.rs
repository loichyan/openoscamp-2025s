extern crate shmipc;

use std::os::unix::net::SocketAddr;

use shmipc::config::SizePercentPair;
use shmipc::consts::MemMapType;
use shmipc::{
    AsyncReadShm, AsyncWriteShm, Error, Listener, SessionManager, SessionManagerConfig, Stream,
};
use tokio::task::spawn_local;

use super::*;

type Result<T, E = Error> = std::result::Result<T, E>;

pub fn bench(id: &str, iters: usize, bufsize: usize) -> Duration {
    let shmid = make_shmid(id);
    let shmpath = format!("/dev/shm/{shmid}");
    let queuepath = format!("/dev/shm/{shmid}_queue");
    let sockpath = format!("/dev/shm/{shmid}_sock");

    let sockaddr = SocketAddr::from_pathname(&sockpath).unwrap();
    let sm_config = {
        let mut config = shmipc::config::Config::new();

        config.mem_map_type = MemMapType::MemMapTypeMemFd;
        config.share_memory_buffer_cap = shmsize(bufsize) as u32;
        config.share_memory_path_prefix = shmpath.clone();
        config.buffer_slice_sizes = vec![
            SizePercentPair {
                size: bufsize as u32 + 1024,
                percent: 70,
            },
            SizePercentPair {
                size: (16 << 10) + 1024,
                percent: 20,
            },
            SizePercentPair {
                size: (64 << 10) + 1024,
                percent: 10,
            },
        ];

        config.queue_cap = 65536;
        config.queue_path = queuepath.clone();
        config.connection_write_timeout = std::time::Duration::from_secs(1);

        SessionManagerConfig::new()
            .with_config(config)
            .with_session_num(1)
    };

    let mut elapsed = Duration::ZERO;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let (exited_tx, mut exited_rx) = tokio::sync::oneshot::channel::<()>();
    std::thread::scope(|cx| {
        // Server
        cx.spawn(|| {
            let resp = make_resp(bufsize);
            tokio_block_on_current(async {
                let mut listener = Listener::new(sockaddr.clone(), sm_config.config().clone())
                    .await
                    .unwrap();
                started_tx.send(()).unwrap();
                let worker = |mut conn: Stream| {
                    let resp = resp.clone();
                    async move {
                        let conn = &mut conn;
                        loop {
                            match read_i32(conn).await {
                                Ok(ping) => {
                                    assert_eq!(ping, PING);
                                    conn.release_read_and_reuse();

                                    write_i32(conn, PONG).unwrap();
                                    write_all(conn, &resp).unwrap();
                                    must_flush(conn, false).await.unwrap();
                                },
                                Err(Error::StreamClosed | Error::EndOfStream) => break,
                                Err(e) => panic!("{e}"),
                            }
                        }
                    }
                };
                loop {
                    tokio::select! {
                        r = listener.accept() => {
                            let conn = r.unwrap().unwrap();
                            spawn_local(worker(conn));
                        },
                        _ = &mut exited_rx => {
                            listener.close().await;
                            break;
                        },
                    }
                }
            });
        });
        // Client
        cx.spawn(|| {
            tokio_block_on_current(async {
                started_rx.await.unwrap();
                let client = SessionManager::new(sm_config.clone(), sockaddr.clone())
                    .await
                    .unwrap();
                let tasks = std::iter::repeat_with(|| {
                    let client = client.clone();
                    async move {
                        let mut conn = client.get_stream().unwrap();
                        let conn = &mut conn;
                        for _ in 0..(iters / CONCURRENCY) {
                            write_i32(conn, PING).unwrap();
                            must_flush(conn, false).await.unwrap();

                            let pong = read_i32(conn).await.unwrap();
                            assert_eq!(pong, PONG);
                            let resp = conn.read_bytes(bufsize).await.unwrap();
                            check_resp(bufsize, &resp);
                            conn.release_read_and_reuse();
                        }
                        conn.close().await.unwrap();
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
                client.close().await;
                exited_tx.send(()).unwrap();
            });
        });
    });

    for f in [shmpath, queuepath, sockpath] {
        _ = std::fs::remove_file(f);
    }
    elapsed
}

pub async fn read_i32(conn: &mut Stream) -> Result<i32> {
    let buf = conn.read_bytes(4).await?;
    Ok(i32::from_be_bytes((*buf).try_into().unwrap()))
}

pub fn write_i32(conn: &mut Stream, i: i32) -> Result<()> {
    write_all(conn, &i.to_be_bytes())
}

pub fn write_all(conn: &mut Stream, mut data: &[u8]) -> Result<()> {
    while !data.is_empty() {
        let n = conn.write_bytes(data)?;
        data = &data[n..];
    }
    Ok(())
}

pub async fn must_flush(conn: &mut Stream, eos: bool) -> Result<()> {
    loop {
        match conn.flush(eos).await {
            Ok(_) => return Ok(()),
            Err(Error::QueueFull) => {
                tokio::time::sleep(Duration::from_micros(1)).await;
                continue;
            },
            Err(e) => return Err(e),
        }
    }
}
