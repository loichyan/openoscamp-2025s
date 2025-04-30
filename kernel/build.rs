fn main() {
    println!("cargo::rerun-if-changed=kernel/linker.ld");
    println!("cargo::rustc-link-arg=-Tkernel/linker.ld");
}
