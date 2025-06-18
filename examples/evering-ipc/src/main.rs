#![feature(layout_for_ptr)]
#![feature(local_waker)]

use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use argh::FromArgs;
use bytesize::ByteSize;
use evering_ipc::{ShmHeader, UringBuilder};

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
        AppType::Client => evering_ipc::start_client(shm),
        AppType::Server => evering_ipc::start_server(shm),
    };

    if disposed {
        args.shmfile.unlink()?;
    }

    Ok(())
}
