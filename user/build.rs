fn main() {
    println!("cargo::rerun-if-changed=user/linker.ld");
    println!("cargo::rustc-link-arg=-Tuser/linker.ld");
}
