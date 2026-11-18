#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| vutric_fuzz::frame_stream(data));
