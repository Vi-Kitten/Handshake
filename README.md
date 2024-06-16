# Handshake

Symmetric one time use channels.

```rs
'_task_a: {
    u.join("Handle Communication".into(), combine)
        .unwrap()
        .map(|s| println!("{}", s));
}

'_task_b: {
    v.join("Symmetrically".into(), combine)
        .unwrap()
        .map(|s| println!("{}", s));
}
```