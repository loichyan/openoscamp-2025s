#![no_std]
#![no_main]
#![feature(format_args_nl)]

#[macro_use]
extern crate user;

#[unsafe(no_mangle)]
fn main() {
    println!("[user] Hello, kernel!");
}
