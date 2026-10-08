use crate::model::{
    channels::{Bus, BusType, Channel, FxSlot},
    flow8::FLOW8Controller,
    message::InterfaceMessage,
};
use crate::view::{
    mixer_page::{build_channel_strip, bus_strips_with_phones},
    widgets::{format_level, format_percent, format_send, h_slider, sync_dot, sync_label, v_slider, UNSYNCED_COLOR},
};
use iced::{
    widget::{button, column, container, row, text, tooltip, Column, Row, Space},
    Center, Element, Fill, Length,
};

const COMPACT_FADER_HEIGHT: f32 = 150.0;
const COMPACT_BUS_FADER_HEIGHT: f32 = 185.0;
const COMPACT_FX_BUS_HEIGHT: f32 = 150.0;
const COMPACT_SEND_HEIGHT: f32 = 80.0;

const SNAP_EMPTY_COLOR: iced::Color = iced::Color { r: 0.45, g: 0.45, b: 0.45, a: 1.0 };
const SNAP_NUM_COLOR: iced::Color = iced::Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
const SNAP_NAME_COLOR: iced::Color = iced::Color { r: 0.85, g: 0.85, b: 0.85, a: 1.0 };

const TOGGLE_ON_COLOR: iced::Color = iced::Color { r: 0.6, g: 0.3, b: 0.9, a: 1.0 };
const MUTE_COLOR: iced::Color = iced::Color { r: 0.9, g: 0.2, b: 0.2, a: 1.0 };

pub fn view_mixer_fx(controller: &FLOW8Controller) -> Element<'_, InterfaceMessage> {
    let channels_row: Vec<Element<InterfaceMessage>> = controller
        .channels
        .iter()
        .map(|c| build_channel_strip(c, COMPACT_FADER_HEIGHT, controller.channel_meters(c.id)))
        .collect();

    let channels_section = container(
        iced::widget::Row::with_children(channels_row)
            .spacing(3)
            .width(Fill),
    )
    .width(Length::FillPortion(7));

    let bus_strips = bus_strips_with_phones(controller, COMPACT_BUS_FADER_HEIGHT);
    let portion = bus_strips.len() as u16;

    let buses_section = container(
        iced::widget::Row::with_children(bus_strips)
            .spacing(3)
            .width(Fill),
    )
    .width(Length::FillPortion(portion));

    let mixer_row = row![channels_section, Space::new().width(6), buses_section]
        .width(Fill)
        .padding([0, 10]);

    let fx_buses: Vec<&Bus> = controller
        .buses
        .iter()
        .filter(|b| b.bus_type == BusType::Fx)
        .collect();

    let fx1_col = build_fx_slot_column(&controller.fx_slots[0]);
    let fx2_col = build_fx_slot_column(&controller.fx_slots[1]);
    let fx1_bus = build_fx_bus_fader(fx_buses.first().copied());
    let fx2_bus = build_fx_bus_fader(fx_buses.get(1).copied());
    let mute_btn = build_mute_btn(controller.fx_muted);
    let tap_btn = build_tap_tempo_btn();

    let fx_row = row![
        container(fx1_col).width(Fill),
        Space::new().width(4),
        fx1_bus,
        Space::new().width(12),
        container(fx2_col).width(Fill),
        Space::new().width(4),
        fx2_bus,
        Space::new().width(16),
        column![mute_btn, Space::new().height(8), tap_btn].align_x(Center),
    ]
    .width(Fill)
    .align_y(Center)
    .padding([0, 10]);

    let snap_row = build_compact_snapshot_row(controller);
    let sends_row = build_compact_sends(controller);

    column![
        mixer_row,
        Space::new().height(10),
        fx_row,
        Space::new().height(10),
        container(
            column![
                text("Snapshots 1–4").size(11).color(iced::Color { r: 0.6, g: 0.6, b: 0.6, a: 1.0 }),
                Space::new().height(4),
                snap_row,
            ]
        ).padding([0, 10]),
        Space::new().height(8),
        container(
            column![
                text("Sends").size(11).color(iced::Color { r: 0.6, g: 0.6, b: 0.6, a: 1.0 }),
                Space::new().height(4),
                sends_row,
            ]
        ).padding([0, 10]),
    ]
    .width(Fill)
    .height(Fill)
    .padding([8, 0])
    .into()
}

fn build_fx_slot_column(fx: &FxSlot) -> Element<'_, InterfaceMessage> {
    let fx_id = fx.id;
    let info = fx.preset_info();
    let is_on = fx.param2_is_on();

    let preset_grid = build_preset_grid(fx);

    let param1_row = row![
        sync_label(info.param1_label, 12.0, fx.param1_synced).width(80),
        h_slider(
            0..=100,
            fx.param1,
            move |v| InterfaceMessage::FxParam1(fx_id, v),
            format_percent(fx.param1),
        ),
    ]
    .align_y(Center)
    .spacing(8)
    .width(Fill);

    let param2_toggle = build_param2_toggle(
        fx_id,
        info.param2_off,
        info.param2_on,
        is_on,
        fx.param2_synced,
    );

    let content = column![
        text(format!("FX {}", fx_id + 1)).size(15),
        Space::new().height(4),
        preset_grid,
        Space::new().height(8),
        param1_row,
        Space::new().height(6),
        param2_toggle,
    ]
    .padding([10, 10])
    .width(Fill)
    .spacing(2);

    container(content)
        .style(container::rounded_box)
        .width(Fill)
        .padding(2)
        .into()
}

fn build_fx_bus_fader(bus: Option<&Bus>) -> Element<'_, InterfaceMessage> {
    if let Some(bus) = bus {
        let bus_idx = bus.index;
        let bus_id = bus.id;
        column![
            text(bus.label()).size(12),
            Space::new().height(4),
            sync_label("Level", 10.0, bus.bus_strip.level_synced),
            v_slider(
                1..=127,
                bus.bus_strip.level,
                COMPACT_FX_BUS_HEIGHT,
                move |v| InterfaceMessage::BusLevel(bus_idx, bus_id, v),
                format_level(bus.bus_strip.level),
            ),
        ]
        .align_x(Center)
        .spacing(4)
        .width(60)
        .into()
    } else {
        Space::new().width(60).into()
    }
}

fn build_mute_btn(fx_muted: bool) -> Element<'static, InterfaceMessage> {
    let label = if fx_muted { "FX MUTED" } else { "FX MUTE" };
    let btn = button(text(label).size(12).center())
        .on_press(InterfaceMessage::FxMute)
        .padding([8, 16])
        .width(120);
    if fx_muted {
        btn.style(move |theme, status| {
            let mut style = button::danger(theme, status);
            style.background = Some(iced::Background::Color(MUTE_COLOR));
            style
        })
        .into()
    } else {
        btn.style(button::secondary).into()
    }
}

fn build_tap_tempo_btn() -> Element<'static, InterfaceMessage> {
    let tap_btn = button(text("TAP TEMPO").size(12).center())
        .on_press(InterfaceMessage::TapTempo)
        .padding([8, 16])
        .width(120)
        .style(button::secondary);
    tooltip(
        tap_btn,
        container(
            text("Tap to set FX2 tempo.\nOnly works with delay/echo (1-12).").size(10),
        )
        .padding(6)
        .style(container::rounded_box),
        tooltip::Position::Top,
    )
    .gap(4)
    .into()
}

fn build_preset_grid(fx: &FxSlot) -> Element<'_, InterfaceMessage> {
    let fx_id = fx.id;
    let current = fx.preset as usize;
    let presets = fx.presets();

    let mut rows: Vec<Element<InterfaceMessage>> = Vec::new();
    for row_idx in 0..4 {
        let mut buttons: Vec<Element<InterfaceMessage>> = Vec::new();
        for col in 0..4 {
            let idx = row_idx * 4 + col;
            let preset = &presets[idx];
            let is_selected = idx == current;
            let label = column![text(preset.name).size(11)].align_x(Center);
            let btn = if is_selected {
                button(label)
                    .padding([6, 4])
                    .width(Fill)
                    .style(button::primary)
            } else {
                button(label)
                    .on_press(InterfaceMessage::FxPreset(fx_id, idx as u8))
                    .padding([6, 4])
                    .width(Fill)
                    .style(button::secondary)
            };
            buttons.push(btn.into());
        }
        rows.push(Row::with_children(buttons).spacing(4).width(Fill).into());
    }

    let mut grid = Column::new().spacing(4).width(Fill);
    for r in rows {
        grid = grid.push(r);
    }

    column![
        sync_label("Preset", 12.0, fx.preset_synced),
        Space::new().height(4),
        grid,
    ]
    .width(Fill)
    .into()
}

fn build_param2_toggle<'a>(
    fx_id: u8,
    off_label: &'a str,
    on_label: &'a str,
    is_on: bool,
    is_synced: bool,
) -> Element<'a, InterfaceMessage> {
    let off_btn = if !is_on {
        button(text(off_label).size(12).center())
            .padding([6, 12])
            .width(Fill)
            .style(move |theme, status| {
                let mut style = button::primary(theme, status);
                style.background = Some(iced::Background::Color(TOGGLE_ON_COLOR));
                style
            })
    } else {
        button(text(off_label).size(12).center())
            .on_press(InterfaceMessage::FxParam2(fx_id, 0))
            .padding([6, 12])
            .width(Fill)
            .style(button::secondary)
    };

    let on_btn = if is_on {
        button(text(on_label).size(12).center())
            .padding([6, 12])
            .width(Fill)
            .style(move |theme, status| {
                let mut style = button::primary(theme, status);
                style.background = Some(iced::Background::Color(TOGGLE_ON_COLOR));
                style
            })
    } else {
        button(text(on_label).size(12).center())
            .on_press(InterfaceMessage::FxParam2(fx_id, 127))
            .padding([6, 12])
            .width(Fill)
            .style(button::secondary)
    };

    let sync_indicator = if is_synced {
        text("")
    } else {
        text("\u{25CF} ").size(10).color(UNSYNCED_COLOR)
    };

    row![sync_indicator, off_btn, on_btn]
        .spacing(4)
        .align_y(Center)
        .width(Fill)
        .into()
}

fn build_compact_snapshot_row(controller: &FLOW8Controller) -> Element<'_, InterfaceMessage> {
    let buttons: Vec<Element<InterfaceMessage>> = (0..4)
        .map(|i| {
            let snapshot_num = i + 1;
            let name = controller.snapshot_names.get(i).and_then(|n| n.as_ref());
            let has_name = name.is_some();

            let btn_content = match name {
                Some(n) => column![
                    text(format!("{:02}", snapshot_num)).size(14).align_x(Center).color(SNAP_NUM_COLOR),
                    text(n.as_str()).size(11).align_x(Center).color(SNAP_NAME_COLOR),
                ]
                .align_x(Center)
                .spacing(2),
                None => column![
                    text(format!("{:02}", snapshot_num)).size(14).align_x(Center).color(SNAP_EMPTY_COLOR),
                    text(" ").size(11),
                ]
                .align_x(Center)
                .spacing(2),
            };

            button(btn_content)
                .on_press(InterfaceMessage::LoadSnapshot(i as u8))
                .padding([8, 8])
                .width(Fill)
                .style(if has_name { button::primary } else { button::secondary })
                .into()
        })
        .collect();

    Row::with_children(buttons).spacing(6).width(Fill).into()
}

fn build_compact_sends(controller: &FLOW8Controller) -> Element<'_, InterfaceMessage> {
    let children: Vec<Element<InterfaceMessage>> = controller
        .channels
        .iter()
        .map(|c| build_compact_channel_sends(c))
        .collect();

    Row::with_children(children).spacing(2).width(Fill).into()
}

fn build_compact_channel_sends(channel: &Channel) -> Element<'_, InterfaceMessage> {
    let ch_id = channel.id;
    let sends = &channel.sends;

    let label = text(channel.display_label()).size(9).align_x(Center);

    let faders = row![
        compact_send_fader("M1", sends.mon1, sends.mon1_synced, move |v| {
            InterfaceMessage::SendMon1(ch_id, v)
        }),
        compact_send_fader("M2", sends.mon2, sends.mon2_synced, move |v| {
            InterfaceMessage::SendMon2(ch_id, v)
        }),
        compact_send_fader("F1", sends.fx1, sends.fx1_synced, move |v| {
            InterfaceMessage::SendFx1(ch_id, v)
        }),
        compact_send_fader("F2", sends.fx2, sends.fx2_synced, move |v| {
            InterfaceMessage::SendFx2(ch_id, v)
        }),
    ]
    .spacing(4)
    .align_y(iced::Alignment::End);

    container(
        column![label, Space::new().height(4), faders]
            .align_x(Center)
            .padding([4, 4]),
    )
    .style(container::rounded_box)
    .width(Fill)
    .into()
}

fn compact_send_fader<'a, F>(
    label: &'a str,
    value: u8,
    is_synced: bool,
    on_change: F,
) -> Column<'a, InterfaceMessage>
where
    F: Fn(u8) -> InterfaceMessage + 'a,
{
    column![
        sync_dot(is_synced),
        v_slider(0..=127, value, COMPACT_SEND_HEIGHT, on_change, format_send(value)),
        text(label).size(9),
    ]
    .align_x(Center)
    .spacing(2)
}
