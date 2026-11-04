//! `cargo xtask bundle audiochaos-plugin --release` builds the VST3 and CLAP
//! bundles into `target/bundled/`: a plugin is more than a shared library,
//! and cargo alone does not lay out the folders hosts expect.

fn main() -> nice_plug_xtask::Result<()> {
    nice_plug_xtask::main()
}
