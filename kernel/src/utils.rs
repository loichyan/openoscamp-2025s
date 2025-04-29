/// # Examples
///
/// ```rust
/// let code = concat_instructions!(
///     "op1" "arg1," "arg2," "arg3";
///     "op2" "arg1," "arg2," "arg3";
///     "op3" "arg1," "arg2," "arg3";
/// );
/// ```
/// Which is expanded to:
/// ```rust
/// let code = "
///     op1 arg1, arg2, arg3
///     op2 arg1, arg2, arg3
///     op3 arg1, arg2, arg3
/// ";
/// ```
macro_rules! concat_instructions {
    ($($($s:literal)+;)*) => { concat!($($($s, ' ',)* '\n',)*) };
}
