macro_rules! concat_asm {
    ($($s:expr,)*) => { concat!($($s, '\n',)*) };
}

// No custom attributes can be used here. See <https://github.com/rust-lang/rust/issues/74087>.
// #[rustfmt::skip]
macro_rules! __asm_index {
    (+) => {
        '+'
    };
    (-) => {
        '-'
    };
    ($i:literal) => {
        $i
    };
    ($i:ident) => {
        concat!("{", stringify!($i), "}")
    };
    ($i:tt) => {
        stringify!($i)
    };
}

macro_rules! load {
    ($rd:ident, $rs:ident[$($i:tt)*]) => {
        concat!(
            "ld ",
            stringify!($rd),
            ", (",
            $($crate::asm::__asm_index!($i),)*
            ")*8(",
            stringify!($rs),
            ")",
        )
    };
}

macro_rules! save {
    ($rd:ident, $rs:ident[$($i:tt)*]) => {
        concat!(
            "sd ",
            stringify!($rd),
            ", (",
            $($crate::asm::__asm_index!($i),)*
            ")*8(",
            stringify!($rs),
            ")",
        )
    };
}

pub(crate) use {__asm_index, concat_asm, load, save};
