use crate::{run_command, update_label};
use gtk::{gio, glib, prelude::*};
use std::{cell::Cell, ffi::OsStr, fs, rc::Rc, time::Duration};

async fn output(args: &[&OsStr]) -> Option<String> {
    let process = gio::Subprocess::newv(
        args,
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE,
    )
    .ok()?;
    let (stdout, _) = process.communicate_utf8_future(None).await.ok()?;
    process
        .is_successful()
        .then_some(stdout?.trim().to_string())
}

fn cpu_load() -> Option<f64> {
    fs::read_to_string("/proc/loadavg")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn memory_percent() -> Option<u64> {
    let contents = fs::read_to_string("/proc/meminfo").ok()?;
    let field = |name| -> Option<u64> {
        contents.lines().find_map(|line| {
            line.strip_prefix(name)?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
    };
    let total = field("MemTotal:")?;
    let available = field("MemAvailable:")?;
    (available * 100)
        .checked_div(total)
        .map(|free| 100u64.saturating_sub(free))
}

pub fn system_usage(cpu: gtk::Label, memory: gtk::Label) {
    glib::timeout_add_seconds_local(2, move || {
        if let Some(load) = cpu_load() {
            update_label(&cpu, &format!(" {load:.2}%"));
        }
        if let Some(percent) = memory_percent() {
            update_label(&memory, &format!(" {percent}%"));
        }
        glib::ControlFlow::Continue
    });
}

pub fn audio(label: gtk::Label) -> gtk::Button {
    const SINK: &str = "@DEFAULT_AUDIO_SINK@";
    let button = gtk::Button::new();
    button.add_css_class("chip");
    button.set_child(Some(&label));
    button.connect_clicked(|_| {
        run_command(&["wpctl", "set-mute", SINK, "toggle"].map(OsStr::new));
    });
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    scroll.connect_scroll(|_, _, dy| {
        if dy != 0.0 {
            let change = if dy < 0.0 { "2%+" } else { "2%-" };
            run_command(&["wpctl", "set-volume", SINK, change].map(OsStr::new));
        }
        glib::Propagation::Stop
    });
    button.add_controller(scroll);
    glib::MainContext::default().spawn_local(watch_audio(label));
    button
}

async fn update_audio(label: &gtk::Label) {
    let line = output(&["wpctl", "get-volume", "@DEFAULT_AUDIO_SINK@"].map(OsStr::new)).await;
    if let Some(line) = line
        && let Some(volume) = line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<f64>().ok())
    {
        let icon = if line.contains("[MUTED]") || volume == 0.0 {
            "󰝟"
        } else if volume < 0.5 {
            "󰕿"
        } else {
            "󰕾"
        };
        // Map the workstation's configured maximum to the bar's 0–100 scale.
        const DISPLAY_MAX_VOLUME: f64 = 1.56;
        let percent = (volume / DISPLAY_MAX_VOLUME).clamp(0.0, 1.0) * 100.0;
        update_label(label, &format!("{icon} {percent:.0}%"));
    } else {
        update_label(label, "󰝟 —");
    }
}

async fn watch_audio(label: gtk::Label) {
    let launcher = gio::SubprocessLauncher::new(
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE,
    );
    launcher.setenv("LC_ALL", "C", true);
    loop {
        let subscription = launcher.spawn(&["pactl", "subscribe"].map(OsStr::new));
        update_audio(&label).await;
        if let Ok(process) = subscription {
            if let Some(stdout) = process.stdout_pipe() {
                let reader = gio::DataInputStream::new(&stdout);
                while let Ok(Some(line)) =
                    reader.read_line_utf8_future(glib::Priority::DEFAULT).await
                {
                    if line.contains(" on sink #") || line.contains(" on server #") {
                        update_audio(&label).await;
                    }
                }
            }
            process.force_exit();
        }
        glib::timeout_future(Duration::from_secs(2)).await;
    }
}

fn bytes(device: &str, counter: &str) -> Option<u64> {
    fs::read_to_string(format!("/sys/class/net/{device}/statistics/{counter}"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn rate(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1}M", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{}K", bytes / 1024)
    }
}

pub fn network(label: gtk::Label) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("chip");
    button.set_child(Some(&label));
    let alternate = Rc::new(Cell::new(false));
    button.connect_clicked({
        let alternate = alternate.clone();
        move |_| alternate.set(!alternate.get())
    });
    glib::MainContext::default().spawn_local({
        let button = button.clone();
        async move {
            let mut previous: Option<(String, u64, u64, std::time::Instant)> = None;
            loop {
                let devices = output(
                    &[
                        "nmcli",
                        "-t",
                        "--escape",
                        "no",
                        "-f",
                        "DEVICE,TYPE,STATE",
                        "device",
                        "status",
                    ]
                    .map(OsStr::new),
                )
                .await;
                let mut devices =
                    devices
                        .as_deref()
                        .unwrap_or_default()
                        .lines()
                        .filter_map(|line| {
                            let mut fields = line.splitn(3, ':');
                            let (device, kind, state) =
                                (fields.next()?, fields.next()?, fields.next()?);
                            (state == "connected" && matches!(kind, "wifi" | "ethernet"))
                                .then_some((device, kind))
                        });
                if let Some((device, kind)) = devices.next() {
                    button.remove_css_class("disconnected");
                    let counters = bytes(device, "rx_bytes").zip(bytes(device, "tx_bytes"));
                    let now = std::time::Instant::now();
                    if alternate.get()
                        && let Some((rx, tx)) = counters
                    {
                        let (down, up) = previous
                            .as_ref()
                            .filter(|(old_device, ..)| old_device == device)
                            .map_or((0, 0), |(_, old_rx, old_tx, instant)| {
                                let elapsed = now.duration_since(*instant).as_secs_f64().max(1.0);
                                (
                                    (rx.saturating_sub(*old_rx) as f64 / elapsed) as u64,
                                    (tx.saturating_sub(*old_tx) as f64 / elapsed) as u64,
                                )
                            });
                        update_label(&label, &format!("↓{} ↑{}", rate(down), rate(up)));
                    } else if kind == "wifi" {
                        let networks = output(
                            &[
                                "nmcli",
                                "-t",
                                "--escape",
                                "no",
                                "-f",
                                "ACTIVE,SIGNAL,SSID",
                                "device",
                                "wifi",
                                "list",
                                "ifname",
                                device,
                                "--rescan",
                                "no",
                            ]
                            .map(OsStr::new),
                        )
                        .await;
                        let active =
                            networks
                                .as_deref()
                                .unwrap_or_default()
                                .lines()
                                .find_map(|line| {
                                    let (signal, ssid) =
                                        line.strip_prefix("yes:")?.split_once(':')?;
                                    Some((signal.parse::<usize>().ok()?, ssid))
                                });
                        if let Some((signal, ssid)) = active {
                            let icon = ["󰤯", "󰤟", "󰤢", "󰤥", "󰤨"][signal.min(100) * 4 / 100];
                            update_label(&label, &format!("{icon} {ssid}"));
                        } else {
                            update_label(&label, &format!("󰖩 {device}"));
                        }
                    } else {
                        update_label(&label, &format!("󰈀 {device}"));
                    }
                    previous = counters.map(|(rx, tx)| (device.to_owned(), rx, tx, now));
                } else {
                    button.add_css_class("disconnected");
                    update_label(&label, "󰖪");
                    previous = None;
                }
                glib::timeout_future(Duration::from_secs(3)).await;
            }
        }
    });
    button
}
