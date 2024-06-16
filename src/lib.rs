use std::{cell::UnsafeCell, fmt::Debug, ptr::NonNull, sync::OnceLock};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Canceled;

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Handshake<T> {
    // NotNull is & unless deduced otherwise
    common: NonNull<OnceLock<UnsafeCell<Option<T>>>>
}

impl<T> Handshake<T> {
    pub fn new() -> (Handshake<T>, Handshake<T>) {
        // check expected to be elided during compilation
        let common = unsafe { NonNull::new_unchecked(Box::into_raw(
            Box::new(OnceLock::new())
        ))};
        (Handshake {common}, Handshake {common})
    }

    pub fn join<U, F: FnOnce(T, T) -> U>(self, value: T, f: F) -> Result<Option<U>, Canceled> {
        let mut value = Some(value);
        // access safe lock
        let res = unsafe { self.common.as_ref() }.get_or_init(||
            UnsafeCell::new(Some(value.take().unwrap()))
        );
        let combined = value.map_or(Ok(None), |value| {
            // unique access if value present
            let combined = unsafe { &mut*res.get() }
                .take()
                .map_or(Err(Canceled), |other| {
                    Ok(Some((f)(value, other)))
                });
            // last reference, drop pointer
            drop(unsafe { Box::from_raw(self.common.as_ptr()) });
            combined
        });
        std::mem::forget(self); // consumes `self`
        combined
    }

    pub fn try_push(self, value: T) -> Result<Result<(), (Self, T)>, T> {
        let mut value = Some(value);
        // access safe lock
        let res = unsafe { self.common.as_ref() }.get_or_init(||
            UnsafeCell::new(Some(value.take().unwrap()))
        );
        if let Some(value) = value {
            // value present, lock inhabited
            if unsafe { &*res.get() }.is_none() {
                // handshake was cancelled
                drop(unsafe { Box::from_raw(self.common.as_ptr()) });
                std::mem::forget(self); // consumes `self`
                Err(value)
            } else {
                Ok(Err((self, value)))
            }
        } else {
            std::mem::forget(self); // consumes `self`
            Ok(Ok(()))
        }
    }

    pub fn try_pull(self) -> Result<Result<T, Self>, Canceled> {
        // access safe lock
        if let Some(res) = unsafe { self.common.as_ref() }.get() {
            // unique access if value present
            if let Some(value) = unsafe { &mut*res.get() }.take() {
                // last reference, drop pointer
                drop(unsafe { Box::from_raw(self.common.as_ptr()) });
                std::mem::forget(self); // consumes `self`
                Ok(Ok(value))
            } else {
                // handshake was cancelled
                drop(unsafe { Box::from_raw(self.common.as_ptr()) });
                std::mem::forget(self); // consumes `self`
                Err(Canceled)
            }

        } else {
            Ok(Err(self))
        }
    }

    pub fn is_set(&self) -> bool {
        // access safe lock
        unsafe { self.common.as_ref() }.get().is_some()
    }
}

impl<T> Drop for Handshake<T> {
    fn drop(&mut self) {
        let mut canceled = false;
        // access safe lock
        let _ = unsafe { self.common.as_ref() }.get_or_init(|| {
            canceled = true;
            UnsafeCell::new(None)
        });
        if canceled { return; }; // handshake cancelled
        // otherwise last reference, drop pointer
        drop(unsafe { Box::from_raw(self.common.as_ptr()) });
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

    use crate::{Canceled, Handshake};

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
        assert_eq!(v.try_pull(), Err(Canceled));

        let (u, v) = Handshake::<()>::new();
        drop(v);
        assert_eq!(u.try_pull(), Err(Canceled));
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
    // Due to the innefective `OnceLock` API and
    // the requirement to keep `self` around for either `std::mem::forget(self)` or return
    // creates a break in aliasing rules as a `&` is coexisting with a `&mut` (even though the `&` is not used).
    // This means that these functions (join, try_push, try_pull) do not pass tests involving miri,
    // however it would appear they are still perfectly safe.
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