mod metrics;
mod tray;

use gtk::{gio, glib, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use serde::Deserialize;
use std::{ffi::OsStr, time::Duration};

#[derive(Deserialize)]
struct MonitorList {
    monitors: Vec<Monitor>,
}

#[derive(Deserialize)]
struct Monitor {
    active: bool,
    layout_symbol: String,
    tags: Vec<Tag>,
    active_client: Option<Client>,
    #[serde(default)]
    hide_clients: u32,
}

#[derive(Deserialize, PartialEq)]
struct Tag {
    index: u32,
    is_active: bool,
    is_urgent: bool,
    #[serde(default)]
    name: Option<String>,
    client_count: u32,
}

#[derive(Deserialize)]
struct Client {
    title: String,
}

fn run_command(args: &[&OsStr]) {
    let result = gio::Subprocess::newv(args, gio::SubprocessFlags::STDOUT_SILENCE);
    match result {
        Ok(process) => process.wait_check_async(gio::Cancellable::NONE, |result| {
            if let Err(error) = result {
                eprintln!("mangobar-rs: {error}");
            }
        }),
        Err(error) => eprintln!("mangobar-rs: {error}"),
    }
}

fn update_label(label: &gtk::Label, text: &str) {
    if label.label() != text {
        label.set_label(text);
    }
}

fn clear(box_: &gtk::Box) {
    while let Some(child) = box_.first_child() {
        box_.remove(&child);
    }
}

fn update_tags(container: &gtk::Box, tags: &[Tag]) {
    clear(container);
    for tag in tags
        .iter()
        .filter(|tag| tag.is_active || tag.is_urgent || tag.client_count > 0)
    {
        let text = tag.name.clone().unwrap_or_else(|| match tag.index {
            4 => "编".to_owned(),
            5 => "网".to_owned(),
            index => index.to_string(),
        });
        let button = gtk::Button::with_label(&text);
        button.add_css_class("tag");
        if tag.client_count > 0 {
            button.add_css_class("occupied");
        }
        if tag.is_active {
            button.add_css_class("active");
        }
        if tag.is_urgent {
            button.add_css_class("urgent");
        }
        let command = format!("view,{}", tag.index);
        button.connect_clicked(move |_| {
            run_command(&[
                OsStr::new("mmsg"),
                OsStr::new("dispatch"),
                OsStr::new(&command),
            ]);
        });
        let right = gtk::GestureClick::new();
        right.set_button(3);
        let command = format!("toggleview,{}", tag.index);
        right.connect_released(move |_, _, _, _| {
            run_command(&["mmsg", "dispatch", &command].map(OsStr::new));
        });
        button.add_controller(right);
        container.append(&button);
    }
}

async fn follow_mango(tags: gtk::Box, layout: gtk::Label, title: gtk::Label, hidden: gtk::Label) {
    loop {
        let process = gio::Subprocess::newv(
            &["mmsg".as_ref(), "watch".as_ref(), "all-monitors".as_ref()],
            gio::SubprocessFlags::STDOUT_PIPE,
        );
        if let Ok(process) = process {
            let Some(stdout) = process.stdout_pipe() else {
                return;
            };
            let reader = gio::DataInputStream::new(&stdout);
            let mut previous = Vec::new();
            while let Ok(Some(line)) = reader.read_line_utf8_future(glib::Priority::DEFAULT).await {
                let Ok(status) = serde_json::from_str::<MonitorList>(&line) else {
                    continue;
                };
                let Some(monitor) = status.monitors.into_iter().find(|monitor| monitor.active)
                else {
                    continue;
                };
                if previous != monitor.tags {
                    update_tags(&tags, &monitor.tags);
                    previous = monitor.tags;
                }
                update_label(&layout, &monitor.layout_symbol);
                hidden.set_visible(monitor.hide_clients > 0);
                update_label(&hidden, &format!(" {}", monitor.hide_clients));
                let window_title = monitor
                    .active_client
                    .as_ref()
                    .map_or("", |client| client.title.as_str());
                if title.label() != window_title {
                    title.set_label(window_title);
                }
            }
            process.force_exit();
        }
        clear(&tags);
        layout.set_label("");
        hidden.set_visible(false);
        title.set_label("Brak połączenia z MangoWC");
        glib::timeout_future(Duration::from_secs(2)).await;
    }
}

fn update_clocks(date: &gtk::Label, clock: &gtk::Label) {
    if let Ok(now) = glib::DateTime::now_local() {
        if let Ok(text) = now.format(" %b%d·%A") {
            update_label(date, &text);
        }
        if let Ok(text) = now.format(" %H:%M") {
            update_label(clock, &text);
        }
    }
}

fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class(class);
    label
}

fn build(app: &gtk::Application) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("MangoBar Rust")
        .build();
    window.init_layer_shell();
    window.set_namespace(Some("mangobar-rs"));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_exclusive_zone(0);
    for edge in [Edge::Left, Edge::Right, Edge::Top] {
        window.set_anchor(edge, true);
    }

    window.add_css_class("bar-window");
    window.set_default_size(-1, 24);
    let bar = gtk::CenterBox::new();
    bar.add_css_class("bar");

    let left = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let apps = gtk::Button::with_label("󰍉 APPS");
    apps.add_css_class("apps");
    apps.connect_clicked(|_| {
        let launcher = glib::home_dir().join(".local/bin/fzf-ultra");
        run_command(&[launcher.as_os_str(), OsStr::new("toggle")]);
    });
    let tags = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let layout = label("", "chip");
    let title = label("", "title");
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_xalign(0.0);
    left.append(&apps);
    left.append(&tags);
    left.append(&layout);
    left.append(&title);
    bar.set_start_widget(Some(&left));

    let date = label("", "date");
    let clock = label("", "chip");
    update_clocks(&date, &clock);
    glib::timeout_add_seconds_local(1, {
        let date = date.clone();
        let clock = clock.clone();
        move || {
            update_clocks(&date, &clock);
            glib::ControlFlow::Continue
        }
    });
    bar.set_center_widget(Some(&date));

    let right = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let hidden = label("", "chip");
    hidden.set_visible(false);
    let tray = tray::build();
    let network = metrics::network(label("󰖪", "value"));
    let cpu = label(" —", "chip");
    let memory = label(" —", "chip");
    metrics::system_usage(cpu.clone(), memory.clone());
    let audio = metrics::audio(label("󰝟 —", "value"));
    right.append(&hidden);
    right.append(&tray);
    right.append(&network);
    right.append(&cpu);
    right.append(&clock);
    right.append(&memory);
    right.append(&audio);
    bar.set_end_widget(Some(&right));
    window.set_child(Some(&bar));
    let style = gtk::CssProvider::new();
    style.load_from_data(include_str!("../style.css"));
    gtk::style_context_add_provider_for_display(
        &gtk::prelude::WidgetExt::display(&window),
        &style,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    glib::MainContext::default().spawn_local(follow_mango(tags, layout, title, hidden));
    window.present();
}

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("io.github.rpaloha.MangoBarRust")
        .build();
    app.connect_activate(|app| {
        if let Some(window) = app.active_window() {
            window.present();
        } else {
            build(app);
        }
    });
    app.run()
}
