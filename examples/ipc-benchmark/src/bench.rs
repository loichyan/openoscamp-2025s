// Credit: https://github.com/cloudwego/shmipc-rs/blob/de966a6ca2d76d574b943f6fd4d3abfa6ff2df5f/benches/bench.rs
//
// Copyright 2025 CloudWeGo Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

mod epoll;
mod evering;
mod io_uring;
mod shmipc;

use std::path::Path;
use std::time::{Duration, Instant};

use bytesize::ByteSize;
use criterion::{Criterion, criterion_group, criterion_main};

const BUFSIZES: &[usize] = &[
    4,
    64,
    512,
    1024,
    4096,
    16 << 10,
    32 << 10,
    64 << 10,
    256 << 10,
    512 << 10,
    1 << 20,
];
const CONCURRENCY: usize = 200;
const SHMSIZE: usize = 256 << 20;

const PING: i32 = 1;
const PONG: i32 = 2;
const BUFVAL: u8 = b'X';

type BenchFn = fn(&str, usize, usize) -> Duration;

fn block_on<T>(fut: impl Future<Output = T>) -> T {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&rt, fut)
}

fn make_shmid(pref: &str) -> String {
    pref.chars()
        .chain(std::iter::repeat_with(fastrand::alphanumeric).take(6))
        .collect()
}

fn groups(c: &mut Criterion) {
    macro_rules! benches {
        ($($name:ident),* $(,)?) => ([$((stringify!($name), self::$name::bench as BenchFn),)*]);
    }

    // TODO: black_box assertions
    let mut g = c.benchmark_group("ipc_benchmark");
    for (i, bufsize) in BUFSIZES.iter().copied().enumerate() {
        let bsize = ByteSize::b(bufsize as u64).display().iec_short();
        for (name, f) in benches![evering, epoll, io_uring, shmipc] {
            let id = format!("ipc_benchmark_{i:02}_{bsize:.0}_{name}");
            g.bench_function(&id, |b| {
                b.iter_custom(|iters| f(&id, iters as usize, bufsize))
            });
        }
    }
}

criterion_group!(ipc_benchmark, groups);
criterion_main!(ipc_benchmark);
