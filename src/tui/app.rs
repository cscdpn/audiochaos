use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::engine::control::Controller;
use crate::engine::graph::Cable;

/// Knob travel per key press, in normalized units.
const FINE_STEP: f32 = 0.01;
const COARSE_STEP: f32 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Knobs,
    Patch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Outputs,
    Inputs,
}

/// A knob, an output or an input: the index of a node and of the item in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub node: usize,
    pub index: usize,
}

/// A cursor over a list of slots that stays in bounds.
pub struct Cursor {
    pub slots: Vec<Slot>,
    pub position: usize,
}

impl Cursor {
    fn new(slots: Vec<Slot>) -> Self {
        Self { slots, position: 0 }
    }

    fn up(&mut self) {
        self.position = self.position.saturating_sub(1);
    }

    fn down(&mut self) {
        self.position = (self.position + 1).min(self.slots.len().saturating_sub(1));
    }

    pub fn selected(&self) -> Slot {
        self.slots[self.position]
    }
}

pub struct App {
    pub controller: Controller,
    pub screen: Screen,
    pub column: Column,
    pub knobs: Cursor,
    pub outputs: Cursor,
    pub inputs: Cursor,
    pub status: String,
    pub quit: bool,
}

impl App {
    pub fn new(controller: Controller) -> Self {
        let slots = |count: &dyn Fn(usize) -> usize| -> Vec<Slot> {
            (0..controller.nodes().len())
                .flat_map(|node| (0..count(node)).map(move |index| Slot { node, index }))
                .collect()
        };
        let nodes = controller.nodes();
        let knobs = slots(&|n| nodes[n].info.params.len());
        let outputs = slots(&|n| nodes[n].info.outputs.len());
        let inputs = slots(&|n| nodes[n].info.num_input_ports());
        Self {
            screen: Screen::Knobs,
            column: Column::Outputs,
            knobs: Cursor::new(knobs),
            outputs: Cursor::new(outputs),
            inputs: Cursor::new(inputs),
            status: String::new(),
            quit: false,
            controller,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        let ctrl_c =
            key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            _ if ctrl_c => self.quit = true,
            KeyCode::Tab => {
                self.screen = match self.screen {
                    Screen::Knobs => Screen::Patch,
                    Screen::Patch => Screen::Knobs,
                };
            }
            _ => match self.screen {
                Screen::Knobs => self.handle_knobs_key(key),
                Screen::Patch => self.handle_patch_key(key),
            },
        }
    }

    fn handle_knobs_key(&mut self, key: KeyEvent) {
        let coarse = key.modifiers.contains(KeyModifiers::SHIFT);
        let step = if coarse { COARSE_STEP } else { FINE_STEP };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.knobs.up(),
            KeyCode::Down | KeyCode::Char('j') => self.knobs.down(),
            KeyCode::Left | KeyCode::Char('h') => self.turn_knob(-step),
            KeyCode::Right | KeyCode::Char('l') => self.turn_knob(step),
            KeyCode::Char('H') => self.turn_knob(-COARSE_STEP),
            KeyCode::Char('L') => self.turn_knob(COARSE_STEP),
            _ => {}
        }
    }

    fn handle_patch_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.column = Column::Outputs,
            KeyCode::Right | KeyCode::Char('l') => self.column = Column::Inputs,
            KeyCode::Up | KeyCode::Char('k') => self.column_cursor().up(),
            KeyCode::Down | KeyCode::Char('j') => self.column_cursor().down(),
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle_cable(),
            _ => {}
        }
    }

    fn column_cursor(&mut self) -> &mut Cursor {
        match self.column {
            Column::Outputs => &mut self.outputs,
            Column::Inputs => &mut self.inputs,
        }
    }

    fn turn_knob(&mut self, delta: f32) {
        let Slot { node, index } = self.knobs.selected();
        let state = &self.controller.nodes()[node];
        let info = &state.info.params[index];
        let value = info.from_normalized(info.to_normalized(state.params[index]) + delta);
        if let Err(e) = self.controller.set_param(state.id, index, value) {
            self.status = format!("knob not changed: {e}");
        }
    }

    /// The cable between the selected output and the selected input.
    pub fn selected_cable(&self) -> Cable {
        let (from, to) = (self.outputs.selected(), self.inputs.selected());
        let nodes = self.controller.nodes();
        Cable {
            from: nodes[from.node].id,
            from_port: from.index,
            to: nodes[to.node].id,
            to_port: to.index,
        }
    }

    fn toggle_cable(&mut self) {
        let cable = self.selected_cable();
        let label = format!(
            "{} → {}",
            self.output_label(self.outputs.selected()),
            self.input_label(self.inputs.selected())
        );
        self.status = match self.controller.toggle_cable(cable) {
            Ok(true) => format!("connected {label}"),
            Ok(false) => format!("disconnected {label}"),
            Err(e) => format!("cannot connect {label}: {e}"),
        };
    }

    pub fn output_label(&self, slot: Slot) -> String {
        let node = &self.controller.nodes()[slot.node];
        format!("{} › {}", node.name, node.info.outputs[slot.index])
    }

    pub fn input_label(&self, slot: Slot) -> String {
        let node = &self.controller.nodes()[slot.node];
        format!("{} › {}", node.name, node.info.input_port_name(slot.index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::control::Engine;
    use crate::rack;

    fn app() -> App {
        let (graph, out) = rack::build(48_000.0, rack::Preset::Bass).unwrap();
        // The engine is dropped: commands pile up in the queue, which is
        // large enough for these tests.
        let (_, controller) = Engine::new(graph, out, 256);
        App::new(controller)
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn selected_knob_value(app: &App) -> f32 {
        let Slot { node, index } = app.knobs.selected();
        app.controller.nodes()[node].params[index]
    }

    #[test]
    fn lists_every_knob_output_and_input_of_the_rack() {
        let app = app();
        let nodes = app.controller.nodes();
        let total = |f: &dyn Fn(usize) -> usize| (0..nodes.len()).map(f).sum::<usize>();
        assert_eq!(
            app.knobs.slots.len(),
            total(&|n| nodes[n].info.params.len())
        );
        assert_eq!(
            app.outputs.slots.len(),
            total(&|n| nodes[n].info.outputs.len())
        );
        assert_eq!(
            app.inputs.slots.len(),
            total(&|n| nodes[n].info.num_input_ports())
        );
    }

    #[test]
    fn cursor_stays_within_the_list() {
        let mut app = app();
        press(&mut app, KeyCode::Up);
        assert_eq!(app.knobs.position, 0);
        for _ in 0..1000 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(app.knobs.position, app.knobs.slots.len() - 1);
    }

    #[test]
    fn arrows_turn_the_selected_knob() {
        let mut app = app();
        let before = selected_knob_value(&app);
        press(&mut app, KeyCode::Right);
        let fine = selected_knob_value(&app);
        assert!(fine > before);
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
        let coarse = selected_knob_value(&app);
        assert!(coarse - fine > fine - before);
        press(&mut app, KeyCode::Left);
        assert!(selected_knob_value(&app) < coarse);
    }

    #[test]
    fn knobs_stop_at_the_end_of_their_range() {
        let mut app = app();
        for _ in 0..50 {
            press(&mut app, KeyCode::Char('L'));
        }
        let Slot { node, index } = app.knobs.selected();
        let max = app.controller.nodes()[node].info.params[index].max;
        assert!((selected_knob_value(&app) - max).abs() < 1e-3);
    }

    #[test]
    fn enter_toggles_the_cable_between_the_selected_ports() {
        let mut app = app();
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.screen, Screen::Patch);
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Down);
        let cable = app.selected_cable();
        assert!(!app.controller.cables().contains(&cable));

        press(&mut app, KeyCode::Enter);
        assert!(app.controller.cables().contains(&cable));
        assert!(app.status.starts_with("connected"));

        press(&mut app, KeyCode::Enter);
        assert!(!app.controller.cables().contains(&cable));
        assert!(app.status.starts_with("disconnected"));
    }

    #[test]
    fn up_and_down_move_only_the_focused_column() {
        let mut app = app();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Down);
        assert_eq!((app.outputs.position, app.inputs.position), (1, 0));
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Down);
        assert_eq!((app.outputs.position, app.inputs.position), (1, 1));
    }

    #[test]
    fn q_quits_and_key_releases_are_ignored() {
        let mut app = app();
        let mut release = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        app.handle_key(release);
        assert!(!app.quit);
        press(&mut app, KeyCode::Char('q'));
        assert!(app.quit);
    }
}
