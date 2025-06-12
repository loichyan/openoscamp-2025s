use alloc::boxed::Box;

pub trait Resource: 'static {
    type Value: ?Sized;
    fn as_ptr(&self) -> *const Self::Value;
}

pub trait ResourceMut: Resource {
    fn as_ptr_mut(&mut self) -> *mut Self::Value;
}

impl<T: 'static + ?Sized> Resource for Box<T> {
    type Value = T;
    fn as_ptr(&self) -> *const Self::Value {
        &raw const **self
    }
}

impl<T: 'static + ?Sized> ResourceMut for Box<T> {
    fn as_ptr_mut(&mut self) -> *mut Self::Value {
        &raw mut **self
    }
}
