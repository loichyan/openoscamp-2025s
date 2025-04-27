pub fn shutdown(code: isize) -> ! {
    if code == 0 {
        sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);
    } else {
        sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::SystemFailure);
    }
    unreachable!();
}
