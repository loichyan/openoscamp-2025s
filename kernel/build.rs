fn main() {
    println!("cargo::rerun-if-changed=scripts/kernel.ld");
    println!("cargo::rustc-link-arg=-Tscripts/kernel.ld");
}
