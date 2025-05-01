#[rustfmt::skip]
macro_rules! __asm_index {
    ($i:literal) => ( $i );
    ($i:ident) => ( concat!("{", stringify!($i), "}") );
    ($i:tt) => ( stringify!($i) );
}
