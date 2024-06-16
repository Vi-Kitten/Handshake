use std::{fmt::Debug, mem::MaybeUninit, ptr::NonNull, sync::Mutex};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct Cancelled;

enum Inner<T> {
    Unset,
    Set(T),
    Dropped(Cancelled),
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Handshake<T> {
    common: NonNull<Mutex<MaybeUninit<Inner<T>>>>
}

impl<T> Handshake<T> {
    pub fn new() -> (Handshake<T>, Handshake<T>) {
        // check expected to be elided during compilation
        let common = unsafe { NonNull::new_unchecked(Box::into_raw(
            Box::new(Mutex::new(MaybeUninit::new(Inner::Unset)))
        ))};
        (Handshake {common}, Handshake {common})
    }

    pub fn join<U, F: FnOnce(T, T) -> U>(self, value: T, f: F) -> Result<Option<U>, Cancelled> {
        let common = self.common;
        let last;
        let res = '_lock: {
            let mut lock = unsafe { self.common.as_ref() }.lock().unwrap();
            let (res, inner) = match unsafe {
                std::mem::replace(&mut*lock, MaybeUninit::uninit()).assume_init()
            } {
                Inner::Unset => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = false;
                    (Ok(None), Inner::Set(value))
                },
                Inner::Set(other) => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = true;
                    (Ok(Some((other, value))), Inner::Unset)
                },
                Inner::Dropped(cancel) => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = true;
                    (Err(cancel), Inner::Dropped(cancel))
                },
            };
            std::mem::swap(&mut*lock, &mut MaybeUninit::new(inner));
            res
        };
        if last {
            // last reference, drop pointer
            drop(unsafe { Box::from_raw(common.as_ptr()) });
        };
        res.map(|opt| opt.map(|(x, y)| (f)(x, y)))
    }

    pub fn try_push(self, value: T) -> Result<Result<(), (Self, T)>, T> {
        let common = self.common;
        let last;
        let res = '_lock: {
            let mut lock = unsafe { common.as_ref() }.lock().unwrap();
            let (res, inner) = match unsafe {
                std::mem::replace(&mut*lock, MaybeUninit::uninit()).assume_init()
            } {
                Inner::Unset => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = false;
                    (Ok(Ok(())), Inner::Set(value))
                },
                Inner::Set(other) => {
                    last = false;
                    (Ok(Err((self, value))), Inner::Set(other))
                },
                Inner::Dropped(cancel) => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = true;
                    (Err(value), Inner::Dropped(cancel))
                },
            };
            std::mem::swap(&mut*lock, &mut MaybeUninit::new(inner));
            res
        };
        if last {
            // last reference, drop pointer
            drop(unsafe { Box::from_raw(common.as_ptr()) });
        };
        res
    }

    pub fn try_pull(self) -> Result<Result<T, Self>, Cancelled> {
        let common = self.common;
        let last;
        let res = '_lock: {
            let mut lock = unsafe { self.common.as_ref() }.lock().unwrap();
            let (res, inner) = match unsafe {
                std::mem::replace(&mut*lock, MaybeUninit::uninit()).assume_init()
            } {
                Inner::Unset => {
                    last = false;
                    (Ok(Err(self)), Inner::Unset)
                },
                Inner::Set(value) => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = true;
                    (Ok(Ok(value)), Inner::Unset)
                },
                Inner::Dropped(cancel) => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = true;
                    (Err(cancel), Inner::Dropped(cancel))
                },
            };
            std::mem::swap(&mut*lock, &mut MaybeUninit::new(inner));
            res
        };
        if last {
            // last reference, drop pointer
            drop(unsafe { Box::from_raw(common.as_ptr()) });
        };
        res
    }

    pub fn is_set(&self) -> Result<bool, Cancelled> {
        '_lock: {
            match unsafe { self.common.as_ref().lock().unwrap().assume_init_ref() } {
                Inner::Unset => Ok(false),
                Inner::Set(_) => Ok(true),
                Inner::Dropped(cancel) => Err(*cancel),
            }
        }
    }
}

impl<T> Drop for Handshake<T> {
    fn drop(&mut self) {
        let last;
        '_lock: {
            let mut lock = unsafe { self.common.as_ref() }.lock().unwrap();
            let inner = match unsafe {
                std::mem::replace(&mut*lock, MaybeUninit::uninit()).assume_init()
            } {
                Inner::Unset => {
                    last = false;
                    Inner::Dropped(Cancelled)
                },
                Inner::Set(value) => {
                    drop(value);
                    last = true;
                    Inner::Unset
                },
                Inner::Dropped(cancel) => {
                    last = true;
                    Inner::Dropped(cancel)
                },
            };
            std::mem::swap(&mut*lock, &mut MaybeUninit::new(inner))
        };
        if last {
            // last reference, drop pointer
            drop(unsafe { Box::from_raw(self.common.as_ptr()) });
        };
    }
}

unsafe impl<T: Send> Sync for Handshake<T> {}

unsafe impl<T: Send> Send for Handshake<T> {}

impl<T: Debug> Debug for Handshake<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // access safe lock
        f.debug_struct("Handshake").field("common", unsafe { self.common.as_ref() }).finish()
    }
}

#[cfg(test)]
mod test {
    use std::convert::identity;

    use crate::{Cancelled, Handshake};

    #[test]
    fn drop_test() {
        let (u, v) = Handshake::<()>::new();
        drop(u);
        drop(v);

        let (u, v) = Handshake::<()>::new();
        drop(v);
        drop(u)
    }

    #[test]
    fn push_drop_test() {
        #[derive(Debug)]
        struct Loud<'a> {
            flag: &'a mut bool
        }

        impl<'a> Drop for Loud<'a> {
            fn drop(&mut self) {
                *self.flag = true;
            }
        }

        let mut dropped = false;
        let (u, v) = Handshake::<Loud>::new();
        u.try_push(Loud { flag: &mut dropped }).unwrap().unwrap();
        drop(v);

        assert_eq!(dropped, true);
    }

    #[test]
    fn pull_test() {
        let (u, v) = Handshake::<()>::new();
        assert_eq!(u.try_pull(), Ok(Err(v)));

        let (u, v) = Handshake::<()>::new();
        assert_eq!(v.try_pull(), Ok(Err(u)))
    }

    #[test]
    fn push_test() {
        let (u, v) = Handshake::<()>::new();
        assert_eq!(u.try_push(()), Ok(Ok(())));
        drop(v);

        let (u, v) = Handshake::<()>::new();
        assert_eq!(v.try_push(()), Ok(Ok(())));
        drop(u)
    }

    #[test]
    fn double_push_test() {
        let (u, v) = Handshake::<()>::new();
        u.try_push(()).unwrap().unwrap();
        drop(v.try_push(()).unwrap().err().unwrap());

        let (u, v) = Handshake::<()>::new();
        v.try_push(()).unwrap().unwrap();
        drop(u.try_push(()).unwrap().err().unwrap())
    }

    #[test]
    fn pull_cancel_test() {
        let (u, v) = Handshake::<()>::new();
        drop(u);
        assert_eq!(v.try_pull(), Err(Cancelled));

        let (u, v) = Handshake::<()>::new();
        drop(v);
        assert_eq!(u.try_pull(), Err(Cancelled));
    }

    #[test]
    fn push_cancel_test() {
        let (u, v) = Handshake::<()>::new();
        drop(u);
        assert_eq!(v.try_push(()), Err(()));

        let (u, v) = Handshake::<()>::new();
        drop(v);
        assert_eq!(u.try_push(()), Err(()));
    }

    #[test]
    fn push_pull_test() {
        let (u, v) = Handshake::<()>::new();
        u.try_push(()).unwrap().unwrap();
        v.try_pull().unwrap().unwrap();

        let (u, v) = Handshake::<()>::new();
        v.try_push(()).unwrap().unwrap();
        u.try_pull().unwrap().unwrap()
    }

    #[test]
    fn join_test() {
        let (u, v) = Handshake::<()>::new();
        assert_eq!(u.join((), |_, _| ()).unwrap(), None);
        assert_eq!(v.join((), |_, _| ()).unwrap(), Some(()));

        let (u, v) = Handshake::<()>::new();
        assert_eq!(v.join((), |_, _| ()).unwrap(), None);
        assert_eq!(u.join((), |_, _| ()).unwrap(), Some(()))
    }

    #[test]
    fn collision_check() {
        use rand::prelude::*;
        const N: usize = 64;

        let mut left: Vec<Handshake<usize>> = vec![];
        let mut right: Vec<Handshake<usize>> = vec![];
        for _ in 0..N {
            let (u, v) = Handshake::<usize>::new();
            left.push(u);
            right.push(v)
        }
        let mut rng = rand::thread_rng();
        left.shuffle(&mut rng);
        right.shuffle(&mut rng);
        let left_thread = std::thread::spawn(|| left
            .into_iter()
            .enumerate()
            .map(|(n, u)| {u.join(n, |x, y| (x, y)).unwrap()})
            .filter_map(identity).collect::<Vec<(usize, usize)>>()
        );
        let right_thread = std::thread::spawn(|| right
            .into_iter()
            .enumerate()
            .map(|(n, v)| {v.join(n, |x, y| (x, y)).unwrap()})
            .filter_map(identity).collect::<Vec<(usize, usize)>>()
        );
        let total = left_thread.join().unwrap().len() + right_thread.join().unwrap().len();
        assert_eq!(total, N)
    }
}