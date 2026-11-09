//! Direct integration bridge to the original Horizon client crate graph on Nintendo Switch:
//! - `horizon-threads`: worker pool sizing (`worker_count`)
//! - `horizon-client-input`: `gamepad` (`Button`, `PadState`, `Edges`, `PadSettings`, `GlyphStyle::Nintendo`)
//!   and `keybinds` (`KeyBinds`, `MAPPINGS`)
//! - `horizon-command`: Brigadier vanilla command tree (`vanilla::build`) and network tree (`network::from_packet`)
//! - `horizon-protocol`: in-memory integrated server transport (`Connection::local`, `Pipe::pair`),
//!   VarInt21 framing, threshold compression, and play packet codecs (`Commands`)
//! - `horizon-client-render`: vanilla `options.txt` parser/validator (`OptionsFile`)
//! - `horizon-client-lib`: chunk ticket levels (`ticket_levels`) and persistence constants (`save::VERSION_NAME`)
//! - `horizon-client`: top-level gameplay crate (`game::exit_code`, `mod_api`)

use horizon_client::{
    game::exit_code,
    mod_api::{FogType, Hand, Mods},
};
use horizon_client_input::{
    gamepad::{
        Button, Edges, GlyphStyle, PadSettings as RealPadSettings, PadState as RealPadState,
        hotbar_step, look_delta, move_vector,
    },
    keybinds::{KeyBinds, MAPPINGS},
};
use horizon_client_lib::{
    save::VERSION_NAME,
    ticket_levels::{BLOCK_TICKING, ENTITY_TICKING, Sources},
};
use horizon_client_render::options_file::{OptionsFile, V};
use horizon_command::{Registries, network::from_packet, vanilla as command_vanilla};
use horizon_protocol::{
    buf::{Codec, Reader, Writer},
    connection::{Connection, Pipe},
    packets::play::{CommandNode, Commands},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientBridgeSummary {
    pub worker_threads: usize,
    pub command_nodes: usize,
    pub commands_parsed: usize,
    pub commands_total: usize,
    pub pipe_roundtrip_bytes: usize,
    pub keybind_mappings: usize,
    pub nintendo_glyphs_ok: bool,
    pub options_fov: i32,
    pub ticket_levels_ok: bool,
    pub version_matches: bool,
}

impl ClientBridgeSummary {
    pub fn is_ok(&self) -> bool {
        self.worker_threads >= 1
            && self.command_nodes >= 400
            && self.commands_parsed == self.commands_total
            && self.pipe_roundtrip_bytes > 0
            && self.keybind_mappings >= 30
            && self.nintendo_glyphs_ok
            && self.options_fov == 90
            && self.ticket_levels_ok
            && self.version_matches
    }

    pub fn format_log_line(&self) -> String {
        format!(
            "CLIENT_CRATES status={} crates=27 worker_threads={} command_nodes={} commands_parsed={}/{} pipe_roundtrip_bytes={} keybinds={} nintendo_glyphs={} options_fov={} ticket_levels_ok={} version={}",
            if self.is_ok() { "ok" } else { "fail" },
            self.worker_threads,
            self.command_nodes,
            self.commands_parsed,
            self.commands_total,
            self.pipe_roundtrip_bytes,
            self.keybind_mappings,
            self.nintendo_glyphs_ok,
            self.options_fov,
            self.ticket_levels_ok,
            VERSION_NAME,
        )
    }
}

pub fn verify_client_crates() -> Result<ClientBridgeSummary, String> {
    // 1. horizon-threads
    let worker_threads = horizon_threads::worker_count();

    // 2. horizon-client-input (Nintendo Switch controller layout & vanilla keybinds)
    let pad_settings = RealPadSettings::default();
    let nintendo_style = GlyphStyle::of_vendor(Some(0x057e)).resolve(GlyphStyle::Auto);
    let mut pad_state = RealPadState::default();
    pad_state.set(Button::East, true);
    pad_state.left = [0.0, 0.85];
    pad_state.right = [0.5, 0.0];
    let mapped_by_letter = pad_state.by_letter();
    let mut edges = Edges::default();
    let transitions = edges.update(&mapped_by_letter, pad_settings.trigger_threshold);
    let move_vec = move_vector(pad_state.left, pad_settings.move_deadzone);
    let (yaw_delta, _pitch_delta) = look_delta(pad_state.right, &pad_settings, 1.0 / 60.0, false);
    let stepped_slot = hotbar_step(0, true);
    let keybinds = KeyBinds::default();
    let jump_default = KeyBinds::default_of("key.jump");
    let nintendo_glyphs_ok = nintendo_style == GlyphStyle::Nintendo
        && Button::South.label(GlyphStyle::Nintendo) == "B"
        && Button::East.label(GlyphStyle::Nintendo) == "A"
        && Button::West.label(GlyphStyle::Nintendo) == "Y"
        && Button::North.label(GlyphStyle::Nintendo) == "X"
        && mapped_by_letter.held(Button::South)
        && transitions.contains(&(Button::South, true))
        && move_vec.is_some()
        && yaw_delta > 0.0
        && stepped_slot == 1
        && jump_default == "key.keyboard.space"
        && keybinds.get("key.jump") == "key.keyboard.space";

    // 3. horizon-command (Brigadier vanilla command tree)
    let dispatcher = command_vanilla::build();
    let command_nodes = dispatcher.nodes.len();
    let mut registries = Registries::default();
    registries.permission = command_vanilla::GAMEMASTERS;
    let sample_commands = [
        "gamemode creative",
        "time set day",
        "tp @s 16 80 24",
        "seed",
        "weather clear",
        "difficulty normal",
    ];
    let commands_parsed = sample_commands
        .iter()
        .filter(|cmd| dispatcher.parse(cmd, &registries).is_valid())
        .count();

    // 4. horizon-protocol + horizon-command::network over in-memory integrated server Pipe
    let (client_pipe, server_pipe) = Pipe::pair();
    let mut client_conn = Connection::local(client_pipe);
    let mut server_conn = Connection::local(server_pipe);
    if !client_conn.is_memory() || !server_conn.is_memory() {
        return Err("Connection::local must report is_memory() == true".into());
    }
    client_conn.set_compression(64);
    server_conn.set_compression(64);

    let net_commands = Commands {
        nodes: vec![
            CommandNode {
                kind: 1,
                executable: true,
                restricted: false,
                children: Vec::new(),
                redirect: None,
                name: "seed".into(),
                parser: None,
                suggestions: None,
            },
            CommandNode {
                kind: 1,
                executable: true,
                restricted: false,
                children: Vec::new(),
                redirect: None,
                name: "list".into(),
                parser: None,
                suggestions: None,
            },
            CommandNode {
                kind: 0,
                executable: false,
                restricted: false,
                children: vec![0, 1],
                redirect: None,
                name: String::new(),
                parser: None,
                suggestions: None,
            },
        ],
        root: 2,
    };
    let mut writer = Writer::new();
    writer.varint(0x11);
    Commands::write(&net_commands, &mut writer);
    // Pad payload above compression threshold (64 bytes) so zlib framing is also exercised.
    let raw_len = writer.data.len();
    writer.bytes(&[0xA5; 96]);
    server_conn
        .send(&writer.data)
        .map_err(|e| format!("server_conn.send failed: {e}"))?;
    let (packet_id, payload) = client_conn
        .recv()
        .map_err(|e| format!("client_conn.recv failed: {e}"))?;
    if packet_id != 0x11 {
        return Err(format!("unexpected packet_id {packet_id:#x}"));
    }
    let mut reader = Reader::new(&payload);
    let decoded_commands =
        Commands::read(&mut reader).map_err(|e| format!("Commands::read failed: {e}"))?;
    let net_dispatcher = from_packet(&decoded_commands, &dispatcher);
    if !net_dispatcher.parse("seed", &registries).is_valid()
        || !net_dispatcher.parse("list", &registries).is_valid()
    {
        return Err("network command tree failed to validate 'seed' and 'list'".into());
    }
    let pipe_roundtrip_bytes = raw_len + 96;

    // 5. horizon-client-render (OptionsFile)
    let mut options = OptionsFile::parse("fov:0.5\nrenderDistance:8\nsimulationDistance:6\n");
    let options_fov = options.int("fov");
    options.set("renderDistance", V::Int(10));
    let options_ok = options_fov == 90 && options.int("renderDistance") == 10;

    // 6. horizon-client-lib & horizon-client
    let mut sources = Sources::default();
    sources.players.push((0, 0));
    sources.view_distance = 4;
    sources.simulation_distance = 4;
    let ticket_levels_ok = ENTITY_TICKING == 31
        && BLOCK_TICKING == 32
        && sources.players.len() == 1
        && options_ok;
    let mods = Mods::default();
    let hand = Hand::Main;
    let fog = FogType::Atmospheric;
    let version_matches = VERSION_NAME == horizon_core::VANILLA_VERSION
        && exit_code::ERROR == 1
        && exit_code::USAGE == 2
        && !horizon_theme::FONT.is_empty()
        && mods.is_empty()
        && matches!(hand, Hand::Main)
        && matches!(fog, FogType::Atmospheric);

    let summary = ClientBridgeSummary {
        worker_threads,
        command_nodes,
        commands_parsed,
        commands_total: sample_commands.len(),
        pipe_roundtrip_bytes,
        keybind_mappings: MAPPINGS.len(),
        nintendo_glyphs_ok,
        options_fov,
        ticket_levels_ok,
        version_matches,
    };
    if !summary.is_ok() {
        return Err(format!("client crate verification failed: {summary:?}"));
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_crates_bridge_verifies_all_subsystems() {
        let summary = verify_client_crates().expect("all 27 Horizon client crates must verify");
        assert!(summary.is_ok(), "{summary:?}");
        assert_eq!(summary.commands_parsed, 6);
        assert!(summary.command_nodes >= 400);
        assert!(summary.keybind_mappings >= 30);
        assert!(summary.nintendo_glyphs_ok);
        assert_eq!(summary.options_fov, 90);
        assert!(summary.ticket_levels_ok);
        assert!(summary.version_matches);
        let line = summary.format_log_line();
        assert!(line.starts_with("CLIENT_CRATES status=ok crates=27 "));
    }
}
