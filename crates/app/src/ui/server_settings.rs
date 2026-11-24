//! A server's settings, as Discord's: a screen of their own over the whole
//! window, the sections in a menu on the left, the open one in the middle,
//! and for the server's profile a card on the right with how it looks.
//! Each section is shown only to whoever may use it.
//!
//! Also here: the two small dialogs of the server menu that are mine and
//! not the server's (its notifications, my nickname in it).

use egui::{
    Align, Align2, Color32, CornerRadius, CursorIcon, Frame, Id, Layout, Margin, Rect, ScrollArea,
    Sense, Stroke, Ui, Vec2, pos2, vec2,
};
use serde_json::json;

use super::Acts;
use super::modals::{reports, requests, submitted};
use super::widgets::{self, Button};
use crate::core::{Action, Core, ErrTo, FormId, Modal, Pick, RoleEditor, ServerTab, Then};
use crate::icons::Icon;
use crate::model::*;
use crate::net::Req;
use crate::prefs::NotifyLevel;
use crate::theme::{self, *};
use crate::{t, tf};

/// Whether `tab` is shown to me in server `s`.
pub fn shown(s: &Server, tab: ServerTab) -> bool {
    match tab {
        ServerTab::Overview => s.can(P_MANAGE_SERVER),
        ServerTab::Members => s.can(P_KICK) || s.can(P_BAN) || s.can(P_MANAGE_ROLES),
        ServerTab::Roles => s.can(P_MANAGE_ROLES) && !s.roles.is_empty(),
        ServerTab::Invites => s.can(P_INVITE) && s.can(P_MANAGE_SERVER),
        ServerTab::Access => s.can(P_MANAGE_SERVER) || s.can(P_KICK),
        ServerTab::Bans => s.can(P_BAN),
        ServerTab::Reports => s.can(P_MANAGE_MESSAGES),
    }
}

/// The menu's width, the content's, and the profile card's.
const NAV_W: f32 = 232.0;
const CONTENT_W: f32 = 580.0;
const CARD_W: f32 = 300.0;

/// The colours "Faixa" offers: the server's colour, which is also its
/// picture's background when it has none.
const BANNERS: [&str; 10] = [
    "#2b2b3a", "#ff2e93", "#e8262a", "#f07a1f", "#ecc232", "#9b4dca", "#1aa3ff", "#3de0cf",
    "#4a8c14", "#5c5c66",
];

/// The screen, drawn over everything in place of the usual dialog.
pub fn draw(ctx: &egui::Context, core: &mut Core, m: &mut Modal, acts: &mut Acts) {
    let Modal::ServerSettings {
        sid,
        name,
        new_role,
        role,
        tab,
        search,
        member,
    } = m
    else {
        return;
    };
    let sid = *sid;
    let Some(s) = core.server(sid) else {
        acts.later(Core::close_modal);
        return;
    };
    // A section I may no longer see (a role taken away meanwhile).
    if !shown(s, *tab) {
        match ServerTab::ALL.into_iter().find(|t| shown(s, *t)) {
            Some(t) => *tab = t,
            None => {
                acts.later(Core::close_modal);
                return;
            }
        }
    }
    let full = ctx.content_rect();
    let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
    let top = if fullscreen { 0.0 } else { TITLEBAR_H };
    let area = Rect::from_min_max(pos2(full.left(), full.top() + top), full.max);
    let id = Id::unique("server-page");
    let resp = egui::Modal::new(id)
        .area(egui::Modal::default_area(id).fixed_pos(area.min))
        .frame(Frame::new().fill(BG2()))
        .backdrop_color(Color32::TRANSPARENT)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(area.size(), Sense::hover());
            // The left, a shade darker, up to the menu's right edge.
            // The menu ends at a third of the width, or further left when
            // the section and the card would not fit beside it.
            let fits = rect.width() - (56.0 + CONTENT_W + 40.0 + CARD_W + 100.0);
            let nav_right =
                rect.left() + (rect.width() * 0.34).min(fits).max(NAV_W + 24.0);
            let side = Rect::from_min_max(rect.min, pos2(nav_right + 12.0, rect.bottom()));
            ui.painter().rect_filled(side, CornerRadius::ZERO, BG1());
            let nav = Rect::from_min_max(
                pos2(nav_right - NAV_W, rect.top() + 48.0),
                pos2(nav_right, rect.bottom() - 12.0),
            );
            let mut menu = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(nav)
                    .layout(Layout::top_down(Align::Min)),
            );
            ScrollArea::vertical()
                .id_salt("server-page-nav")
                .auto_shrink([false, false])
                .show(&mut menu, |ui| nav_menu(ui, core, sid, tab, acts));

            // The way out, at the top right, as Discord's: a ring and "ESC".
            let close_c = pos2(rect.right() - 60.0, rect.top() + 66.0);
            let ring = Rect::from_center_size(close_c, vec2(36.0, 36.0));
            let x = ui
                .interact(ring, Id::unique("server-page-close"), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);
            crate::smoke::target(|| "fechar-pagina".into(), ring);
            ui.painter().circle_stroke(
                close_c,
                17.0,
                Stroke::new(2.0, if x.hovered() { TEXT() } else { MUTED() }),
            );
            Icon::X
                .image(16.0, if x.hovered() { TEXT() } else { MUTED() })
                .paint_at(ui, Rect::from_center_size(close_c, vec2(16.0, 16.0)));
            ui.painter().text(
                pos2(close_c.x, ring.bottom() + 12.0),
                Align2::CENTER_CENTER,
                "ESC",
                theme::bold(11.0),
                MUTED(),
            );
            if x.clicked() {
                acts.later(Core::close_modal);
            }

            // The section, and for the profile the card at its right.
            let left = nav_right + 56.0;
            let room = ring.left() - 24.0 - left;
            let with_card = *tab == ServerTab::Overview && room >= 420.0 + 40.0 + CARD_W;
            let content_w = if with_card {
                (room - 40.0 - CARD_W).min(CONTENT_W)
            } else {
                room.min(CONTENT_W + 160.0)
            };
            let body = Rect::from_min_max(
                pos2(left, rect.top() + 48.0),
                pos2(left + content_w, rect.bottom()),
            );
            let dirty = *tab == ServerTab::Overview
                && core
                    .server(sid)
                    .is_some_and(|s| name.trim() != s.name && !name.trim().is_empty());
            let mut content = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(body)
                    .layout(Layout::top_down(Align::Min)),
            );
            let mut scroll = ScrollArea::vertical()
                .id_salt(("server-page-body", tab.key()))
                .auto_shrink([false, false]);
            if let Some(y) = core.modal_scroll.take() {
                scroll = scroll.vertical_scroll_offset(y);
            }
            scroll.show(&mut content, |ui| {
                ui.set_max_width(content_w);
                ui.spacing_mut().item_spacing.y = 4.0;
                match *tab {
                    ServerTab::Overview => overview(ui, core, sid, name, acts),
                    ServerTab::Members => members(ui, core, sid, search, member, acts),
                    ServerTab::Roles => roles(ui, core, sid, new_role, role, acts),
                    ServerTab::Invites => invites(ui, core, sid, acts),
                    ServerTab::Access => access(ui, core, sid, acts),
                    ServerTab::Bans => bans(ui, core, sid, acts),
                    ServerTab::Reports => {
                        title(ui, t!("modal.reports_title"), t!("server.reports_hint"));
                        reports(ui, core, sid, acts, true);
                    }
                }
                ui.add_space(if dirty { 90.0 } else { 40.0 });
            });
            if with_card {
                let card = Rect::from_min_size(pos2(body.right() + 40.0, body.top()), vec2(CARD_W, 280.0));
                profile_card(ui, core, sid, name, card);
            }
            // Changes not saved yet: the bar at the bottom, as Discord's.
            if dirty {
                save_bar(ui, core, sid, name, body, acts);
            }
        });
    if resp.should_close() {
        acts.later(Core::close_modal);
    }
}

type NavItem<'a> = (ServerTab, Icon, &'a str, usize);

/// The menu: the sections in groups, the first under the server's name,
/// then deleting the server.
fn nav_menu(ui: &mut Ui, core: &Core, sid: i64, tab: &mut ServerTab, acts: &mut Acts) {
    let Some(s) = core.server(sid) else { return };
    ui.spacing_mut().item_spacing.y = 2.0;
    let groups: [(&str, Vec<NavItem>); 3] = [
        (
            s.name.as_str(),
            vec![(ServerTab::Overview, Icon::Info, t!("server.tab_overview"), 0)],
        ),
        (
            t!("server.nav_people"),
            vec![
                (ServerTab::Members, Icon::Users, t!("server.tab_members"), 0),
                (ServerTab::Roles, Icon::Shield, t!("modal.roles_title"), 0),
                (ServerTab::Invites, Icon::UserPlus, t!("server.tab_invites"), 0),
                (
                    ServerTab::Access,
                    Icon::Link,
                    t!("server.tab_access"),
                    if s.can(P_KICK) { s.requests.len() } else { 0 },
                ),
            ],
        ),
        (
            t!("server.nav_moderation"),
            vec![
                (ServerTab::Bans, Icon::Ban, t!("modal.bans_title"), 0),
                (
                    ServerTab::Reports,
                    Icon::Flag,
                    t!("modal.reports_title"),
                    s.reports.len(),
                ),
            ],
        ),
    ];
    let mut first = true;
    for (label, items) in groups {
        let items: Vec<&NavItem> = items.iter().filter(|i| shown(s, i.0)).collect();
        if items.is_empty() {
            continue;
        }
        if !first {
            line(ui);
        }
        first = false;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(label)
                    .font(theme::body())
                    .size(12.5)
                    .color(MUTED()),
            );
        });
        ui.add_space(2.0);
        for (which, icon, text, count) in items {
            let item = item(ui, *icon, text, *tab == *which, false);
            crate::smoke::target(|| format!("servidor-config:{}", which.key()), item.rect);
            if *count > 0 {
                widgets::badge_at(
                    ui,
                    pos2(item.rect.right() - 16.0, item.rect.center().y),
                    *count as u32,
                    None,
                );
            }
            if item.clicked() {
                *tab = *which;
            }
        }
        ui.add_space(6.0);
    }
    if s.owner_id == core.me_id() {
        line(ui);
        ui.add_space(4.0);
        if item(ui, Icon::Trash, t!("modal.delete_server"), false, true).clicked() {
            acts.later(move |c| {
                c.confirm(
                    t!("modal.delete_server"),
                    t!("modal.delete_server_body"),
                    t!("modal.delete_server"),
                    Action::DeleteServer(sid),
                )
            });
        }
    }
}

fn line(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 13.0), Sense::hover());
    ui.painter().hline(
        r.x_range().shrink(8.0),
        r.center().y,
        Stroke::new(1.0, BG3()),
    );
}

/// One section in the menu, lit when it is open.
fn item(ui: &mut Ui, icon: Icon, text: &str, on: bool, danger: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if on {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(RADIUS_MD), BG3());
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(RADIUS_MD), BG2());
    }
    let ink = match (danger, on || resp.hovered()) {
        (true, _) => DANGER,
        (false, true) => TEXT(),
        (false, false) => MUTED(),
    };
    icon.image(18.0, ink).paint_at(
        ui,
        Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(18.0, 18.0)),
    );
    ui.painter().text(
        pos2(rect.left() + 36.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        if on { theme::bold(14.5) } else { theme::body() },
        ink,
    );
    resp
}

/// A section's title, with a line under it saying what it is for.
fn title(ui: &mut Ui, text: &str, hint: &str) {
    ui.label(egui::RichText::new(text).font(theme::bold(20.0)).color(TEXT()));
    if !hint.is_empty() {
        ui.add_space(2.0);
        ui.label(egui::RichText::new(hint).size(13.0).color(TEXT().gamma_multiply(0.85)));
    }
    ui.add_space(18.0);
}

/// A part of a section: its name and, under it, what it does.
fn part(ui: &mut Ui, text: &str, hint: &str) {
    ui.label(egui::RichText::new(text).font(theme::bold(15.0)).color(TEXT()));
    if !hint.is_empty() {
        ui.label(egui::RichText::new(hint).size(13.0).color(MUTED()));
    }
    ui.add_space(6.0);
}

fn rule(ui: &mut Ui) {
    ui.add_space(18.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(r.x_range(), r.center().y, Stroke::new(1.0, BG4()));
    ui.add_space(18.0);
}

// ---------------------------------------------------------------- the server

fn overview(ui: &mut Ui, core: &Core, sid: i64, name: &mut String, acts: &mut Acts) {
    let Some(s) = core.server(sid) else { return };
    title(ui, t!("server.tab_overview"), t!("server.overview_hint"));
    part(ui, t!("server.name"), "");
    let r = widgets::input(ui, name, 100, false, t!("modal.server_name"));
    crate::smoke::target(|| "nome-servidor".into(), r.rect);
    widgets::error_line(ui, core.errors.get(&FormId::RenameServer));
    if submitted(ui, &r) && name.trim() != s.name && !name.trim().is_empty() {
        let n = name.trim().to_owned();
        acts.later(move |c| rename(c, sid, n));
    }
    rule(ui);
    part(ui, t!("server.icon"), t!("server.icon_hint"));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if Button::primary(t!("server.change_icon")).small().show(ui).clicked() {
            acts.later(move |c| c.pick_files(Pick::ServerIcon(sid)));
        }
        if s.icon.is_some()
            && Button::danger(t!("server.remove_icon"))
                .small()
                .show(ui)
                .clicked()
        {
            acts.later(move |c| {
                c.call(
                    Req::patch(format!("/servers/{sid}")).with(json!({ "icon": null })),
                    Then::Nothing,
                    ErrTo::Toast,
                )
            });
        }
    });
    rule(ui);
    part(ui, t!("server.banner"), t!("server.banner_hint"));
    ui.add_space(4.0);
    let gap = 8.0;
    let w = ((ui.available_width() - 4.0 * gap) / 5.0).floor();
    for row in BANNERS.chunks(5) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for hex in row {
                let (r, resp) = ui.allocate_exact_size(vec2(w, 62.0), Sense::click());
                let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
                crate::smoke::target(|| format!("faixa:{hex}"), r);
                let c = theme::parse_hex(hex).unwrap_or(ACCENT());
                paint_banner(ui, r, c, 8, true);
                let on = s.color.eq_ignore_ascii_case(hex);
                if on || resp.hovered() {
                    ui.painter().rect_stroke(
                        r.expand(2.0),
                        CornerRadius::same(10),
                        Stroke::new(2.0, if on { TEXT() } else { MUTED() }),
                        egui::StrokeKind::Outside,
                    );
                }
                if resp.clicked() && !on {
                    let color = (*hex).to_owned();
                    acts.later(move |c| {
                        c.call(
                            Req::patch(format!("/servers/{sid}")).with(json!({ "color": color })),
                            Then::Nothing,
                            ErrTo::Toast,
                        )
                    });
                }
            }
        });
        ui.add_space(gap);
    }
}

fn rename(c: &mut Core, sid: i64, name: String) {
    c.call(
        Req::patch(format!("/servers/{sid}")).with(json!({ "name": name })),
        Then::Toast(t!("modal.name_saved").into()),
        ErrTo::Form(FormId::RenameServer),
    );
}

/// A band of colour, lighter at the bottom, as the banners are; its top
/// corners rounded by `radius` and, for a tile (`round_bottom`), the bottom
/// ones too.
fn paint_banner(ui: &Ui, r: Rect, c: Color32, radius: u8, round_bottom: bool) {
    let light = c.lerp_to_gamma(Color32::WHITE, 0.28);
    let rad = f32::from(radius);
    // The rounded ends in the colour each end has, the gradient between.
    if radius > 0 {
        let top = Rect::from_min_size(r.min, vec2(r.width(), rad * 2.0));
        ui.painter().rect_filled(top, CornerRadius::same(radius), c);
        if round_bottom {
            let bottom = Rect::from_min_max(pos2(r.left(), r.bottom() - rad * 2.0), r.max);
            ui.painter().rect_filled(bottom, CornerRadius::same(radius), light);
        }
    }
    let inner = Rect::from_min_max(
        pos2(r.left(), r.top() + rad),
        pos2(r.right(), if round_bottom { r.bottom() - rad } else { r.bottom() }),
    );
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(inner.left_top(), c);
    mesh.colored_vertex(inner.right_top(), c);
    mesh.colored_vertex(inner.right_bottom(), light);
    mesh.colored_vertex(inner.left_bottom(), light);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(mesh);
}

/// How the server looks to someone given an invite: its colour, its
/// picture, its name, who is here, and since when it exists.
fn profile_card(ui: &mut Ui, core: &Core, sid: i64, name: &str, card: Rect) {
    let Some(s) = core.server(sid) else { return };
    let painter = ui.painter();
    painter.rect_filled(card, CornerRadius::same(12), BG1());
    painter.rect_stroke(
        card,
        CornerRadius::same(12),
        Stroke::new(1.0, BG3()),
        egui::StrokeKind::Inside,
    );
    let band = Rect::from_min_size(card.min, vec2(card.width(), 120.0));
    let color = theme::parse_hex(&s.color).unwrap_or(ACCENT());
    paint_banner(ui, band, color, 12, false);
    let icon = Rect::from_min_size(pos2(card.left() + 16.0, band.bottom() - 36.0), vec2(72.0, 72.0));
    ui.painter()
        .rect_filled(icon.expand(4.0), CornerRadius::same(22), BG1());
    widgets::paint_server(
        ui,
        core,
        s.icon.as_deref(),
        &s.initials(),
        icon,
        20,
        theme::color_or(&s.color, ACCENT()),
        ACCENT_INK(),
    );
    let mut y = icon.bottom() + 14.0;
    let shown = if name.trim().is_empty() { &s.name } else { name.trim() };
    ui.painter().text(
        pos2(card.left() + 16.0, y),
        Align2::LEFT_TOP,
        shown,
        theme::bold(17.0),
        TEXT(),
    );
    y += 26.0;
    let online = s
        .members
        .iter()
        .filter(|m| m.status.public() != Status::Offline)
        .count();
    let x = card.left() + 16.0;
    ui.painter().circle_filled(pos2(x + 4.0, y + 8.0), 4.0, ONLINE);
    let g = ui
        .painter()
        .layout_no_wrap(tf!("server.online_n", n = online), theme::small(), MUTED());
    let w = g.size().x;
    ui.painter().galley(pos2(x + 12.0, y), g, MUTED());
    let x2 = x + 12.0 + w + 12.0;
    ui.painter().circle_filled(pos2(x2 + 4.0, y + 8.0), 4.0, FAINT());
    ui.painter().text(
        pos2(x2 + 12.0, y),
        Align2::LEFT_TOP,
        tf!("server.members_n", n = s.members.len()),
        theme::small(),
        MUTED(),
    );
    y += 22.0;
    if let Some(since) = since(s.created) {
        ui.painter().text(
            pos2(x, y),
            Align2::LEFT_TOP,
            since,
            theme::small(),
            MUTED(),
        );
    }
}

/// "Desde nov. de 2026": when the server was made.
fn since(ms: i64) -> Option<String> {
    use chrono::{Datelike, Local, TimeZone};
    if ms <= 0 {
        return None;
    }
    let d = Local.timestamp_millis_opt(ms).single()?;
    let month = crate::lang::lookup(&format!("month.{}", d.month())).unwrap_or("");
    Some(tf!("server.since", month = month, year = d.year()))
}

/// The bar that floats at the bottom while the name is changed and not
/// saved: undo, or save.
fn save_bar(ui: &mut Ui, core: &Core, sid: i64, name: &mut String, body: Rect, acts: &mut Acts) {
    let bar = Rect::from_min_size(
        pos2(body.left() - 12.0, body.bottom() - 74.0),
        vec2(body.width() + 24.0, 54.0),
    );
    ui.painter()
        .rect_filled(bar, CornerRadius::same(10), BG0());
    ui.painter().rect_stroke(
        bar,
        CornerRadius::same(10),
        Stroke::new(1.0, BG3()),
        egui::StrokeKind::Inside,
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar.shrink2(vec2(16.0, 10.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.label(
        egui::RichText::new(t!("server.unsaved"))
            .size(14.0)
            .color(TEXT()),
    );
    child.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let busy = core.busy.contains(&FormId::RenameServer);
        let save = Button::primary(t!("server.save_changes"))
            .enabled(!busy)
            .small()
            .show(ui);
        crate::smoke::target(|| "salvar-alteracoes".into(), save.rect);
        if save.clicked() && !busy {
            let n = name.trim().to_owned();
            acts.later(move |c| rename(c, sid, n));
        }
        if widgets::link(ui, t!("server.reset")).clicked()
            && let Some(s) = core.server(sid)
        {
            *name = s.name.clone();
        }
    });
}

fn invites(ui: &mut Ui, core: &Core, sid: i64, acts: &mut Acts) {
    let Some(s) = core.server(sid) else { return };
    title(ui, t!("server.tab_invites"), t!("server.invites_hint"));
    widgets::sub(
        ui,
        if s.private {
            t!("server.invites_private")
        } else {
            t!("server.invites_public")
        },
    );
    ui.add_space(14.0);
    if Button::primary(t!("server.create_invite")).show(ui).clicked() {
        acts.later(Core::invite);
    }
}

/// Who gets in: open or by request, and the requests waiting.
fn access(ui: &mut Ui, core: &Core, sid: i64, acts: &mut Acts) {
    let Some(s) = core.server(sid) else { return };
    title(ui, t!("server.tab_access"), t!("server.access_hint"));
    if s.can(P_MANAGE_SERVER) {
        for (private, label, hint) in [
            (
                false,
                t!("server.access_open"),
                t!("server.access_open_hint"),
            ),
            (
                true,
                t!("server.access_private"),
                t!("server.access_private_hint"),
            ),
        ] {
            if radio_row(ui, label, hint, s.private == private) && s.private != private {
                acts.later(move |c| c.set_private(sid, private));
            }
        }
        rule(ui);
    }
    if s.can(P_KICK) {
        part(ui, t!("modal.join_requests_title"), "");
        requests(ui, core, sid, acts, true);
    }
}

fn roles(
    ui: &mut Ui,
    core: &Core,
    sid: i64,
    new_role: &mut String,
    editor: &mut Option<RoleEditor>,
    acts: &mut Acts,
) {
    let Some(s) = core.server(sid) else { return };
    title(ui, t!("modal.roles_title"), t!("modal.roles_sub"));
    // The selected role, loaded into the editor when it changes.
    let wanted = editor
        .as_ref()
        .map(|e| e.id)
        .filter(|id| s.role(*id).is_some())
        .unwrap_or(s.roles[0].id);
    let reload = editor
        .as_ref()
        .is_none_or(|e| e.id != wanted || e.perms < 0);
    if reload && let Some(r) = s.role(wanted) {
        let color = r
            .color
            .as_deref()
            .and_then(theme::parse_hex)
            .unwrap_or(Color32::from_rgb(0x96, 0x9b, 0xa8));
        *editor = Some(RoleEditor {
            id: r.id,
            name: r.name.clone(),
            color: [color.r(), color.g(), color.b()],
            perms: r.perms,
        });
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        ui.vertical(|ui| {
            ui.set_width(180.0);
            for r in &s.roles {
                let on = editor.as_ref().is_some_and(|e| e.id == r.id);
                let (rect, resp) = ui.allocate_exact_size(vec2(180.0, 30.0), Sense::click());
                if on {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(RADIUS_SM), BG3());
                } else if resp.hovered() {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(RADIUS_SM), BG1());
                }
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect.shrink2(vec2(6.0, 5.0)))
                        .layout(Layout::left_to_right(Align::Center)),
                );
                widgets::pill(&mut child, &r.name, r.color.as_deref());
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    let id = r.id;
                    acts.later(move |c| {
                        if let Some(Modal::ServerSettings { role, .. }) = &mut c.modal {
                            *role = Some(RoleEditor {
                                id,
                                name: String::new(),
                                color: [0; 3],
                                perms: -1,
                            });
                        }
                    });
                }
            }
            ui.add_space(8.0);
            let r = widgets::input(ui, new_role, 100, false, t!("modal.new_role_placeholder"));
            let busy = core.busy.contains(&FormId::CreateRole);
            if (Button::ghost(t!("modal.create_role_btn"))
                .small()
                .wide()
                .enabled(!busy)
                .show(ui)
                .clicked()
                || submitted(ui, &r))
                && !busy
                && !new_role.trim().is_empty()
            {
                let n = new_role.trim().to_owned();
                acts.later(move |c| {
                    c.call(
                        Req::post(format!("/servers/{sid}/roles")).with(json!({ "name": n })),
                        Then::RoleCreated,
                        ErrTo::Form(FormId::CreateRole),
                    )
                });
            }
            widgets::error_line(ui, core.errors.get(&FormId::CreateRole));
        });
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            let Some(e) = editor.as_mut() else { return };
            let Some(role) = s.role(e.id) else { return };
            if role.is_everyone() {
                widgets::sub(ui, t!("modal.everyone_perms"));
                ui.add_space(6.0);
            } else {
                widgets::field(ui, t!("modal.role_name"), &mut e.name, 100, false);
                ui.label(
                    egui::RichText::new(t!("modal.color_label"))
                        .font(theme::bold(12.0))
                        .color(MUTED())
                        .extra_letter_spacing(0.5),
                );
                egui::color_picker::color_edit_button_srgb(ui, &mut e.color);
                ui.add_space(10.0);
            }
            for (bit, label, hint) in perm_labels() {
                let mut on = e.perms & bit != 0;
                let grantable = s.perms & bit != 0;
                if widgets::check_row(ui, label, hint, &mut on, grantable) {
                    if on {
                        e.perms |= bit;
                    } else {
                        e.perms &= !bit;
                    }
                }
            }
            widgets::error_line(ui, core.errors.get(&FormId::SaveRole));
            let busy = core.busy.contains(&FormId::SaveRole);
            let rid = e.id;
            let everyone = role.is_everyone();
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if Button::primary(t!("modal.save_role_btn"))
                    .enabled(!busy)
                    .show(ui)
                    .clicked()
                    && !busy
                {
                    acts.later(Core::save_role);
                }
                if !everyone
                    && Button::danger(t!("modal.delete_role_btn"))
                        .show(ui)
                        .clicked()
                {
                    acts.later(move |c| {
                        c.call(
                            Req::delete(format!("/roles/{rid}")),
                            Then::RoleDeleted,
                            ErrTo::Toast,
                        )
                    });
                }
            });
        });
    });
}

// ---------------------------------------------------------------- people

fn members(
    ui: &mut Ui,
    core: &Core,
    sid: i64,
    search: &mut String,
    open: &mut Option<i64>,
    acts: &mut Acts,
) {
    let Some(s) = core.server(sid) else { return };
    title(
        ui,
        &tf!("server.members_title", n = s.members.len()),
        t!("server.members_hint"),
    );
    let r = widgets::input(ui, search, 64, false, t!("server.search_members"));
    crate::smoke::target(|| "buscar-membros".into(), r.rect);
    ui.add_space(10.0);
    let q = crate::emoji::fold(search.trim());
    let me = core.me_id();
    let found: Vec<&User> = s
        .members
        .iter()
        .filter(|m| {
            q.is_empty()
                || crate::emoji::fold(s.name_of(m)).contains(&q)
                || crate::emoji::fold(m.name()).contains(&q)
                || crate::emoji::fold(&m.username).contains(&q)
        })
        .collect();
    if found.is_empty() {
        widgets::sub(ui, t!("server.no_members_found"));
    }
    for m in found {
        let uid = m.id;
        let is_open = *open == Some(uid);
        Frame::new()
            .fill(if is_open { BG3() } else { BG1() })
            .corner_radius(CornerRadius::same(RADIUS_SM))
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let (row, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
                crate::smoke::target(|| format!("membro:{}", m.name()), row);
                let av = Rect::from_center_size(
                    pos2(row.left() + 18.0, row.center().y),
                    Vec2::splat(36.0),
                );
                widgets::paint_avatar(ui, core, m, av, true, BG1());
                let color = s
                    .role_color(m)
                    .and_then(theme::parse_hex)
                    .unwrap_or(TEXT());
                let x = av.right() + 12.0;
                let shown = s.name_of(m);
                ui.painter().text(
                    pos2(x, row.center().y - 8.0),
                    Align2::LEFT_CENTER,
                    if m.id == s.owner_id {
                        format!("{shown}  ★")
                    } else {
                        shown.to_owned()
                    },
                    theme::bold(14.5),
                    color,
                );
                let handle = if shown != m.name() {
                    format!("{} · {}", m.name(), widgets::handle(m))
                } else {
                    widgets::handle(m)
                };
                ui.painter().text(
                    pos2(x, row.center().y + 10.0),
                    Align2::LEFT_CENTER,
                    handle,
                    theme::small(),
                    MUTED(),
                );
                // Their roles, at the right.
                let mut right = row.right() - 4.0;
                for r in s
                    .roles
                    .iter()
                    .rev()
                    .filter(|r| !r.is_everyone() && m.roles.contains(&r.id))
                    .take(3)
                {
                    let g = ui
                        .painter()
                        .layout_no_wrap(r.name.clone(), theme::small(), TEXT());
                    let w = g.size().x + 22.0;
                    let pill =
                        Rect::from_center_size(pos2(right - w / 2.0, row.center().y), vec2(w, 22.0));
                    if pill.left() < x + 160.0 {
                        break;
                    }
                    ui.painter()
                        .rect_filled(pill, CornerRadius::same(11), BG2());
                    let dot = r
                        .color
                        .as_deref()
                        .and_then(theme::parse_hex)
                        .unwrap_or(MUTED());
                    ui.painter()
                        .circle_filled(pos2(pill.left() + 10.0, pill.center().y), 4.0, dot);
                    ui.painter().galley(
                        pos2(pill.left() + 17.0, pill.center().y - g.size().y / 2.0),
                        g,
                        TEXT(),
                    );
                    right = pill.left() - 6.0;
                }
                if resp.clicked() {
                    *open = if is_open { None } else { Some(uid) };
                }
                if is_open {
                    member_actions(ui, s, m, me, acts);
                }
            });
        ui.add_space(4.0);
    }
}

/// What can be done to a member opened in Membros: their roles, a message,
/// their profile, and moderating them.
fn member_actions(ui: &mut Ui, s: &Server, m: &User, me: i64, acts: &mut Acts) {
    let (sid, uid) = (s.id, m.id);
    ui.add_space(8.0);
    let other_roles: Vec<&Role> = s.roles.iter().filter(|r| !r.is_everyone()).collect();
    if s.can(P_MANAGE_ROLES) && !other_roles.is_empty() {
        ui.label(
            egui::RichText::new(t!("modal.roles_title").to_uppercase())
                .font(theme::bold(11.5))
                .color(MUTED())
                .extra_letter_spacing(0.5),
        );
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for r in other_roles {
                let on = m.roles.contains(&r.id);
                let grantable = r.perms & !s.perms == 0;
                let label = format!("{} {}", if on { "✓" } else { "+" }, r.name);
                let b = ui.add_enabled(
                    grantable,
                    egui::Button::new(egui::RichText::new(label).color(if on {
                        TEXT()
                    } else {
                        MUTED()
                    }))
                    .fill(if on { ACCENT_SOFT() } else { BG2() })
                    .corner_radius(CornerRadius::same(11)),
                );
                if b.clicked() {
                    let rid = r.id;
                    let path = format!("/servers/{sid}/members/{uid}/roles/{rid}");
                    let req = if on {
                        Req::delete(path)
                    } else {
                        Req::put(path)
                    };
                    acts.later(move |c| c.call(req, Then::Nothing, ErrTo::Toast));
                }
            }
        });
        ui.add_space(8.0);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if Button::ghost(t!("server.view_profile"))
            .small()
            .show(ui)
            .clicked()
        {
            acts.later(move |c| c.open_modal(Modal::Profile { uid }));
        }
        if uid != me
            && Button::ghost(t!("modal.message_btn"))
                .small()
                .show(ui)
                .clicked()
        {
            acts.later(move |c| {
                c.close_modal();
                c.open_dm(uid);
            });
        }
        if uid != me && uid != s.owner_id {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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
            });
        }
    });
}

fn bans(ui: &mut Ui, core: &Core, sid: i64, acts: &mut Acts) {
    let Some(s) = core.server(sid) else { return };
    title(ui, t!("modal.bans_title"), t!("server.bans_hint"));
    if s.bans.is_empty() {
        widgets::sub(ui, t!("modal.no_bans"));
    }
    for u in &s.bans {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            widgets::avatar(ui, core, u, 30.0, false, BG2());
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(
                    egui::RichText::new(u.name())
                        .font(theme::bold(14.0))
                        .color(TEXT()),
                );
                ui.label(
                    egui::RichText::new(widgets::handle(u))
                        .size(13.0)
                        .color(MUTED()),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if Button::ghost(t!("modal.remove_ban_btn"))
                    .small()
                    .show(ui)
                    .clicked()
                {
                    let uid = u.id;
                    acts.later(move |c| {
                        c.call(
                            Req::delete(format!("/servers/{sid}/bans/{uid}")),
                            Then::Nothing,
                            ErrTo::Toast,
                        )
                    });
                }
            });
        });
        ui.add_space(6.0);
    }
}

/// A choice among several: a round mark, lit when it is the one.
pub(super) fn radio_row(ui: &mut Ui, label: &str, hint: &str, on: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    crate::smoke::target(|| format!("opcao:{label}"), rect);
    let fill = if on {
        BG3()
    } else if resp.hovered() {
        BG1()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(RADIUS_SM), fill);
    let c = pos2(rect.left() + 20.0, rect.center().y);
    ui.painter()
        .circle_stroke(c, 9.0, Stroke::new(2.0, if on { ACCENT() } else { MUTED() }));
    if on {
        ui.painter().circle_filled(c, 5.0, ACCENT());
    }
    ui.painter().text(
        pos2(rect.left() + 40.0, rect.center().y - 9.0),
        Align2::LEFT_CENTER,
        label,
        theme::bold(14.5),
        TEXT(),
    );
    ui.painter().text(
        pos2(rect.left() + 40.0, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        hint,
        theme::small(),
        MUTED(),
    );
    ui.add_space(2.0);
    resp.clicked()
}

// ---------------------------------------------------------------- mine

/// "Config. de notificação": what this server warns about, and what is
/// silenced in it (on this PC).
pub fn notify_dialog(ui: &mut Ui, core: &Core, sid: i64, acts: &mut Acts) {
    let Some(s) = core.server(sid) else {
        acts.later(Core::close_modal);
        return;
    };
    widgets::h3(ui, t!("server.notify_settings"));
    widgets::sub(ui, &tf!("server.notify_hint", server = s.name.as_str()));
    ui.add_space(10.0);
    let mut muted = core.server_muted(sid);
    if widgets::check_row(
        ui,
        &tf!("server.mute_named", server = s.name.as_str()),
        t!("server.mute_server_hint"),
        &mut muted,
        true,
    ) {
        acts.later(move |c| c.toggle_mute_server(sid));
    }
    widgets::divider(ui);
    widgets::caption(ui, &t!("server.notify_title").to_uppercase());
    ui.add_space(4.0);
    let level = core.notify_level(sid);
    for (which, label, hint) in [
        (
            NotifyLevel::All,
            t!("server.notify_all"),
            t!("server.notify_all_hint"),
        ),
        (
            NotifyLevel::Mentions,
            t!("server.notify_mentions"),
            t!("server.notify_mentions_hint"),
        ),
        (
            NotifyLevel::Nothing,
            t!("server.notify_nothing"),
            t!("server.notify_nothing_hint"),
        ),
    ] {
        if radio_row(ui, label, hint, level == which) {
            acts.later(move |c| c.set_notify_level(sid, which));
        }
    }
    widgets::divider(ui);
    let mut hide = core.hides_muted(sid);
    if widgets::check_row(
        ui,
        t!("server.hide_muted"),
        t!("server.hide_muted_hint"),
        &mut hide,
        true,
    ) {
        acts.later(move |c| c.toggle_hide_muted(sid));
    }
    widgets::divider(ui);
    widgets::caption(ui, &t!("server.mute_channels").to_uppercase());
    widgets::sub(ui, t!("server.mute_channels_hint"));
    ui.add_space(4.0);
    for (cat, list) in s.by_category() {
        ui.add_space(4.0);
        widgets::caption(ui, cat);
        for c in list {
            let mut on = core.channel_muted(c.id);
            let label = format!("{} {}", if c.is_voice() { "🔊" } else { "#" }, c.label());
            if widgets::check_row(ui, &label, "", &mut on, true) {
                let cid = c.id;
                acts.later(move |core| core.toggle_mute_channel(cid));
            }
        }
    }
    super::modals::foot(ui, |ui| {
        if Button::primary(t!("common.close")).show(ui).clicked() {
            acts.later(Core::close_modal);
        }
    });
}

/// "Editar perfil por servidor": my nickname here, as the others will
/// see it.
pub fn nick_dialog(ui: &mut Ui, core: &Core, sid: i64, nick: &mut String, acts: &mut Acts) {
    let (Some(s), Some(me)) = (core.server(sid), core.me.clone()) else {
        acts.later(Core::close_modal);
        return;
    };
    widgets::h3(ui, t!("server.edit_profile"));
    widgets::sub(ui, &tf!("server.profile_hint", server = s.name.as_str()));
    ui.add_space(12.0);
    // How the others see me here, as I type.
    Frame::new()
        .fill(BG1())
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                widgets::avatar(ui, core, &me, 52.0, true, BG1());
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.add_space(6.0);
                    let shown = if nick.trim().is_empty() {
                        me.name()
                    } else {
                        nick.trim()
                    };
                    ui.label(
                        egui::RichText::new(shown)
                            .font(theme::bold(17.0))
                            .color(TEXT()),
                    );
                    ui.label(
                        egui::RichText::new(widgets::handle(&me))
                            .size(13.0)
                            .color(MUTED()),
                    );
                });
            });
        });
    ui.add_space(14.0);
    ui.label(
        egui::RichText::new(t!("server.nick").to_uppercase())
            .font(theme::bold(12.0))
            .color(MUTED())
            .extra_letter_spacing(0.5),
    );
    let r = widgets::input(ui, nick, 64, false, me.name());
    crate::smoke::target(|| "apelido".into(), r.rect);
    widgets::error_line(ui, core.errors.get(&FormId::Nick));
    let saved = s
        .member(me.id)
        .and_then(|m| m.nick.clone())
        .unwrap_or_default();
    super::modals::foot(ui, |ui| {
        let save = Button::primary(t!("server.save_nick")).show(ui);
        crate::smoke::target(|| "salvar-apelido".into(), save.rect);
        if save.clicked() || submitted(ui, &r) {
            let n = nick.clone();
            acts.later(move |c| c.set_nick(sid, n));
        }
        if !saved.is_empty() && Button::ghost(t!("server.reset_nick")).show(ui).clicked() {
            acts.later(move |c| c.set_nick(sid, String::new()));
        }
        if Button::ghost(t!("common.cancel")).show(ui).clicked() {
            acts.later(Core::close_modal);
        }
    });
}
