use std::{cell::UnsafeCell, ptr::NonNull, sync::OnceLock};

pub struct Canceled;

pub struct Handshake<T> {
    // NotNull is &
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
        std::mem::forget(self);
        combined
    }

    pub fn try_push(self, value: T) -> Result<Result<(), (Self, T)>, T> {
        let mut value = Some(value);
        // access safe lock
        let res = unsafe { self.common.as_ref() }.get_or_init(||
            UnsafeCell::new(Some(value.take().unwrap()))
        );
        value.map_or(Ok(Ok(())), |value| {
            // value present, lock inhabited
            if unsafe { &*res.get() }.is_none() {
                // handshake was cancelled
                drop(unsafe { Box::from_raw(self.common.as_ptr()) });
                std::mem::forget(self);
                Err(value)
            } else {
                Ok(Err((self, value)))
            }
        })
    }

    pub fn try_pull(self) -> Result<Result<T, Self>, Canceled> {
        // access safe lock
        if let Some(res) = unsafe { self.common.as_ref() }.get() {
            // unique access if value present
            if let Some(value) = unsafe { &mut*res.get() }.take() {
                // last reference, drop pointer
                drop(unsafe { Box::from_raw(self.common.as_ptr()) });
                std::mem::forget(self);
                Ok(Ok(value))
            } else {
                // handshake was cancelled
                drop(unsafe { Box::from_raw(self.common.as_ptr()) });
                std::mem::forget(self);
                Err(Canceled)
            }

        } else {
            Ok(Err(self))
        }
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
