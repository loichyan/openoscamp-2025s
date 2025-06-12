#![feature(local_waker)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod driver;
pub mod op;
pub mod resource;
pub mod uring;
