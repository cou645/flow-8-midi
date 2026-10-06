use crate::model::{
    flow8::FLOW8Controller,
    message::InterfaceMessage,
    routing::{Setting, FX_ROUTE_DESTS, SETTINGS},
};
use crate::view::widgets::sync_dot;
use iced::{
    widget::{button, column, container, row, text, Column, Space},
    Center, Element, Fill,
};

pub fn view_routing(controller: &FLOW8Controller) -> Element<'_, InterfaceMessage> {
    let mut left = Column::new().spacing(6);
    let mut section = "";
    for s in SETTINGS {
        if s.section != section {
            section = s.section;
            left = left.push(Space::new().height(6)).push(text(section).size(15));
        }
        left = left.push(setting_row(s, controller.settings.get(&s.id).copied()));
    }

    let right = column![
        text("Effects returns").size(15),
        fx_route_column(controller, 0),
        fx_route_column(controller, 1),
        Space::new().height(12),
        text("Needs Bluetooth. Values are read from the mixer on connect;\n\
              a yellow dot means not yet confirmed by the mixer.\n\
              FX routing can't be read back — it shows what was last set here.")
            .size(10),
    ]
    .spacing(8);

    row![
        container(left).width(Fill),
        container(right).width(Fill),
    ]
    .spacing(24)
    .padding([12, 16])
    .into()
}

fn setting_row(s: &'static Setting, current: Option<u8>) -> Element<'static, InterfaceMessage> {
    let mut buttons = row![].spacing(4);
    for &(label, value) in s.options {
        let b = button(text(label).size(11)).padding([4, 10]);
        buttons = buttons.push(if current == Some(value) {
            b.style(button::primary)
        } else {
            b.on_press(InterfaceMessage::SetSetting(s.id, value)).style(button::secondary)
        });
    }
    row![
        sync_dot(current.is_some()),
        container(text(s.label).size(12)).width(170),
        buttons,
    ]
    .spacing(6)
    .align_y(Center)
    .into()
}

fn fx_route_column(controller: &FLOW8Controller, fx: usize) -> Element<'_, InterfaceMessage> {
    let known = controller.fx_routes[fx];
    // ponytail: unknown state assumed all-on (mixer default) for the first toggle.
    let mask = known.unwrap_or(0b111);
    let mut buttons = row![].spacing(4);
    for &(label, bit) in &FX_ROUTE_DESTS {
        let on = mask & bit != 0;
        buttons = buttons.push(
            button(text(label).size(11))
                .padding([4, 10])
                .on_press(InterfaceMessage::FxRoute(fx, mask ^ bit))
                .style(if on { button::primary } else { button::secondary }),
        );
    }
    row![
        sync_dot(known.is_some()),
        container(text(format!("FX {}", fx + 1)).size(12)).width(60),
        buttons,
    ]
    .spacing(6)
    .align_y(Center)
    .into()
}
