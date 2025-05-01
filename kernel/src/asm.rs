macro_rules! concat_asm {
    ($($s:expr,)*) => { concat!($($s, '\n',)*) };
}

macro_rules! load {
    ($rd:ident, $rs:ident[$i:literal]) => {
        concat!(
            "ld ",
            stringify!($rd),
            ", ",
            $i,
            "*8(",
            stringify!($rs),
            ")",
        )
    };
}

macro_rules! save {
    ($rd:ident, $rs:ident[$i:literal]) => {
        concat!(
            "sd ",
            stringify!($rd),
            ", ",
            $i,
            "*8(",
            stringify!($rs),
            ")",
        )
    };
}

pub(crate) use {concat_asm, load, save};
