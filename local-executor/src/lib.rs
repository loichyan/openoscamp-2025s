#![feature(local_waker)]
#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod task;
pub use task::Task;

mod executor;
pub use executor::{Executor, ExecutorHandle, spawn, yield_now};

#[test]
fn tick_counter() {
    use std::cell::Cell;
    use std::rc::Rc;

    let executor = Rc::new(Executor::new());
    executor.block_on(async {
        let signal = Rc::new(Cell::new(false));
        spawn(Rc::downgrade(&executor), {
            let signal = signal.clone();
            async move {
                for i in (0..10).rev() {
                    println!("tick: {i}");
                    yield_now().await;
                }
                signal.set(true);
            }
        });

        let mut i = 0;
        loop {
            println!("count: {i}");
            if signal.get() {
                break;
            }
            yield_now().await;
            i += 1;
        }
    });
}
