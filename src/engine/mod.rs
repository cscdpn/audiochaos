//! The real-time core: everything that runs on, or talks to, the audio
//! thread. Nothing here allocates, locks or waits on the audio thread; work
//! that would (building modules, loading sounds, freeing them, writing files)
//! happens on other threads and crosses over through lock-free queues and
//! atomics.

#[cfg(feature = "app")]
pub mod audio;
pub mod control;
pub mod graph;
pub mod input;
pub mod module;
pub mod param;
pub mod record;
pub mod sample;
pub mod transport;
