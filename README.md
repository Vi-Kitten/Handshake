# Handshake

Symmetric one time use channels.

## Example

Allows each end of the handshake to send or receive information for bi-directional movement of data:

```rs
let (u, v) = handshake::channel::<Box<str>>();
let combine = |x, y| format!("{} {}!", x, y);

'_task_a: {
    u.join("Handle Communication".into(), combine)
        .unwrap()
        .map(|s| println!("{}", s));
} // None

'_task_b: {
    v.join("Symmetrically".into(), combine)
        .unwrap()
        .map(|s| println!("{}", s));
} // Some(())
```