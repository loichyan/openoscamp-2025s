macro_rules! with {
    ($buf:ident = $expr:expr) => {{
        let r;
        (r, $buf) = $expr;
        r
    }};
}

macro_rules! tri {
    ($buf:ident = $expr:expr) => {{
        let r;
        (r, $buf) = $expr;
        match r {
            Ok(t) => t,
            Err(e) => return (Err(e.into()), $buf.into()),
        }
    }};
}
