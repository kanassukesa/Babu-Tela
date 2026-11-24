//! The dialogs (`MODALS` in the old page) and the small floating menus
//! (`pop`): the server menu and the emoji pickers.

use egui::{
    Align, Area, Color32, ComboBox, CornerRadius, CursorIcon, Frame, Id, Key, Layout, Margin,
    Order, Rect, ScrollArea, Sense, Stroke, TextEdit, Ui, pos2, vec2,
};
use serde_json::json;

use std::time::{Duration, Instant};

use super::Acts;
use super::widgets::{self, Button};
use crate::core::{
    Action, Core, ErrTo, FormId, Modal, PopKind, ShareTab,
    StickerTab, Then,
};
use crate::giphy::{self, FeedView, Frames, Lookup};
use crate::icons::Icon;
use crate::markdown;
use crate::model::*;
use crate::net::{self, Req};
use crate::theme::{self, *};
use crate::voice::QUALITIES;
use crate::{t, tf};

const WIDTH: f32 = 460.0 - 44.0;
const WIDE: f32 = 720.0 - 44.0;

fn is_wide(m: &Modal) -> bool {
    matches!(
        m,
        Modal::EditChannel { .. }
            | Modal::Reports { .. }
            | Modal::Share { .. }
            | Modal::Player { .. }
    )
}

pub fn draw(ctx: &egui::Context, core: &mut Core, acts: &mut Acts) {
    let Some(mut m) = core.modal.take() else {
        return;
    };
    // Configurações has a page of its own, nearly the whole window.
    if let Modal::Settings { display_name, tab } = &mut m {
        super::settings::page(ctx, core, display_name, tab, acts);
        if core.modal.is_none() {
            core.modal = Some(m);
        }
        return;
    }
    // A server's settings too.
    if let Modal::ServerSettings { .. } = &m {
        super::server_settings::draw(ctx, core, &mut m, acts);
        if core.modal.is_none() {
            core.modal = Some(m);
        }
        return;
    }
    let focus = std::mem::take(&mut core.modal_focus);
    let width = if is_wide(&m) { WIDE } else { WIDTH };
    let max_h = (ctx.content_rect().height() * 0.88 - 44.0).max(200.0);
    let frame = Frame::new()
        .fill(BG2())
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(22))
        .shadow(egui::epaint::Shadow {
            offset: [0, 20],
            blur: 60,
            spread: 0,
            color: Color32::from_black_alpha(0xaa),
        });
    let resp = egui::Modal::new(Id::unique("modal"))
        .frame(frame)
        .backdrop_color(Color32::from_black_alpha(0xaa))
        .show(ctx, |ui| {
            ui.set_width(width);
            // An area only offers the size it had last frame (400 px at
            // first), and a scroll area never grows past what it is offered;
            // asking for the full height here lets a long dialog use it.
            ui.allocate_ui(vec2(width, max_h), |ui| {
                let mut area = ScrollArea::vertical()
                    .max_height(max_h)
                    .auto_shrink([false, true]);
                if let Some(y) = core.modal_scroll.take() {
                    area = area.vertical_scroll_offset(y);
                }
                area.show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    body(ui, core, &mut m, focus, acts);
                });
            });
        });
    if resp.should_close() {
        acts.later(Core::close_modal);
    }
    // Put it back unless the frame asked for something else.
    if core.modal.is_none() {
        core.modal = Some(m);
    }
}

fn body(ui: &mut Ui, core: &mut Core, m: &mut Modal, focus: bool, acts: &mut Acts) {
    match m {
        Modal::AddServer { name, code } => add_server(ui, core, name, code, focus, acts),
        Modal::NewChannel {
            sid,
            kind,
            name,
            category,
            quick,
        } => {
            if *quick {
                quick_channel(ui, core, *sid, kind, name, category, focus, acts);
            } else {
                new_channel(ui, core, *sid, kind, name, category, focus, acts);
            }
        }
        Modal::EditChannel {
            cid,
            name,
            topic,
            category,
        } => edit_channel(ui, core, *cid, name, topic, category, focus, acts),
        Modal::Invite { code } => invite(ui, core, code, acts),
        Modal::Ttl { cid } => ttl(ui, core, *cid, acts),
        // Drawn by `server_settings::draw`.
        Modal::ServerSettings { .. } => {}
        Modal::Notify { sid } => super::server_settings::notify_dialog(ui, core, *sid, acts),
        Modal::Nick { sid, nick } => {
            super::server_settings::nick_dialog(ui, core, *sid, nick, acts)
        }
        Modal::Profile { uid } => profile(ui, core, *uid, acts),
        // Drawn by `settings_page`.
        Modal::Settings { .. } => {}
        Modal::Report {
            mid,
            reason,
            detail,
        } => report(ui, core, *mid, reason, detail, acts),
        Modal::Reports { sid } => reports(ui, core, *sid, acts, false),
        Modal::Requests { sid } => requests(ui, core, *sid, acts, false),
        Modal::Share {
            tab,
            items,
            incoming,
        } => share(ui, core, tab, items, incoming, acts),
        Modal::Music { link } => super::music::modal(ui, core, link, focus, acts),
        Modal::Player { attachment } => {
            super::player::enlarged(ui, core, attachment, acts);
            foot(ui, |ui| {
                if Button::ghost(t!("common.close")).show(ui).clicked() {
                    acts.later(Core::close_modal);
                }
            });
        }
        Modal::Licenses => licenses(ui, acts),
        Modal::ApproveDevice { code, error, .. } => {
            super::security::approve_device(ui, code, error.as_deref(), focus, acts);
        }
        Modal::NewDevice => super::security::new_device(ui, core, acts),
        Modal::Security { uid } => super::security::number(ui, core, *uid, acts),
        Modal::Confirm {
            title,
            text,
            label,
            action,
        } => {
            widgets::h3(ui, title);
            widgets::sub(ui, text);
            ui.add_space(12.0);
            let action = *action;
            foot(ui, |ui| {
                if Button::danger(label).show(ui).clicked() {
                    acts.later(move |c| c.confirmed(action));
                }
                if Button::ghost(t!("common.cancel")).show(ui).clicked() {
                    acts.later(Core::close_modal);
                }
            });
        }
    }
}

/// The works of others the app carries (the fonts, the noise suppression's
/// network) or downloads (the emoji lists), with what their licences ask:
/// who made them, under which licence, and the full text of the font
/// licence, which has to go with every copy of the font.
fn licenses(ui: &mut Ui, acts: &mut Acts) {
    widgets::h3(ui, t!("modal.licenses_title"));
    widgets::sub(ui, t!("modal.licenses_intro"));
    let items = [
        ("Noto Color Emoji", t!("modal.license_noto")),
        ("Twemoji Country Flags", t!("modal.license_twemoji")),
        (
            t!("modal.license_emoji_list_name"),
            t!("modal.license_emoji_list"),
        ),
        ("DPDFNet", t!("modal.license_dpdfnet")),
        ("BetterOffice", t!("modal.license_betteroffice")),
        (
            t!("modal.license_libraries_name"),
            t!("modal.license_libraries"),
        ),
    ];
    for (name, text) in items {
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(name)
                .font(theme::bold(14.0))
                .color(TEXT()),
        );
        ui.label(egui::RichText::new(text).color(MUTED()));
    }
    ui.add_space(14.0);
    ui.label(
        egui::RichText::new("SIL Open Font License 1.1")
            .font(theme::bold(14.0))
            .color(TEXT()),
    );
    ui.label(
        egui::RichText::new(crate::emoji_font::NOTO_LICENSE)
            .font(theme::tiny())
            .color(FAINT()),
    );
    ui.add_space(14.0);
    ui.label(
        egui::RichText::new(t!("modal.license_unicode_heading"))
            .font(theme::bold(14.0))
            .color(TEXT()),
    );
    ui.label(
        egui::RichText::new(crate::emoji::UNICODE_LICENSE)
            .font(theme::tiny())
            .color(FAINT()),
    );
    foot(ui, |ui| {
        if Button::ghost(t!("common.close")).show(ui).clicked() {
            acts.later(Core::close_modal);
        }
    });
}

/// The buttons row at the bottom of a dialog, aligned right (`.foot`).
/// Add the buttons right to left.
pub(super) fn foot(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(18.0);
    // One row high: a right-to-left layout on its own would take all the
    // height left and centre the buttons in it, stretching the dialog.
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 34.0),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            add(ui);
        },
    );
}

pub(super) fn submitted(ui: &Ui, resp: &egui::Response) -> bool {
    resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
}

pub(super) fn select(ui: &mut Ui, id: &'static str, label: &str, value: &mut usize, options: &[&str]) -> bool {
    if !label.is_empty() {
        ui.label(
            egui::RichText::new(label.to_uppercase())
                .font(theme::bold(12.0))
                .color(MUTED())
                .extra_letter_spacing(0.5),
        );
        ui.add_space(2.0);
    }
    let before = *value;
    ComboBox::from_id_salt(id)
        .width(ui.available_width())
        .selected_text(options.get(*value).copied().unwrap_or_default())
        .show_ui(ui, |ui| {
            for (i, o) in options.iter().enumerate() {
                ui.selectable_value(value, i, *o);
            }
        });
    ui.add_space(10.0);
    *value != before
}

/// A microphone or output to pick: Windows' default, or one of `list` (None
/// while it is still being asked). Returns the new choice when one was
/// picked (None inside: the default).
pub(super) fn device_select(
    ui: &mut Ui,
    id: &'static str,
    label: &str,
    saved: Option<&crate::devices::Device>,
    list: Option<&[crate::devices::Device]>,
) -> Option<Option<crate::devices::Device>> {
    let list = list.unwrap_or_default();
    let mut names: Vec<String> = std::iter::once(t!("device.windows_default").to_owned())
        .chain(list.iter().map(|d| d.name.clone()))
        .collect();
    let mut value = match saved {
        None => 0,
        Some(s) => match list.iter().position(|d| d.id == s.id) {
            Some(i) => i + 1,
            // Not there (unplugged), or not listed yet.
            None => {
                names.push(tf!("device.disconnected", name = s.name));
                names.len() - 1
            }
        },
    };
    let options: Vec<&str> = names.iter().map(String::as_str).collect();
    if !select(ui, id, label, &mut value, &options) {
        return None;
    }
    match value {
        0 => Some(None),
        i if i <= list.len() => Some(Some(list[i - 1].clone())),
        // The one that is not there: nothing changes.
        _ => None,
    }
}

// ---------------------------------------------------------------- servers

fn add_server(
    ui: &mut Ui,
    core: &Core,
    name: &mut String,
    code: &mut String,
    focus: bool,
    acts: &mut Acts,
) {
    widgets::h3(ui, t!("modal.create_server_title"));
    widgets::sub(ui, t!("modal.create_server_sub"));
    ui.add_space(12.0);
    let r = widgets::field(ui, t!("modal.server_name"), name, 100, false);
    if focus {
        r.request_focus();
    }
    widgets::error_line(ui, core.errors.get(&FormId::CreateServer));
    let busy = core.busy.contains(&FormId::CreateServer);
    if (Button::primary(t!("modal.create_server_btn"))
        .wide()
        .enabled(!busy)
        .show(ui)
        .clicked()
        || submitted(ui, &r))
        && !busy
    {
        let name = name.trim().to_owned();
        acts.later(move |c| {
            c.call(
                Req::post("/servers").with(json!({ "name": name })),
                Then::Arrive,
                ErrTo::Form(FormId::CreateServer),
            )
        });
    }
    widgets::divider(ui);
    widgets::h3(ui, t!("modal.join_server_title"));
    widgets::sub(ui, t!("modal.join_server_sub"));
    ui.add_space(12.0);
    let r = widgets::field(ui, t!("modal.invite_code_label"), code, 200, false);
    widgets::error_line(ui, core.errors.get(&FormId::JoinServer));
    let busy = core.busy.contains(&FormId::JoinServer);
    if (Button::ghost(t!("modal.join_btn"))
        .wide()
        .enabled(!busy)
        .show(ui)
        .clicked()
        || submitted(ui, &r))
        && !busy
    {
        // The code goes to the server; the link's secret stays here, to
        // sign my entry with (e2e/members.rs).
        let (code, secret) = crate::e2e::members::parse_link(code);
        let code = net::escape(&code);
        acts.later(move |c| {
            c.joining = secret;
            c.call(
                Req::post(format!("/invites/{code}/join")),
                Then::Arrive,
                ErrTo::Form(FormId::JoinServer),
            )
        });
    }
    // Private servers I asked to join.
    if !core.join_requests.is_empty() {
        widgets::divider(ui);
        widgets::h3(ui, t!("modal.sent_requests_title"));
        widgets::sub(ui, t!("modal.sent_requests_sub"));
        ui.add_space(8.0);
        for r in &core.join_requests {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let (rect, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                widgets::paint_server(
                    ui,
                    core,
                    r.server.icon.as_deref(),
                    &r.server.initials(),
                    rect,
                    12,
                    theme::color_or(&r.server.color, ACCENT()),
                    ACCENT_INK(),
                );
                ui.vertical(|ui| {
                    // Room left for the button.
                    ui.set_max_width((ui.available_width() - 140.0).max(80.0));
                    ui.spacing_mut().item_spacing.y = 0.0;
                    widgets::ellipsis(ui, &r.server.name, theme::bold(14.0), TEXT());
                    ui.label(
                        egui::RichText::new(tf!(
                            "modal.waiting_since",
                            time = super::chat::fmt_time(r.created)
                        ))
                        .size(13.0)
                        .color(MUTED()),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if Button::ghost(t!("modal.cancel_request_btn"))
                        .small()
                        .show(ui)
                        .clicked()
                    {
                        let sid = r.server.id;
                        acts.later(move |c| c.cancel_request(sid));
                    }
                });
            });
            ui.add_space(6.0);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn new_channel(
    ui: &mut Ui,
    core: &Core,
    sid: i64,
    kind: &mut ChannelKind,
    name: &mut String,
    category: &mut String,
    focus: bool,
    acts: &mut Acts,
) {
    widgets::h3(ui, t!("modal.create_channel"));
    let server = core.server(sid).map(|s| s.name.clone()).unwrap_or_default();
    widgets::sub(ui, &tf!("modal.in_server", server = server));
    ui.add_space(12.0);
    let mut k = usize::from(*kind == ChannelKind::Voice);
    if select(
        ui,
        "new-channel-kind",
        t!("modal.kind_label"),
        &mut k,
        &[t!("modal.kind_text"), t!("modal.kind_voice")],
    ) {
        *kind = if k == 1 {
            ChannelKind::Voice
        } else {
            ChannelKind::Text
        };
    }
    let r = widgets::field(ui, t!("modal.channel_name"), name, 100, false);
    if focus {
        r.request_focus();
    }
    widgets::field(ui, t!("modal.category"), category, 100, false);
    widgets::error_line(ui, core.errors.get(&FormId::CreateChannel));
    let busy = core.busy.contains(&FormId::CreateChannel);
    let mut go = submitted(ui, &r);
    foot(ui, |ui| {
        go |= Button::primary(t!("modal.create_channel"))
            .enabled(!busy)
            .show(ui)
            .clicked();
        if Button::ghost(t!("common.cancel")).show(ui).clicked() {
            acts.later(Core::close_modal);
        }
    });
    if go && !busy {
        create_channel(acts, sid, *kind, name, category);
    }
}

/// The "+" of a category: a channel in it, of the type it has, asking
/// only the name (and the type when the category has both kinds).
#[allow(clippy::too_many_arguments)]
fn quick_channel(
    ui: &mut Ui,
    core: &Core,
    sid: i64,
    kind: &mut ChannelKind,
    name: &mut String,
    category: &str,
    focus: bool,
    acts: &mut Acts,
) {
    let Some(s) = core.server(sid) else { return };
    let mixed = category_mixes_kinds(s, category);
    widgets::h3(ui, quick_title(mixed, *kind));
    widgets::sub(
        ui,
        &tf!("modal.in_category", category = category, server = s.name),
    );
    ui.add_space(12.0);
    if mixed {
        let mut k = usize::from(*kind == ChannelKind::Voice);
        if select(
            ui,
            "quick-channel-kind",
            t!("modal.kind_label"),
            &mut k,
            &[t!("modal.kind_text"), t!("modal.kind_voice")],
        ) {
            *kind = if k == 1 {
                ChannelKind::Voice
            } else {
                ChannelKind::Text
            };
        }
    }
    let r = widgets::field(ui, t!("modal.channel_name"), name, 100, false);
    if focus {
        r.request_focus();
    }
    widgets::error_line(ui, core.errors.get(&FormId::CreateChannel));
    let busy = core.busy.contains(&FormId::CreateChannel);
    let mut go = submitted(ui, &r);
    foot(ui, |ui| {
        go |= Button::primary(t!("modal.create_btn"))
            .enabled(!busy)
            .show(ui)
            .clicked();
        if Button::ghost(t!("common.cancel")).show(ui).clicked() {
            acts.later(Core::close_modal);
        }
    });
    if go && !busy {
        create_channel(acts, sid, *kind, name, category);
    }
}

/// Whether a category of `s` has both text and voice channels.
pub(super) fn category_mixes_kinds(s: &Server, category: &str) -> bool {
    let mut kinds = s
        .channels
        .iter()
        .filter(|c| c.category_name() == category)
        .map(|c| c.kind);
    let first = kinds.next();
    kinds.any(|k| Some(k) != first)
}

/// What the "+" of a category makes (its title and its tip).
pub(super) fn quick_title(mixed: bool, kind: ChannelKind) -> &'static str {
    match (mixed, kind) {
        (true, _) => t!("modal.quick_channel_category"),
        (false, ChannelKind::Voice) => t!("modal.quick_channel_voice"),
        (false, _) => t!("modal.quick_channel_text"),
    }
}

fn create_channel(acts: &mut Acts, sid: i64, kind: ChannelKind, name: &str, category: &str) {
    let body = json!({ "type": kind.wire(), "name": name.trim(), "category": category.trim() });
    acts.later(move |c| {
        c.call(
            Req::post(format!("/servers/{sid}/channels")).with(body),
            Then::CloseModal,
            ErrTo::Form(FormId::CreateChannel),
        )
    });
}

#[allow(clippy::too_many_arguments)]
fn edit_channel(
    ui: &mut Ui,
    core: &Core,
    cid: i64,
    name: &mut String,
    topic: &mut String,
    category: &mut String,
    focus: bool,
    acts: &mut Acts,
) {
    let Some(s) = core.current_server() else {
        return;
    };
    let Some(c) = s.channel(cid) else { return };
    let voice = c.is_voice();
    widgets::h3(ui, t!("modal.edit_channel_title"));
    widgets::sub(
        ui,
        if voice {
            t!("modal.voice_channel")
        } else {
            t!("modal.text_channel")
        },
    );
    ui.add_space(12.0);
    let r = widgets::field(ui, t!("modal.name"), name, 100, false);
    if focus {
        r.request_focus();
    }
    if !voice {
        widgets::field(ui, t!("modal.topic"), topic, 1024, false);
    }
    widgets::field(ui, t!("modal.category"), category, 100, false);
    widgets::error_line(ui, core.errors.get(&FormId::EditChannel));
    let busy = core.busy.contains(&FormId::EditChannel);
    if Button::primary(t!("modal.save"))
        .enabled(!busy)
        .show(ui)
        .clicked()
        && !busy
    {
        let mut body = json!({ "name": name.trim(), "category": category.trim() });
        if !voice {
            body["topic"] = json!(topic.trim());
        }
        acts.later(move |c| {
            c.call(
                Req::patch(format!("/channels/{cid}")).with(body),
                Then::Toast(t!("modal.channel_saved").into()),
                ErrTo::Form(FormId::EditChannel),
            )
        });
    }
    if s.can(P_MANAGE_ROLES) {
        widgets::divider(ui);
        widgets::h3(ui, t!("modal.channel_perms_title"));
        widgets::sub(ui, t!("modal.channel_perms_sub"));
        ui.add_space(8.0);
        let bits: &[(i64, &str)] = if voice {
            &[
                (P_VIEW, t!("modal.perm_view")),
                (P_CONNECT, t!("modal.perm_connect")),
            ]
        } else {
            &[
                (P_VIEW, t!("modal.perm_view")),
                (P_SEND, t!("modal.perm_send")),
                (P_ATTACH, t!("modal.perm_attach")),
            ]
        };
        egui::Grid::new("overrides")
            .num_columns(bits.len() + 1)
            .spacing(vec2(12.0, 8.0))
            .striped(false)
            .show(ui, |ui| {
                widgets::caption(ui, t!("modal.role"));
                for (_, label) in bits {
                    widgets::caption(ui, label);
                }
                ui.end_row();
                for role in &s.roles {
                    widgets::pill(ui, &role.name, role.color.as_deref());
                    let o = c
                        .overrides
                        .iter()
                        .find(|o| o.role_id == role.id)
                        .copied()
                        .unwrap_or(Override {
                            role_id: role.id,
                            allow: 0,
                            deny: 0,
                        });
                    for &(bit, _) in bits {
                        let mut v = if o.allow & bit != 0 {
                            1
                        } else if o.deny & bit != 0 {
                            2
                        } else {
                            0
                        };
                        let before = v;
                        ComboBox::from_id_salt(("ov", cid, role.id, bit))
                            .width(110.0)
                            .selected_text(
                                [
                                    t!("modal.override_default"),
                                    t!("modal.override_allow"),
                                    t!("modal.override_deny"),
                                ][v],
                            )
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut v, 0, t!("modal.override_default"));
                                ui.selectable_value(&mut v, 1, t!("modal.override_allow"));
                                ui.selectable_value(&mut v, 2, t!("modal.override_deny"));
                            });
                        if v != before {
                            let (mut allow, mut deny) = (o.allow & !bit, o.deny & !bit);
                            match v {
                                1 => allow |= bit,
                                2 => deny |= bit,
                                _ => {}
                            }
                            let rid = role.id;
                            acts.later(move |c| {
                                c.call(
                                    Req::put(format!("/channels/{cid}/overrides/{rid}"))
                                        .with(json!({ "allow": allow, "deny": deny })),
                                    Then::Nothing,
                                    ErrTo::Toast,
                                )
                            });
                        }
                    }
                    ui.end_row();
                }
            });
    }
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        if Button::danger(t!("modal.delete_channel"))
            .show(ui)
            .clicked()
        {
            acts.later(move |c| {
                c.confirm(
                    t!("modal.delete_channel"),
                    t!("modal.delete_channel_body"),
                    t!("modal.delete_channel"),
                    Action::DeleteChannel(cid),
                )
            });
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if Button::ghost(t!("common.close")).show(ui).clicked() {
                acts.later(Core::close_modal);
            }
        });
    });
}

fn invite(ui: &mut Ui, core: &Core, code: &str, acts: &mut Acts) {
    widgets::h3(ui, t!("modal.invite_title"));
    // The invite is for the server on screen (`Core::invite`).
    let private = core.current_server().is_some_and(|s| s.private);
    widgets::sub(
        ui,
        if private {
            t!("modal.invite_sub_private")
        } else {
            t!("modal.invite_sub_public")
        },
    );
    if code.contains('.') {
        widgets::sub(ui, t!("modal.invite_secret_note"));
    }
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let width = (ui.available_width() - 90.0).max(100.0);
        // Read-only: whatever is typed is thrown away every frame.
        let mut shown = code.to_owned();
        ui.allocate_ui(vec2(width, 38.0), |ui| {
            ui.add(
                TextEdit::singleline(&mut shown)
                    .desired_width(f32::INFINITY)
                    .margin(Margin::symmetric(12, 9))
                    .background_color(BG0())
                    .font(theme::body()),
            );
        });
        if Button::primary(t!("modal.copy_btn")).show(ui).clicked() {
            ui.ctx().copy_text(code.to_owned());
            acts.later(|c| c.toast(t!("modal.code_copied")));
        }
    });
    foot(ui, |ui| {
        if Button::ghost(t!("common.close")).show(ui).clicked() {
            acts.later(Core::close_modal);
        }
    });
}

/// Where the Tor inside the app stands (tor.rs), in a line.
pub fn tor_text(status: &crate::tor::Status) -> String {
    use crate::tor::Status;
    match status {
        Status::Ready => t!("tor.ready").to_owned(),
        Status::Joining(f) => tf!("tor.joining", pct = format!("{:.0}", f * 100.0)),
        Status::Blocked(why) => tf!("tor.blocked", why = why),
    }
}

/// How long the messages of a conversation last (sealing.rs `set_ttl`).
fn ttl(ui: &mut Ui, core: &Core, cid: i64, acts: &mut Acts) {
    widgets::h3(ui, t!("modal.ttl_title"));
    widgets::sub(ui, t!("modal.ttl_body1"));
    widgets::sub(ui, t!("modal.ttl_body2"));
    ui.add_space(12.0);
    let now = core.ttl_of(cid);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (secs, label) in [
            (0, t!("modal.ttl_off")),
            (86_400, t!("modal.ttl_1day")),
            (7 * 86_400, t!("modal.ttl_7days")),
        ] {
            let text = if secs == now {
                format!("✓ {label}")
            } else {
                label.to_owned()
            };
            let button = if secs == now {
                Button::primary(&text)
            } else {
                Button::ghost(&text)
            };
            if button.show(ui).clicked() {
                if secs == now {
                    acts.later(Core::close_modal);
                } else {
                    acts.later(move |c| c.set_ttl(cid, secs));
                }
            }
        }
    });
    foot(ui, |ui| {
        if Button::ghost(t!("common.close")).show(ui).clicked() {
            acts.later(Core::close_modal);
        }
    });
}

// ---------------------------------------------------------------- people

fn profile(ui: &mut Ui, core: &Core, uid: i64, acts: &mut Acts) {
    let s = core.current_server();
    let member = s.and_then(|s| s.member(uid));
    let Some(u) = member.cloned().or_else(|| core.users.get(&uid).cloned()) else {
        acts.later(Core::close_modal);
        return;
    };
    let me = uid == core.me_id();
    let friend = core.friends.iter().any(|f| f.user.id == uid);
    ui.vertical_centered(|ui| {
        widgets::avatar(ui, core, &u, 84.0, true, BG2());
        ui.add_space(8.0);
        widgets::h3(ui, u.name());
        ui.label(
            egui::RichText::new(format!(
                "{} · {}",
                widgets::handle(&u),
                u.status.public().label()
            ))
            .color(MUTED()),
        );
        let roles: Vec<&Role> = match (s, member) {
            (Some(s), Some(m)) => s.roles.iter().filter(|r| m.roles.contains(&r.id)).collect(),
            _ => Vec::new(),
        };
        ui.add_space(12.0);
        if !roles.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for r in roles {
                    widgets::pill(ui, &r.name, r.color.as_deref());
                }
            });
            ui.add_space(12.0);
        }
        if !me {
            if core.blocks.contains(&uid) {
                widgets::sub(ui, t!("modal.you_blocked"));
                ui.add_space(6.0);
                if Button::ghost(t!("modal.unblock_btn")).show(ui).clicked() {
                    acts.later(move |c| {
                        c.call(
                            Req::delete(format!("/blocks/{uid}")),
                            Then::Nothing,
                            ErrTo::Toast,
                        )
                    });
                }
            } else {
                ui.horizontal(|ui| {
                    let n = if friend { 2.0 } else { 3.0 };
                    ui.add_space(((ui.available_width() - n * 120.0) / 2.0).max(0.0));
                    if Button::primary(t!("modal.message_btn")).show(ui).clicked() {
                        acts.later(move |c| c.open_dm(uid));
                    }
                    if !friend && Button::ghost(t!("modal.add_friend_btn")).show(ui).clicked() {
                        let name = u.username.clone();
                        acts.later(move |c| {
                            c.call(
                                Req::post("/friends").with(json!({ "username": name })),
                                Then::FriendRequested { close_modal: true },
                                ErrTo::Toast,
                            )
                        });
                    }
                    if Button::ghost(t!("modal.block")).show(ui).clicked() {
                        acts.later(move |c| {
                            c.confirm(
                                t!("modal.block"),
                                t!("modal.block_body"),
                                t!("modal.block"),
                                Action::Block(uid),
                            )
                        });
                    }
                });
            }
            // The end-to-end encryption with them (ui/security.rs).
            ui.add_space(8.0);
            let label = match core.e2e.pin_view(uid) {
                crate::e2e::PinView::Changed => t!("modal.e2e_key_changed"),
                crate::e2e::PinView::Pinned { verified: true } => t!("modal.e2e_code_verified"),
                _ => t!("modal.e2e_code"),
            };
            if widgets::link(ui, label).clicked() {
                acts.later(move |c| super::security::open(c, uid));
            }
        }
    });
    let (Some(s), Some(m)) = (s, member) else {
        return;
    };
    let sid = s.id;
    let other_roles: Vec<&Role> = s.roles.iter().filter(|r| !r.is_everyone()).collect();
    if !me && s.can(P_MANAGE_ROLES) && !other_roles.is_empty() {
        widgets::divider(ui);
        ui.label(
            egui::RichText::new(t!("modal.roles_title"))
                .font(theme::bold(14.0))
                .color(TEXT()),
        );
        ui.add_space(4.0);
        for r in other_roles {
            let mut on = m.roles.contains(&r.id);
            let grantable = r.perms & !s.perms == 0;
            let rid = r.id;
            ui.horizontal(|ui| {
                widgets::pill(ui, &r.name, r.color.as_deref());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_enabled_ui(grantable, |ui| {
                        if ui.checkbox(&mut on, "").changed() {
                            let path = format!("/servers/{sid}/members/{uid}/roles/{rid}");
                            let req = if on {
                                Req::put(path)
                            } else {
                                Req::delete(path)
                            };
                            acts.later(move |c| c.call(req, Then::Nothing, ErrTo::Toast));
                        }
                    });
                });
            });
        }
    }
    // Nobody in the server signed them in (an invite from before signed
    // ones, or an account the server added on its own): no keys for them.
    if !core.member_vouched(sid, uid) {
        widgets::divider(ui);
        ui.label(egui::RichText::new(t!("modal.not_vouched")).color(WARN));
        if core.member_vouched(sid, core.me_id())
            && Button::ghost(t!("modal.confirm_member_btn"))
                .small()
                .show(ui)
                .clicked()
        {
            acts.later(move |c| c.vouch(sid, uid));
        }
    }
    if !me && uid != s.owner_id && (s.can(P_KICK) || s.can(P_BAN)) {
        widgets::divider(ui);
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - 180.0) / 2.0).max(0.0));
                if s.can(P_KICK)
                    && Button::danger(t!("modal.kick_btn"))
                        .small()
                        .show(ui)
                        .clicked()
                {
                    acts.later(move |c| {
                        c.confirm(
                            t!("modal.kick_title"),
                            t!("modal.kick_body"),
                            t!("modal.kick_btn"),
                            Action::Kick { sid, uid },
                        )
                    });
                }
                if s.can(P_BAN)
                    && Button::danger(t!("modal.ban_btn"))
                        .small()
                        .show(ui)
                        .clicked()
                {
                    acts.later(move |c| {
                        c.confirm(
                            t!("modal.ban_title"),
                            t!("modal.ban_body"),
                            t!("modal.ban_btn"),
                            Action::Ban { sid, uid },
                        )
                    });
                }
            });
        });
    }
}

// ---------------------------------------------------------------- moderation

fn report(
    ui: &mut Ui,
    core: &Core,
    mid: i64,
    reason: &mut usize,
    detail: &mut String,
    acts: &mut Acts,
) {
    widgets::h3(ui, t!("modal.report_title"));
    widgets::sub(ui, t!("modal.report_sub"));
    ui.add_space(12.0);
    let reasons = report_reasons();
    let labels: Vec<&str> = reasons.iter().map(|(_, l)| *l).collect();
    select(
        ui,
        "report-reason",
        t!("modal.reason_label"),
        reason,
        &labels,
    );
    ui.label(
        egui::RichText::new(t!("modal.details_optional"))
            .font(theme::bold(12.0))
            .color(MUTED())
            .extra_letter_spacing(0.5),
    );
    ui.add_space(2.0);
    ui.add(
        TextEdit::multiline(detail)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .char_limit(2000)
            .background_color(BG0())
            .margin(Margin::symmetric(12, 9)),
    );
    widgets::error_line(ui, core.errors.get(&FormId::Report));
    let busy = core.busy.contains(&FormId::Report);
    let key = reasons.get(*reason).map_or("outro", |(k, _)| *k);
    foot(ui, |ui| {
        if Button::danger(t!("modal.send_report_btn"))
            .enabled(!busy)
            .show(ui)
            .clicked()
            && !busy
        {
            let body = json!({ "reason": key, "detail": detail.trim() });
            acts.later(move |c| {
                c.call(
                    Req::post(format!("/messages/{mid}/report")).with(body),
                    Then::CloseModalToast(t!("modal.report_sent").into()),
                    ErrTo::Form(FormId::Report),
                )
            });
        }
        if Button::ghost(t!("common.cancel")).show(ui).clicked() {
            acts.later(Core::close_modal);
        }
    });
}

pub(super) fn reports(ui: &mut Ui, core: &Core, sid: i64, acts: &mut Acts, in_page: bool) {
    let Some(s) = core.server(sid) else {
        acts.later(Core::close_modal);
        return;
    };
    if !in_page {
        widgets::h3(ui, t!("modal.reports_title"));
    }
    let n = s.reports.len();
    let count = match n {
        0 => t!("modal.reports_none_pending").to_owned(),
        1 => t!("modal.reports_one_pending").to_owned(),
        n => tf!("modal.reports_n_pending", n = n),
    };
    widgets::sub(ui, &format!("{} · {count}", s.name));
    ui.add_space(12.0);
    let can_ban = s.can(P_BAN);
    for r in &s.reports {
        Frame::new()
            .fill(BG1())
            .stroke(Stroke::new(1.0, BG4()))
            .corner_radius(CornerRadius::same(RADIUS))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let author = r.message.author.clone().unwrap_or_else(|| User::unknown(0));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    widgets::avatar(ui, core, &author, 30.0, false, BG1());
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(
                            egui::RichText::new(author.name())
                                .font(theme::bold(14.0))
                                .color(TEXT()),
                        );
                        let place = r.channel.as_deref().unwrap_or("?");
                        ui.label(
                            egui::RichText::new(tf!(
                                "modal.report_in_channel",
                                channel = place,
                                time = super::chat::fmt_time(r.message.created)
                            ))
                            .size(13.0)
                            .color(MUTED()),
                        );
                    });
                });
                ui.add_space(6.0);
                let quote = ui.horizontal(|ui| {
                    ui.add_space(13.0);
                    ui.vertical(|ui| {
                        if r.message.content.is_empty() {
                            ui.label(
                                egui::RichText::new(t!("modal.attachment"))
                                    .italics()
                                    .color(MUTED()),
                            );
                        } else {
                            let pieces =
                                markdown::parse(&r.message.content, &|n| core.known_name(n));
                            let named =
                                |user: &str| core.user_named(user).map(|u| u.name().to_owned());
                            if let Some(markdown::Click::Link(url)) =
                                markdown::show(ui, &pieces, &|_| true, &named, false)
                            {
                                super::chat::open_external(url);
                            }
                        }
                    });
                });
                let qr = quote.response.rect;
                ui.painter().rect_filled(
                    Rect::from_min_size(qr.min, vec2(3.0, qr.height())),
                    CornerRadius::ZERO,
                    BG4(),
                );
                ui.add_space(6.0);
                let reporter = r
                    .reporter
                    .as_ref()
                    .map_or(t!("modal.unknown"), |u| u.name());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(egui::RichText::new(t!("modal.reason_colon")).color(MUTED()));
                    ui.label(
                        egui::RichText::new(reason_label(&r.reason))
                            .font(theme::bold(14.0))
                            .color(MUTED()),
                    );
                    ui.label(
                        egui::RichText::new(tf!("modal.reported_by", reporter = reporter))
                            .color(MUTED()),
                    );
                });
                let (rid, mid, author_id) = (r.id, r.message.id, author.id);
                foot(ui, |ui| {
                    if Button::danger(t!("modal.delete_message_title"))
                        .small()
                        .show(ui)
                        .clicked()
                    {
                        acts.later(move |c| {
                            c.confirm(
                                t!("modal.delete_message_title"),
                                t!("modal.delete_reported_body"),
                                t!("modal.delete_btn"),
                                Action::DeleteReported { sid, mid },
                            )
                        });
                    }
                    if can_ban
                        && author_id != s.owner_id
                        && author_id != 0
                        && Button::danger(t!("modal.ban_author"))
                            .small()
                            .show(ui)
                            .clicked()
                    {
                        acts.later(move |c| {
                            c.confirm(
                                t!("modal.ban_author"),
                                t!("modal.ban_body"),
                                t!("modal.ban_btn"),
                                Action::BanAuthor {
                                    sid,
                                    uid: author_id,
                                },
                            )
                        });
                    }
                    if Button::ghost(t!("modal.ignore_btn"))
                        .small()
                        .show(ui)
                        .clicked()
                    {
                        acts.later(move |c| {
                            c.call(
                                Req::delete(format!("/reports/{rid}")),
                                Then::Nothing,
                                ErrTo::Toast,
                            )
                        });
                    }
                });
            });
        ui.add_space(10.0);
    }
    if !in_page {
        foot(ui, |ui| {
            if Button::ghost(t!("common.close")).show(ui).clicked() {
                acts.later(Core::close_modal);
            }
        });
    }
}

/// Who asked to join a private server, to let in or turn down.
pub(super) fn requests(ui: &mut Ui, core: &Core, sid: i64, acts: &mut Acts, in_page: bool) {
    let Some(s) = core.server(sid) else {
        acts.later(Core::close_modal);
        return;
    };
    if !in_page {
        widgets::h3(ui, t!("modal.join_requests_title"));
    }
    let count = match s.requests.len() {
        0 => t!("modal.requests_none_waiting").to_owned(),
        1 => t!("modal.requests_one_waiting").to_owned(),
        n => tf!("modal.requests_n_waiting", n = n),
    };
    widgets::sub(ui, &format!("{} · {count}", s.name));
    ui.add_space(12.0);
    if s.requests.is_empty() {
        widgets::sub(
            ui,
            if s.private {
                t!("modal.requests_empty_private")
            } else {
                t!("modal.requests_empty_public")
            },
        );
    }
    for r in &s.requests {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            widgets::avatar(ui, core, &r.user, 30.0, false, BG2());
            ui.vertical(|ui| {
                // Room left for the two buttons.
                ui.set_max_width((ui.available_width() - 190.0).max(80.0));
                ui.spacing_mut().item_spacing.y = 0.0;
                widgets::ellipsis(ui, r.user.name(), theme::bold(14.0), TEXT());
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {}",
                        widgets::handle(&r.user),
                        super::chat::fmt_time(r.created)
                    ))
                    .size(13.0)
                    .color(MUTED()),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let uid = r.user.id;
                if Button::ghost(t!("modal.decline_btn"))
                    .small()
                    .show(ui)
                    .clicked()
                {
                    acts.later(move |c| c.deny_request(sid, uid));
                }
                if Button::primary(t!("modal.accept_btn"))
                    .small()
                    .show(ui)
                    .clicked()
                {
                    acts.later(move |c| c.accept_request(sid, uid));
                }
            });
        });
        ui.add_space(6.0);
    }
    if !in_page {
        foot(ui, |ui| {
            if Button::ghost(t!("common.close")).show(ui).clicked() {
                acts.later(Core::close_modal);
            }
        });
    }
}

// ---------------------------------------------------------------- sharing

/// A card in the share picker: a picture of the screen or window, its name
/// and what it is.
const CARD: egui::Vec2 = vec2(326.0, 236.0);
const THUMB: egui::Vec2 = vec2(302.0, 170.0);

fn share(
    ui: &mut Ui,
    core: &Core,
    tab: &mut ShareTab,
    items: &mut Vec<crate::core::ShareItem>,
    incoming: &mut Option<std::sync::mpsc::Receiver<crate::voice::sources::Choice>>,
    acts: &mut Acts,
) {
    use crate::voice::sources::Source;
    // Whatever the picture thread has found since the last frame.
    if let Some(rx) = incoming {
        loop {
            match rx.try_recv() {
                Ok(c) => items.push(crate::core::ShareItem {
                    source: c.source,
                    title: c.title,
                    detail: c.detail,
                    thumb: c.thumb.map(|img| {
                        ui.ctx()
                            .load_texture("compartilhar", img, egui::TextureOptions::LINEAR)
                    }),
                }),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    *incoming = None;
                    break;
                }
            }
        }
    }
    let loading = incoming.is_some();
    let is_window = |i: &crate::core::ShareItem| matches!(i.source, Source::Window(_));
    let window_count = items.iter().filter(|i| is_window(i)).count();
    let screen_count = items.len() - window_count;

    widgets::h3(ui, t!("modal.share_title"));
    widgets::sub(ui, t!("modal.share_sub"));
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (text, which) in [
            (
                tf!("modal.share_screens", n = screen_count),
                ShareTab::Screens,
            ),
            (
                tf!("modal.share_windows", n = window_count),
                ShareTab::Windows,
            ),
            (t!("modal.share_slides").to_owned(), ShareTab::Slides),
        ] {
            let button = if *tab == which {
                Button::primary(&text)
            } else {
                Button::ghost(&text)
            };
            if button.small().show(ui).clicked() {
                *tab = which;
            }
        }
    });
    ui.add_space(12.0);
    if *tab == ShareTab::Slides {
        present(ui, core, acts);
        return;
    }
    let windows = *tab == ShareTab::Windows;
    // The quality of what is shared (and of the camera). Changed while
    // sharing, the share starts over at the new one.
    let mut q = QUALITIES
        .iter()
        .position(|q| q.key == core.quality_key())
        .unwrap_or(1);
    let labels: Vec<&str> = QUALITIES.iter().map(|q| q.label).collect();
    if select(
        ui,
        "share-quality",
        t!("modal.quality_label"),
        &mut q,
        &labels,
    ) {
        let key = QUALITIES[q].key;
        acts.later(move |c| c.set_quality(key));
    }
    // The PC's sound along with the picture (for a window, its program's).
    let mut audio = core.voice.share_audio();
    let hint = if windows {
        t!("modal.share_audio_window")
    } else {
        t!("modal.share_audio_pc")
    };
    if widgets::check_row(ui, t!("modal.share_audio_label"), hint, &mut audio, true) {
        acts.later(move |c| c.set_share_audio(audio));
    }
    ui.add_space(8.0);

    let current = core.voice.source();
    let shown: Vec<&crate::core::ShareItem> =
        items.iter().filter(|i| is_window(i) == windows).collect();
    if shown.is_empty() {
        ui.horizontal(|ui| {
            if loading {
                ui.add(egui::Spinner::new().size(16.0));
                ui.label(egui::RichText::new(t!("modal.searching")).color(MUTED()));
            } else if windows {
                ui.label(egui::RichText::new(t!("modal.no_windows")).color(MUTED()));
            } else {
                ui.label(egui::RichText::new(t!("modal.no_screens")).color(MUTED()));
            }
        });
    }
    for row in shown.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for item in row {
                if share_card(ui, item, current == Some(item.source)).clicked() {
                    let source = item.source;
                    acts.later(move |c| c.share(source));
                }
            }
        });
        ui.add_space(12.0);
    }
    if loading && !shown.is_empty() {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(14.0));
            ui.label(
                egui::RichText::new(t!("modal.searching_more"))
                    .font(theme::small())
                    .color(FAINT()),
            );
        });
    }
    foot(ui, |ui| {
        if Button::ghost(t!("common.cancel")).show(ui).clicked() {
            acts.later(Core::close_modal);
        }
        if matches!(current, Some(Source::Monitor(_) | Source::Window(_)))
            && Button::danger(t!("modal.stop_sharing_btn"))
                .show(ui)
                .clicked()
        {
            acts.later(|c| {
                c.close_modal();
                c.voice.share(None);
            });
        }
    });
}

/// The picker's presentation tab: a .pptx or a PDF, drawn on this PC and
/// sent to everyone as pictures (slides/).
fn present(ui: &mut Ui, core: &Core, acts: &mut Acts) {
    ui.label(egui::RichText::new(t!("modal.present_body1")).color(TEXT()));
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(t!("modal.present_body2"))
            .font(theme::small())
            .color(MUTED()),
    );
    ui.add_space(14.0);
    let presenting = core.voice.slides().mine();
    if let Some(mine) = &presenting {
        ui.label(
            egui::RichText::new(tf!("modal.presenting_file", name = mine.name))
                .font(theme::bold(13.0))
                .color(ACCENT()),
        );
        ui.add_space(8.0);
    }
    let label = if presenting.is_some() {
        t!("modal.present_another")
    } else {
        t!("modal.choose_file")
    };
    if Button::primary(label).show(ui).clicked() {
        acts.later(|c| c.pick_files(crate::core::Pick::Slides));
    }
    foot(ui, |ui| {
        if Button::ghost(t!("common.cancel")).show(ui).clicked() {
            acts.later(Core::close_modal);
        }
        if presenting.is_some()
            && Button::danger(t!("modal.stop_presenting_btn"))
                .show(ui)
                .clicked()
        {
            acts.later(|c| {
                c.close_modal();
                c.voice.stop_presenting();
            });
        }
    });
}

fn share_card(ui: &mut Ui, item: &crate::core::ShareItem, active: bool) -> egui::Response {
    use crate::voice::sources::Source;
    let (rect, resp) = ui.allocate_exact_size(CARD, Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let painter = ui.painter_at(rect);
    let fill = if resp.hovered() { BG4() } else { BG3() };
    let stroke = if active {
        Stroke::new(2.0, ACCENT())
    } else {
        Stroke::NONE
    };
    painter.rect(
        rect,
        CornerRadius::same(RADIUS),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    let pic = Rect::from_min_size(rect.min + vec2(12.0, 12.0), THUMB);
    painter.rect_filled(pic, CornerRadius::same(RADIUS_SM), BG0());
    match &item.thumb {
        Some(tex) => {
            // Fitted inside the box, keeping the picture's shape.
            let size = tex.size_vec2();
            let scale = (THUMB.x / size.x).min(THUMB.y / size.y);
            let fitted = Rect::from_center_size(pic.center(), size * scale);
            painter.image(
                tex.id(),
                fitted,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            let icon = if matches!(item.source, Source::Window(_)) {
                Icon::Window
            } else {
                Icon::Monitor
            };
            let r = Rect::from_center_size(pic.center(), vec2(40.0, 40.0));
            icon.image(40.0, FAINT()).paint_at(ui, r);
        }
    }
    let text = Rect::from_min_max(
        pos2(pic.left() + 2.0, pic.bottom() + 8.0),
        pos2(pic.right() - 2.0, rect.bottom() - 8.0),
    );
    // A child that takes no room of its own: the card already did.
    let mut lines = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(text)
            .layout(Layout::top_down(Align::Min)),
    );
    lines.spacing_mut().item_spacing.y = 2.0;
    widgets::ellipsis(&mut lines, &item.title, theme::bold(13.0), TEXT());
    widgets::ellipsis(&mut lines, &item.detail, theme::small(), MUTED());
    resp
}

// ---------------------------------------------------------------- floating menus

pub fn pop(ctx: &egui::Context, core: &mut Core, acts: &mut Acts) {
    let Some(p) = core.pop else { return };
    // The annotations' menus go with their toolbar.
    let ink_menu = matches!(
        p.kind,
        PopKind::InkColor | PopKind::InkClear(_) | PopKind::InkWho(_)
    );
    if ink_menu && core.voice.ink().active().is_none() {
        acts.later(|c| c.pop = None);
        return;
    }
    let screen = ctx.content_rect();
    let (w, h) = match p.kind {
        PopKind::ServerMenu => server_menu_size(core, p.anchor),
        PopKind::ChannelMenu(cid) => (CHANNEL_MENU_W + 12.0, channel_menu_height(core, cid)),
        PopKind::RailMenu(_) => (RAIL_MENU_W + 12.0, rail_menu_height()),
        PopKind::React(_) | PopKind::Emoji => (EMOJI_W + 12.0, EMOJI_H + 12.0),
        PopKind::Sticker => (STICKER_W + 12.0, STICKER_H + 12.0),
        PopKind::Person(uid) => (PERSON_W + 12.0, person_height(core, uid) + 12.0),
        PopKind::InkColor => (
            super::ink::COLOR_POP.x + 12.0,
            super::ink::COLOR_POP.y + 12.0,
        ),
        PopKind::InkClear(_) => (222.0, 2.0 * 34.0 + 12.0),
        PopKind::InkWho(_) => (PERSON_W + 12.0, 2.0 * PERSON_ROW_H + 12.0),
    };
    let x = p
        .anchor
        .left()
        .clamp(8.0, (screen.right() - w - 8.0).max(8.0));
    let y = if p.anchor.bottom() + h + 8.0 > screen.bottom() {
        (p.anchor.top() - h - 6.0).max(8.0)
    } else {
        p.anchor.bottom() + 6.0
    };
    let resp = Area::new(Id::unique("pop"))
        .order(Order::Foreground)
        .fixed_pos(pos2(x, y))
        .show(ctx, |ui| {
            Frame::new()
                .fill(BG0())
                .stroke(Stroke::new(1.0, BG4()))
                .corner_radius(CornerRadius::same(RADIUS))
                .inner_margin(Margin::same(6))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 10],
                    blur: 30,
                    spread: 0,
                    color: Color32::from_black_alpha(0xaa),
                })
                .show(ui, |ui| match p.kind {
                    PopKind::ServerMenu => server_menu(ui, core, w - 12.0, acts),
                    PopKind::RailMenu(sid) => rail_menu(ui, core, sid, acts),
                    PopKind::ChannelMenu(cid) => channel_menu(ui, core, cid, acts),
                    PopKind::React(mid) => {
                        emoji_picker(ui, core, acts, move |c, e| c.react(mid, e))
                    }
                    PopKind::Emoji => {
                        let ctx = ui.ctx().clone();
                        emoji_picker(ui, core, acts, move |c, e| {
                            super::chat::insert_emoji(&ctx, c, e);
                            c.pop = None;
                        })
                    }
                    PopKind::Sticker => sticker_picker(ui, core, acts),
                    PopKind::Person(uid) => person_menu(ui, core, uid, acts),
                    PopKind::InkColor => super::ink::color_pop(ui, core, acts),
                    PopKind::InkClear(s) => super::ink::clear_pop(ui, core, s, acts),
                    PopKind::InkWho(s) => super::ink::who_pop(ui, core, s, acts),
                });
        });
    if resp.response.clicked_elsewhere()
        && !p
            .anchor
            .contains(ctx.input(|i| i.pointer.interact_pos().unwrap_or_default()))
    {
        // Only this one: a right click on someone else just opened theirs.
        let (kind, anchor) = (p.kind, p.anchor);
        acts.later(move |c| {
            if c.pop
                .is_some_and(|now| now.kind == kind && now.anchor == anchor)
            {
                c.pop = None;
            }
        });
    }
}

/// The menu of someone in a call.
const PERSON_W: f32 = 236.0;
const PERSON_TITLE_H: f32 = 30.0;
const PERSON_ROW_H: f32 = 34.0;
const PERSON_LABEL_H: f32 = 20.0;
const PERSON_SLIDER_H: f32 = 26.0;
const PERSON_GAP: f32 = 8.0;

/// How tall the menu of `uid` is: their voice, and the sound of their
/// screen share while they share.
fn person_height(core: &Core, uid: i64) -> f32 {
    let block = PERSON_ROW_H + PERSON_LABEL_H + PERSON_SLIDER_H;
    let screen = if core.shares_screen(uid) {
        PERSON_GAP + 1.0 + block
    } else {
        0.0
    };
    PERSON_TITLE_H + block + screen + PERSON_GAP
}

/// Silence or turn up or down `uid` here, as Discord's right click does:
/// their voice, and apart from it the PC's sound of their screen share.
/// Only on this PC; the others hear them as before.
fn person_menu(ui: &mut Ui, core: &Core, uid: i64, acts: &mut Acts) {
    ui.set_width(PERSON_W);
    ui.spacing_mut().item_spacing.y = 0.0;
    let audio = core.person_audio(uid);
    let (rect, _) = ui.allocate_exact_size(vec2(PERSON_W, PERSON_TITLE_H), Sense::hover());
    let name = core.user(uid).name().to_owned();
    let galley = ui.painter().layout_job({
        let mut job = egui::text::LayoutJob::simple_singleline(name, theme::bold(13.5), TEXT());
        job.wrap = egui::text::TextWrapping::truncate_at_width(PERSON_W - 20.0);
        job
    });
    ui.painter().galley(
        pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
        galley,
        TEXT(),
    );
    let set = move |acts: &mut Acts, audio: crate::prefs::PersonAudio, keep: bool| {
        acts.later(move |c| c.set_person_audio(uid, audio, keep));
    };
    if menu_check(ui, t!("modal.mute"), audio.voice_muted) {
        set(
            acts,
            crate::prefs::PersonAudio {
                voice_muted: !audio.voice_muted,
                ..audio
            },
            true,
        );
    }
    if let Some((voice, done)) = volume(ui, t!("modal.volume"), audio.voice, audio.voice_muted) {
        set(
            acts,
            crate::prefs::PersonAudio {
                voice,
                voice_muted: false,
                ..audio
            },
            done,
        );
    }
    if core.shares_screen(uid) {
        ui.add_space(PERSON_GAP);
        let (line, _) = ui.allocate_exact_size(vec2(PERSON_W, 1.0), Sense::hover());
        ui.painter()
            .hline(line.x_range(), line.center().y, Stroke::new(1.0, BG4()));
        if menu_check(ui, t!("modal.mute_stream"), audio.screen_muted) {
            set(
                acts,
                crate::prefs::PersonAudio {
                    screen_muted: !audio.screen_muted,
                    ..audio
                },
                true,
            );
        }
        if let Some((screen, done)) = volume(
            ui,
            t!("modal.volume_stream"),
            audio.screen,
            audio.screen_muted,
        ) {
            set(
                acts,
                crate::prefs::PersonAudio {
                    screen,
                    screen_muted: false,
                    ..audio
                },
                done,
            );
        }
    }
    ui.add_space(PERSON_GAP);
}

/// A menu row with a box to tick at its right. Returns whether it was
/// clicked.
pub(super) fn menu_check(ui: &mut Ui, text: &str, on: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(PERSON_W, PERSON_ROW_H), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let hovered = resp.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(RADIUS_SM), ACCENT_2());
    }
    let fg = if hovered { Color32::WHITE } else { TEXT() };
    ui.painter().text(
        pos2(rect.left() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        theme::body(),
        fg,
    );
    let tick = Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), vec2(16.0, 16.0));
    if on {
        ui.painter().rect_filled(
            tick,
            CornerRadius::same(4),
            if hovered { Color32::WHITE } else { ACCENT() },
        );
        let mark = if hovered { ACCENT_2() } else { Color32::WHITE };
        let stroke = Stroke::new(2.0, mark);
        let (a, b, c) = (
            pos2(tick.left() + 3.5, tick.center().y),
            pos2(tick.left() + 6.5, tick.bottom() - 4.0),
            pos2(tick.right() - 3.0, tick.top() + 4.0),
        );
        ui.painter().line_segment([a, b], stroke);
        ui.painter().line_segment([b, c], stroke);
    } else {
        ui.painter().rect_stroke(
            tick,
            CornerRadius::same(4),
            Stroke::new(1.5, if hovered { Color32::WHITE } else { MUTED() }),
            egui::StrokeKind::Inside,
        );
    }
    resp.clicked()
}

/// A volume from 0 to 200 %, its label above with the value. Returns the
/// new value while it is clicked or dragged, and whether the change is done
/// (released), which is when it is written down. 100 % holds the knob a
/// little, as it is where most people come back to.
fn volume(ui: &mut Ui, label: &str, value: f32, muted: bool) -> Option<(f32, bool)> {
    slider(ui, PERSON_W, label, value, 2.0, muted)
}

/// A volume from 0 to `max` (1 for 100 %, 2 for 200 %), `width` wide, its
/// label above with the value. Returns the new value while it is clicked
/// or dragged, and whether the change is done (released). Going past 100 %,
/// a mark shows where 100 % is, and the knob stops there a little.
pub(super) fn slider(
    ui: &mut Ui,
    width: f32,
    label: &str,
    value: f32,
    max: f32,
    muted: bool,
) -> Option<(f32, bool)> {
    let (row, _) = ui.allocate_exact_size(vec2(width, PERSON_LABEL_H), Sense::hover());
    ui.painter().text(
        pos2(row.left() + 10.0, row.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        theme::small(),
        MUTED(),
    );
    let shown = if muted { 0.0 } else { value };
    ui.painter().text(
        pos2(row.right() - 10.0, row.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{:.0} %", shown * 100.0),
        theme::small(),
        if muted { DANGER } else { TEXT() },
    );
    let (rect, resp) =
        ui.allocate_exact_size(vec2(width, PERSON_SLIDER_H), Sense::click_and_drag());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let track = Rect::from_center_size(rect.center(), vec2(width - 28.0, 4.0));
    let painter = ui.painter();
    painter.rect_filled(track, CornerRadius::same(2), BG4());
    let fraction = (shown / max).clamp(0.0, 1.0);
    let knob = pos2(track.left() + track.width() * fraction, track.center().y);
    painter.rect_filled(
        Rect::from_min_max(track.min, pos2(knob.x, track.max.y)),
        CornerRadius::same(2),
        if muted { FAINT() } else { ACCENT() },
    );
    let past_full = max > 1.0;
    if past_full {
        // Where 100 % is.
        let mark = track.left() + track.width() / max;
        painter.vline(mark, track.y_range().expand(3.0), Stroke::new(1.0, MUTED()));
    }
    let r = if resp.hovered() || resp.dragged() {
        7.0
    } else {
        6.0
    };
    painter.circle_filled(knob, r, TEXT());
    if !(resp.clicked() || resp.dragged() || resp.drag_stopped()) {
        return None;
    }
    let pointer = resp
        .interact_pointer_pos()
        .or_else(|| ui.ctx().input(|i| i.pointer.latest_pos()))?;
    let mut v = ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0) * max;
    if past_full && (v - 1.0).abs() < 0.05 {
        v = 1.0;
    }
    let v = (v * 100.0).round() / 100.0;
    Some((v, resp.clicked() || resp.drag_stopped()))
}

pub(super) fn menu_item(ui: &mut Ui, text: &str, danger: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(210.0, 34.0), Sense::click());
    crate::smoke::target(|| format!("menu:{text}"), rect);
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let (bg, fg) = match (resp.hovered(), danger) {
        (true, true) => (Some(DANGER), Color32::WHITE),
        (true, false) => (Some(ACCENT_2()), Color32::WHITE),
        (false, true) => (None, DANGER),
        (false, false) => (None, TEXT()),
    };
    if let Some(bg) = bg {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(RADIUS_SM), bg);
    }
    ui.painter().text(
        pos2(rect.left() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        theme::body(),
        fg,
    );
    resp.clicked()
}

/// A row of the drop-down menus (the server's, a channel's, a server's in
/// the rail): its height, and the gap a line between groups takes.
const DROP_ROW_H: f32 = 34.0;
const DROP_LINE_H: f32 = 9.0;

/// What a row of the server menu does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    Invite,
    Settings,
    NewChannel,
    NewCategory,
    Notify,
    Nick,
    HideMuted,
    Leave,
}

/// The server menu's rows in groups, as Discord's: what the owner (or
/// whoever may manage something) sees has the settings and the new
/// channels; the others leave instead.
fn server_menu_groups(core: &Core, s: &Server) -> Vec<Vec<Entry>> {
    let mut first = Vec::new();
    if s.can(P_INVITE) {
        first.push(Entry::Invite);
    }
    if crate::core::ServerTab::ALL
        .iter()
        .any(|t| super::server_settings::shown(s, *t))
    {
        first.push(Entry::Settings);
    }
    if s.can(P_MANAGE_CHANNELS) {
        first.push(Entry::NewChannel);
        first.push(Entry::NewCategory);
    }
    let mut groups = vec![first, vec![Entry::Notify], vec![Entry::Nick, Entry::HideMuted]];
    if s.owner_id != core.me_id() {
        groups.push(vec![Entry::Leave]);
    }
    groups.retain(|g| !g.is_empty());
    groups
}

/// How tall a drop-down menu of these groups is, its margins included.
fn drop_height(groups: &[Vec<Entry>]) -> f32 {
    let rows: usize = groups.iter().map(Vec::len).sum();
    rows as f32 * DROP_ROW_H + groups.len().saturating_sub(1) as f32 * DROP_LINE_H + 12.0
}

/// How big the server menu is: as wide as the channel list under it.
fn server_menu_size(core: &Core, anchor: Rect) -> (f32, f32) {
    let w = anchor.width().max(264.0);
    let Some(s) = core.current_server() else {
        return (w, 60.0);
    };
    (w, drop_height(&server_menu_groups(core, s)))
}

/// The server menu, a list under the server's name.
fn server_menu(ui: &mut Ui, core: &Core, width: f32, acts: &mut Acts) {
    let Some(s) = core.current_server() else {
        return;
    };
    let sid = s.id;
    ui.set_width(width);
    ui.spacing_mut().item_spacing.y = 0.0;
    // What waits in the settings: reports and people asking to come in.
    let waiting = if s.can(P_MANAGE_MESSAGES) {
        s.reports.len()
    } else {
        0
    } + if s.can(P_KICK) { s.requests.len() } else { 0 };
    for (i, group) in server_menu_groups(core, s).into_iter().enumerate() {
        if i > 0 {
            drop_line(ui);
        }
        for e in group {
            let (icon, text, danger, trail) = match e {
                Entry::Invite => (Icon::UserPlus, t!("server.invite"), false, Trail::None),
                Entry::Settings => (
                    Icon::Gear,
                    t!("server.settings"),
                    false,
                    Trail::Count(waiting),
                ),
                Entry::NewChannel => (Icon::Plus, t!("modal.create_channel"), false, Trail::None),
                Entry::NewCategory => (
                    Icon::FolderPlus,
                    t!("server.create_category"),
                    false,
                    Trail::None,
                ),
                Entry::Notify => (Icon::Bell, t!("server.notify_settings"), false, Trail::None),
                Entry::Nick => (Icon::Edit, t!("server.edit_profile"), false, Trail::None),
                Entry::HideMuted => (
                    Icon::EyeOff,
                    t!("server.hide_muted"),
                    false,
                    Trail::Check(core.hides_muted(sid)),
                ),
                Entry::Leave => (Icon::LogOut, t!("modal.leave_server_title"), true, Trail::None),
            };
            if !drop_row(ui, Some(icon), text, danger, trail) {
                continue;
            }
            match e {
                Entry::Invite => acts.later(Core::invite),
                Entry::Settings => acts.later(move |c| c.open_server_settings(sid)),
                Entry::NewChannel => acts.later(move |c| c.new_channel(sid, false)),
                Entry::NewCategory => acts.later(move |c| c.new_channel(sid, true)),
                Entry::Notify => acts.later(move |c| c.open_notify_settings(sid)),
                Entry::Nick => acts.later(move |c| c.open_nick(sid)),
                Entry::HideMuted => acts.later(move |c| c.toggle_hide_muted(sid)),
                Entry::Leave => {
                    let back = if s.private {
                        t!("modal.leave_server_body_private")
                    } else {
                        t!("modal.leave_server_body_public")
                    };
                    acts.later(move |c| {
                        c.confirm(
                            t!("modal.leave_server_title"),
                            back,
                            t!("modal.leave_btn"),
                            Action::LeaveServer(sid),
                        )
                    });
                }
            }
        }
    }
}

/// What a drop-down row has at its right.
#[derive(Clone, Copy)]
enum Trail {
    None,
    Chevron,
    Count(usize),
    Check(bool),
}

/// The line between a drop-down menu's groups.
fn drop_line(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), DROP_LINE_H), Sense::hover());
    ui.painter().hline(
        r.x_range().shrink(6.0),
        r.center().y,
        Stroke::new(1.0, BG3()),
    );
}

/// A row of a drop-down menu: an icon, the text, and what it has at its
/// right. Returns whether it was clicked.
fn drop_row(ui: &mut Ui, icon: Option<Icon>, text: &str, danger: bool, trail: Trail) -> bool {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, DROP_ROW_H), Sense::click());
    crate::smoke::target(|| format!("menu:{text}"), rect);
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let hovered = resp.hovered();
    if hovered {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(RADIUS_SM),
            if danger {
                DANGER.gamma_multiply(0.18)
            } else {
                BG3()
            },
        );
    }
    let ink = if danger {
        DANGER
    } else if hovered {
        TEXT()
    } else {
        TEXT().gamma_multiply(0.88)
    };
    let mut x = rect.left() + 10.0;
    if let Some(icon) = icon {
        icon.image(18.0, ink).paint_at(
            ui,
            Rect::from_center_size(pos2(x + 9.0, rect.center().y), vec2(18.0, 18.0)),
        );
        x += 28.0;
    }
    let right = rect.right() - 10.0;
    let text_w = (right - x - 26.0).max(20.0);
    let galley = ui.painter().layout_job({
        let mut job =
            egui::text::LayoutJob::simple_singleline(text.to_owned(), theme::body(), ink);
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_w);
        job
    });
    ui.painter()
        .galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, ink);
    match trail {
        Trail::None => {}
        Trail::Chevron => {
            Icon::Right.image(14.0, MUTED()).paint_at(
                ui,
                Rect::from_center_size(pos2(right - 6.0, rect.center().y), vec2(14.0, 14.0)),
            );
        }
        Trail::Count(n) => {
            if n > 0 {
                widgets::badge_at(ui, pos2(right - 8.0, rect.center().y), n as u32, None);
            }
        }
        Trail::Check(on) => {
            let tick = Rect::from_center_size(pos2(right - 9.0, rect.center().y), vec2(18.0, 18.0));
            if on {
                ui.painter()
                    .rect_filled(tick, CornerRadius::same(4), ACCENT());
                let stroke = Stroke::new(2.0, ACCENT_INK());
                let (a, b, c) = (
                    pos2(tick.left() + 4.0, tick.center().y),
                    pos2(tick.left() + 7.5, tick.bottom() - 4.5),
                    pos2(tick.right() - 3.5, tick.top() + 4.5),
                );
                ui.painter().line_segment([a, b], stroke);
                ui.painter().line_segment([b, c], stroke);
            } else {
                ui.painter().rect_stroke(
                    tick,
                    CornerRadius::same(4),
                    Stroke::new(1.5, MUTED()),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
    resp.clicked()
}

const CHANNEL_MENU_W: f32 = 230.0;

/// Whether I may edit channel `cid` of the server on screen.
fn manages_channel(core: &Core, cid: i64) -> bool {
    core.current_server()
        .is_some_and(|s| s.can(P_MANAGE_CHANNELS) && s.channel(cid).is_some())
}

fn channel_menu_height(core: &Core, cid: i64) -> f32 {
    let rows = if manages_channel(core, cid) { 3.0 } else { 2.0 };
    rows * DROP_ROW_H + DROP_LINE_H * (rows - 1.0) + 12.0
}

/// A channel's own menu (a right click on it): read, silence, edit.
fn channel_menu(ui: &mut Ui, core: &Core, cid: i64, acts: &mut Acts) {
    ui.set_width(CHANNEL_MENU_W);
    ui.spacing_mut().item_spacing.y = 0.0;
    if drop_row(ui, None, t!("server.mark_read"), false, Trail::None) {
        acts.later(move |c| {
            c.pop = None;
            c.mark_read(cid);
        });
    }
    drop_line(ui);
    let muted = core.channel_muted(cid);
    if drop_row(ui, None, t!("server.mute_channel"), false, Trail::Check(muted)) {
        acts.later(move |c| c.toggle_mute_channel(cid));
    }
    if manages_channel(core, cid) {
        drop_line(ui);
        if drop_row(ui, None, t!("channel.edit"), false, Trail::Chevron) {
            acts.later(move |c| {
                c.pop = None;
                c.open_edit_channel(cid);
            });
        }
    }
}

const RAIL_MENU_W: f32 = 230.0;

fn rail_menu_height() -> f32 {
    3.0 * DROP_ROW_H + DROP_LINE_H + 12.0
}

/// A server's menu in the rail (a right click on it): read, silence,
/// notifications.
fn rail_menu(ui: &mut Ui, core: &Core, sid: i64, acts: &mut Acts) {
    ui.set_width(RAIL_MENU_W);
    ui.spacing_mut().item_spacing.y = 0.0;
    if drop_row(ui, None, t!("server.mark_read"), false, Trail::None) {
        acts.later(move |c| c.mark_server_read(sid));
    }
    drop_line(ui);
    let muted = core.server_muted(sid);
    if drop_row(ui, None, t!("server.mute_server"), false, Trail::Check(muted)) {
        acts.later(move |c| c.toggle_mute_server(sid));
    }
    if drop_row(ui, None, t!("server.notify_settings"), false, Trail::Chevron) {
        acts.later(move |c| c.open_notify_settings(sid));
    }
}

// ---------------------------------------------------------------- emoji

const EMOJI_COLS: usize = 9;
const EMOJI_CELL: f32 = 36.0;
/// How big an emoji is drawn in its cell.
const EMOJI_GLYPH: f32 = 22.0;
/// A group's name above its emoji.
const EMOJI_HEAD: f32 = 26.0;
/// The tabs, one per group (and the skin tones, in their place).
const EMOJI_TAB: f32 = 32.0;
/// Seven rows and a half: the half says there is more below.
const EMOJI_GRID_H: f32 = 7.5 * EMOJI_CELL;
/// The grid, and room at the right for the scroll bar.
const EMOJI_W: f32 = EMOJI_COLS as f32 * EMOJI_CELL + 10.0;
/// The line under the grid: the name of the emoji under the pointer.
const EMOJI_FOOT: f32 = 30.0;
/// The search, the tabs, the grid and the line with the name, with the
/// gaps.
const EMOJI_H: f32 = 36.0 + EMOJI_TAB + EMOJI_GRID_H + EMOJI_FOOT + 3.0 * 6.0;
/// The hand on the skin tone button, in the tone chosen.
const TONE_HAND: &str = "\u{270B}";
/// The recent ones' tab.
const RECENT_ICON: &str = "\u{1F558}";

/// One emoji of the picker in its cell, lit under the pointer and when it
/// is the one chosen.
fn emoji_cell(ui: &Ui, rect: Rect, id: Id, text: &str, chosen: bool) -> egui::Response {
    let resp = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    if resp.hovered() || chosen {
        let fill = if resp.hovered() { BG3() } else { BG2() };
        ui.painter()
            .rect_filled(rect, CornerRadius::same(RADIUS_SM), fill);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(EMOJI_GLYPH),
        TEXT(),
    );
    resp
}

/// One stretch of the grid: its heading (none for a search's results)
/// and its emoji as they show, in the tone chosen.
struct EmojiSection<'a> {
    label: &'a str,
    items: Vec<&'a str>,
}

/// The emoji picker, for the composer and for reactions: every emoji of
/// the official list this PC draws (emoji.rs), group by group, with a
/// search in Portuguese, the skin tone chosen and the ones used last.
fn emoji_picker(
    ui: &mut Ui,
    core: &mut Core,
    acts: &mut Acts,
    pick: impl Fn(&mut Core, &str) + 'static,
) {
    use crate::emoji::Now;
    ui.set_width(EMOJI_W);
    ui.spacing_mut().item_spacing.y = 6.0;
    let now = core.emoji.now();
    let catalog = match &now {
        Now::Ready(c) => Some(c.clone()),
        _ => None,
    };
    let tone = core.prefs.emoji_tone;
    // An emoji in a tone, even before the list is here.
    let toned = |base: &str, t: u8| -> String {
        match catalog.as_ref().and_then(|c| c.entry_of(base)) {
            Some(e) => e.with_tone(t).to_owned(),
            None if (1..=5).contains(&t) => {
                format!("{base}{}", crate::emoji::MODIFIERS[usize::from(t - 1)])
            }
            None => base.to_owned(),
        }
    };
    let tone_name = |t: u8| -> String {
        match (t, catalog.as_ref()) {
            (0, _) => t!("modal.no_skin_tone").to_owned(),
            (t, Some(c)) => c.tone_names[usize::from(t - 1)].clone(),
            (t, None) => crate::emoji::TONE_NAMES[usize::from(t - 1)].to_owned(),
        }
    };

    // The search, and the skin tone at its right.
    let mut enter = false;
    let mut typed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let edit = ui
            .allocate_ui(vec2(EMOJI_W - 36.0 - 6.0, 36.0), |ui| {
                ui.add(
                    TextEdit::singleline(&mut core.emoji_query)
                        .char_limit(60)
                        .hint_text(egui::RichText::new(t!("modal.search_emoji")).color(FAINT()))
                        .desired_width(f32::INFINITY)
                        .font(theme::body())
                        .margin(Margin::symmetric(12, 9))
                        .background_color(BG2()),
                )
            })
            .inner;
        if edit.has_focus() {
            ui.painter().rect_stroke(
                edit.rect,
                CornerRadius::same(RADIUS_MD),
                Stroke::new(1.0, ACCENT()),
                egui::StrokeKind::Inside,
            );
        }
        if std::mem::take(&mut core.emoji_focus) {
            edit.request_focus();
        }
        enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        typed = edit.changed();
        let (rect, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
        let resp = resp
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(t!("modal.skin_tone"));
        crate::smoke::target(|| "emoji-tons".into(), rect);
        if resp.hovered() || core.emoji_tones_open {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(RADIUS_SM), BG3());
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            toned(TONE_HAND, tone),
            egui::FontId::proportional(20.0),
            TEXT(),
        );
        if resp.clicked() {
            acts.later(|c| c.emoji_tones_open = !c.emoji_tones_open);
        }
    });

    // What the grid shows: a search's results, or the recent ones and
    // then every group.
    let query = core.emoji_query.trim().to_owned();
    let recent = core.prefs.emoji_recent.clone();
    let searching = !query.is_empty() && catalog.is_some();
    let mut tabs: Vec<(String, String, usize)> = Vec::new();
    let mut sections: Vec<EmojiSection> = Vec::new();
    if !recent.is_empty() {
        tabs.push((RECENT_ICON.into(), t!("modal.recent_emoji").into(), 0));
    }
    if let Some(c) = &catalog {
        let first = usize::from(!recent.is_empty());
        for (i, g) in c.groups.iter().enumerate() {
            tabs.push((g.icon.clone(), g.label.clone(), first + i));
        }
    }
    if let (true, Some(c)) = (searching, &catalog) {
        let items = c
            .search(&query)
            .into_iter()
            .map(|i| c.entries[i].with_tone(tone))
            .collect();
        sections.push(EmojiSection { label: "", items });
    } else {
        if !recent.is_empty() {
            sections.push(EmojiSection {
                label: t!("modal.recent_emoji"),
                items: recent.iter().map(String::as_str).collect(),
            });
        }
        if let Some(c) = &catalog {
            for g in &c.groups {
                sections.push(EmojiSection {
                    label: &g.label,
                    items: c.entries[g.range.clone()]
                        .iter()
                        .map(|e| e.with_tone(tone))
                        .collect(),
                });
            }
        }
    }

    // The tabs, or the skin tones in their place.
    let active_id = Id::unique("emoji-picker-active");
    let active: usize = ui.ctx().data(|d| d.get_temp(active_id)).unwrap_or(0);
    ui.horizontal(|ui| {
        ui.set_height(EMOJI_TAB);
        ui.spacing_mut().item_spacing.x = 2.0;
        let cell = |ui: &mut Ui, text: &str, tip: String, on: bool, size: f32| {
            let (rect, resp) = ui.allocate_exact_size(vec2(EMOJI_TAB, EMOJI_TAB), Sense::click());
            let resp = resp
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(tip);
            if on {
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(RADIUS_SM), BG3());
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(RADIUS_SM), BG2());
            }
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(size),
                TEXT(),
            );
            (rect, resp)
        };
        if core.emoji_tones_open {
            for t in 0..=5u8 {
                let (rect, resp) = cell(ui, &toned(TONE_HAND, t), tone_name(t), t == tone, 20.0);
                crate::smoke::target(|| format!("emoji-tom:{t}"), rect);
                if resp.clicked() {
                    acts.later(move |c| c.set_emoji_tone(t));
                }
            }
            return;
        }
        for (icon, label, section) in &tabs {
            let on = !searching && active == *section;
            let size = if on { 19.0 } else { 16.0 };
            let (rect, resp) = cell(ui, icon, label.clone(), on, size);
            if on {
                ui.painter().hline(
                    rect.x_range().shrink(6.0),
                    rect.bottom() - 1.0,
                    Stroke::new(2.0, ACCENT()),
                );
            }
            crate::smoke::target(|| format!("emoji-aba:{label}"), rect);
            if resp.clicked() {
                let section = *section;
                acts.later(move |c| {
                    c.emoji_query.clear();
                    c.emoji_pairs = None;
                    c.emoji_jump = Some(section);
                });
            }
        }
    });

    // Where each section starts, and how tall they all are.
    let mut tops = Vec::with_capacity(sections.len());
    let mut total = 0.0;
    for s in &sections {
        tops.push(total);
        if !s.label.is_empty() {
            total += EMOJI_HEAD;
        }
        total += s.items.len().div_ceil(EMOJI_COLS) as f32 * EMOJI_CELL;
    }
    let jump = core.emoji_jump.take().and_then(|s| tops.get(s).copied());

    // An emoji of two people, a tone for each (a right click on it, in the
    // grid or among a search's results). Typing takes the grid back.
    if typed && core.emoji_pairs.is_some() {
        acts.later(|c| c.emoji_pairs = None);
    }
    let pairs_of = catalog
        .as_ref()
        .zip(core.emoji_pairs.as_deref())
        .and_then(|(c, t)| c.entry_of(t))
        .filter(|e| e.pairs.is_some());

    let mut hovered: Option<String> = None;
    let mut picked: Option<String> = None;
    if let Some(entry) = pairs_of {
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), EMOJI_GRID_H), Sense::hover());
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), EMOJI_HEAD));
        ui.painter().text(
            pos2(head.left() + 6.0, head.center().y),
            egui::Align2::LEFT_CENTER,
            t!("modal.tone_per_person_heading"),
            theme::bold(11.0),
            FAINT(),
        );
        let mut back = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(head.shrink2(vec2(8.0, 0.0)))
                .layout(Layout::right_to_left(Align::Center)),
        );
        let resp = widgets::link(&mut back, t!("modal.back"));
        crate::smoke::target(|| "emoji-voltar".into(), resp.rect);
        if resp.clicked() {
            acts.later(|c| c.emoji_pairs = None);
        }
        // The one picked last, or both people in the tone chosen.
        let chosen = recent
            .iter()
            .find(|r| *r != &entry.text && entry.pair_of(r).is_some())
            .map(String::as_str)
            .or_else(|| entry.pair(tone, tone));
        // The first person's tone by row, the second's by column.
        let side = 5.0 * EMOJI_CELL;
        let left = rect.left() + (rect.width() - side) / 2.0;
        let top = head.bottom() + 4.0;
        for a in 1..=5u8 {
            for b in 1..=5u8 {
                let Some(text) = entry.pair(a, b) else {
                    continue;
                };
                let cell = Rect::from_min_size(
                    pos2(
                        left + f32::from(b - 1) * EMOJI_CELL,
                        top + f32::from(a - 1) * EMOJI_CELL,
                    ),
                    egui::Vec2::splat(EMOJI_CELL),
                );
                let resp = emoji_cell(
                    ui,
                    cell,
                    Id::unique(("emoji-par", a, b)),
                    text,
                    chosen == Some(text),
                );
                crate::smoke::target(|| format!("emoji-par:{text}"), cell);
                if resp.hovered() {
                    hovered = Some(text.to_owned());
                }
                if resp.clicked() {
                    picked = Some(text.to_owned());
                }
            }
        }
        // And without a tone.
        let cell = Rect::from_min_size(pos2(left, top + side + 6.0), egui::Vec2::splat(EMOJI_CELL));
        let resp = emoji_cell(ui, cell, Id::unique("emoji-par-sem"), &entry.text, false);
        crate::smoke::target(|| format!("emoji-par:{}", entry.text), cell);
        ui.painter().text(
            pos2(cell.right() + 8.0, cell.center().y),
            egui::Align2::LEFT_CENTER,
            t!("modal.no_skin_tone"),
            theme::small(),
            MUTED(),
        );
        if resp.hovered() {
            hovered = Some(entry.text.clone());
        }
        if resp.clicked() {
            picked = Some(entry.text.clone());
        }
    } else if sections.iter().all(|s| s.items.is_empty()) {
        let text = match &now {
            _ if searching => tf!("modal.no_emoji_found", query = query),
            // What went wrong is in the log; here, what to do.
            Now::Failed(_) => t!("modal.emoji_download_failed").to_owned(),
            _ => t!("modal.loading_emoji").to_owned(),
        };
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), EMOJI_GRID_H), Sense::hover());
        let mut middle = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(24.0, 0.0)))
                .layout(Layout::top_down(Align::Center)),
        );
        middle.add_space(rect.height() / 2.0 - 24.0);
        if matches!(now, Now::Loading) && !searching {
            middle.add(egui::Spinner::new().size(18.0).color(ACCENT()));
            middle.add_space(6.0);
        }
        middle.add(
            egui::Label::new(egui::RichText::new(text).color(MUTED()))
                .halign(Align::Center)
                .wrap(),
        );
    } else {
        let mut area = ScrollArea::vertical()
            .id_salt(("emoji-grid", searching))
            .auto_shrink([false, false])
            .max_height(EMOJI_GRID_H)
            .min_scrolled_height(EMOJI_GRID_H);
        if let Some(y) = jump {
            area = area.vertical_scroll_offset(y);
        }
        area.show_viewport(ui, |ui, viewport| {
            ui.set_height(total);
            // The content's own corner: rows go `y` below it.
            let (left, top) = (ui.max_rect().left(), ui.max_rect().top());
            let mut current = 0;
            for (si, s) in sections.iter().enumerate() {
                let mut y = tops[si];
                if y <= viewport.top() + 1.0 {
                    current = si;
                }
                if !s.label.is_empty() {
                    if y + EMOJI_HEAD >= viewport.top() && y <= viewport.bottom() {
                        ui.painter().text(
                            pos2(left + 6.0, top + y + EMOJI_HEAD / 2.0),
                            egui::Align2::LEFT_CENTER,
                            s.label.to_uppercase(),
                            theme::bold(11.0),
                            FAINT(),
                        );
                    }
                    y += EMOJI_HEAD;
                }
                let rows = s.items.len().div_ceil(EMOJI_COLS);
                let first = ((viewport.top() - y) / EMOJI_CELL).floor().max(0.0) as usize;
                let last =
                    (((viewport.bottom() - y) / EMOJI_CELL).ceil().max(0.0) as usize).min(rows);
                for row in first..last {
                    for col in 0..EMOJI_COLS {
                        let at = row * EMOJI_COLS + col;
                        let Some(item) = s.items.get(at) else { break };
                        let cell = Rect::from_min_size(
                            pos2(
                                left + col as f32 * EMOJI_CELL,
                                top + y + row as f32 * EMOJI_CELL,
                            ),
                            egui::Vec2::splat(EMOJI_CELL),
                        );
                        let resp =
                            emoji_cell(ui, cell, Id::unique(("emoji-cell", si, at)), item, false);
                        crate::smoke::target(|| format!("emoji:{item}"), cell);
                        // Two people: a tone for each, on a right click.
                        let two = catalog
                            .as_ref()
                            .and_then(|c| c.entry_of(item))
                            .filter(|e| e.pairs.is_some());
                        let resp = match two {
                            Some(_) => resp.on_hover_text(t!("modal.right_click_tones")),
                            None => resp,
                        };
                        if resp.hovered() {
                            hovered = Some((*item).to_owned());
                        }
                        if resp.clicked() {
                            picked = Some((*item).to_owned());
                        }
                        if let Some(e) = two
                            && resp.secondary_clicked()
                        {
                            let base = e.text.clone();
                            acts.later(move |c| c.emoji_pairs = Some(base));
                        }
                    }
                }
            }
            ui.ctx().data_mut(|d| d.insert_temp(active_id, current));
        });
    }

    // Under the grid: the emoji under the pointer, or where the list came
    // from (or why it did not).
    ui.horizontal(|ui| {
        ui.set_height(EMOJI_FOOT);
        ui.spacing_mut().item_spacing.x = 8.0;
        match (&hovered, &now) {
            (Some(e), _) => {
                ui.label(egui::RichText::new(e.as_str()).size(20.0));
                let name = catalog
                    .as_ref()
                    .and_then(|c| c.name_of(e))
                    .unwrap_or_default();
                widgets::ellipsis(ui, &name, theme::body(), TEXT());
            }
            (None, Now::Failed(_)) => {
                ui.label(
                    egui::RichText::new(t!("modal.emoji_list_failed"))
                        .size(12.0)
                        .color(DANGER),
                );
                if widgets::link(ui, t!("modal.try_again")).clicked() {
                    acts.later(|c| c.emoji.want());
                }
            }
            (None, Now::Loading) => {
                ui.label(
                    egui::RichText::new(t!("modal.loading_emoji_list"))
                        .size(12.0)
                        .color(FAINT()),
                );
            }
            (None, Now::Ready(c)) => {
                ui.label(
                    egui::RichText::new(tf!("modal.emoji_version", version = c.version))
                        .size(12.0)
                        .color(FAINT()),
                );
            }
        }
    });

    // Enter picks the first one found (not with the tones of one on show).
    if enter
        && searching
        && pairs_of.is_none()
        && let Some(first) = sections.first().and_then(|s| s.items.first())
    {
        picked = Some((*first).to_owned());
    }
    if let Some(e) = picked {
        acts.later(move |c| {
            c.used_emoji(&e);
            pick(c, &e);
        });
    }
}

// ---------------------------------------------------------------- stickers

const STICKER_COLS: usize = 4;
const STICKER_CELL: f32 = 80.0;
const STICKER_GAP: f32 = 4.0;
/// Three rows and a half: the half says there is more below.
const STICKER_GRID_H: f32 = 3.5 * (STICKER_CELL + STICKER_GAP);
/// The grid, and room at the right for the scroll bar.
const STICKER_W: f32 = STICKER_COLS as f32 * (STICKER_CELL + STICKER_GAP) - STICKER_GAP + 10.0;
/// How tall GIPHY's mark is at the bottom: its small "POWERED BY" still
/// reads.
const STICKER_MARK_H: f32 = 20.0;
/// Search, tabs, grid and the line with GIPHY's mark, with the gaps.
const STICKER_H: f32 = 38.0 + 26.0 + STICKER_GRID_H + STICKER_MARK_H + 3.0 * 6.0;

/// A search goes to GIPHY this long after the last key (it counts every
/// call), and once it has two letters.
const SEARCH_AFTER: Duration = Duration::from_millis(400);
const SEARCH_MIN: usize = 2;

/// The sticker picker: a search on GIPHY, what is trending there and my
/// favourites. GIPHY's results show as they come, in its order.
fn sticker_picker(ui: &mut Ui, core: &mut Core, acts: &mut Acts) {
    ui.set_width(STICKER_W);
    ui.spacing_mut().item_spacing.y = 6.0;
    // The dark input box, a shade lighter: the picker is as dark as it.
    let edit = ui.add(
        TextEdit::singleline(&mut core.sticker_query)
            .char_limit(giphy::MAX_QUERY)
            .hint_text(egui::RichText::new(t!("modal.search_stickers")).color(FAINT()))
            .desired_width(f32::INFINITY)
            .font(theme::body())
            .margin(Margin::symmetric(12, 9))
            .background_color(BG2()),
    );
    if edit.has_focus() {
        ui.painter().rect_stroke(
            edit.rect,
            CornerRadius::same(RADIUS_MD),
            Stroke::new(1.0, ACCENT()),
            egui::StrokeKind::Inside,
        );
    }
    if std::mem::take(&mut core.sticker_focus) {
        edit.request_focus();
    }
    if edit.changed() {
        core.sticker_typed = Some(Instant::now());
    }
    let query = core.sticker_query.trim().to_owned();
    let searching = query.chars().count() >= SEARCH_MIN;
    let wait = core.sticker_typed.map_or(Duration::ZERO, |at| {
        SEARCH_AFTER.saturating_sub(at.elapsed())
    });
    if wait.is_zero() {
        core.sticker_typed = None;
    } else {
        ui.ctx().request_repaint_after(wait);
    }
    let typing = searching && core.sticker_typed.is_some();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (tab, label) in [
            (StickerTab::Trending, t!("modal.trending")),
            (StickerTab::Favorites, t!("modal.favorites")),
        ] {
            if picker_tab(ui, label, !searching && core.sticker_tab == tab) {
                acts.later(move |c| {
                    c.sticker_tab = tab;
                    c.sticker_query.clear();
                    c.sticker_typed = None;
                });
            }
        }
    });

    // GIPHY's feed for the search or for trending; the favourites are mine.
    let feed = if searching {
        Some(query.clone())
    } else if core.sticker_tab == StickerTab::Trending {
        Some(String::new())
    } else {
        None
    };
    let view = match &feed {
        Some(_) if typing => FeedView {
            loading: true,
            ..FeedView::default()
        },
        Some(q) => {
            core.giphy.show(q);
            core.giphy.feed(q)
        }
        None => FeedView {
            ids: core.stickers.clone(),
            done: true,
            ..FeedView::default()
        },
    };

    let rows = view.ids.len().div_ceil(STICKER_COLS);
    if rows == 0 {
        let text = if view.loading {
            t!("modal.loading").to_owned()
        } else if let Some(e) = &view.error {
            e.clone()
        } else if feed.is_none() {
            t!("modal.no_favorites").to_owned()
        } else if searching {
            tf!("modal.no_stickers_found", query = query)
        } else {
            t!("modal.nothing_trending").to_owned()
        };
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), STICKER_GRID_H), Sense::hover());
        let mut middle = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(24.0, 0.0)))
                .layout(Layout::top_down(Align::Center)),
        );
        middle.add_space(rect.height() / 2.0 - 24.0);
        middle.add(
            egui::Label::new(egui::RichText::new(text).color(MUTED()))
                .halign(Align::Center)
                .wrap(),
        );
    } else {
        let mut near_end = false;
        let core_ref: &Core = core;
        ui.scope(|ui| {
            // show_rows reads the gap between rows from here.
            ui.spacing_mut().item_spacing.y = STICKER_GAP;
            ScrollArea::vertical()
                .id_salt(("sticker-grid", &feed))
                .auto_shrink([false, false])
                .max_height(STICKER_GRID_H)
                .min_scrolled_height(STICKER_GRID_H)
                .show_rows(ui, STICKER_CELL, rows, |ui, range| {
                    near_end = range.end >= rows;
                    for row in range {
                        let (line, _) = ui.allocate_exact_size(
                            vec2(ui.available_width(), STICKER_CELL),
                            Sense::hover(),
                        );
                        for col in 0..STICKER_COLS {
                            let Some(id) = view.ids.get(row * STICKER_COLS + col) else {
                                break;
                            };
                            let x = line.left() + col as f32 * (STICKER_CELL + STICKER_GAP);
                            let cell = Rect::from_min_size(
                                pos2(x, line.top()),
                                egui::Vec2::splat(STICKER_CELL),
                            );
                            sticker_tile(ui, core_ref, id, cell, acts);
                        }
                    }
                });
        });
        if near_end
            && !view.done
            && let Some(q) = &feed
        {
            core.giphy.more(q);
        }
    }

    ui.horizontal(|ui| {
        ui.set_height(STICKER_MARK_H);
        if let (Some(e), Some(q)) = (&view.error, &feed) {
            // With nothing on show, the error is in the middle already.
            if rows > 0 {
                ui.label(egui::RichText::new(e).size(12.0).color(DANGER));
            }
            if widgets::link(ui, t!("modal.try_again")).clicked() {
                let q = q.clone();
                acts.later(move |c| c.giphy.retry(&q));
            }
        } else if view.loading && rows > 0 {
            ui.label(
                egui::RichText::new(t!("modal.loading"))
                    .size(12.0)
                    .color(FAINT()),
            );
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // GIPHY's terms: its words and its logo wherever its search is.
            match giphy::mark(ui.ctx(), STICKER_MARK_H) {
                Some((texture, size)) => {
                    ui.add(
                        egui::Image::from_texture(egui::load::SizedTexture::new(
                            texture.id(),
                            size,
                        ))
                        .alt_text("Powered by GIPHY"),
                    );
                }
                None => {
                    ui.label(
                        egui::RichText::new("Powered by GIPHY")
                            .font(theme::bold(11.0))
                            .color(MUTED()),
                    );
                }
            }
        });
    });
}

/// One sticker of the picker: a click sends it; hovering shows its name
/// and the star that keeps it among the favourites.
fn sticker_tile(ui: &mut Ui, core: &Core, id: &str, cell: Rect, acts: &mut Acts) {
    let inner = STICKER_CELL - 8.0;
    let resp = ui
        .interact(cell, Id::unique(("sticker-tile", id)), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    if resp.hovered() {
        ui.painter()
            .rect_filled(cell, CornerRadius::same(RADIUS_SM), BG3());
    }
    let placeholder = |ui: &Ui, rect: Rect| {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(RADIUS_SM), BG1());
    };
    match core.giphy.sticker(id) {
        Lookup::Ready(st) => {
            let size = giphy::fit(st.small.size, inner);
            let rect = Rect::from_center_size(cell.center(), size);
            match core.giphy.anim(&st.small, inner) {
                Frames::Ready(anim) => {
                    giphy::paint(ui, &anim, rect, core.stickers_move());
                    core.giphy.seen(id);
                }
                Frames::Loading => placeholder(ui, rect),
                Frames::Failed => {
                    placeholder(ui, rect);
                    ui.painter().text(
                        cell.center(),
                        egui::Align2::CENTER_CENTER,
                        t!("modal.unavailable"),
                        theme::tiny(),
                        FAINT(),
                    );
                }
            }
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, st.label()));
            // The title and who made it (GIPHY asks that creators be named).
            if resp.on_hover_text(st.about()).clicked() {
                let id = id.to_owned();
                acts.later(move |c| c.send_sticker(&id));
            }
        }
        Lookup::Loading => placeholder(ui, cell.shrink(4.0)),
        Lookup::Failed => {
            placeholder(ui, cell.shrink(4.0));
            ui.painter().text(
                cell.center(),
                egui::Align2::CENTER_CENTER,
                t!("modal.failed_to_load"),
                theme::tiny(),
                FAINT(),
            );
        }
        Lookup::Missing | Lookup::Off => {
            ui.painter().text(
                cell.center(),
                egui::Align2::CENTER_CENTER,
                t!("modal.unavailable"),
                theme::tiny(),
                FAINT(),
            );
        }
    }
    if ui.rect_contains_pointer(cell) {
        let kept = core.stickers.iter().any(|s| s == id);
        let star = Rect::from_min_size(
            pos2(cell.right() - 26.0, cell.top() + 2.0),
            egui::Vec2::splat(24.0),
        );
        let star_id = Id::unique(("sticker-tile-star", id));
        if super::chat::star_button(ui, star, star_id, kept).clicked() {
            let id = id.to_owned();
            acts.later(move |c| c.toggle_favorite(&id));
        }
    }
}

/// A tab of the sticker picker.
fn picker_tab(ui: &mut Ui, label: &str, on: bool) -> bool {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme::bold(13.0), TEXT());
    let size = galley.size() + vec2(18.0, 8.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let radius = CornerRadius::same(RADIUS_SM);
    let fg = if on {
        ui.painter().rect_filled(rect, radius, BG4());
        TEXT()
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, radius, BG3());
        TEXT()
    } else {
        MUTED()
    };
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, fg);
    resp.clicked()
}
