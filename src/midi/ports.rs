//! The computer's MIDI ports, for the standalone program; the plugin gets
//! its MIDI from the host.

use std::error::Error;
use std::io;
use std::sync::Arc;

use midir::{MidiInput, MidiInputConnection, MidiInputPort};

use super::{BEND_CENTRE, Parser, State};

/// Open MIDI connections. Input stops when this is dropped.
pub struct Inputs {
    _connections: Vec<MidiInputConnection<()>>,
    pub names: Vec<String>,
}

/// Listens to every MIDI input port, so any plugged-in controller plays.
/// Controllers plugged in later are only picked up by connecting again.
pub fn connect(state: &Arc<State>) -> Result<Inputs, Box<dyn Error>> {
    let mut connections = Vec::new();
    let mut names = Vec::new();
    for (port, name) in ports()? {
        let state = Arc::clone(state);
        let mut parser = Parser::default();
        let input = MidiInput::new("audiochaos")?;
        let connection = input
            .connect(
                &port,
                "audiochaos",
                move |_, message, _| parser.handle(message, &state),
                (),
            )
            .map_err(|e| format!("cannot open {name}: {e}"))?;
        connections.push(connection);
        names.push(name);
    }
    Ok(Inputs {
        _connections: connections,
        names,
    })
}

/// Prints every message from every MIDI input until Enter is pressed, to
/// check what a controller sends.
pub fn monitor() -> Result<(), Box<dyn Error>> {
    let mut connections = Vec::new();
    for (port, name) in ports()? {
        println!("listening to {name}");
        let input = MidiInput::new("audiochaos monitor")?;
        let label = name.clone();
        let connection = input
            .connect(
                &port,
                "audiochaos monitor",
                move |_, message, _| println!("{label}: {}", describe(message)),
                (),
            )
            .map_err(|e| format!("cannot open {name}: {e}"))?;
        connections.push(connection);
    }
    if connections.is_empty() {
        return Err("no MIDI controller found".into());
    }
    println!("play something; press Enter to stop");
    io::stdin().read_line(&mut String::new())?;
    Ok(())
}

/// Every input port but software loopbacks, which only echo other programs.
fn ports() -> Result<Vec<(MidiInputPort, String)>, Box<dyn Error>> {
    let input = MidiInput::new("audiochaos")?;
    let mut ports = Vec::new();
    for port in input.ports() {
        let name = input.port_name(&port)?;
        if !name.contains("Through") {
            ports.push((port, name));
        }
    }
    Ok(ports)
}

/// One message in words, channels counted from 1 as on the hardware.
pub fn describe(message: &[u8]) -> String {
    let &[status, data1, data2] = message else {
        return format!("{message:02x?}");
    };
    let channel = (status & 0x0f) + 1;
    match (status & 0xf0, data2) {
        (0x90, velocity) if velocity > 0 => {
            format!("channel {channel} note {data1} on, velocity {velocity}")
        }
        (0x80, _) | (0x90, _) => format!("channel {channel} note {data1} off"),
        (0xb0, value) => format!("channel {channel} controller {data1} = {value}"),
        (0xe0, msb) => format!(
            "channel {channel} pitch bend {}",
            (u16::from(msb) << 7 | u16::from(data1)) as i32 - i32::from(BEND_CENTRE)
        ),
        _ => format!("{message:02x?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_messages_for_the_monitor() {
        assert_eq!(
            describe(&[0x90, 60, 100]),
            "channel 1 note 60 on, velocity 100"
        );
        assert_eq!(describe(&[0x89, 36, 0]), "channel 10 note 36 off");
        assert_eq!(describe(&[0xb0, 3, 42]), "channel 1 controller 3 = 42");
        assert_eq!(describe(&[0xe0, 0, 0x40]), "channel 1 pitch bend 0");
        assert_eq!(describe(&[0xf8]), "[f8]");
    }
}
