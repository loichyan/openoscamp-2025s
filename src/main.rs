use std::alloc::Layout;
use std::future::Future;
use std::pin::Pin;
use std::ptr::NonNull;

pub trait AsyncIterator {
    type Item;
    fn next(&mut self) -> impl Future<Output = Option<Self::Item>>;
}

pub trait DynAsyncIterator {
    type Item;
    // 这里实际上把对返回值的储存委托给了调用方，而调用方可以自由选择储存方案．
    fn next<'this, 'store>(
        &'this mut self,
        store: &'store mut dyn Storage,
    ) -> Pin<Object<'store, dyn 'this + Future<Output = Option<Self::Item>>>>;
}

impl<T: AsyncIterator> DynAsyncIterator for T {
    type Item = T::Item;

    fn next<'this, 'store>(
        &'this mut self,
        store: &'store mut dyn Storage,
    ) -> Pin<Object<'store, dyn 'this + Future<Output = Option<Self::Item>>>> {
        #[rustfmt::skip]
        const fn return_type_layout<A1, R, F: FnOnce(A1) -> R>(_: &F) -> Layout { Layout::new::<R>() }

        let layout = return_type_layout(&<T as AsyncIterator>::next);
        let mut ptr = store.acquire(layout).cast();
        debug_assert!(ptr.is_aligned());

        // 这里要求返回的 pointer 必须被 Pin 到内存上
        unsafe {
            ptr.write(T::next(self));
            Pin::new_unchecked(Object(ptr.as_mut()))
        }
    }
}

pub struct Object<'a, T: ?Sized>(&'a mut T);

impl<T: ?Sized> std::ops::Deref for Object<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<T: ?Sized> std::ops::DerefMut for Object<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
}

impl<T: ?Sized> Drop for Object<'_, T> {
    fn drop(&mut self) {
        unsafe {
            core::ptr::drop_in_place(self.0);
        }
    }
}

// 用来存放返回的 Future，本质上是一个一次性地内存分配器．
// 实现时，可以使用 SmallVec 来避免因小 Future 导致的内存分配．
pub trait Storage {
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

#[tokio::test]
async fn test_async_iterator() {
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

    try_async_iterator(&mut AsyncCounter(10)).await;
}

pub fn main() {
    println!("Hello, Rust!");
}
