#![allow(dead_code)]

use std::cell::Cell;
use std::fmt;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

use super::{Allocator, ShmHeader, ShmToken};

pub struct ShmBox<T: ?Sized>(NonNull<T>);

impl<T: ?Sized> ShmBox<T> {
    pub fn as_shm(this: &Self) -> ShmToken<T> {
        ShmHandle::get().get_shm(this.0)
    }

    pub fn into_raw(self) -> NonNull<T> {
        let ptr = self.0;
        std::mem::forget(self);
        ptr
    }

    /// # Safety
    ///
    /// The given `ptr` must be a valid pointer managed by [`Allocator`].
    pub unsafe fn from_raw(ptr: NonNull<T>) -> Self {
        ShmBox(ptr)
    }
}

impl<T> ShmBox<T> {
    pub fn new(val: T) -> Self {
        unsafe { Self::from_raw(AloHandle::get().alloc(val)) }
    }

    pub fn into_uninit(self) -> ShmBox<MaybeUninit<T>> {
        unsafe { ShmBox::from_raw(self.into_raw().cast()) }
    }
}

impl<T> ShmBox<MaybeUninit<T>> {
    pub fn new_uninit() -> Self {
        unsafe { Self::from_raw(AloHandle::get().alloc_uninit()) }
    }

    /// # Safety
    ///
    /// The underlying value must be initialized properly. See
    /// [`MaybeUninit::assume_init`] for more information.
    pub unsafe fn assume_init(self) -> ShmBox<T> {
        unsafe { ShmBox::from_raw(self.into_raw().cast()) }
    }
}

impl<T> ShmBox<[T]> {
    pub fn new_slice_copied(src: &[T]) -> Self
    where
        T: Copy,
    {
        unsafe { Self::from_raw(AloHandle::get().alloc_slice_copied(src)) }
    }

    pub fn new_slice_filled(val: T, n: usize) -> Self
    where
        T: Copy,
    {
        unsafe { Self::from_raw(AloHandle::get().alloc_slice_filled(val, n)) }
    }

    pub fn into_uninit(self) -> ShmBox<[MaybeUninit<T>]> {
        unsafe {
            let ptr = self.into_raw().as_ptr();
            ShmBox::from_raw(NonNull::new_unchecked(ptr as *mut [MaybeUninit<T>]))
        }
    }
}

impl<T> ShmBox<[MaybeUninit<T>]> {
    pub fn new_slice_uninit(n: usize) -> Self {
        unsafe { Self::from_raw(AloHandle::get().alloc_slice_uninit(n)) }
    }

    /// # Safety
    ///
    /// Each element in the underlying slice must be initialized properly. See
    /// [`MaybeUninit::assume_init`] for more information.
    pub unsafe fn assume_init(self) -> ShmBox<[T]> {
        unsafe {
            let ptr = self.into_raw().as_ptr();
            ShmBox::from_raw(NonNull::new_unchecked(ptr as *mut [T]))
        }
    }
}

impl<T: ?Sized> Drop for ShmBox<T> {
    fn drop(&mut self) {
        unsafe {
            let ptr = self.0;
            ptr.drop_in_place();
            AloHandle::get().dealloc(ptr);
        }
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for ShmBox<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        T::fmt(self, f)
    }
}

impl<T: ?Sized> std::ops::Deref for ShmBox<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref() }
    }
}

impl<T: ?Sized> std::ops::DerefMut for ShmBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.0.as_mut() }
    }
}

thread_local! {
    static SHM: Cell<Option<&'static ShmHeader>> = const { Cell::new(None) };
    static ALO: Cell<Option<&'static Allocator>> = const { Cell::new(None) };
}

pub struct ShmHandle;

impl ShmHandle {
    pub fn init(shm: &'static ShmHeader) {
        if SHM.get().is_some() {
            panic!("shm has been initialized");
        }
        SHM.set(Some(shm))
    }

    pub fn get() -> &'static ShmHeader {
        SHM.get().expect("shm is not initialized")
    }
}

pub struct AloHandle;

impl AloHandle {
    pub fn init(shm: &'static ShmHeader) {
        if ALO.get().is_some() {
            panic!("allocator has been initialized");
        }
        ALO.set(Some(shm.get_allocator()))
    }

    pub fn get() -> &'static Allocator {
        ALO.get().expect("allocator is not initialized")
    }
}

pub fn init_client(shm: &'static ShmHeader) {
    ShmHandle::init(shm);
    AloHandle::init(shm);
}

pub fn init_server(shm: &'static ShmHeader) {
    ShmHandle::init(shm);
}
