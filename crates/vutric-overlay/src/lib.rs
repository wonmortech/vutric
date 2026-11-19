//! Vutric desktop effects.
//!
//! [`timeline`] is pure math (no GPU) and fully unit-tested; [`renderer::Renderer`] turns a
//! [`timeline::Frame`] into pixels with one scissored full-screen draw. Getting those pixels onto the desktop
//! without stealing focus is the job of the platform host crates.

pub mod card;
pub mod effects;
pub mod palette;
pub mod renderer;
pub mod timeline;

pub use effects::{Effects, Fling};
pub use palette::Palette;
pub use renderer::Renderer;
pub use timeline::{Frame, Halqa, HalqaConfig};

/// The WGSL source, exposed so tools and tests can validate it without a GPU.
pub const HALQA_WGSL: &str = include_str!("halqa.wgsl");

#[cfg(test)]
mod shader_tests {
    #[test]
    fn halqa_wgsl_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(super::HALQA_WGSL).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
    }
}
