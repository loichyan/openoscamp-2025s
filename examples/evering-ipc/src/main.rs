#![feature(layout_for_ptr)]
#![feature(local_waker)]

use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use argh::FromArgs;
use bytesize::ByteSize;
use evering::uring::Uring;
use evering_ipc::{
    ClientUring, Rqe, RqeData, Runtime, RuntimeHandle, ServerUring, ShmBox, ShmHeader, Sqe,
    SqeData, UringBuilder, op,
};

#[derive(Debug, FromArgs)]
/// IPC based on shared memory
#[argh(help_triggers("--help"))]
struct Args {
    /// fd or path to shared memory
    #[argh(option, arg_name = "int|path")]
    shmfile: Shmfile,
    /// size of shared memory
    #[argh(option, arg_name = "int")]
    shmsize: ByteSize,
    /// create the specified shmfile
    #[argh(switch)]
    create: bool,
    /// type of this app, may be "client" or "server"
    #[argh(option, long = "app")]
    app: AppType,
}

#[derive(Debug)]
enum Shmfile {
    Fd(i32),
    Path(String),
}

impl Shmfile {
    fn to_fd(&self, create: bool) -> Result<OwnedFd> {
        match self {
            Shmfile::Fd(_) if create => Err(anyhow!("fd as shmfile cannot be created")),
            // SAFETY: The fd's validity is guaranteed by the parent process.
            Shmfile::Fd(f) => unsafe { Ok(OwnedFd::from_raw_fd(*f)) },
            Shmfile::Path(p) => {
                use nix::fcntl::OFlag;
                use nix::sys::stat::Mode;

                tracing::info!("created shmfile, path=/dev/shm/{p}");
                let mut oflag = OFlag::O_RDWR;
                if create {
                    oflag |= OFlag::O_CREAT | OFlag::O_EXCL;
                }
                let mode = Mode::from_bits(0o600).unwrap();
                nix::sys::mman::shm_open(p.as_str(), oflag, mode)
                    .with_context(|| format!("failed to create /dev/shm/{p}"))
            },
        }
    }

    fn unlink(&self) -> Result<()> {
        match self {
            Shmfile::Fd(_) => Ok(()),
            Shmfile::Path(p) => {
                tracing::info!("removed shmfile, path=/dev/shm/{p}");
                nix::sys::mman::shm_unlink(p.as_str())
                    .with_context(|| format!("failed to remove /dev/shm/{p}"))
            },
        }
    }
}

impl FromStr for Shmfile {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            Err("shmfile must not be empty")
        } else {
            Ok(s.parse()
                .map_or_else(|_| Self::Path(s.to_owned()), Self::Fd))
        }
    }
}

#[derive(Debug)]
enum AppType {
    Client,
    Server,
}

impl FromStr for AppType {
    type Err = &'static str;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "client" => Ok(AppType::Client),
            "server" => Ok(AppType::Server),
            _ => Err("invalid app type"),
        }
    }
}

pub fn main() -> Result<()> {
    let args = argh::from_env::<Args>();
    tracing_subscriber::fmt()
        .with_thread_names(true)
        .without_time()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let shmfd = args.shmfile.to_fd(args.create)?;
    let shmfd = shmfd.as_fd();
    let shmsize = args.shmsize.0 as usize;
    if shmsize == 0 {
        return Err(anyhow!("shmsize must not be zero"));
    }

    // SAFETY: The fd's validity is guaranteed by the parent process.
    let shm = if args.create {
        let h = UringBuilder::new().build_header();
        unsafe { ShmHeader::create(shmfd, shmsize, h)?.as_ref() }
    } else {
        unsafe { ShmHeader::open(shmfd, shmsize)?.as_ref() }
    };

    let disposed = match args.app {
        AppType::Client => start_client(shm),
        AppType::Server => start_server(shm),
    };

    if disposed {
        args.shmfile.unlink()?;
    }

    Ok(())
}

fn start_client(shm: &'static ShmHeader) -> bool {
    evering_ipc::shm::init_client(shm);
    let sq = unsafe { ClientUring::from_raw(shm.build_raw_uring()) };
    tracing::info!("started client, connected={}", sq.is_connected());

    let rt = Runtime::new(sq);
    rt.block_on(async {
        let tasks = (0..)
            .map(|i| async move {
                let delay = fastrand::u64(50..500);
                tracing::info!("requested ping({i}), delay={delay:?}ms");

                let buf = ShmBox::new_uninit_slice(fastrand::usize(8..=32));
                let now = std::time::Instant::now();
                let (pong, buf) = op::ping(fastrand::i32(..), buf).await;
                let elapsed = now.elapsed().as_millis();

                let bstr = std::str::from_utf8(&buf).unwrap();
                tracing::info!(
                    "responded pong({i}), elapsed={elapsed}ms, pong={pong:x}, buf={bstr}"
                );
            })
            .map(RuntimeHandle::spawn)
            .take(fastrand::usize(32..=64))
            .collect::<Vec<_>>();

        for task in tasks {
            task.await;
        }
        op::exit().await;
        tracing::info!("exited client");
    });

    rt.into_uring().dispose_raw().is_ok()
}

fn start_server(shm: &'static ShmHeader) -> bool {
    evering_ipc::shm::init_server(shm);
    let mut rq = unsafe { ServerUring::from_raw(shm.build_raw_uring()) };
    tracing::info!("started server, connected={}", rq.is_connected());

    let mut local_queue = Vec::new();
    loop {
        let mut should_exit = false;
        if let Some(Sqe { id, data }) = rq.recv() {
            tracing::info!("accepted request, data={data:x?}");
            let data = match data {
                SqeData::Exit => {
                    should_exit = true;
                    RqeData::Exited
                },
                SqeData::Ping { ping, buf } => {
                    let delay = (ping as u64 % 450) + 50;
                    std::thread::sleep(Duration::from_millis(delay));
                    unsafe {
                        let mut buf = buf.as_ptr();
                        for c in buf.as_mut().iter_mut() {
                            c.write(fastrand::alphanumeric() as u32 as u8);
                        }
                    }
                    RqeData::Pong {
                        pong: fastrand::i32(..),
                    }
                },
            };
            local_queue.push(Rqe { id, data });
        }

        if local_queue.is_empty() {
            std::thread::yield_now();
        } else if should_exit || fastrand::bool() {
            // Randomize the returned response
            fastrand::shuffle(&mut local_queue);
            for rqe in local_queue.drain(..) {
                tracing::info!("replied response, data={:x?}", rqe.data);
                rq.send(rqe).expect("out of capacity");
            }
        }

        if should_exit {
            tracing::info!("exited server");
            break;
        }
    }

    rq.dispose_raw().is_ok()
}
