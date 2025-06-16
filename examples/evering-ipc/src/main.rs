#![feature(local_waker)]

mod op;
mod reactor;
mod shm;

use std::num::NonZeroUsize;
use std::os::fd::{FromRawFd, OwnedFd};
use std::rc::Rc;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Result, anyhow};
use argh::FromArgs;
use evering::uring;
use evering::uring::Uring;
use local_executor::Executor;
use nix::fcntl::OFlag;
use nix::sys::mman;
use nix::sys::mman::{MapFlags, ProtFlags};
use nix::sys::stat::Mode;

use self::op::{Rqe, RqeData, Sqe, SqeData};
use self::reactor::Reactor;

type ShmHeader = self::shm::ShmHeader<Sqe, Rqe>;
type Sender = uring::UringA<Sqe, Rqe>;
type Receiver = uring::UringB<Sqe, Rqe>;
type UringBuilder = uring::Builder<Sqe, Rqe>;

#[derive(Debug, FromArgs)]
/// IPC based on shared memory
#[argh(help_triggers("--help"))]
struct Args {
    /// fd or path to shared memory
    #[argh(option, arg_name = "int|path")]
    shmfile: Shmfile,
    /// size of shared memory
    #[argh(option, arg_name = "int")]
    shmsize: usize,
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
                tracing::info!("created shmfile, path=/dev/shm/{p}");
                let mut oflag = OFlag::O_RDWR;
                if create {
                    oflag |= OFlag::O_CREAT | OFlag::O_EXCL;
                }
                let mode = Mode::from_bits(0o644).unwrap();
                mman::shm_open(p.as_str(), oflag, mode).map_err(|e| anyhow!("shm_open({p}): {e}'"))
            },
        }
    }

    fn unlink(&self) -> Result<()> {
        match self {
            Shmfile::Fd(_) => Ok(()),
            Shmfile::Path(p) => {
                tracing::info!("removed shmfile, path=/dev/shm/{p}");
                mman::shm_unlink(p.as_str()).map_err(|e| anyhow!("shm_unlink({p}): {e}"))
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
    let shmsize =
        NonZeroUsize::new(args.shmsize).ok_or_else(|| anyhow!("shmsize must not be zero"))?;
    // SAFETY: The fd's validity is guaranteed by the parent process.
    let shmaddr = unsafe {
        nix::unistd::ftruncate(&shmfd, shmsize.get() as i64)
            .map_err(|e| anyhow!("failed to initialize shared memory: {e}"))?;
        mman::mmap(
            None,
            shmsize,
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            MapFlags::MAP_SHARED,
            &shmfd,
            0,
        )?
        .cast::<u8>()
    };

    let header = if args.create {
        let h = UringBuilder::new().build_header();
        unsafe { ShmHeader::init(shmaddr, shmsize.get(), h).as_ref() }
    } else {
        unsafe { shmaddr.cast().as_ref() }
    };

    let disposed = match args.app {
        AppType::Client => start_client(header),
        AppType::Server => start_server(header),
    };

    if disposed {
        args.shmfile.unlink()?;
    }

    Ok(())
}

fn start_client(h: &'static ShmHeader) -> bool {
    let (allocator, sq);
    unsafe {
        allocator = h.take_allocator();
        sq = Sender::from_raw(h.build_raw_uring());
    }
    tracing::info!("started client, connected={}", sq.is_connected());

    let reactor = Reactor::new(sq);
    let rt = Rc::new(Executor::new());
    rt.block_on(reactor.run_on(async {
        let tasks = (0..16)
            .map(|i| async move {
                let delay = fastrand::u64(50..500);
                tracing::info!("requested ping({i}), delay={delay:?}ms");

                let delay = fastrand::u64(0..500);
                let token = unsafe { allocator.alloc_array_uninit(fastrand::usize(8..=32)) };

                let now = std::time::Instant::now();
                let token = op::ping(h, Duration::from_millis(delay), token).await;
                let elapsed = now.elapsed().as_millis();

                let token_str = std::str::from_utf8(&token).unwrap();
                tracing::info!("responded pong({i}), elapsed={elapsed}ms, token={token_str}");

                unsafe { allocator.dealloc(token) }
            })
            .map(|fut| local_executor::spawn(Rc::downgrade(&rt), fut))
            .collect::<Vec<_>>();

        for task in tasks {
            task.await;
        }
        op::exit().await;
        tracing::info!("exited client");
    }));

    reactor.into_sender().dispose_raw().is_ok()
}

fn start_server(h: &'static ShmHeader) -> bool {
    let mut rq = unsafe { Receiver::from_raw(h.build_raw_uring()) };
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
                SqeData::Ping { delay, token } => {
                    std::thread::sleep(delay);
                    unsafe {
                        let mut token = h.get_ptr(token);
                        for c in token.as_mut().iter_mut() {
                            c.write(fastrand::alphanumeric() as u32 as u8);
                        }
                    }
                    RqeData::Pong
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
