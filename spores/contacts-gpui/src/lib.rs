//! Skykeep Contacts, drawn by the Spore itself.
//!
//! The native path: layout, text shaping and drawing are the guest's, and the
//! host receives one texture per frame through `soil:present`. The shape here
//! follows `spores/terminal-gpui` and `spores/probe-gpui` in the spores
//! repository (MIT); the view and the records reading are this package's.
//!
//! The look is the one macOS Contacts and Skykeep's contacts window share: a
//! rail of names on the left, a white card on the right with the person's name
//! at the top and the fields they actually have beneath it — no empty rows for
//! an email somebody never gave.

wit_bindgen::generate!({
    path: "wit",
    world: "runner",
    with: {
        "wasi:webgpu/webgpu@0.3.0-rc.2": wgpu_wasi_webgpu::wasi::webgpu::webgpu,
    },
    generate_all,
});

use std::borrow::Cow;
use std::sync::Arc;

use gpui::{
    AnyWindowHandle, AppContext as _, Context, FontWeight, HeadlessAppContext, IntoElement,
    PlatformTextSystem as _, Render, SharedString, Window, div, prelude::*, px, rgb, size,
};
use gpui_wgpu::{CosmicTextSystem, WgpuContext, WgpuHeadlessRenderer};
use soil::present::{cursor, input, surface};
use soil::records::records;
use serde_json::Value;

const LILEX: &[u8] = include_bytes!("../assets/Lilex-Regular.ttf");
const LILEX_BOLD: &[u8] = include_bytes!("../assets/Lilex-Bold.ttf");

const CONTACT_SCHEMA: &str = "fungos.contacts/contact";
const PAGE: u32 = 256;
const MAX: usize = 4_000;

const HEADER_H: f32 = 52.0;
const RAIL_W: f32 = 268.0;
const ROW_H: f32 = 34.0;

// The paper palette: what a person sees on a Mac, not a terminal.
const BG: u32 = 0xeff1f4;
const HEAD: u32 = 0xf8f9fb;
const RAIL: u32 = 0xfafbfc;
const CARD: u32 = 0xffffff;
const INK: u32 = 0x1f2328;
const MUTED: u32 = 0x6b7280;
const ACCENT: u32 = 0x2f6fd6;
const SELECTED: u32 = 0xdae6f8;
const RULE: u32 = 0xe3e6ea;

/// One person, as much of a card as the view draws.
#[derive(Clone, Default)]
struct Person {
    id: String,
    name: String,
    initials: String,
    organization: String,
    title: String,
    emails: Vec<(String, String)>,
    phones: Vec<(String, String)>,
    birthday: String,
    note: String,
}

impl Person {
    fn from_body(id: String, body: &Value) -> Self {
        fn text(body: &Value, key: &str) -> String {
            body.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        }
        fn methods(body: &Value, key: &str) -> Vec<(String, String)> {
            body.get(key)
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|item| {
                            let value = item.get("value")?.as_str()?.to_string();
                            if value.is_empty() {
                                return None;
                            }
                            let label = item
                                .get("label")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            Some((label, value))
                        })
                        .collect()
                })
                .unwrap_or_default()
        }

        let given = text(body, "given_name");
        let family = text(body, "family_name");
        let display = {
            let display = text(body, "display_name");
            if display.is_empty() {
                format!("{given} {family}").trim().to_string()
            } else {
                display
            }
        };
        let name = if display.is_empty() {
            methods(body, "emails")
                .first()
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| "No name".to_string())
        } else {
            display
        };
        let initials = [given.chars().next(), family.chars().next()]
            .into_iter()
            .flatten()
            .collect::<String>()
            .to_uppercase();
        Self {
            id,
            name,
            initials: if initials.is_empty() {
                "?".to_string()
            } else {
                initials
            },
            organization: text(body, "organization"),
            title: text(body, "title"),
            emails: methods(body, "emails"),
            phones: methods(body, "phones"),
            birthday: text(body, "birthday"),
            note: text(body, "note"),
        }
    }
}

/// The address book, read once per frame loop from `soil:records`.
fn load() -> (Vec<Person>, String) {
    let mut people = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = match records::scan(CONTACT_SCHEMA, cursor.as_deref(), PAGE) {
            Ok(page) => page,
            Err(error) => {
                // A reader that renders nothing must say why on screen: a guest's
                // stderr is not something a host is obliged to show anybody.
                return (people, format!("records: {error:?}"));
            }
        };
        for entry in page.entries {
            if entry.deleted || entry.schema != CONTACT_SCHEMA {
                continue;
            }
            if let Ok(body) = serde_json::from_slice::<Value>(&entry.body) {
                people.push(Person::from_body(entry.key.id, &body));
            }
        }
        if people.len() >= MAX {
            break;
        }
        match page.next {
            Some(next) if Some(next.as_str()) != cursor.as_deref() => cursor = Some(next),
            _ => break,
        }
    }
    people.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    let status = if people.is_empty() {
        "records: no contacts in this book".to_string()
    } else {
        format!("records: {} contacts", people.len())
    };
    (people, status)
}

struct Contacts {
    width: u32,
    height: u32,
    scale: f64,
    people: Vec<Person>,
    status: String,
    selected: usize,
}

impl Contacts {
    fn new() -> Self {
        let (width, height) = surface::size();
        let (people, status) = load();
        Self {
            width,
            height,
            scale: surface::scale(),
            people,
            status,
            selected: 0,
        }
    }

    fn chosen(&self) -> Option<&Person> {
        self.people.get(self.selected)
    }

    fn rail(&self) -> impl IntoElement {
        let mut rows = Vec::new();
        for (index, person) in self.people.iter().enumerate() {
            let selected = index == self.selected;
            rows.push(
                div()
                    .h(px(ROW_H))
                    .flex()
                    .items_center()
                    .px(px(14.0))
                    .gap(px(10.0))
                    .when(selected, |row| row.bg(rgb(SELECTED)))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(if selected { ACCENT } else { MUTED }))
                            .child(SharedString::from(person.initials.clone())),
                    )
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(rgb(INK))
                            .child(SharedString::from(person.name.clone())),
                    ),
            );
        }
        if self.people.is_empty() {
            rows.push(
                div()
                    .px(px(14.0))
                    .py(px(10.0))
                    .text_size(px(12.0))
                    .text_color(rgb(MUTED))
                    .child("No contacts yet."),
            );
        }
        div()
            .w(px(RAIL_W))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(RAIL))
            .border_r(px(1.0))
            .border_color(rgb(RULE))
            .child(
                div()
                    .h(px(30.0))
                    .flex()
                    .items_center()
                    .px(px(14.0))
                    .text_size(px(10.0))
                    .text_color(rgb(MUTED))
                    .child("CONTACTS"),
            )
            .children(rows)
    }

    fn field(&self, label: &'static str, value: SharedString) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(3.0))
            .child(
                div()
                    .text_size(px(9.0))
                    .text_color(rgb(MUTED))
                    .child(label),
            )
            .child(div().text_size(px(14.0)).text_color(rgb(INK)).child(value))
    }

    fn card(&self) -> impl IntoElement {
        let mut fields = Vec::new();
        if let Some(person) = self.chosen() {
            for (label, value) in &person.emails {
                let line = if label.is_empty() {
                    value.clone()
                } else {
                    format!("{label}  {value}")
                };
                fields.push(self.field("EMAIL", SharedString::from(line)).into_any_element());
            }
            for (label, value) in &person.phones {
                let line = if label.is_empty() {
                    value.clone()
                } else {
                    format!("{label}  {value}")
                };
                fields.push(self.field("PHONE", SharedString::from(line)).into_any_element());
            }
            if !person.birthday.is_empty() {
                fields.push(
                    self.field("BIRTHDAY", SharedString::from(person.birthday.clone()))
                        .into_any_element(),
                );
            }
            if !person.note.is_empty() {
                fields.push(
                    self.field("NOTE", SharedString::from(person.note.clone()))
                        .into_any_element(),
                );
            }
        }

        let head = self.chosen().map(|person| {
            div()
                .flex()
                .items_center()
                .gap(px(18.0))
                .child(
                    div()
                        .w(px(64.0))
                        .h(px(64.0))
                        .rounded(px(14.0))
                        .bg(rgb(ACCENT))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(22.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(0xffffff))
                        .child(SharedString::from(person.initials.clone())),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(26.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(INK))
                                .child(SharedString::from(person.name.clone())),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(rgb(MUTED))
                                .child(SharedString::from(format!(
                                    "{} {}",
                                    person.organization, person.title
                                )
                                .trim()
                                .to_string())),
                        ),
                )
        });

        div()
            .flex_1()
            .h_full()
            .p(px(24.0))
            .child(
                div()
                    .w_full()
                    .h_full()
                    .p(px(26.0))
                    .flex()
                    .flex_col()
                    .gap(px(18.0))
                    .rounded(px(12.0))
                    .bg(rgb(CARD))
                    .border(px(1.0))
                    .border_color(rgb(RULE))
                    .children(head)
                    .child(div().h(px(1.0)).w_full().bg(rgb(RULE)))
                    .child(div().flex().flex_col().gap(px(14.0)).children(fields)),
            )
    }
}

impl Render for Contacts {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.people.len();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .font_family("Lilex")
            .child(
                div()
                    .h(px(HEADER_H))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(22.0))
                    .bg(rgb(HEAD))
                    .border_b(px(1.0))
                    .border_color(rgb(RULE))
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(INK))
                            .child("Contacts"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(MUTED))
                            .child(SharedString::from(format!(
                                "{count} people · {} · {}×{} at {}x",
                                self.status, self.width, self.height, self.scale
                            ))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .overflow_hidden()
                    .child(self.rail())
                    .child(self.card()),
            )
    }
}

fn present_frame(context: &WgpuContext, width: u32, height: u32, rgba: &[u8]) -> anyhow::Result<()> {
    let texture = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("contacts frame"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    context.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let texture = wgpu_wasi_webgpu::host_texture(texture);
    surface::present(&texture).map_err(|error| anyhow::anyhow!("present: {error}"))?;
    Ok(())
}

fn run_contacts() -> anyhow::Result<()> {
    let (width, height) = surface::size();
    let instance = wgpu_wasi_webgpu::instance();
    let adapter = wit_bindgen::block_on(instance.request_adapter(&Default::default()))?;
    let (device, queue) = wit_bindgen::block_on(adapter.request_device(&Default::default()))?;
    let (context, format) = WgpuContext::from_offscreen(instance, adapter, device, queue)?;

    let text = Arc::new(CosmicTextSystem::new_without_system_fonts("Lilex"));
    text.add_fonts(vec![Cow::Borrowed(LILEX), Cow::Borrowed(LILEX_BOLD)])?;
    let factory_context = context.clone();
    let mut cx = HeadlessAppContext::with_platform(text, Arc::new(()), move || {
        Ok(Some(Box::new(WgpuHeadlessRenderer::with_context(
            factory_context.clone(),
            format,
        )?)))
    });

    let scale = surface::scale() as f32;
    let window = cx.open_window_scaled(
        size(px(width as f32), px(height as f32)),
        scale,
        |_, cx| cx.new(|_| Contacts::new()),
    )?;
    let any: AnyWindowHandle = window.into();

    let mut window_size = (width, height);
    let mut dirty = true;
    loop {
        let size_now = surface::size();
        if size_now != window_size {
            window_size = size_now;
            window.update(&mut cx, |view, window, _| {
                view.width = size_now.0;
                view.height = size_now.1;
                window.resize(gpui::size(px(size_now.0 as f32), px(size_now.1 as f32)));
            })?;
            dirty = true;
        }

        if dirty {
            cx.update_window(any, |_, window, app| {
                let _ = window.draw(app);
            })?;
            let image = cx.capture_screenshot(any)?;
            present_frame(&context, image.width(), image.height(), image.as_raw())?;
            dirty = false;
        }

        if !input::interactive() {
            break;
        }
        let mut any_event = false;
        while let Some(event) = input::poll(0) {
            any_event = true;
            match event {
                input::Event::KeyInput(key) => {
                    if key.key == "escape" {
                        return Ok(());
                    }
                }
                input::Event::Button(button) => {
                    if button.pressed {
                        let y = button.y - HEADER_H - 30.0;
                        if button.x < RAIL_W && y >= 0.0 {
                            let row = (y / ROW_H).floor() as usize;
                            window.update(&mut cx, |view, _, cx| {
                                if row < view.people.len() {
                                    view.selected = row;
                                    cx.notify();
                                }
                            })?;
                        }
                    }
                }
                _ => {}
            }
        }
        if any_event {
            dirty = true;
        }
        let _ = cursor::set(cursor::CursorStyle::Default);
        if !any_event {
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    }
    Ok(())
}

struct Component;

impl Guest for Component {
    async fn run() -> u32 {
        match run_contacts() {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("contacts-gpui: {error:#}");
                1
            }
        }
    }
}

export!(Component);
