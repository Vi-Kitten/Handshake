#![doc = include_str!("../README.md")]
use std::{fmt::Debug, ptr::NonNull, sync::Mutex};

/// An empty struct signalling cancellation for [`Handshake`].
/// 
/// A [`channel`] can only be cancelled by a call to [`Drop::drop`] or [`take`].
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct Cancelled;

#[derive(Debug)]
enum Inner<T> {
    Unset,
    Set(T)
}

/// A joint sender and receiver for a symmetric one time use channel.
/// 
/// # Examples
/// 
/// Using [`join`]:
/// 
/// ```
/// let (u, v) = oneshot_handshake::channel::<u8>();
/// 
/// '_task_a: {
///     let fst = u.join(1, std::ops::Add::add).unwrap();
///     assert_eq!(fst, None)
/// }
///
/// '_task_b: {
///     let snd = v.join(2, std::ops::Add::add).unwrap();
///     assert_eq!(snd, Some(3))
/// }
/// ```
/// 
/// Using [`try_push`] and [`try_pull`]:
/// 
/// ```
/// let (u, v) = oneshot_handshake::channel::<u8>();
/// 
/// let a = u.try_push(3).unwrap();
/// assert_eq!(a, Ok(()));
///
/// let b = v.try_pull().unwrap();
/// assert_eq!(b, Ok(3))
/// ```
/// 
/// [`join`]: Handshake::join
/// [`try_push`]: Handshake::try_push
/// [`try_pull`]: Handshake::try_pull
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Handshake<T> {
    common: NonNull<Mutex<Option<Inner<T>>>>
}

/// Creates a symmetric one time use channel.
/// 
/// Allows each end of the handshake to send or receive information for bi-directional movement of data.
/// 
/// # Examples
/// 
/// Using [`join`]:
/// 
/// ```
/// let (u, v) = oneshot_handshake::channel::<u8>();
/// 
/// '_task_a: {
///     let fst = u.join(1, std::ops::Add::add).unwrap();
///     assert_eq!(fst, None)
/// }
///
/// '_task_b: {
///     let snd = v.join(2, std::ops::Add::add).unwrap();
///     assert_eq!(snd, Some(3))
/// }
/// ```
/// 
/// Using [`try_push`] and [`try_pull`]:
/// 
/// ```
/// let (u, v) = oneshot_handshake::channel::<u8>();
/// 
/// let a = u.try_push(3).unwrap();
/// assert_eq!(a, Ok(()));
///
/// let b = v.try_pull().unwrap();
/// assert_eq!(b, Ok(3))
/// ```
/// 
/// [`join`]: Handshake::join
/// [`try_push`]: Handshake::try_push
/// [`try_pull`]: Handshake::try_pull
pub fn channel<T>() -> (Handshake<T>, Handshake<T>) {
    // check expected to be elided during compilation
    let common = unsafe { NonNull::new_unchecked(Box::into_raw(
        Box::new(Mutex::new(Some(Inner::Unset)))
    ))};
    (Handshake {common}, Handshake {common})
}


impl<T> Handshake<T> {

    /// Creates a channel that has already been pushed to.
    /// 
    /// The expression:
    /// ```
    /// let _ = oneshot_handshake::Handshake::<u8>::wrap(1);
    /// ```
    /// 
    /// Is the same as the expression:
    /// ```
    /// let _ = {
    ///     let (u, v) = oneshot_handshake::channel::<u8>();
    ///     u.try_push(1).unwrap().unwrap();
    ///     v
    /// };
    /// ```
    pub fn wrap(value: T) -> Handshake<T> {
        Handshake { common: unsafe {
            NonNull::new_unchecked(Box::into_raw(
                Box::new(Mutex::new(Some(Inner::Set(value))))
            ))
        } }
    }

    /// Pulls and pushes at the same time, garunteeing consumption of `self`.
    /// 
    /// If `self` is [`Unset`] `f` will not be ran and `value` will be stored returning `Ok(None)`,
    /// if `self` is [`Set`] with some `other` instance then `f` will be called with `other` and `value`
    /// returning `Ok(return_value)`.
    /// 
    /// Otherwise on cancellation `Err(value)` will be returned.
    /// 
    /// If you only need to send or receive `value`, instead call [`try_push`] or [`try_pull`] respectively.
    /// 
    /// [`try_push`]: Handshake::try_push
    /// [`try_pull`]: Handshake::try_pull
    /// 
    /// [`Set`]: Handshake::Set
    /// [`Unset`]: Handshake::Unset
    /// 
    /// # Example
    /// 
    /// ```
    /// let (u, v) = oneshot_handshake::channel::<u8>();
    /// 
    /// '_task_a: {
    ///     let fst = u.join(1, std::ops::Add::add).unwrap();
    ///     assert_eq!(fst, None)
    /// }
    ///
    /// '_task_b: {
    ///     let snd = v.join(2, std::ops::Add::add).unwrap();
    ///     assert_eq!(snd, Some(3))
    /// }
    /// ```
    pub fn join<U, F: FnOnce(T, T) -> U>(self, value: T, f: F) -> Result<Option<U>, T> {
        let common = self.common;
        let last;
        let res = '_lock: {
            let mut lock = unsafe { common.as_ref() }.lock().unwrap();
            match lock.take() {
                Some(Inner::Unset) => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = false;
                    let _ = lock.insert(Inner::Set(value));
                    Ok(None)
                },
                Some(Inner::Set(other)) => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = true;
                    let _ = lock.insert(Inner::Unset);
                    Ok(Some((other, value)))
                },
                None => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = true;
                    Err(value)
                },
            }
        };
        if last {
            // last reference, drop pointer
            drop(unsafe { Box::from_raw(common.as_ptr()) })
        };
        // isolate potential panic
        res.map(|opt| opt.map(|(x, y)| (f)(x, y)))
    }

    /// Attempts to send a value through the channel.
    /// 
    /// If `self` is [`Unset`] `value` will be stored returning `Ok(Ok(()))`,
    /// if `self` is [`Set`] with some `other` instance then pushing will fail
    /// and `Ok(Err((self, value)))` will be returned.
    /// 
    /// Otherwise on cancellation `Err(value)` will be returned.
    /// 
    /// If you are handling `value` symetrically, consider calling [`join`].
    /// 
    /// [`join`]: Handshake::join
    /// 
    /// [`Set`]: Handshake::Set
    /// [`Unset`]: Handshake::Unset
    /// 
    /// # Example
    /// 
    /// ```
    /// let (u, v) = oneshot_handshake::channel::<u8>();
    /// 
    /// let a = u.try_push(3).unwrap();
    /// assert_eq!(a, Ok(()));
    ///
    /// let b = v.try_pull().unwrap();
    /// assert_eq!(b, Ok(3))
    /// ```
    pub fn try_push(self, value: T) -> Result<Result<(), (Self, T)>, T> {
        let common = self.common;
        let last;
        let res = '_lock: {
            let mut lock = unsafe { common.as_ref() }.lock().unwrap();
            match lock.take() {
                Some(Inner::Unset) => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = false;
                    let _ = lock.insert(Inner::Set(value));
                    Ok(Ok(()))
                },
                Some(Inner::Set(other)) => {
                    last = false;
                    let _ = lock.insert(Inner::Set(other));
                    Ok(Err((self, value)))
                },
                None => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = true;
                    Err(value)
                },
            }
        };
        if last {
            // last reference, drop pointer
            drop(unsafe { Box::from_raw(common.as_ptr()) })
        };
        res
    }

    /// Attempts to receive a value through the channel.
    /// 
    /// If `self` is [`Unset`] then pulling will fail returning `Ok(Err(self))`,
    /// if `self` is [`Set`] with some `value` then `Ok(Ok(value))` will be returned.
    /// 
    /// Otherwise on cancellation `Err(Cancelled)` will be returned.
    /// 
    /// If you are handling `value` symetrically, consider calling [`join`].
    /// 
    /// [`join`]: Handshake::join
    /// 
    /// [`Set`]: Handshake::Set
    /// [`Unset`]: Handshake::Unset
    /// 
    /// # Example
    /// 
    /// ```
    /// let (u, v) = oneshot_handshake::channel::<u8>();
    /// 
    /// let a = u.try_push(3).unwrap();
    /// assert_eq!(a, Ok(()));
    ///
    /// let b = v.try_pull().unwrap();
    /// assert_eq!(b, Ok(3))
    /// ```
    pub fn try_pull(self) -> Result<Result<T, Self>, Cancelled> {
        let common = self.common;
        let last;
        let res = '_lock: {
            let mut lock = unsafe { common.as_ref() }.lock().unwrap();
            match lock.take() {
                Some(Inner::Unset) => {
                    last = false;
                    let _ = lock.insert(Inner::Unset);
                    Ok(Err(self))
                },
                Some(Inner::Set(value)) => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = true;
                    let _ = lock.insert(Inner::Unset);
                    Ok(Ok(value))
                },
                None => {
                    // consumes `self`
                    std::mem::forget(self);
                    last = true;
                    Err(Cancelled)
                },
            }
        };
        if last {
            // last reference, drop pointer
            drop(unsafe { Box::from_raw(common.as_ptr()) })
        };
        res
    }

    /// Checks the channel to see if there is a value present.
    /// 
    /// If the channel is cancelled then `Err(Cancelled)` will be returned, otherwise
    /// a boolean value will be returned indicating whether or not the channel is set.
    /// 
    /// # Example
    /// 
    /// ```
    /// let (u, v) = oneshot_handshake::channel::<u8>();
    /// 
    /// assert_eq!(v.is_set().unwrap(), false);
    /// let _ = u.try_push(3).unwrap();
    /// assert_eq!(v.is_set().unwrap(), true)
    /// ```
    pub fn is_set(&self) -> Result<bool, Cancelled> {
        '_lock: {
            match &mut* unsafe { self.common.as_ref() }.lock().unwrap() {
                Some(Inner::Unset) => Ok(false),
                Some(Inner::Set(_)) => Ok(true),
                None => Err(Cancelled),
            }
        }
    }
}

/// Pulls a value "now or never" garunteeing consumption of `self`.
/// The channel will be cancelled if no value is set.
/// 
/// If you do not handle cancellation on the other side of the handshake
/// and have no garuntees that both parts will be cancelled in unison then use [`try_pull`] instead.
/// 
/// This function is provided as an alternative to [`Drop::drop`]
/// that prevents blowing the stack from deeply nested channels.
/// 
/// [`try_pull`]: Handshake::try_pull
/// 
/// # Example
/// 
/// Without using [`take`]:
/// 
/// ```
/// enum MyRecursiveType {
///     // recursive channel
///     Channel(std::mem::ManuallyDrop<oneshot_handshake::Handshake<MyRecursiveType>>),
///     Data(Box<[u8]>)
/// }
/// 
/// impl Drop for MyRecursiveType {
///     // a recursive drop implementaiton is unavoidable
///     fn drop(&mut self) {
///         match self {
///             MyRecursiveType::Channel(channel) => {
///                 let channel = unsafe { std::mem::ManuallyDrop::take(channel) };
///                 // forced to call `Drop::drop` to garuntee consumption
///                 std::mem::drop(channel)
///             },
///             MyRecursiveType::Data(_) => ()
///         };
///     }
/// }
/// ```
/// 
/// Using [`take`]:
/// 
/// ```
/// enum MyRecursiveType {
///     // recursive channel
///     Channel(std::mem::ManuallyDrop<oneshot_handshake::Handshake<MyRecursiveType>>),
///     Data(Box<[u8]>)
/// }
/// 
/// impl Drop for MyRecursiveType {
///     fn drop(&mut self) {
///         // handling dropping by ref
///         match self {
///             MyRecursiveType::Channel(channel) => {
///                 let channel = unsafe { std::mem::ManuallyDrop::take(channel) };
///                 // handling dropping by value
///                 let mut next = oneshot_handshake::take(channel);
///                 // iterative drop
///                 while let Some(mut obj) = next.take() {
///                     match &mut obj {
///                         MyRecursiveType::Channel(channel) =>
///                             next = oneshot_handshake::take(unsafe {
///                                 std::mem::ManuallyDrop::take(channel) 
///                             }), // avoids recursion
///                         MyRecursiveType::Data(_) => (),
///                     }
///                 }
///             },
///             MyRecursiveType::Data(_) => ()
///         };
///     }
/// }
/// ```
pub fn take<T>(handshake: Handshake<T>) -> Option<T> {
    let value;
    if match unsafe { handshake.common.as_ref() }.lock().unwrap().take() {
        Some(Inner::Unset) => { value = None; false },
        Some(Inner::Set(inner_value)) => { value = Some(inner_value); true },
        None => {value = None; true },
    } {
        // last reference, drop pointer
        drop(unsafe { Box::from_raw(handshake.common.as_ptr()) })
    };
    // avoid double drop
    std::mem::forget(handshake);
    value
}

impl<T> Drop for Handshake<T> {
    fn drop(&mut self) {
        if match unsafe { self.common.as_ref() }.lock().unwrap().take() {
            Some(Inner::Unset) => false,
            Some(Inner::Set(value)) => { drop(value); true },
            None => true,
        } {
            // last reference, drop pointer
            drop(unsafe { Box::from_raw(self.common.as_ptr()) })
        }
    }
}

unsafe impl<T: Send> Sync for Handshake<T> {}

unsafe impl<T: Send> Send for Handshake<T> {}

impl<T: Debug> Debug for Handshake<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handshake").field("common", unsafe { self.common.as_ref() }).finish()
    }
}

#[cfg(test)]
mod test {
    use std::convert::identity;
    use super::*;

    #[test]
    fn drop_test() {
        let (u, v) = channel::<()>();
        drop(u);
        drop(v);

        let (u, v) = channel::<()>();
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
        let (u, v) = channel::<Loud>();
        u.try_push(Loud { flag: &mut dropped }).unwrap().unwrap();
        drop(v);

        assert_eq!(dropped, true);
    }

    #[test]
    fn wrap_drop_test() {
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
        let u = Handshake::wrap(Loud { flag: &mut dropped });
        drop(u);

        assert_eq!(dropped, true);
    }

    #[test]
    fn pull_test() {
        let (u, v) = channel::<()>();
        assert_eq!(u.try_pull(), Ok(Err(v)));

        let (u, v) = channel::<()>();
        assert_eq!(v.try_pull(), Ok(Err(u)))
    }

    #[test]
    fn push_test() {
        let (u, v) = channel::<()>();
        assert_eq!(u.try_push(()), Ok(Ok(())));
        drop(v);

        let (u, v) = channel::<()>();
        assert_eq!(v.try_push(()), Ok(Ok(())));
        drop(u)
    }

    #[test]
    fn double_push_test() {
        let (u, v) = channel::<()>();
        u.try_push(()).unwrap().unwrap();
        drop(v.try_push(()).unwrap().err().unwrap());

        let (u, v) = channel::<()>();
        v.try_push(()).unwrap().unwrap();
        drop(u.try_push(()).unwrap().err().unwrap())
    }

    #[test]
    fn pull_cancel_test() {
        let (u, v) = channel::<()>();
        drop(u);
        assert_eq!(v.try_pull(), Err(Cancelled));

        let (u, v) = channel::<()>();
        drop(v);
        assert_eq!(u.try_pull(), Err(Cancelled));
    }

    #[test]
    fn push_cancel_test() {
        let (u, v) = channel::<()>();
        drop(u);
        assert_eq!(v.try_push(()), Err(()));

        let (u, v) = channel::<()>();
        drop(v);
        assert_eq!(u.try_push(()), Err(()));
    }

    #[test]
    fn push_pull_test() {
        let (u, v) = channel::<()>();
        u.try_push(()).unwrap().unwrap();
        v.try_pull().unwrap().unwrap();

        let (u, v) = channel::<()>();
        v.try_push(()).unwrap().unwrap();
        u.try_pull().unwrap().unwrap()
    }

    #[test]
    fn wrap_pull_test() {
        let u = Handshake::wrap(());
        u.try_pull().unwrap().unwrap()
    }

    #[test]
    fn join_test() {
        let (u, v) = channel::<()>();
        assert_eq!(u.join((), |_, _| ()).unwrap(), None);
        assert_eq!(v.join((), |_, _| ()).unwrap(), Some(()));

        let (u, v) = channel::<()>();
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
            let (u, v) = channel::<usize>();
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