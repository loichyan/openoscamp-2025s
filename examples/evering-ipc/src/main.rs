#![feature(local_waker)]

mod op;
mod reactor;

use std::alloc::Layout;
use std::mem::{align_of, size_of};
use std::num::NonZeroUsize;
use std::os::fd::{FromRawFd, OwnedFd};
use std::ptr::NonNull;
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

type Sender = uring::UringA<Sqe, Rqe>;
type Receiver = uring::UringB<Sqe, Rqe>;
type UringBuilder = uring::Builder<Sqe, Rqe>;
type RawUring = uring::RawUring<Sqe, Rqe>;

#[derive(Debug, FromArgs)]
/// IPC based on shared memory
#[argh(help_triggers("--help"))]
struct Args {
    /// fd or path to shared memory
    #[argh(option, arg_name = "int|path")]
    memfile: Memfile,
    /// size of shared memory
    #[argh(option, arg_name = "int")]
    memsize: usize,
    /// create the specified memfile
    #[argh(switch)]
    create: bool,
    /// type of this app, may be "client" or "server"
    #[argh(option, long = "type")]
    typ: AppType,
}

#[derive(Debug)]
enum Memfile {
    Fd(i32),
    Path(String),
}

impl Memfile {
    fn to_fd(&self, create: bool) -> Result<OwnedFd> {
        match self {
            Memfile::Fd(_) if create => Err(anyhow!("fd as memfile cannot be created")),
            // SAFETY: The fd's validity is guaranteed by the parent process.
            Memfile::Fd(f) => unsafe { Ok(OwnedFd::from_raw_fd(*f)) },
            Memfile::Path(p) => {
                tracing::info!("created memfile, path=/dev/shm/{p}");
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
            Memfile::Fd(_) => Ok(()),
            Memfile::Path(p) => {
                tracing::info!("removed memfile, path=/dev/shm/{p}");
                mman::shm_unlink(p.as_str()).map_err(|e| anyhow!("shm_unlink({p}): {e}"))
            },
        }
    }
}

impl FromStr for Memfile {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            Err("memfile must not be empty")
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

#[repr(C)] // Enforce `header` is at the first field
struct UringOffsets<Ext> {
    header: uring::Header<Ext>,
    // Relative offsets of uring buffers
    buf_a: usize,
    buf_b: usize,
    // Size of the entier Uring
    size: usize,
}

pub fn main() -> Result<()> {
    let args = argh::from_env::<Args>();
    tracing_subscriber::fmt()
        .with_thread_names(true)
        .without_time()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let memfd = args.memfile.to_fd(args.create)?;
    let memsize =
        NonZeroUsize::new(args.memsize).ok_or_else(|| anyhow!("memsize must not be zero"))?;
    // SAFETY: The fd's validity is guaranteed by the parent process.
    let memaddr = unsafe {
        nix::unistd::ftruncate(&memfd, memsize.get() as i64)
            .map_err(|e| anyhow!("failed to initialize shared memory: {e}"))?;
        mman::mmap(
            None,
            memsize,
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            MapFlags::MAP_SHARED,
            &memfd,
            0,
        )?
        .cast::<u8>()
    };

    let header = if args.create {
        let h = UringBuilder::new().build_header();
        unsafe { init_uring::<Rqe, Sqe, ()>(memaddr, memsize.get(), h) }
    } else {
        memaddr.cast()
    };

    let build_raw = || {
        let mut raw = RawUring::dangling();
        unsafe {
            let h = header.as_ref();
            raw.header = header.cast();
            raw.buf_a = header.byte_add(h.buf_a).cast();
            raw.buf_b = header.byte_add(h.buf_b).cast();
        }
        raw
    };

    let disposed = match args.typ {
        AppType::Client => start_client(unsafe { Sender::from_raw(build_raw()) }),
        AppType::Server => start_server(unsafe { Receiver::from_raw(build_raw()) }),
    };

    if disposed {
        args.memfile.unlink()?;
    }

    Ok(())
}

fn start_client(sq: Sender) -> bool {
    tracing::info!("started client, connected={}", sq.is_connected());

    let reactor = Reactor::new(sq);
    let rt = Rc::new(Executor::new());
    rt.block_on(reactor.run_on(async {
        let tasks = (0..16)
            .map(|i| async move {
                let delay = fastrand::u64(50..500);
                tracing::info!("requested ping({i}), delay={delay:?}ms");

                let now = std::time::Instant::now();
                let token = op::ping(Duration::from_millis(fastrand::u64(0..500))).await;
                let elapsed = now.elapsed().as_millis();
                tracing::info!("responded pong({i}), elapsed={elapsed}ms, token={token:x}");
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

fn start_server(mut rq: Receiver) -> bool {
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
                SqeData::Ping { delay } => {
                    std::thread::sleep(delay);
                    RqeData::Pong {
                        token: fastrand::u64(..),
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

unsafe fn init_uring<A, B, Ext>(
    start: NonNull<u8>,
    len: usize,
    header: uring::Header<Ext>,
) -> NonNull<UringOffsets<Ext>> {
    let page_size = 4096; // TODO: use sysconf(2)

    // Check alignments
    assert!(start.addr().get() & (page_size - 1) == 0);
    assert!(align_of::<UringOffsets<Ext>>() < page_size);
    assert!(align_of::<A>() < page_size);
    assert!(align_of::<B>() < page_size);

    // Check overflows
    let mut size;

    let offsets = start.cast::<UringOffsets<Ext>>();
    size = size_of::<UringOffsets<Ext>>();

    let layout_a = Layout::array::<A>(header.size_a()).unwrap();
    let buf_a = align_up(size, layout_a.align());
    size = buf_a + layout_a.size();

    let layout_b = Layout::array::<B>(header.size_b()).unwrap();
    let buf_b = align_up(size, layout_b.align());
    size = buf_b + layout_b.size();

    assert!(size <= len);

    // Initialize the Uring
    unsafe {
        offsets.write(UringOffsets {
            header,
            buf_a,
            buf_b,
            size,
        });
    }

    offsets
}

const fn align_up(n: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (n + align - 1) & !(align - 1)
}
