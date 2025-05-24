use std::alloc::Layout;
use std::future::Future;
use std::pin::Pin;
use std::ptr::NonNull;

trait AsyncIterator {
    type Item;
    async fn next(&mut self) -> Option<Self::Item>;
}

trait DynAsyncIterator {
    type Item;
    // 这里实际上把对返回值的储存委托给了调用方，而调用方可以自由选择储存方案．
    fn next(
        &mut self,
        store: &mut dyn Storage,
    ) -> Pin<&mut dyn Future<Output = Option<Self::Item>>>;
}

impl<T: AsyncIterator> DynAsyncIterator for T {
    type Item = T::Item;

    fn next(
        &mut self,
        store: &mut dyn Storage,
    ) -> Pin<&mut dyn Future<Output = Option<Self::Item>>> {
        #[rustfmt::skip]
        const fn return_type_layout<A1, R, F: FnOnce(A1) -> R>(_: &F) -> Layout { Layout::new::<R>() }

        let layout = return_type_layout(&<T as AsyncIterator>::next);
        let mut ptr = store.acquire(layout).cast();
        debug_assert!(ptr.is_aligned());

        // 这里要求返回的 pointer 必须被 Pin 到内存上
        unsafe {
            ptr.write(T::next(self));
            Pin::new_unchecked(ptr.as_mut())
        }
    }
}

// 用来存放返回的 Future，本质上是一个一次性地内存分配器．
// 实现时，可以使用 SmallVec 来避免因小 Future 导致的内存分配．
trait Storage {
    fn acquire(&mut self, layout: Layout) -> NonNull<u8>;
}

impl Storage for Vec<u8> {
    fn acquire(&mut self, layout: Layout) -> NonNull<u8> {
        self.reserve((layout.size() + layout.align()).saturating_sub(self.len()));
        let start = self.as_mut_ptr();
        let offset = start.align_offset(layout.align());
        let aligned = start.wrapping_add(offset);
        debug_assert!(aligned.wrapping_add(layout.size()) <= start.wrapping_add(self.capacity()));
        NonNull::new(aligned).unwrap()
    }
}

struct AsyncCounter(usize);

impl AsyncIterator for AsyncCounter {
    type Item = usize;

    async fn next(&mut self) -> Option<Self::Item> {
        if self.0 == 0 {
            return None;
        } else if self.0 % 2 == 0 {
            tokio::task::yield_now().await;
        }
        self.0 -= 1;
        Some(self.0)
    }
}

// 在这里就能愉快的使用 dyn AsyncIterator :)
async fn try_async_iterator(iter: &mut dyn DynAsyncIterator<Item = usize>) {
    let mut store = vec![];
    while let Some(i) = iter.next(&mut store).await {
        println!("{i}");
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    try_async_iterator(&mut AsyncCounter(10)).await;
}
