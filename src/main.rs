#![feature(local_waker)]

mod task;
pub use task::{Task, yield_now};

mod executor;
pub use executor::Executor;

#[test]
fn tick_counter() {
    use std::cell::Cell;
    use std::rc::Rc;

    let executor = Executor::new();
    let signal = Rc::new(Cell::new(false));
    executor.spawn({
        let signal = signal.clone();
        async move {
            for i in (0..10).rev() {
                println!("tick: {i}");
                yield_now().await;
            }
            signal.set(true);
        }
    });
    executor.spawn(async move {
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
    executor.run();
}

fn main() {
    println!("Hello, Rust!");
}
