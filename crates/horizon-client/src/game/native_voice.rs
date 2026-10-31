//! Game-thread integration for native voice services, independent of WASM.
use super::{Game, Screen};
use horizon_client::{lang, mod_list::NativePermissionButton};
use horizon_mods::native::{Declaration, PermissionKind, Session};
use horizon_protocol::packets::{
    configuration::CustomPayload,
    play::{Clientbound, Serverbound},
};
use std::collections::BTreeSet;

pub(super) fn uuid(value: u128) -> String {
    let h = format!("{value:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}
impl Game<'_> {
    pub(super) fn draw_native_microphone(&self, batch: &mut horizon_client::hud::HudBatch) {
        let capturing = self.native_voice.capturing();
        if !capturing.is_empty() {
            self.native_text(
                batch,
                &lang::tr("horizon.voice.capturing", &[&capturing.join(", ")]),
                [4.0, 4.0],
                0xff5555,
                true,
            );
        }
    }
    pub(super) fn native_voice_player_snapshot(&self) -> Vec<horizon_mods::native::Player> {
        if !self.options.execution_role.is_client() {
            return self
                .native_server_players()
                .into_iter()
                .map(|p| horizon_mods::native::Player {
                    id: uuid(p.uuid),
                    name: p.name,
                    position: Some(p.position),
                    dimension: Some(p.dimension),
                })
                .collect();
        }
        let mut players = self.native_remote_voice_players();
        if let (Some(remote), Some(player)) = (&self.remote, &self.player) {
            let id = uuid(remote.profile);
            players.retain(|p| p.id != id);
            players.push(horizon_mods::native::Player {
                id,
                name: remote.profile_name.clone(),
                position: Some([player.position.x, player.position.y, player.position.z]),
                dimension: Some(self.dim.id().to_owned()),
            });
        }
        players.sort_by(|a, b| a.id.cmp(&b.id));
        players
    }
    pub(super) fn refresh_native_voice(&mut self) {
        let mut declarations = Vec::<Declaration>::new();
        self.native_voice_mods(&mut declarations);
        let mut ids = BTreeSet::new();
        for declaration in declarations {
            if !self.native_on(&declaration.id) {
                continue;
            }
            let id = declaration.id.clone();
            if self.native_voice.register(declaration).is_ok() {
                ids.insert(id);
            }
        }
        self.native_voice.retain(&ids);
    }
    /// Synchronize engine permission snapshots without calling any mod settings hooks.
    /// Normal buttons and the mod's description are kept intact. Unchanged pages are not rebuilt.
    pub(super) fn refresh_native_voice_permissions(&mut self) {
        let pages: Vec<_> = self
            .settings
            .mods
            .installed
            .iter()
            .enumerate()
            .filter(|(_, m)| m.native.is_some())
            .map(|(i, m)| (i, self.native_permission_buttons(&m.id)))
            .collect();
        for (i, permissions) in pages {
            if let Some(page) = self.settings.mods.installed[i].native.as_mut()
                && page.permissions != permissions
            {
                page.permissions = permissions;
            }
        }
    }
    fn native_permission_buttons(&self, id: &str) -> Vec<NativePermissionButton> {
        let Some(d) = self.native_voice.declaration(id) else {
            return Vec::new();
        };
        let server = self
            .native_voice
            .session()
            .server
            .clone()
            .unwrap_or_else(|| lang::tr("horizon.voice.no_server", &[]));
        let tooltip = lang::tr(
            "horizon.voice.permission_summary",
            &[&d.title, &d.version, &d.author, &d.purpose, &server],
        );
        self.native_voice
            .permissions(id)
            .into_iter()
            .map(|p| {
                let (label, active) = match p.kind {
                    PermissionKind::Microphone { requested } => {
                        let key = if !requested {
                            "horizon.voice.microphone_not_requested"
                        } else if p.allowed {
                            "horizon.voice.microphone_allowed"
                        } else {
                            "horizon.voice.microphone_blocked"
                        };
                        (
                            lang::tr(key, &[]),
                            requested && self.native_voice.session().connected,
                        )
                    }
                    kind => {
                        let (kind_key, host, port) = match kind {
                            PermissionKind::Dns { host, port } => ("horizon.voice.dns", host, port),
                            PermissionKind::Udp { host, port, local } => (
                                if local {
                                    "horizon.voice.udp_local"
                                } else {
                                    "horizon.voice.udp_public"
                                },
                                host,
                                port,
                            ),
                            PermissionKind::Microphone { .. } => unreachable!(),
                        };
                        let action = lang::tr(
                            if p.allowed {
                                "horizon.voice.revoke"
                            } else {
                                "horizon.voice.allow"
                            },
                            &[],
                        );
                        (
                            lang::tr(
                                "horizon.voice.endpoint",
                                &[&lang::tr(kind_key, &[]), &host, &port, &action],
                            ),
                            self.native_voice.session().connected,
                        )
                    }
                };
                NativePermissionButton {
                    id: p.id,
                    label,
                    tooltip: tooltip.clone(),
                    active,
                }
            })
            .collect()
    }
    pub(super) fn sync_native_voice(&mut self) {
        self.refresh_native_voice();
        if self.native_voice.is_empty() {
            if self.screen == Screen::Options
                && matches!(
                    self.ui.options.page(),
                    horizon_client::options_ui::Page::NativeMod(_)
                )
            {
                self.refresh_native_voice_permissions();
            }
            self.flush_native_voice();
            return;
        }
        let server_side = !self.options.execution_role.is_client();
        let local_player = self
            .remote
            .as_ref()
            .filter(|_| self.player.is_some())
            .map(|r| uuid(r.profile));
        let players = self
            .native_voice_player_snapshot()
            .into_iter()
            .map(|p| p.id)
            .collect();
        let session = Session {
            connected: server_side || local_player.is_some(),
            connection: self.remote.as_ref().map_or(0, |r| r.session.connection_id),
            server: self.remote.as_ref().map(|r| r.session.address.clone()),
            local_player,
            server_side,
            focused: self.focused,
            players,
        };
        let position = self
            .player
            .as_ref()
            .map_or([0.0; 3], |p| [p.position.x, p.position.y, p.position.z]);
        self.native_voice
            .sync(session, position, self.yaw, self.settings.master_volume);
        if !self.focused && self.native_window() {
            self.native_voice.stop_capture();
        }
        if self.screen == Screen::Options
            && matches!(
                self.ui.options.page(),
                horizon_client::options_ui::Page::NativeMod(_)
            )
        {
            self.refresh_native_voice_permissions();
        }
        self.flush_native_voice();
    }
    pub(super) fn flush_native_voice(&mut self) {
        let mut messages = self.native_voice.announcements(BTreeSet::new());
        messages.extend(self.native_voice.drain_out());
        for message in messages {
            let payload = CustomPayload {
                channel: message.channel,
                data: message.data,
            };
            if let Some(player) = message.player {
                if !self.options.execution_role.is_client()
                    && let Some(host) = self.lan.as_mut()
                    && let Some(conn) = host
                        .hub
                        .guests
                        .iter()
                        .find(|g| g.placed && uuid(g.uuid) == player)
                        .map(|g| g.conn)
                {
                    host.hub.send_to(conn, &Clientbound::CustomPayload(payload));
                }
            } else if self.remote.is_some() {
                self.send(Serverbound::CustomPayload(payload));
            }
        }
    }
}

// Compile the actual mixin helpers against Game when checking the binary's tests.
#[cfg(test)]
#[allow(dead_code)]
mod example {
    mod list {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/native-voice-lab/list.rs"
        ));
    }
    mod services {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/native-voice-lab/services.rs"
        ));
    }
    mod key {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/native-voice-lab/key.rs"
        ));
    }
    mod frame {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/native-voice-lab/frame.rs"
        ));
    }
    mod command {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/native-voice-lab/command.rs"
        ));
    }
}
