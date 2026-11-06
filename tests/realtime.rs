//! The audio thread must never allocate: the allocator can take a lock or
//! ask the system for memory, and the sound breaks up while it waits. This
//! test counts the allocations made while the engine computes samples, with
//! every preset, notes, knob turns and live edits, and expects none.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use audiochaos::engine::control::Engine;
use audiochaos::engine::graph::Cable;
use audiochaos::engine::input::{self, Callbacks, LiveInput};
use audiochaos::midi::{Parser, State};
use audiochaos::modules::{self, Context, oscillator};
use audiochaos::presets;

/// Counts the allocations of the threads that ask for it.
struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.get() {
            ALLOCATIONS.set(ALLOCATIONS.get() + 1);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if COUNTING.get() {
            ALLOCATIONS.set(ALLOCATIONS.get() + 1);
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Allocations and frees made while running `audio`, as the audio thread.
fn allocations_in(audio: impl FnOnce()) -> usize {
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    audio();
    COUNTING.set(false);
    ALLOCATIONS.get()
}

const SR: f32 = 48_000.0;
const SECOND: usize = SR as usize;

#[test]
fn the_count_sees_allocations() {
    let allocations = allocations_in(|| {
        std::hint::black_box(vec![0u8; 16]);
    });
    assert_eq!(allocations, 2, "one allocation, one free");
}

#[test]
fn playing_any_preset_with_notes_and_knobs_never_allocates() {
    for name in presets::names() {
        let midi = Arc::new(State::default());
        let context = Context {
            midi: Arc::clone(&midi),
            ..Context::new(SR)
        };
        let (graph, output) = presets::get(name).unwrap().build(&context).unwrap();
        let (mut engine, _controller) = Engine::new(graph, output, 64);
        let mut parser = Parser::default();
        let allocations = allocations_in(|| {
            for frame in 0..2 * SECOND {
                // As the plugin does, MIDI between frames: chords, bends,
                // knobs and pads, on and off.
                match frame % 6_000 {
                    0 => {
                        for note in [48, 55, 60, 64, 67] {
                            parser.handle(&[0x90, note, 100], &midi);
                        }
                    }
                    1_000 => parser.handle(&[0xe0, 0, 0x50], &midi),
                    2_000 => parser.handle(&[0xb0, 1, 90], &midi),
                    2_500 => parser.handle(&[0x90, 36, 127], &midi),
                    3_000 => {
                        for note in [48, 55, 60, 64, 67, 36] {
                            parser.handle(&[0x80, note, 0], &midi);
                        }
                    }
                    _ => {}
                }
                engine.next_frame();
            }
        });
        assert_eq!(allocations, 0, "{name}");
    }
}

#[test]
fn live_edits_never_allocate_on_the_audio_side() {
    let context = Context::new(SR);
    let (graph, output) = presets::get("chaos").unwrap().build(&context).unwrap();
    let (mut engine, mut controller) = Engine::new(graph, output, 64);
    let run = |engine: &mut Engine| {
        allocations_in(|| {
            (0..SECOND / 10).for_each(|_| {
                engine.next_frame();
            })
        })
    };
    assert_eq!(run(&mut engine), 0, "playing");

    // Each edit is prepared on this thread, the UI's, then applied by the
    // engine, which is what is counted.
    let osc = controller
        .add(
            "osc 9".to_string(),
            modules::build("osc", &context, 1).unwrap(),
        )
        .unwrap();
    controller
        .connect(Cable {
            from: osc,
            from_port: oscillator::OUT_SAW,
            to: controller.output(),
            to_port: 0,
        })
        .unwrap();
    controller.set_param(osc, oscillator::FREQ, 440.0).unwrap();
    assert_eq!(run(&mut engine), 0, "adding and patching a module");

    controller.remove(osc).unwrap();
    assert_eq!(run(&mut engine), 0, "removing it");

    let (graph, output) = presets::get("bass").unwrap().build(&context).unwrap();
    controller.replace(graph, output).unwrap();
    assert_eq!(run(&mut engine), 0, "loading another patch");

    controller.set_running(false);
    assert_eq!(run(&mut engine), 0, "stopping");
}

#[test]
fn the_sound_cards_input_never_allocates() {
    let live = Arc::new(LiveInput::default());
    let context = Context {
        input: Arc::clone(&live),
        ..Context::new(SR)
    };
    let (graph, output) = presets::get("looper").unwrap().build(&context).unwrap();
    let (mut engine, _controller) = Engine::new(graph, output, 64);
    let glitches = Arc::new(AtomicUsize::new(0));
    let (mut capture, feed) = input::queue(live, Arc::new(Callbacks::default()), glitches);
    engine.set_input(feed);
    let frames = vec![0.25f32; 2 * 256];
    let allocations = allocations_in(|| {
        for _ in 0..SECOND / 256 {
            // The input callback, then the output callback.
            capture.push(&frames, 2);
            for _ in 0..256 {
                engine.next_frame();
            }
        }
    });
    assert_eq!(allocations, 0);
}
