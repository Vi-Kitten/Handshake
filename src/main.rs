fn main() {
    let (u, v) = oneshot_handshake::channel::<Box<str>>();
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
}