//! Configurações, as Discord has them: a large dialog with the menu on the
//! left (who I am, a search, the sections in groups, the open one's parts
//! under it) and the open section on the right under a bar with its name.
//! The sections are rows: a name at the left, the value and a button at
//! the right, or a switch.

use egui::{
    Align, Align2, Color32, ComboBox, CornerRadius, CursorIcon, Frame, Id, Layout, Margin, Rect,
    ScrollArea, Sense, Stroke, TextEdit, Ui, pos2, vec2,
};
use serde_json::json;

use super::Acts;
use super::modals::{device_select, select, submitted, tor_text};
use super::widgets::{self, Button};
use crate::core::{Action, Core, ErrTo, FormId, Modal, Pick, SettingsTab, Then};
use crate::icons::Icon;
use crate::model::*;
use crate::net::Req;
use crate::sounds::Cue;
use crate::theme::{self, *};
use crate::voice::{Noise, QUALITIES};
use crate::{t, tf};

/// The menu's width, and how wide a section's column grows.
const NAV_W: f32 = 236.0;
const COLUMN_W: f32 = 700.0;
const HEAD_H: f32 = 48.0;

/// Where in egui's memory the page keeps what is only on screen: the
/// search, the part to scroll to, the part last picked, the name being
/// edited and the e-mail shown.
fn mem_id(what: &str) -> Id {
    Id::unique(("babutela-settings", what))
}

fn get_str(ui: &Ui, what: &str) -> String {
    ui.data(|d| d.get_temp::<String>(mem_id(what)))
        .unwrap_or_default()
}

fn set_str(ui: &Ui, what: &str, v: String) {
    ui.data_mut(|d| d.insert_temp(mem_id(what), v));
}

fn get_flag(ui: &Ui, what: &str) -> bool {
    ui.data(|d| d.get_temp::<bool>(mem_id(what))).unwrap_or(false)
}

fn set_flag(ui: &Ui, what: &str, v: bool) {
    ui.data_mut(|d| d.insert_temp(mem_id(what), v));
}

/// A section in the menu: which, its icon, its name, and its parts (the
/// key each one's heading has, and its name).
struct NavItem {
    tab: SettingsTab,
    icon: Icon,
    label: &'static str,
    parts: Vec<(&'static str, &'static str)>,
}

fn groups() -> Vec<(&'static str, Vec<NavItem>)> {
    vec![
        (
            "",
            vec![
                NavItem {
                    tab: SettingsTab::Account,
                    icon: Icon::User,
                    label: t!("settings.account"),
                    parts: vec![
                        ("info", t!("settings.sub_info")),
                        ("perfil", t!("settings.profile")),
                        ("gerenciar", t!("settings.sub_manage")),
                    ],
                },
                NavItem {
                    tab: SettingsTab::Privacy,
                    icon: Icon::Shield,
                    label: t!("settings.tab_privacy"),
                    parts: vec![],
                },
                NavItem {
                    tab: SettingsTab::Notifications,
                    icon: Icon::Bell,
                    label: t!("settings.notifications"),
                    parts: vec![
                        ("avisos", t!("settings.sub_alerts")),
                        ("sons", t!("settings.tab_sounds")),
                    ],
                },
            ],
        ),
        (
            t!("settings.nav_experience"),
            vec![
                NavItem {
                    tab: SettingsTab::Voice,
                    icon: Icon::Mic,
                    label: t!("settings.voice_video"),
                    parts: vec![
                        ("voz", t!("settings.tab_voice")),
                        ("transmissao", t!("settings.tab_stream")),
                    ],
                },
                NavItem {
                    tab: SettingsTab::Appearance,
                    icon: Icon::Palette,
                    label: t!("settings.tab_appearance"),
                    parts: vec![
                        ("tema", t!("settings.theme")),
                        ("destaque", t!("settings.accent")),
                        ("previa", t!("settings.preview")),
                    ],
                },
                NavItem {
                    tab: SettingsTab::Language,
                    icon: Icon::Language,
                    label: t!("settings.language"),
                    parts: vec![],
                },
                NavItem {
                    tab: SettingsTab::System,
                    icon: Icon::System,
                    label: t!("settings.tab_system"),
                    parts: vec![
                        ("windows", t!("settings.sub_windows")),
                        ("versao", t!("settings.sub_version")),
                    ],
                },
            ],
        ),
    ]
}

fn tab_label(tab: SettingsTab) -> &'static str {
    groups()
        .into_iter()
        .flat_map(|(_, items)| items)
        .find(|i| i.tab == tab)
        .map_or("", |i| i.label)
}

/// The page, drawn in place of the usual dialog.
pub fn page(
    ctx: &egui::Context,
    core: &mut Core,
    display_name: &mut String,
    tab: &mut SettingsTab,
    acts: &mut Acts,
) {
    let screen = ctx.content_rect();
    let size = vec2(
        (screen.width() - 64.0).clamp(620.0, 1440.0),
        (screen.height() - TITLEBAR_H - 48.0).max(380.0),
    );
    let id = Id::unique("settings-page");
    let frame = Frame::new()
        .fill(BG0())
        .stroke(Stroke::new(1.0, BG3()))
        .corner_radius(CornerRadius::same(12))
        .shadow(egui::epaint::Shadow {
            offset: [0, 20],
            blur: 60,
            spread: 0,
            color: Color32::from_black_alpha(0xaa),
        });
    let resp = egui::Modal::new(id)
        .area(
            egui::Modal::default_area(id)
                .anchor(Align2::CENTER_CENTER, vec2(0.0, TITLEBAR_H / 2.0)),
        )
        .frame(frame)
        .backdrop_color(Color32::from_black_alpha(0xb0))
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let nav = Rect::from_min_size(rect.min, vec2(NAV_W, rect.height()));
            ui.painter().vline(
                nav.right(),
                nav.y_range(),
                Stroke::new(1.0, BG3()),
            );
            let mut menu = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(nav.shrink2(vec2(8.0, 10.0)))
                    .layout(Layout::top_down(Align::Min)),
            );
            ScrollArea::vertical()
                .id_salt("settings-nav")
                .auto_shrink([false, false])
                .show(&mut menu, |ui| nav_menu(ui, core, tab, acts));

            // The bar at the top: the section's name and the way out.
            let body = Rect::from_min_max(pos2(nav.right() + 1.0, rect.top()), rect.max);
            let head = Rect::from_min_size(body.min, vec2(body.width(), HEAD_H));
            ui.painter().text(
                pos2(head.left() + 16.0, head.center().y),
                Align2::LEFT_CENTER,
                tab_label(*tab),
                theme::bold(15.0),
                TEXT(),
            );
            ui.painter().hline(
                head.x_range(),
                head.bottom(),
                Stroke::new(1.0, BG3()),
            );
            let close = Rect::from_center_size(
                pos2(head.right() - 24.0, head.center().y),
                vec2(32.0, 32.0),
            );
            let mut corner = ui.new_child(egui::UiBuilder::new().max_rect(close));
            let x = widgets::icon_button(
                &mut corner,
                Icon::X,
                32.0,
                widgets::Ib::Bare,
                t!("settings.close"),
            );
            crate::smoke::target(|| "fechar-pagina".into(), x.rect);
            if x.clicked() {
                acts.later(Core::close_modal);
            }

            // The section, a column in the middle.
            let below = Rect::from_min_max(pos2(body.left(), head.bottom() + 1.0), body.max);
            let w = (below.width() - 80.0).min(COLUMN_W);
            let left = below.center().x - w / 2.0;
            let column = Rect::from_min_max(
                pos2(left, below.top()),
                pos2(below.right() - 6.0, below.bottom()),
            );
            let mut content = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(column)
                    .layout(Layout::top_down(Align::Min)),
            );
            let mut area = ScrollArea::vertical()
                .id_salt(("settings-body", tab.key()))
                .auto_shrink([false, false]);
            if let Some(y) = core.modal_scroll.take() {
                area = area.vertical_scroll_offset(y);
            }
            area.show(&mut content, |ui| {
                ui.set_max_width(w);
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.add_space(36.0);
                match tab {
                    SettingsTab::Account => account(ui, core, display_name, acts),
                    SettingsTab::Privacy => privacy(ui, core, acts),
                    SettingsTab::Notifications => notifications(ui, core, acts),
                    SettingsTab::Voice => voice(ui, core, acts),
                    SettingsTab::Appearance => appearance(ui, core, acts),
                    SettingsTab::Language => language(ui, acts),
                    SettingsTab::System => system(ui, core, acts),
                }
                ui.add_space(48.0);
            });
        });
    if resp.should_close() {
        acts.later(Core::close_modal);
    }
}

// ---------------------------------------------------------------- the menu

fn nav_menu(ui: &mut Ui, core: &Core, tab: &mut SettingsTab, acts: &mut Acts) {
    ui.spacing_mut().item_spacing.y = 2.0;
    // Who I am, and "Editar perfil".
    if let Some(me) = core.me.clone() {
        let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
        let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
        if resp.hovered() {
            ui.painter()
                .rect_filled(row, CornerRadius::same(RADIUS_MD), BG2());
        }
        let av = Rect::from_center_size(pos2(row.left() + 28.0, row.center().y), vec2(44.0, 44.0));
        widgets::paint_avatar(ui, core, &me, av, false, BG0());
        let x = av.right() + 10.0;
        let name = ui.painter().layout_job({
            let mut job = egui::text::LayoutJob::simple_singleline(
                me.name().to_owned(),
                theme::bold(15.0),
                TEXT(),
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(row.right() - x - 6.0);
            job
        });
        ui.painter()
            .galley(pos2(x, row.center().y - name.size().y), name, TEXT());
        let edit = ui.painter().layout_no_wrap(
            t!("settings.edit_profile").to_owned(),
            theme::small(),
            MUTED(),
        );
        let at = pos2(x, row.center().y + 2.0);
        let edit_w = edit.size().x;
        ui.painter().galley(at, edit, MUTED());
        Icon::Edit.image(12.0, MUTED()).paint_at(
            ui,
            Rect::from_min_size(pos2(at.x + edit_w + 5.0, at.y + 2.0), vec2(12.0, 12.0)),
        );
        if resp.clicked() {
            pick(ui, tab, SettingsTab::Account, Some("perfil"));
        }
        ui.add_space(6.0);
    }
    // The search, over the sections and their parts.
    let mut q = get_str(ui, "search");
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
    ui.painter()
        .rect_filled(bar, CornerRadius::same(RADIUS_MD), BG1());
    ui.painter().rect_stroke(
        bar,
        CornerRadius::same(RADIUS_MD),
        Stroke::new(1.0, BG3()),
        egui::StrokeKind::Inside,
    );
    Icon::Search.image(16.0, MUTED()).paint_at(
        ui,
        Rect::from_center_size(pos2(bar.left() + 18.0, bar.center().y), vec2(16.0, 16.0)),
    );
    let field = Rect::from_min_max(
        pos2(bar.left() + 34.0, bar.center().y - 10.0),
        pos2(bar.right() - 8.0, bar.center().y + 10.0),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field));
    let r = child.add(
        TextEdit::singleline(&mut q)
            .frame(Frame::NONE)
            .hint_text(t!("settings.search"))
            .desired_width(field.width())
            .min_size(field.size()),
    );
    crate::smoke::target(|| "buscar-config".into(), r.rect);
    if r.changed() {
        set_str(ui, "search", q.clone());
    }
    ui.add_space(8.0);
    let q = crate::emoji::fold(q.trim());
    for (label, items) in groups() {
        let items: Vec<NavItem> = items
            .into_iter()
            .filter(|i| {
                q.is_empty()
                    || crate::emoji::fold(i.label).contains(&q)
                    || i.parts
                        .iter()
                        .any(|(_, p)| crate::emoji::fold(p).contains(&q))
            })
            .collect();
        if items.is_empty() {
            continue;
        }
        if !label.is_empty() {
            line(ui);
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(label).size(12.5).color(MUTED()));
            });
            ui.add_space(2.0);
        }
        for item in items {
            let on = *tab == item.tab;
            let r = nav_row(ui, item.icon, item.label, on);
            crate::smoke::target(|| format!("config:{}", item.tab.key()), r.rect);
            if r.clicked() {
                pick(ui, tab, item.tab, item.parts.first().map(|p| p.0));
            }
            // The open section's parts, under it, on a line.
            if (on || !q.is_empty()) && !item.parts.is_empty() {
                let current = get_str(ui, "part");
                let first = item.parts[0].0;
                for (key, text) in &item.parts {
                    let lit = on
                        && (current == *key || (*key == first && !item.parts.iter().any(|p| p.0 == current)));
                    let r = part_row(ui, text, lit);
                    crate::smoke::target(|| format!("parte:{key}"), r.rect);
                    if r.clicked() {
                        pick(ui, tab, item.tab, Some(key));
                    }
                }
                ui.add_space(2.0);
            }
        }
    }
    line(ui);
    if nav_row_danger(ui, Icon::LogOut, t!("settings.signout")).clicked() {
        acts.later(|c| c.logout(false));
    }
}

/// Opens section `to` at its part `part` (scrolling to it).
fn pick(ui: &Ui, tab: &mut SettingsTab, to: SettingsTab, part: Option<&str>) {
    *tab = to;
    if let Some(p) = part {
        set_str(ui, "part", p.to_owned());
        set_str(ui, "jump", p.to_owned());
    } else {
        set_str(ui, "part", String::new());
    }
}

fn line(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 13.0), Sense::hover());
    ui.painter().hline(
        r.x_range().shrink(6.0),
        r.center().y,
        Stroke::new(1.0, BG3()),
    );
}

fn nav_row(ui: &mut Ui, icon: Icon, text: &str, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if on {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(RADIUS_MD), BG3());
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(RADIUS_MD), BG2());
    }
    let ink = if on || resp.hovered() { TEXT() } else { MUTED() };
    icon.image(18.0, ink).paint_at(
        ui,
        Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(18.0, 18.0)),
    );
    ui.painter().text(
        pos2(rect.left() + 38.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        if on { theme::bold(14.5) } else { theme::body() },
        ink,
    );
    resp
}

fn nav_row_danger(ui: &mut Ui, icon: Icon, text: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(RADIUS_MD),
            DANGER.gamma_multiply(0.15),
        );
    }
    icon.image(18.0, DANGER).paint_at(
        ui,
        Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(18.0, 18.0)),
    );
    ui.painter().text(
        pos2(rect.left() + 38.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        theme::body(),
        DANGER,
    );
    resp
}

/// A part of the open section, on the line under it.
fn part_row(ui: &mut Ui, text: &str, lit: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let x = rect.left() + 18.0;
    ui.painter().vline(
        x,
        rect.y_range(),
        Stroke::new(if lit { 2.0 } else { 1.0 }, if lit { TEXT() } else { BG4() }),
    );
    ui.painter().text(
        pos2(x + 16.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        theme::body(),
        if lit || resp.hovered() { TEXT() } else { MUTED() },
    );
    resp
}

// ---------------------------------------------------------------- the rows

/// A part's heading; the menu's part `key` scrolls here.
fn heading(ui: &mut Ui, key: &str, text: &str) {
    let r = ui.label(egui::RichText::new(text).font(theme::bold(22.0)).color(TEXT()));
    if get_str(ui, "jump") == key {
        ui.scroll_to_rect(r.rect.expand(24.0), Some(Align::TOP));
        set_str(ui, "jump", String::new());
    }
    ui.add_space(14.0);
}

fn rule(ui: &mut Ui) {
    ui.add_space(26.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(r.x_range(), r.center().y, Stroke::new(1.0, BG3()));
    ui.add_space(30.0);
}

/// A dark button with a border, as the rows have.
fn row_button(ui: &mut Ui, text: &str, danger: bool) -> egui::Response {
    let g = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::bold(14.0), TEXT());
    let size = vec2(g.size().x + 40.0, 38.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let (fill, ink) = match (danger, resp.hovered()) {
        (true, true) => (DANGER, Color32::WHITE),
        (true, false) => (DANGER.gamma_multiply(0.15), DANGER),
        (false, true) => (BG3(), TEXT()),
        (false, false) => (BG2(), TEXT()),
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(RADIUS_MD), fill);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(RADIUS_MD),
        Stroke::new(1.0, if danger { DANGER.gamma_multiply(0.5) } else { BG4() }),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        theme::bold(14.0),
        ink,
    );
    resp
}

/// A row: its name at the left (and what it is, under it), and at the right
/// whatever `right` adds, right to left.
fn row(ui: &mut Ui, label: &str, hint: &str, right: impl FnOnce(&mut Ui)) {
    let h = if hint.is_empty() { 48.0 } else { 58.0 };
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), h),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_height(h);
            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width() * 0.62);
                ui.spacing_mut().item_spacing.y = 2.0;
                // Centred in the row, as the value at the right is.
                ui.add_space(if hint.is_empty() { 13.0 } else { 9.0 });
                ui.label(egui::RichText::new(label).size(15.5).color(TEXT()));
                if !hint.is_empty() {
                    ui.label(egui::RichText::new(hint).size(13.0).color(MUTED()));
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                right(ui);
            });
        },
    );
    ui.add_space(8.0);
}

/// A row with a switch at its right. Returns whether it was flipped.
fn switch_row(ui: &mut Ui, label: &str, hint: &str, on: bool) -> bool {
    let mut flipped = false;
    row(ui, label, hint, |ui| {
        let (r, resp) = ui.allocate_exact_size(vec2(44.0, 26.0), Sense::click());
        crate::smoke::target(|| format!("chave:{label}"), r);
        let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
        paint_switch(ui, r, on, resp.hovered());
        flipped = resp.clicked();
    });
    flipped
}

fn paint_switch(ui: &Ui, r: Rect, on: bool, hovered: bool) {
    let fill = if on {
        ACCENT()
    } else if hovered {
        BG4().lerp_to_gamma(Color32::WHITE, 0.1)
    } else {
        BG4()
    };
    ui.painter().rect_filled(r, CornerRadius::same(13), fill);
    let knob = if on { r.right() - 13.0 } else { r.left() + 13.0 };
    ui.painter()
        .circle_filled(pos2(knob, r.center().y), 9.5, Color32::WHITE);
}

// ---------------------------------------------------------------- Conta

fn account(ui: &mut Ui, core: &Core, display_name: &mut String, acts: &mut Acts) {
    let Some(me) = core.me.clone() else { return };
    heading(ui, "info", t!("settings.sub_info"));
    // The name others see, edited right in its row.
    let editing = get_flag(ui, "editing-name");
    row(ui, t!("settings.display_name"), "", |ui| {
        if !editing && row_button(ui, t!("settings.edit"), false).clicked() {
            *display_name = me.display_name.clone();
            set_flag(ui, "editing-name", true);
        }
        if !editing {
            ui.label(egui::RichText::new(me.name()).size(15.0).color(TEXT()));
        }
    });
    if editing {
        let r = widgets::input(ui, display_name, 64, false, t!("settings.display_name"));
        widgets::error_line(ui, core.errors.get(&FormId::Profile));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let busy = core.busy.contains(&FormId::Profile);
            if (Button::primary(t!("settings.save"))
                .small()
                .enabled(!busy)
                .show(ui)
                .clicked()
                || submitted(ui, &r))
                && !busy
            {
                let n = display_name.trim().to_owned();
                set_flag(ui, "editing-name", false);
                acts.later(move |c| {
                    c.call(
                        Req::patch("/me").with(json!({ "displayName": n })),
                        Then::Toast(t!("settings.name_saved").into()),
                        ErrTo::Form(FormId::Profile),
                    )
                });
            }
            if Button::ghost(t!("common.cancel")).small().show(ui).clicked() {
                set_flag(ui, "editing-name", false);
            }
        });
        ui.add_space(10.0);
    }
    row(ui, t!("settings.username"), "", |ui| {
        ui.label(
            egui::RichText::new(widgets::handle(&me))
                .size(15.0)
                .color(TEXT()),
        );
    });
    let email = core.account.email.clone();
    row(ui, t!("settings.email"), "", |ui| {
        if email.is_empty() {
            ui.label(
                egui::RichText::new(t!("settings.google_account"))
                    .size(15.0)
                    .color(MUTED()),
            );
            return;
        }
        let shown = get_flag(ui, "show-email");
        if widgets::link(
            ui,
            if shown {
                t!("settings.hide")
            } else {
                t!("settings.show")
            },
        )
        .clicked()
        {
            set_flag(ui, "show-email", !shown);
        }
        let text = if shown { email.clone() } else { masked(&email) };
        ui.label(egui::RichText::new(text).size(15.0).color(TEXT()));
    });
    widgets::sub(ui, t!("settings.account_google"));

    rule(ui);
    heading(ui, "perfil", t!("settings.profile"));
    row(ui, t!("settings.photo"), "", |ui| {
        if me.avatar.is_some() && widgets::link(ui, t!("settings.remove")).clicked() {
            acts.later(|c| {
                c.call(
                    Req::patch("/me").with(json!({ "avatar": null })),
                    Then::Nothing,
                    ErrTo::Toast,
                )
            });
        }
        if row_button(ui, t!("settings.change"), false).clicked() {
            acts.later(|c| c.pick_files(Pick::Avatar));
        }
        widgets::avatar(ui, core, &me, 40.0, false, BG0());
    });
    row(ui, t!("settings.status"), "", |ui| {
        let current = me.status;
        ComboBox::from_id_salt("settings-status")
            .selected_text(current.label())
            .width(180.0)
            .show_ui(ui, |ui| {
                for s in Status::PICKABLE {
                    if ui
                        .selectable_label(s == current, s.label())
                        .clicked()
                    {
                        let picked = s;
                        acts.later(move |c| c.set_status(picked));
                    }
                }
            });
    });

    rule(ui);
    heading(ui, "gerenciar", t!("settings.sub_manage"));
    row(
        ui,
        t!("settings.signout"),
        t!("settings.signout_hint"),
        |ui| {
            if row_button(ui, t!("settings.signout_btn"), false).clicked() {
                acts.later(|c| c.logout(false));
            }
        },
    );
    row(
        ui,
        t!("settings.delete_account"),
        t!("settings.delete_hint"),
        |ui| {
            if row_button(ui, t!("settings.delete_account"), true).clicked() {
                acts.later(|c| {
                    c.confirm(
                        t!("settings.delete_account"),
                        t!("settings.delete_account_confirm"),
                        t!("settings.delete_forever"),
                        Action::DeleteAccount,
                    )
                });
            }
        },
    );
}

/// "***********@gmail.com": the address with its name hidden.
fn masked(email: &str) -> String {
    match email.split_once('@') {
        Some((name, host)) => format!("{}@{host}", "*".repeat(name.chars().count().max(4))),
        None => "*".repeat(email.chars().count()),
    }
}

// ---------------------------------------------------------------- the rest

fn privacy(ui: &mut Ui, core: &Core, acts: &mut Acts) {
    heading(ui, "privacidade", t!("settings.tab_privacy"));
    let hide_ip = core.prefs.hide_ip;
    if switch_row(ui, t!("settings.hide_ip"), t!("settings.hide_ip_hint"), hide_ip) {
        acts.later(move |c| c.set_hide_ip(!hide_ip));
    }
    let typing = !core.prefs.no_typing;
    if switch_row(
        ui,
        t!("settings.show_typing"),
        t!("settings.show_typing_hint"),
        typing,
    ) {
        acts.later(move |c| c.set_send_typing(!typing));
    }
    let tor = core.prefs.tor;
    if switch_row(ui, t!("settings.tor"), t!("settings.tor_hint"), tor) {
        acts.later(move |c| c.set_tor(!tor));
    }
    if let Some(status) = crate::tor::status() {
        widgets::sub(ui, &tor_text(&status));
    }
}

fn notifications(ui: &mut Ui, core: &Core, acts: &mut Acts) {
    heading(ui, "avisos", t!("settings.sub_alerts"));
    let on = core.shell.settings().notifications;
    if switch_row(
        ui,
        t!("settings.desktop_notifications"),
        t!("settings.notifications_hint"),
        on,
    ) {
        core.shell.set_flag("notifications", !on);
    }
    widgets::sub(ui, t!("settings.server_notify_hint"));
    rule(ui);
    heading(ui, "sons", t!("settings.tab_sounds"));
    widgets::sub(
        ui,
        &tf!(
            "settings.sounds_header",
            on = core.sounds.enabled_count(),
            total = Cue::ALL.len()
        ),
    );
    ui.add_space(8.0);
    for cue in Cue::ALL {
        let enabled = core.sounds.enabled(cue);
        let mut flip = false;
        row(ui, cue.label(), "", |ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(44.0, 26.0), Sense::click());
            let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
            paint_switch(ui, r, enabled, resp.hovered());
            flip = resp.clicked();
            let label = tf!("settings.hear", name = cue.label());
            if widgets::icon_button_sized(ui, Icon::Play, 30.0, 11.0, widgets::Ib::Normal, &label)
                .clicked()
            {
                core.sounds.play_always(cue);
            }
        });
        if flip {
            core.sounds.set_enabled(cue, !enabled);
            if !enabled {
                core.sounds.play_always(cue);
            }
            acts.later(Core::save_prefs);
        }
    }
}

fn voice(ui: &mut Ui, core: &Core, acts: &mut Acts) {
    heading(ui, "voz", t!("settings.tab_voice"));
    let devices = core.audio_devices.as_ref();
    ui.columns(2, |cols| {
        if let Some(choice) = device_select(
            &mut cols[0],
            "input-device",
            t!("settings.microphone"),
            core.input_device(),
            devices.map(|d| d.inputs.as_slice()),
        ) {
            acts.later(move |c| c.set_input_device(choice));
        }
        if let Some(choice) = device_select(
            &mut cols[1],
            "output-device",
            t!("settings.audio_output"),
            core.output_device(),
            devices.map(|d| d.outputs.as_slice()),
        ) {
            acts.later(move |c| c.set_output_device(choice));
        }
    });
    widgets::sub(ui, t!("settings.output_hint"));
    ui.add_space(18.0);
    let mut noise = Noise::ALL
        .iter()
        .position(|n| *n == core.noise())
        .unwrap_or(0);
    let labels: Vec<&str> = Noise::ALL.iter().map(|n| n.label()).collect();
    if select(ui, "noise", t!("settings.noise"), &mut noise, &labels) {
        let picked = Noise::ALL[noise];
        acts.later(move |c| c.set_noise(picked));
    }
    widgets::sub(
        ui,
        match Noise::ALL[noise] {
            Noise::Ia => t!("settings.noise_ia"),
            Noise::Padrao => t!("settings.noise_padrao"),
            Noise::Desligada => t!("settings.noise_desligada"),
        },
    );
    rule(ui);
    heading(ui, "transmissao", t!("settings.tab_stream"));
    let mut q = QUALITIES
        .iter()
        .position(|q| q.key == core.quality_key())
        .unwrap_or(1);
    let labels: Vec<&str> = QUALITIES.iter().map(|q| q.label).collect();
    if select(
        ui,
        "settings-quality",
        t!("modal.quality_label"),
        &mut q,
        &labels,
    ) {
        let key = QUALITIES[q].key;
        acts.later(move |c| c.set_quality(key));
    }
    widgets::sub(ui, t!("settings.stream_hint"));
    ui.add_space(12.0);
    let audio = core.voice.share_audio();
    if switch_row(
        ui,
        t!("modal.share_audio_label"),
        t!("modal.share_audio_pc"),
        audio,
    ) {
        acts.later(move |c| c.set_share_audio(!audio));
    }
}

fn language(ui: &mut Ui, acts: &mut Acts) {
    heading(ui, "idioma", t!("settings.language"));
    widgets::sub(ui, t!("settings.language_hint"));
    ui.add_space(10.0);
    let current = crate::lang::current();
    for l in crate::lang::Lang::ALL {
        if super::server_settings::radio_row(ui, l.label(), "", current == l) && current != l {
            acts.later(move |c| c.set_language(l));
        }
    }
}

fn system(ui: &mut Ui, core: &Core, acts: &mut Acts) {
    heading(ui, "windows", t!("settings.sub_windows"));
    let s = core.shell.settings();
    for (key, label, hint, value) in [
        (
            "openOnStartup",
            t!("settings.startup"),
            t!("settings.startup_hint"),
            s.open_on_startup,
        ),
        (
            "startMinimized",
            t!("settings.start_min"),
            t!("settings.start_min_hint"),
            s.start_minimized,
        ),
        (
            "minimizeToTray",
            t!("settings.minimize_tray"),
            t!("settings.minimize_tray_hint"),
            s.minimize_to_tray,
        ),
    ] {
        if switch_row(ui, label, hint, value) {
            core.shell.set_flag(key, !value);
        }
    }
    rule(ui);
    heading(ui, "versao", t!("settings.sub_version"));
    row(
        ui,
        &tf!("settings.version_label", version = babutela_buildinfo::VERSION),
        t!("settings.version_hint"),
        |ui| {
            if row_button(ui, t!("settings.check_updates"), false).clicked() {
                acts.later(|c| c.check_update(crate::core::Check::Button));
            }
        },
    );
    row(ui, t!("settings.licenses"), t!("settings.licenses_hint"), |ui| {
        if row_button(ui, t!("settings.see"), false).clicked() {
            acts.later(|c| c.open_modal(Modal::Licenses));
        }
    });
}

// ---------------------------------------------------------------- Aparência

/// The themes to pick from: the surfaces' tint (none: the app's indigo).
const THEMES: [(&str, Option<[u8; 3]>); 6] = [
    ("settings.theme_indigo", None),
    ("settings.theme_black", Some([0x00, 0x00, 0x00])),
    ("settings.theme_graphite", Some([0x1c, 0x1d, 0x21])),
    ("settings.theme_navy", Some([0x0a, 0x1a, 0x36])),
    ("settings.theme_forest", Some([0x0c, 0x24, 0x19])),
    ("settings.theme_wine", Some([0x2c, 0x0b, 0x17])),
];

/// The accents to pick from (none: the app's periwinkle).
const ACCENTS: [(&str, Option<[u8; 3]>); 8] = [
    ("settings.accent_default", None),
    ("settings.accent_blue", Some([0x3b, 0x8b, 0xff])),
    ("settings.accent_green", Some([0x34, 0xd3, 0x99])),
    ("settings.accent_pink", Some([0xf4, 0x72, 0xb6])),
    ("settings.accent_orange", Some([0xfb, 0x92, 0x3c])),
    ("settings.accent_red", Some([0xef, 0x4a, 0x4a])),
    ("settings.accent_purple", Some([0xb0, 0x6c, 0xff])),
    ("settings.accent_white", Some([0xe8, 0xe8, 0xee])),
];

/// A colour square's side.
const SQUARE: f32 = 52.0;

fn appearance(ui: &mut Ui, core: &Core, acts: &mut Acts) {
    let base = core.prefs.theme_base;
    let accent = core.prefs.theme_accent;
    let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
    let name = |key: &'static str| crate::lang::lookup(key).unwrap_or(key);

    heading(ui, "tema", t!("settings.theme"));
    ui.label(egui::RichText::new(t!("settings.theme_hint")).size(13.0).color(MUTED()));
    ui.add_space(12.0);
    let current_theme = THEMES
        .iter()
        .find(|(_, v)| *v == base)
        .map_or(t!("settings.custom"), |(k, _)| name(k));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
        let mut preset = false;
        for (key, value) in THEMES {
            let p = theme::Palette::new(value.map(rgb), None);
            let on = base == value;
            preset |= on;
            let r = square(ui, [p.bg2, p.bg0], on).on_hover_text(name(key));
            crate::smoke::target(|| format!("tema:{key}"), r.rect);
            if r.clicked() {
                acts.later(move |c| c.set_theme(value, accent));
            }
        }
        let mut custom = base.unwrap_or([0x30, 0x1a, 0x4a]);
        if custom_square(ui, &mut custom, !preset) {
            acts.later(move |c| c.set_theme(Some(custom), accent));
        }
    });
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(tf!("settings.theme_current", name = current_theme))
            .size(13.0)
            .color(MUTED()),
    );

    rule(ui);
    heading(ui, "destaque", t!("settings.accent"));
    ui.label(egui::RichText::new(t!("settings.accent_hint")).size(13.0).color(MUTED()));
    ui.add_space(12.0);
    let current_accent = ACCENTS
        .iter()
        .find(|(_, v)| *v == accent)
        .map_or(t!("settings.custom"), |(k, _)| name(k));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
        let mut preset = false;
        for (key, value) in ACCENTS {
            let p = theme::Palette::new(None, value.map(rgb));
            let on = accent == value;
            preset |= on;
            let r = square(ui, [p.grad_a, p.accent_2], on).on_hover_text(name(key));
            crate::smoke::target(|| format!("destaque:{key}"), r.rect);
            if r.clicked() {
                acts.later(move |c| c.set_theme(base, value));
            }
        }
        let mut custom = accent.unwrap_or([0x85, 0x90, 0xff]);
        if custom_square(ui, &mut custom, !preset) {
            acts.later(move |c| c.set_theme(base, Some(custom)));
        }
    });
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(tf!("settings.accent_current", name = current_accent))
            .size(13.0)
            .color(MUTED()),
    );

    rule(ui);
    heading(ui, "previa", t!("settings.preview"));
    preview(ui, core);
    ui.add_space(16.0);
    if (base.is_some() || accent.is_some())
        && row_button(ui, t!("settings.theme_reset"), false).clicked()
    {
        acts.later(|c| c.set_theme(None, None));
    }
}

/// A colour to pick: a rounded square, `colors[0]` above going into
/// `colors[1]` below, with a tick at its corner when it is the one in use.
fn square(ui: &mut Ui, colors: [Color32; 2], on: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(SQUARE, SQUARE), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    paint_square(ui, r, colors, on, resp.hovered());
    resp
}

fn paint_square(ui: &Ui, r: Rect, colors: [Color32; 2], on: bool, hovered: bool) {
    let painter = ui.painter();
    painter.rect_filled(r, CornerRadius::same(10), colors[1]);
    let top = Rect::from_min_max(r.min, pos2(r.right(), r.center().y + 6.0));
    painter.rect_filled(
        top,
        CornerRadius {
            nw: 10,
            ne: 10,
            sw: 0,
            se: 0,
        },
        colors[0],
    );
    let ring = if on {
        Stroke::new(2.5, ACCENT())
    } else if hovered {
        Stroke::new(1.5, MUTED())
    } else {
        Stroke::new(1.0, BG4())
    };
    painter.rect_stroke(r, CornerRadius::same(10), ring, egui::StrokeKind::Outside);
    if on {
        let c = pos2(r.right() - 2.0, r.top() + 2.0);
        painter.circle_filled(c, 10.0, ACCENT());
        painter.circle_stroke(c, 10.0, Stroke::new(2.0, BG0()));
        let ink = Stroke::new(2.0, ACCENT_INK());
        painter.line_segment([pos2(c.x - 4.5, c.y), pos2(c.x - 1.2, c.y + 3.5)], ink);
        painter.line_segment([pos2(c.x - 1.2, c.y + 3.5), pos2(c.x + 4.5, c.y - 3.5)], ink);
    }
}

/// "Personalizado": egui's colour picker under a square with a palette.
/// Returns whether the colour changed.
fn custom_square(ui: &mut Ui, color: &mut [u8; 3], on: bool) -> bool {
    let before = *color;
    let c = Color32::from_rgb(color[0], color[1], color[2]);
    ui.scope(|ui| {
        ui.spacing_mut().interact_size = vec2(SQUARE, SQUARE);
        let resp = egui::widgets::color_picker::color_edit_button_srgb(ui, color)
            .on_hover_text(t!("settings.custom"));
        let tint = if on { c } else { BG2() };
        paint_square(ui, resp.rect, [tint, tint], on, resp.hovered());
        Icon::Palette.image(22.0, if on { Color32::WHITE } else { MUTED() }).paint_at(
            ui,
            Rect::from_center_size(resp.rect.center(), vec2(22.0, 22.0)),
        );
    });
    *color != before
}

/// A pretend corner of the chat in the colours in use.
fn preview(ui: &mut Ui, core: &Core) {
    let Some(me) = core.me.clone() else { return };
    Frame::new()
        .fill(BG2())
        .stroke(Stroke::new(1.0, BG4()))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                widgets::avatar(ui, core, &me, 36.0, false, BG2());
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(me.name())
                                .font(theme::bold(14.0))
                                .color(TEXT()),
                        );
                        ui.label(egui::RichText::new("12:34").font(theme::tiny()).color(FAINT()));
                    });
                    ui.label(egui::RichText::new(t!("settings.preview_text")).color(TEXT()));
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("@babu")
                                .color(ACCENT())
                                .background_color(ACCENT_SOFT()),
                        );
                        ui.label(egui::RichText::new("·").color(MUTED()));
                        ui.label(egui::RichText::new(t!("settings.tab_appearance")).color(MUTED()));
                    });
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let _ = Button::primary(t!("settings.save")).small().show(ui);
                let _ = Button::ghost(t!("common.cancel")).small().show(ui);
            });
        });
}
