use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

use super::app::{App, Column, Screen, Slot};
use crate::engine::graph::Cable;

const BAR_WIDTH: usize = 24;

pub fn draw(frame: &mut Frame, app: &App, audio_errors: usize) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(2),
    ])
    .areas(frame.area());

    draw_header(frame, header, app.screen, audio_errors);
    match app.screen {
        Screen::Knobs => draw_knobs(frame, body, app),
        Screen::Patch => draw_patch(frame, body, app),
    }
    draw_footer(frame, footer, app);
}

fn draw_header(frame: &mut Frame, area: Rect, screen: Screen, audio_errors: usize) {
    let tab = |name: &'static str, active: bool| {
        if active {
            Span::from(format!(" {name} ")).reversed()
        } else {
            Span::from(format!(" {name} "))
        }
    };
    let line = Line::from(vec![
        " audiochaos ".bold(),
        tab("Knobs", screen == Screen::Knobs),
        tab("Patch", screen == Screen::Patch),
        format!("   audio errors: {audio_errors}").dim(),
    ]);
    frame.render_widget(line, area);
}

fn draw_knobs(frame: &mut Frame, area: Rect, app: &App) {
    let nodes = app.controller.nodes();
    let mut items = Vec::new();
    let mut selected_row = 0;
    for (n, node) in nodes.iter().enumerate() {
        if node.info.params.is_empty() {
            continue;
        }
        items.push(ListItem::new(Line::from(node.name.clone().bold())));
        for (p, info) in node.info.params.iter().enumerate() {
            if app.knobs.selected() == (Slot { node: n, index: p }) {
                selected_row = items.len();
            }
            let value = node.params[p];
            items.push(ListItem::new(format!(
                "  {:<10} {} {}",
                info.name,
                bar(info.to_normalized(value)),
                info.format(value)
            )));
        }
    }
    let list = List::new(items)
        .block(Block::bordered().title(" Knobs "))
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(Some(selected_row));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_patch(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(area);
    let cables = app.controller.cables();
    let nodes = app.controller.nodes();
    let source = app.outputs.selected();
    let source_id = nodes[source.node].id;

    let outputs: Vec<ListItem> = app
        .outputs
        .slots
        .iter()
        .map(|&slot| {
            let id = nodes[slot.node].id;
            let used = cables
                .iter()
                .any(|c| c.from == id && c.from_port == slot.index);
            ListItem::new(format!("{} {}", marker(used), app.output_label(slot)))
        })
        .collect();

    let inputs: Vec<ListItem> = app
        .inputs
        .slots
        .iter()
        .map(|&slot| {
            let id = nodes[slot.node].id;
            let into_this = |c: &Cable| c.to == id && c.to_port == slot.index;
            let from_source = |c: &Cable| c.from == source_id && c.from_port == source.index;
            let symbol = if cables.iter().any(|c| into_this(c) && from_source(c)) {
                "●"
            } else if cables.iter().any(into_this) {
                "○"
            } else {
                " "
            };
            ListItem::new(format!("{symbol} {}", app.input_label(slot)))
        })
        .collect();

    draw_column(
        frame,
        left,
        outputs,
        " Outputs ",
        app.outputs.position,
        app.column == Column::Outputs,
    );
    draw_column(
        frame,
        right,
        inputs,
        " Inputs  (● from selected output, ○ from elsewhere) ",
        app.inputs.position,
        app.column == Column::Inputs,
    );
}

fn draw_column(
    frame: &mut Frame,
    area: Rect,
    items: Vec<ListItem>,
    title: &str,
    selected: usize,
    focused: bool,
) {
    let block = Block::bordered().title(title.to_string());
    let (block, highlight) = if focused {
        (block.bold(), Style::new().add_modifier(Modifier::REVERSED))
    } else {
        (block.dim(), Style::new().add_modifier(Modifier::UNDERLINED))
    };
    let list = List::new(items).block(block).highlight_style(highlight);
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let help = match app.screen {
        Screen::Knobs => "↑↓ select   ←→ turn   shift+←→ or H/L turn faster   Tab patch   q quit",
        Screen::Patch => {
            "←→ outputs/inputs   ↑↓ select   Enter connect/disconnect   Tab knobs   q quit"
        }
    };
    let text = vec![Line::from(app.status.clone()), Line::from(help).dim()];
    frame.render_widget(Paragraph::new(text), area);
}

fn marker(on: bool) -> &'static str {
    if on { "●" } else { " " }
}

fn bar(normalized: f32) -> String {
    let filled = (normalized * BAR_WIDTH as f32).round() as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(BAR_WIDTH - filled))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::control::Engine;
    use crate::rack;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn bar_fills_with_the_knob_position() {
        assert_eq!(bar(0.0).chars().filter(|&c| c == '█').count(), 0);
        assert_eq!(
            bar(0.5).chars().filter(|&c| c == '█').count(),
            BAR_WIDTH / 2
        );
        assert_eq!(bar(1.0).chars().filter(|&c| c == '█').count(), BAR_WIDTH);
    }

    fn render(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| draw(frame, app, 3)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn both_screens_render_the_rack() {
        let (graph, out) = rack::build(48_000.0, &rack::preset("bass").unwrap()).unwrap();
        let (_, controller) = Engine::new(graph, out, 16);
        let mut app = App::new(controller);

        let knobs = render(&app);
        assert!(knobs.contains("osc 1"));
        assert!(knobs.contains("cutoff"));
        assert!(knobs.contains("audio errors: 3"));

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let patch = render(&app);
        assert!(patch.contains("osc 1 › saw"));
        assert!(patch.contains("filter › cutoff cv"));
    }
}
