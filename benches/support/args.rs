//! Positional arguments shared by the custom Cargo benchmark harnesses.

pub fn args() -> impl Iterator<Item = String> {
    // Cargo appends this marker even with harness = false. It is not a workload
    // size, and may appear before or after user-supplied positional arguments.
    std::env::args().skip(1).filter(|argument| argument != "--bench")
}
