use egui_snarl::ui::SnarlViewer;

use super::layout::COLUMN_HEIGHT;
use super::widgets::{AUDIO_PIN, CV_PIN, LED_RADIUS, knob};
use super::*;
use crate::engine::control::Engine;
use crate::engine::param::ParamInfo;
use crate::modules::{clock, delay, filter, master, oscillator, vca};
use crate::presets;

fn editor(preset: &str) -> Editor {
    let patch = presets::get(preset).unwrap();
    let (graph, out) = patch.build(&Context::new(48_000.0)).unwrap();
    // Large enough for every command these tests send without an engine
    // draining the queue.
    let (_, controller) = Engine::new(graph, out, 1024);
    Editor::new(controller, Context::new(48_000.0), &patch.positions)
}

fn module(editor: &Editor, name: &str) -> NodeId {
    let nodes = editor.controller.nodes();
    nodes.iter().find(|n| n.name == name).unwrap().id
}

fn block_of(editor: &Editor, name: &str) -> BlockId {
    block(&editor.snarl, module(editor, name))
}

fn wires(editor: &Editor) -> Vec<(OutPinId, InPinId)> {
    let mut wires: Vec<_> = editor.snarl.wires().collect();
    wires.sort();
    wires
}

/// The wires the controller's cables should be drawn as.
fn expected_wires(editor: &Editor) -> Vec<(OutPinId, InPinId)> {
    let mut wires: Vec<_> = editor
        .controller
        .cables()
        .iter()
        .map(|&c| wire(&editor.snarl, c))
        .collect();
    wires.sort();
    wires
}

/// Runs `f` with a viewer, as egui-snarl would when the user edits wires.
fn with_viewer(editor: &mut Editor, f: impl FnOnce(&mut Viewer, &mut Snarl<NodeId>)) {
    let mut viewer = Viewer {
        controller: &mut editor.controller,
        context: &editor.context,
        status: &mut editor.status,
        sizes: &mut editor.sizes,
        show_origin: &mut editor.show_origin,
        taps: &mut editor.taps,
        note_views: &mut editor.note_views,
        to_global: &mut editor.to_global,
        to_duplicate: &mut editor.to_duplicate,
        view: Rect::ZERO,
    };
    f(&mut viewer, &mut editor.snarl);
}

/// osc 1 saw into the filter's cutoff CV, as pins.
fn osc_to_cutoff(editor: &Editor) -> (OutPinId, InPinId) {
    (
        OutPinId {
            node: block_of(editor, "osc 1"),
            output: oscillator::OUT_SAW,
        },
        InPinId {
            node: block_of(editor, "filter"),
            input: filter::INFO.cv_port(filter::CUTOFF),
        },
    )
}

#[test]
fn draws_one_block_per_module_and_one_wire_per_cable() {
    let editor = editor("chaos");
    assert_eq!(
        editor.snarl.nodes().count(),
        editor.controller.nodes().len()
    );
    assert_eq!(wires(&editor), expected_wires(&editor));
    assert!(!wires(&editor).is_empty());
}

#[test]
fn dragging_a_wire_connects_the_cable() {
    let mut editor = editor("bass");
    let (from, to) = osc_to_cutoff(&editor);
    with_viewer(&mut editor, |viewer, snarl| {
        viewer.connect(&snarl.out_pin(from), &snarl.in_pin(to), snarl);
    });
    assert!(editor.snarl.wires().any(|w| w == (from, to)));
    assert_eq!(wires(&editor), expected_wires(&editor));
}

#[test]
fn a_refused_cable_draws_no_wire_and_says_why() {
    let mut editor = editor("bass");
    let out = block_of(&editor, "master");
    let (from, to) = (
        OutPinId {
            node: out,
            output: master::OUT_LEFT,
        },
        InPinId {
            node: out,
            input: master::IN,
        },
    );
    with_viewer(&mut editor, |viewer, snarl| {
        viewer.connect(&snarl.out_pin(from), &snarl.in_pin(to), snarl);
        // The same cable twice is refused by the controller.
        viewer.connect(&snarl.out_pin(from), &snarl.in_pin(to), snarl);
    });
    assert_eq!(editor.status, "cannot connect: already connected");
    assert_eq!(wires(&editor), expected_wires(&editor));
}

#[test]
fn removing_wires_disconnects_the_cables() {
    let mut editor = editor("chaos");
    let fm = InPinId {
        node: block_of(&editor, "osc 1"),
        input: oscillator::IN_FM,
    };
    let fm_wires = |e: &Editor| e.snarl.wires().filter(|(_, to)| *to == fm).count();
    // The chaos preset feeds osc 2 and the delay into osc 1's FM.
    assert_eq!(fm_wires(&editor), 2);
    with_viewer(&mut editor, |viewer, snarl| {
        viewer.drop_inputs(&snarl.in_pin(fm), snarl);
    });
    assert_eq!(fm_wires(&editor), 0);
    assert_eq!(wires(&editor), expected_wires(&editor));
}

#[test]
fn wires_yanked_off_by_the_widget_disconnect_their_cables() {
    let mut editor = editor("chaos");
    let fm = InPinId {
        node: block_of(&editor, "osc 1"),
        input: oscillator::IN_FM,
    };
    let before = editor.controller.cables().len();
    // What egui-snarl does on Ctrl+drag from a pin, bypassing the viewer.
    editor.snarl.drop_inputs(fm);
    editor.disconnect_cables_without_wires();
    assert_eq!(editor.controller.cables().len(), before - 2);
    assert_eq!(wires(&editor), expected_wires(&editor));
}

#[test]
fn loading_a_patch_redraws_wires_and_positions() {
    let mut editor = editor("chaos");
    let mut bass = presets::get("bass").unwrap();
    bass.positions.insert("osc 1".to_string(), [123.0, 456.0]);
    editor.load(&bass).unwrap();
    assert_eq!(wires(&editor), expected_wires(&editor));
    assert_eq!(editor.controller.cables().len(), bass.cables.len());
    let osc = block_of(&editor, "osc 1");
    let pos = editor.snarl.get_node_info(osc).unwrap().pos;
    assert_eq!([pos.x, pos.y], [123.0, 456.0]);
}

#[test]
fn another_engine_plays_the_same_blocks_in_the_same_places() {
    let mut editor = editor("chaos");
    let osc = block_of(&editor, "osc 1");
    editor.snarl.get_node_info_mut(osc).unwrap().pos = egui::pos2(123.0, 456.0);
    editor
        .controller
        .set_param(module(&editor, "osc 1"), oscillator::FREQ, 220.0)
        .unwrap();
    let before = editor.patch();
    // As built on another sound card, at another rate.
    let context = Context::new(44_100.0);
    let (graph, out) = before.build(&context).unwrap();
    let (_, controller) = Engine::new(graph, out, 1024);
    editor.replace_engine(controller, context);
    assert_eq!(editor.patch(), before);
    assert_eq!(wires(&editor), expected_wires(&editor));
}

#[test]
fn saved_patch_round_trips_with_positions() {
    let mut editor = editor("chaos");
    let saved = editor.patch();
    assert_eq!(saved.positions.len(), editor.controller.nodes().len());
    editor
        .load(&Patch::from_json(&saved.to_json()).unwrap())
        .unwrap();
    assert_eq!(editor.patch(), saved);
}

#[test]
fn the_palette_adds_a_module_and_its_block_where_clicked() {
    let mut editor = editor("bass");
    let before = editor.controller.nodes().len();
    with_viewer(&mut editor, |viewer, snarl| {
        viewer.add("filter", egui::pos2(10.0, 20.0), snarl);
    });
    assert_eq!(editor.status, "added filter 2");
    assert_eq!(editor.controller.nodes().len(), before + 1);
    let pos = editor
        .snarl
        .get_node_info(block_of(&editor, "filter 2"))
        .unwrap()
        .pos;
    assert_eq!([pos.x, pos.y], [10.0, 20.0]);
    assert_eq!(editor.patch().positions["filter 2"], [10.0, 20.0]);
}

#[test]
fn samplers_take_a_sound_from_a_file_or_say_why_not() {
    let mut editor = editor("bass");
    with_viewer(&mut editor, |viewer, snarl| {
        viewer.add("sampler", egui::pos2(0.0, 0.0), snarl);
    });
    let id = module(&editor, "sampler");

    with_viewer(&mut editor, |viewer, _| {
        viewer.load_sample(id, "/nowhere/kick.wav");
    });
    assert!(
        editor.status.starts_with("cannot load kick.wav: "),
        "{}",
        editor.status
    );
    assert!(editor.controller.node(id).sample.is_none());

    let path = std::env::temp_dir().join(format!("audiochaos-editor-{}.wav", std::process::id()));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav = hound::WavWriter::create(&path, spec).unwrap();
    wav.write_sample(1000_i16).unwrap();
    wav.finalize().unwrap();
    let path = path.to_str().unwrap();
    with_viewer(&mut editor, |viewer, _| viewer.load_sample(id, path));
    assert!(editor.status.starts_with("loaded audiochaos-editor-"));
    let saved = editor.patch();
    let entry = saved.modules.iter().find(|m| m.kind == "sampler").unwrap();
    assert_eq!(entry.file.as_deref(), Some(path));
}

#[test]
fn removing_a_block_removes_its_module_and_wires() {
    let mut editor = editor("chaos");
    let osc = block_of(&editor, "osc 1");
    with_viewer(&mut editor, |viewer, snarl| viewer.remove(osc, snarl));
    assert_eq!(editor.status, "removed osc 1");
    assert!(editor.controller.nodes().iter().all(|n| n.name != "osc 1"));
    assert_eq!(
        editor.snarl.nodes().count(),
        editor.controller.nodes().len()
    );
    assert_eq!(wires(&editor), expected_wires(&editor));
    // Still consistent on the next frame.
    editor.disconnect_cables_without_wires();
    assert_eq!(wires(&editor), expected_wires(&editor));
}

#[test]
fn the_master_stays() {
    let mut editor = editor("bass");
    let out = block_of(&editor, "master");
    with_viewer(&mut editor, |viewer, snarl| viewer.remove(out, snarl));
    assert_eq!(
        editor.status,
        "cannot remove master: the output module cannot be removed"
    );
    assert!(editor.snarl.get_node_info(out).is_some());
}

#[test]
fn loading_a_patch_with_other_modules_redraws_every_block() {
    let mut editor = editor("chaos");
    let small = Patch::from_json(
        r#"{ "version": 2, "modules": [
            { "name": "lfo", "type": "lfo" },
            { "name": "master", "type": "master" }
        ], "cables": [
            { "from": "lfo", "output": "sine", "to": "master", "input": "in" }
        ] }"#,
    )
    .unwrap();
    editor.load(&small).unwrap();
    assert_eq!(editor.snarl.nodes().count(), 2);
    assert_eq!(wires(&editor), expected_wires(&editor));
    assert_eq!(wires(&editor).len(), 1);
}

#[test]
fn default_layout_keeps_columns_within_their_height() {
    let editor = editor("bass");
    let layout = default_layout(&editor.controller);
    assert!(layout.iter().all(|p| p.y < COLUMN_HEIGHT));
    assert!(layout.windows(2).all(|w| w[1].x >= w[0].x));
    assert!(layout.last().unwrap().x > 0.0, "everything in one column");
}

#[test]
fn right_clicking_a_knob_opens_its_menu() {
    let param = &filter::INFO.params[filter::CUTOFF];
    let mut value = param.default;
    let ctx = egui::Context::default();
    // Returns where the knob is and whether a menu is open.
    let mut frame = |events: Vec<egui::Event>| {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut seen = (egui::Rect::NOTHING, false);
        let mut output = ctx.run_ui(input, |ui| {
            knob(param, &mut value, None, ui);
            seen = (ui.min_rect(), egui::Popup::is_any_open(ui.ctx()));
        });
        output.textures_delta.clear();
        seen
    };
    let (slider, opened) = frame(vec![]);
    assert!(!opened);
    let pos = egui::pos2(slider.left() + 20.0, slider.center().y);
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(vec![egui::Event::PointerMoved(pos)]);
    frame(vec![button(true)]);
    frame(vec![button(false)]);
    assert!(frame(vec![]).1);
}

/// One frame of the editor as a window would draw it, flattened into what
/// gets painted.
fn paint(editor: &mut Editor, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<egui::Shape> {
    paint_input(editor, ctx, window(events))
}

fn window(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        events,
        // The window's size: without one, egui makes the screen huge and
        // everything is in view.
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1400.0, 900.0))),
        ..Default::default()
    }
}

fn paint_input(
    editor: &mut Editor,
    ctx: &egui::Context,
    input: egui::RawInput,
) -> Vec<egui::Shape> {
    let mut output = ctx.run_ui(input, |ui| editor.show_graph(ui));
    // No renderer consumes the font textures here, and egui insists they
    // are dropped explicitly.
    output.textures_delta.clear();
    let mut shapes = Vec::new();
    fn flatten(shape: egui::Shape, into: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(inner) => inner.into_iter().for_each(|s| flatten(s, into)),
            other => into.push(other),
        }
    }
    for clipped in output.shapes {
        flatten(clipped.shape, &mut shapes);
    }
    shapes
}

fn texts(shapes: &[egui::Shape]) -> Vec<(String, egui::Rect)> {
    shapes
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Text(t) if !t.galley.text().is_empty() => {
                Some((t.galley.text().to_string(), t.visual_bounding_rect()))
            }
            _ => None,
        })
        .collect()
}

fn text_at(shapes: &[egui::Shape], text: &str) -> egui::Rect {
    texts(shapes)
        .into_iter()
        .find(|(t, _)| t == text)
        .unwrap_or_else(|| panic!("{text:?} is not on screen"))
        .1
}

fn click(pos: Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn no_label_runs_into_a_pin_or_another_label() {
    for preset in presets::names() {
        let mut editor = editor(preset);
        let ctx = egui::Context::default();
        paint(&mut editor, &ctx, vec![]);
        let shapes = paint(&mut editor, &ctx, vec![]);
        let pins: Vec<_> = shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Circle(c) if c.fill == AUDIO_PIN || c.fill == CV_PIN => {
                    Some(shape.visual_bounding_rect())
                }
                _ => None,
            })
            .collect();
        assert!(!pins.is_empty());
        let texts = texts(&shapes);
        for (i, (text, rect)) in texts.iter().enumerate() {
            assert!(
                pins.iter().all(|pin| !pin.intersects(*rect)),
                "{preset}: {text:?} touches a pin"
            );
            for (other, other_rect) in &texts[i + 1..] {
                assert!(
                    !rect.intersects(*other_rect),
                    "{preset}: {text:?} touches {other:?}"
                );
            }
        }
    }
}

#[test]
fn a_patch_without_positions_is_arranged_by_the_real_sizes_of_its_blocks() {
    // A clock block is wider than the default layout guesses.
    let patch = presets::get("song").unwrap();
    let (graph, out) = patch.build(&Context::new(48_000.0)).unwrap();
    let (_, controller) = Engine::new(graph, out, 1024);
    let mut editor = Editor::new(controller, Context::new(48_000.0), &BTreeMap::new());
    let ctx = egui::Context::default();
    paint(&mut editor, &ctx, vec![]);
    paint(&mut editor, &ctx, vec![]);
    let rects: Vec<Rect> = editor
        .snarl
        .node_ids()
        .map(|(block, _)| {
            Rect::from_min_size(
                editor.snarl.get_node_info(block).unwrap().pos,
                editor.sizes[&block],
            )
        })
        .collect();
    for (i, a) in rects.iter().enumerate() {
        for b in &rects[i + 1..] {
            assert!(!a.intersects(*b), "{a:?} overlaps {b:?}");
        }
    }
}

#[test]
fn the_menu_of_a_knob_resets_it_in_the_playing_patch() {
    use egui::PointerButton::{Primary, Secondary};

    let mut editor = editor("keys");
    let delay = module(&editor, "delay");
    let param = &delay::INFO.params[delay::FEEDBACK];
    assert_ne!(
        editor.controller.node(delay).params[delay::FEEDBACK],
        param.min
    );

    let ctx = egui::Context::default();
    paint(&mut editor, &ctx, vec![]);
    let shapes = paint(&mut editor, &ctx, vec![]);
    // The slider lies left of its value and name.
    let name = text_at(&shapes, param.name);
    let on_slider = egui::pos2(name.left() - 100.0, name.center().y);
    paint(
        &mut editor,
        &ctx,
        vec![egui::Event::PointerMoved(on_slider)],
    );
    paint(&mut editor, &ctx, vec![click(on_slider, Secondary, true)]);
    paint(&mut editor, &ctx, vec![click(on_slider, Secondary, false)]);
    let shapes = paint(&mut editor, &ctx, vec![]);

    let (label, _) = param.resets()[0];
    let item = text_at(&shapes, label).center();
    paint(&mut editor, &ctx, vec![egui::Event::PointerMoved(item)]);
    paint(&mut editor, &ctx, vec![click(item, Primary, true)]);
    paint(&mut editor, &ctx, vec![click(item, Primary, false)]);

    assert_eq!(
        editor.controller.node(delay).params[delay::FEEDBACK],
        param.min
    );
    assert!(editor.status.is_empty(), "{}", editor.status);
}

/// Every block as drawn on the last frame, in graph space.
fn block_rects(editor: &Editor) -> Vec<(String, Rect)> {
    editor
        .snarl
        .node_ids()
        .map(|(block, &id)| {
            let pos = editor.snarl.get_node_info(block).unwrap().pos;
            let rect = Rect::from_min_size(pos, editor.sizes[&block]);
            (editor.controller.node(id).name.clone(), rect)
        })
        .collect()
}

#[test]
fn arranging_pulls_piled_up_blocks_apart() {
    let mut editor = editor("keys");
    with_viewer(&mut editor, |viewer, snarl| {
        for kind in ["chorus", "flanger", "phaser", "sampler"] {
            viewer.add(kind, egui::pos2(0.0, 0.0), snarl);
        }
    });
    for (block, _) in editor
        .snarl
        .node_ids()
        .map(|(b, &id)| (b, id))
        .collect::<Vec<_>>()
    {
        editor.snarl.get_node_info_mut(block).unwrap().pos = egui::pos2(0.0, 0.0);
    }
    let ctx = egui::Context::default();
    paint(&mut editor, &ctx, vec![]);
    paint(&mut editor, &ctx, vec![]);
    editor.arrange();
    paint(&mut editor, &ctx, vec![]);

    let rects = block_rects(&editor);
    for (i, (name, rect)) in rects.iter().enumerate() {
        for (other, other_rect) in &rects[i + 1..] {
            assert!(!rect.intersects(*other_rect), "{name} overlaps {other}");
        }
    }
    let rightmost = rects.iter().map(|(_, r)| r.left()).fold(0.0, f32::max);
    let master = &rects.iter().find(|(n, _)| n == "master").unwrap().1;
    assert_eq!(master.left(), rightmost, "the master is in the last column");
}

#[test]
fn arranging_brings_the_blocks_into_view() {
    use egui::PointerButton::Primary;

    let mut editor = editor("chaos");
    let ctx = egui::Context::default();
    let first = editor.controller.nodes()[0].name.clone();
    let in_view = |shapes: &[egui::Shape]| {
        texts(shapes)
            .iter()
            .any(|(t, r)| *t == first && ctx.content_rect().contains_rect(*r))
    };
    // Blocks far away, and the view taken there by a double click on the
    // empty canvas, which egui-snarl answers by centring on the blocks.
    for block in editor.snarl.node_ids().map(|(b, _)| b).collect::<Vec<_>>() {
        editor.snarl.get_node_info_mut(block).unwrap().pos = egui::pos2(5000.0, 5000.0);
    }
    let empty = egui::pos2(700.0, 450.0);
    paint(&mut editor, &ctx, vec![egui::Event::PointerMoved(empty)]);
    for _ in 0..2 {
        paint(&mut editor, &ctx, vec![click(empty, Primary, true)]);
        paint(&mut editor, &ctx, vec![click(empty, Primary, false)]);
    }
    assert!(in_view(&paint(&mut editor, &ctx, vec![])), "not centred");

    editor.arrange();
    paint(&mut editor, &ctx, vec![]);
    assert!(in_view(&paint(&mut editor, &ctx, vec![])));
}

#[test]
fn blocks_with_a_light_show_it_once_the_patch_plays() {
    let patch = presets::get("chaos").unwrap();
    let (graph, out) = patch.build(&Context::new(48_000.0)).unwrap();
    let (mut engine, controller) = Engine::new(graph, out, 1024);
    let mut editor = Editor::new(controller, Context::new(48_000.0), &patch.positions);
    let lights = |shapes: &[egui::Shape]| -> Vec<Rect> {
        shapes
            .iter()
            .filter_map(|shape| match shape {
                // Every other circle is a pin; the view's zoom scales radii.
                egui::Shape::Circle(c) if c.fill != AUDIO_PIN && c.fill != CV_PIN => {
                    Some(shape.visual_bounding_rect())
                }
                _ => None,
            })
            .collect()
    };
    let ctx = egui::Context::default();
    paint(&mut editor, &ctx, vec![]);
    assert!(lights(&paint(&mut editor, &ctx, vec![])).is_empty());
    assert!(!editor.moves());

    for _ in 0..crate::engine::module::BLOCK {
        engine.next_frame();
    }
    let shapes = paint(&mut editor, &ctx, vec![]);
    let lit: Vec<_> = editor
        .controller
        .nodes()
        .iter()
        .filter(|n| editor.controller.led(n.id).is_some())
        .map(|n| n.name.clone())
        .collect();
    assert!(lit.contains(&"delay".to_string()), "{lit:?}");
    assert!(editor.moves());
    let lights = lights(&shapes);
    assert_eq!(lights.len(), lit.len());
    // Each beside its block's title, whose glyphs may be shorter than it.
    for name in &lit {
        let title = text_at(&shapes, name).expand2(egui::vec2(0.0, LED_RADIUS));
        let beside = |l: &Rect| l.left() > title.right() && title.y_range().contains(l.center().y);
        assert!(lights.iter().any(beside), "{name}");
    }
}

#[test]
fn tapping_sets_the_clock_tempo() {
    use egui::PointerButton::Primary;

    let mut editor = editor("bass");
    with_viewer(&mut editor, |viewer, snarl| {
        viewer.add("clock", egui::pos2(0.0, 0.0), snarl);
    });
    let id = module(&editor, "clock");
    let ctx = egui::Context::default();
    paint(&mut editor, &ctx, vec![]);
    // Away from the blocks it was dropped on.
    editor.arrange();
    paint(&mut editor, &ctx, vec![]);
    let button = text_at(&paint(&mut editor, &ctx, vec![]), "Tap").center();
    // Clicks 0.4 s apart: 150 BPM.
    let tap_three_times = |editor: &mut Editor, start: f64| {
        for time in [start, start + 0.4, start + 0.8] {
            let at = |events| egui::RawInput {
                time: Some(time),
                ..window(events)
            };
            paint_input(editor, &ctx, at(vec![egui::Event::PointerMoved(button)]));
            paint_input(editor, &ctx, at(vec![click(button, Primary, true)]));
            paint_input(editor, &ctx, at(vec![click(button, Primary, false)]));
        }
        editor.controller.node(id).params[clock::TEMPO]
    };
    let tempo = tap_three_times(&mut editor, 10.0);
    assert!((tempo - 150.0).abs() < 1.0, "{tempo}");

    // A pad in the tap input sets the tempo; the button would change a
    // knob that no longer counts.
    let gate = OutPinId {
        node: block_of(&editor, "seq"),
        output: crate::modules::sequencer::OUT_GATE,
    };
    let tap = InPinId {
        node: block_of(&editor, "clock"),
        input: clock::IN_TAP,
    };
    with_viewer(&mut editor, |viewer, snarl| {
        viewer.connect(&snarl.out_pin(gate), &snarl.in_pin(tap), snarl);
    });
    editor.controller.set_param(id, clock::TEMPO, 90.0).unwrap();
    assert_eq!(tap_three_times(&mut editor, 20.0), 90.0);
}

#[test]
fn hovering_a_knob_says_what_it_does() {
    let mut editor = editor("keys");
    let param = &delay::INFO.params[delay::FEEDBACK];
    let ctx = egui::Context::default();
    paint(&mut editor, &ctx, vec![]);
    let shapes = paint(&mut editor, &ctx, vec![]);
    let name = text_at(&shapes, param.name);
    let on_slider = egui::pos2(name.left() - 100.0, name.center().y);
    let mut shown = false;
    // Tooltips wait for the pointer to rest.
    for n in 0..20 {
        let events = match n {
            0 => vec![egui::Event::PointerMoved(on_slider)],
            _ => vec![],
        };
        let input = egui::RawInput {
            time: Some(n as f64 * 0.1),
            ..window(events)
        };
        let shapes = paint_input(&mut editor, &ctx, input);
        shown |= texts(&shapes).iter().any(|(t, _)| t == param.help);
    }
    assert!(shown, "no tooltip");
}

/// One frame of a lone knob; returns what was painted.
fn paint_knob(param: &ParamInfo, value: f32, modulated: Option<f32>) -> Vec<egui::Shape> {
    let ctx = egui::Context::default();
    let mut value = value;
    let mut output = ctx.run_ui(window(vec![]), |ui| {
        knob(param, &mut value, modulated, ui);
    });
    output.textures_delta.clear();
    output.shapes.into_iter().map(|c| c.shape).collect()
}

fn marks(shapes: &[egui::Shape]) -> Vec<f32> {
    shapes
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::LineSegment { points, stroke } if stroke.color == CV_PIN => {
                Some(points[0].x)
            }
            _ => None,
        })
        .collect()
}

/// The handle: the only shape narrower than it is tall.
fn handle_x(shapes: &[egui::Shape]) -> f32 {
    let handles: Vec<f32> = shapes
        .iter()
        .map(|shape| shape.visual_bounding_rect())
        .filter(|r| r.is_positive() && r.width() < r.height())
        .map(|r| r.center().x)
        .collect();
    assert_eq!(handles.len(), 1, "{shapes:?}");
    handles[0]
}

#[test]
fn the_modulation_mark_sits_where_the_handle_would() {
    // Logarithmic and linear knobs.
    for param in [
        &filter::INFO.params[filter::CUTOFF],
        &delay::INFO.params[delay::FEEDBACK],
    ] {
        let modulated = param.from_normalized(0.7);
        let marked = marks(&paint_knob(param, param.min, Some(modulated)));
        assert_eq!(marked.len(), 1, "{}", param.name);
        let handle = handle_x(&paint_knob(param, modulated, None));
        assert!(
            (marked[0] - handle).abs() < 0.5,
            "{}: {marked:?} vs {handle}",
            param.name
        );
    }
}

#[test]
fn no_mark_where_the_knob_is_not_moved() {
    let param = &delay::INFO.params[delay::FEEDBACK];
    assert!(marks(&paint_knob(param, 0.5, Some(0.5))).is_empty());
    assert!(marks(&paint_knob(param, 0.5, None)).is_empty());
}

#[test]
fn modulated_knobs_are_marked_once_the_patch_plays() {
    let patch = presets::get("chaos").unwrap();
    let (graph, out) = patch.build(&Context::new(48_000.0)).unwrap();
    let (mut engine, controller) = Engine::new(graph, out, 1024);
    let mut editor = Editor::new(controller, Context::new(48_000.0), &patch.positions);
    // Past the knobs' glides.
    for _ in 0..48_000 {
        engine.next_frame();
    }
    let moved = editor
        .controller
        .nodes()
        .iter()
        .flat_map(|n| {
            let c = &editor.controller;
            n.info.params.iter().enumerate().filter(move |&(p, param)| {
                marked(param, n.params[p], c.modulated(n.id, p)).is_some()
            })
        })
        .count();
    assert!(moved > 0, "the chaos preset modulates knobs");
    let ctx = egui::Context::default();
    paint(&mut editor, &ctx, vec![]);
    let shapes = paint(&mut editor, &ctx, vec![]);
    assert_eq!(marks(&shapes).len(), moved);
}

#[test]
fn a_modulated_knob_alone_keeps_the_editor_redrawing() {
    // No module here has a light.
    let patch = Patch::from_json(
        r#"{ "version": 2, "modules": [
            { "name": "noise", "type": "noise" },
            { "name": "filter", "type": "filter" },
            { "name": "master", "type": "master" }
        ], "cables": [
            { "from": "noise", "output": "white", "to": "filter", "input": "cutoff cv" },
            { "from": "filter", "output": "lowpass", "to": "master", "input": "in" }
        ] }"#,
    )
    .unwrap();
    let (graph, out) = patch.build(&Context::new(48_000.0)).unwrap();
    let (mut engine, controller) = Engine::new(graph, out, 1024);
    let editor = Editor::new(controller, Context::new(48_000.0), &patch.positions);
    assert!(!editor.moves());
    for _ in 0..crate::engine::module::BLOCK {
        engine.next_frame();
    }
    assert!(editor.moves());
}

fn names(editor: &Editor) -> Vec<String> {
    editor
        .controller
        .nodes()
        .iter()
        .map(|n| n.name.clone())
        .collect()
}

/// The keys preset, arranged and drawn, so blocks have their sizes.
fn drawn_keys(ctx: &egui::Context) -> (Editor, Vec<egui::Shape>) {
    let mut editor = editor("keys");
    editor.arrange();
    paint(&mut editor, ctx, vec![]);
    let shapes = paint(&mut editor, ctx, vec![]);
    (editor, shapes)
}

#[test]
fn duplicating_copies_blocks_and_their_wires_just_below() {
    let ctx = egui::Context::default();
    let (mut editor, _) = drawn_keys(&ctx);
    let originals = [block_of(&editor, "filter"), block_of(&editor, "vca")];
    editor.duplicate(&originals);
    assert_eq!(editor.status, "duplicated 2 blocks");
    assert!(names(&editor).ends_with(&["filter 2".into(), "vca 2".into()]));
    let (filter, vca) = (module(&editor, "filter 2"), module(&editor, "vca 2"));
    let cables = editor.controller.cables();
    assert!(cables.contains(&Cable {
        from: filter,
        from_port: filter::OUT_LOWPASS,
        to: vca,
        to_port: vca::IN_SIGNAL,
    }));
    // The gate into the original VCA comes from outside: not copied.
    assert_eq!(cables.iter().filter(|c| c.to == vca).count(), 1);
    assert_eq!(wires(&editor), expected_wires(&editor));
    assert_eq!(editor.controller.node(filter).params, {
        let original = module(&editor, "filter");
        editor.controller.node(original).params.clone()
    });

    paint(&mut editor, &ctx, vec![]);
    let rects = block_rects(&editor);
    let rect = |name: &str| rects.iter().find(|(n, _)| n == name).unwrap().1;
    let bottom = rect("filter").bottom().max(rect("vca").bottom());
    for copy in ["filter 2", "vca 2"] {
        assert!(rect(copy).top() > bottom, "{copy} overlaps the originals");
    }
    assert_eq!(rect("filter 2").left(), rect("filter").left());
}

#[test]
fn the_master_is_not_duplicated() {
    let mut editor = editor("keys");
    let before = names(&editor);
    editor.duplicate(&[block_of(&editor, "master")]);
    assert_eq!(names(&editor), before);
    assert_eq!(editor.status, NOTHING_SELECTED);
}

fn shift_click(editor: &mut Editor, ctx: &egui::Context, pos: Pos2) {
    for pressed in [true, false] {
        let input = window(vec![
            egui::Event::ModifiersChanged(egui::Modifiers::SHIFT),
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::SHIFT,
            },
        ]);
        paint_input(editor, ctx, input);
    }
    let release = egui::Event::ModifiersChanged(egui::Modifiers::NONE);
    paint(editor, ctx, vec![release]);
}

fn ctrl(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    }
}

#[test]
fn dragging_a_title_moves_its_block() {
    let ctx = egui::Context::default();
    let (mut editor, shapes) = drawn_keys(&ctx);
    let block = block_of(&editor, "filter");
    let before = editor.snarl.get_node_info(block).unwrap().pos;
    let start = text_at(&shapes, "filter").center();
    let end = start + egui::vec2(60.0, 40.0);
    paint(&mut editor, &ctx, vec![egui::Event::PointerMoved(start)]);
    paint(
        &mut editor,
        &ctx,
        vec![click(start, egui::PointerButton::Primary, true)],
    );
    for step in 1..=4 {
        let pos = start + (end - start) * (step as f32 / 4.0);
        paint(&mut editor, &ctx, vec![egui::Event::PointerMoved(pos)]);
    }
    paint(
        &mut editor,
        &ctx,
        vec![click(end, egui::PointerButton::Primary, false)],
    );
    let moved = editor.snarl.get_node_info(block).unwrap().pos - before;
    assert!(moved.length() > 30.0, "moved by {moved:?}");
}

#[test]
fn shift_clicked_blocks_duplicate_with_ctrl_d() {
    let ctx = egui::Context::default();
    let (mut editor, shapes) = drawn_keys(&ctx);
    shift_click(&mut editor, &ctx, text_at(&shapes, "filter").center());
    shift_click(&mut editor, &ctx, text_at(&shapes, "vca").center());
    let selection = editor.selection(&ctx);
    assert_eq!(
        selection,
        [block_of(&editor, "filter"), block_of(&editor, "vca")]
    );
    paint(&mut editor, &ctx, vec![ctrl(egui::Key::D)]);
    assert!(names(&editor).ends_with(&["filter 2".into(), "vca 2".into()]));
}

#[test]
fn copied_blocks_paste_in_the_middle_of_the_view() {
    let ctx = egui::Context::default();
    let (mut editor, shapes) = drawn_keys(&ctx);
    shift_click(&mut editor, &ctx, text_at(&shapes, "osc").center());
    let output = ctx.run_ui(window(vec![egui::Event::Copy]), |ui| editor.show_graph(ui));
    let copied = output
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .expect("copied to the clipboard");
    assert_eq!(Patch::from_json(&copied).unwrap().modules.len(), 1);

    paint(&mut editor, &ctx, vec![egui::Event::Paste(copied)]);
    assert_eq!(editor.status, "added osc 2");
    let pos = editor
        .snarl
        .get_node_info(block_of(&editor, "osc 2"))
        .unwrap()
        .pos;
    let on_screen = editor.to_global * pos;
    assert!(
        (on_screen - editor.view.center()).length() < 1.0,
        "{on_screen:?} is not in the middle of {:?}",
        editor.view
    );

    paint(&mut editor, &ctx, vec![egui::Event::Paste("hello".into())]);
    assert_eq!(editor.status, "the clipboard holds no blocks");
}

#[test]
fn renders_without_a_window() {
    let mut editor = editor("chaos");
    let ctx = egui::Context::default();
    for _ in 0..2 {
        paint(&mut editor, &ctx, vec![]);
    }
    assert!(editor.status.is_empty(), "{}", editor.status);
}

fn editor_for(json: &str) -> Editor {
    let patch = Patch::from_json(json).unwrap();
    let (graph, out) = patch.build(&Context::new(48_000.0)).unwrap();
    let (_, controller) = Engine::new(graph, out, 1024);
    Editor::new(controller, Context::new(48_000.0), &patch.positions)
}

/// Moves the pointer to `pos` and clicks there, a frame each.
fn click_at(editor: &mut Editor, ctx: &egui::Context, pos: Pos2, button: egui::PointerButton) {
    paint(editor, ctx, vec![egui::Event::PointerMoved(pos)]);
    paint(editor, ctx, vec![click(pos, button, true)]);
    paint(editor, ctx, vec![click(pos, button, false)]);
}

#[test]
fn clicking_a_song_cell_picks_the_next_pattern() {
    use crate::engine::score::Score;
    use egui::PointerButton::{Primary, Secondary};

    let mut editor = editor_for(
        r#"{ "version": 2, "modules": [
            { "name": "song", "type": "song", "sections": [{ "name": "verse", "bars": 4 }] },
            { "name": "lead", "type": "notes" },
            { "name": "master", "type": "master" }
        ], "cables": [{ "from": "song", "output": "track 2", "to": "lead", "input": "play" }] }"#,
    );
    let ctx = egui::Context::default();
    paint(&mut editor, &ctx, vec![]);
    let shapes = paint(&mut editor, &ctx, vec![]);
    // Tracks are named after what they feed.
    let cell = egui::pos2(
        text_at(&shapes, "verse").center().x,
        text_at(&shapes, "2 lead").center().y,
    );
    let id = module(&editor, "song");
    let track_2 = |editor: &Editor| match &editor.controller.node(id).score {
        Some(Score::Arrangement(a)) => a.sections[0].tracks[1],
        _ => panic!("no arrangement"),
    };
    click_at(&mut editor, &ctx, cell, Primary);
    assert_eq!(track_2(&editor), Some(0));
    click_at(&mut editor, &ctx, cell, Primary);
    assert_eq!(track_2(&editor), Some(1));
    click_at(&mut editor, &ctx, cell, Secondary);
    assert_eq!(track_2(&editor), None);
}

#[test]
fn clicking_the_piano_roll_plays_a_note_there() {
    use super::score::STEP_WIDTH;
    use crate::engine::score::{Score, Step, parse_note};
    use egui::PointerButton::Primary;

    let mut editor = editor_for(
        r#"{ "version": 2, "modules": [
            { "name": "bass", "type": "notes", "key": "C major", "patterns": { "A": ". . . ." } },
            { "name": "master", "type": "master" }
        ] }"#,
    );
    let ctx = egui::Context::default();
    paint(&mut editor, &ctx, vec![]);
    let shapes = paint(&mut editor, &ctx, vec![]);
    // Note names end a little left of the grid; the second step.
    let label = text_at(&shapes, "E3");
    let scale = editor.to_global.scaling;
    let step_2 = egui::pos2(
        label.right() + (3.0 + 1.5 * STEP_WIDTH) * scale,
        label.center().y,
    );
    click_at(&mut editor, &ctx, step_2, Primary);
    let id = module(&editor, "bass");
    let Some(Score::Patterns(patterns)) = &editor.controller.node(id).score else {
        panic!("no patterns")
    };
    let e3 = parse_note("E3").unwrap();
    assert_eq!(
        patterns.patterns[0],
        [Step::Rest, Step::Note(e3), Step::Rest, Step::Rest]
    );
}
