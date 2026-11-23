use gtk::{gdk, gio, glib, prelude::*};
use std::time::Duration;

const WATCHER: &str = "org.kde.StatusNotifierWatcher";

async fn proxy(name: &str, path: &str, interface: &str) -> Option<gio::DBusProxy> {
    gio::DBusProxy::for_bus_future(
        gio::BusType::Session,
        gio::DBusProxyFlags::DO_NOT_AUTO_START,
        None,
        name,
        path,
        interface,
    )
    .await
    .ok()
}

fn property<T: glib::variant::FromVariant>(object: &gio::DBusProxy, name: &str) -> Option<T> {
    object.cached_property(name)?.get()
}

fn pixels(object: &gio::DBusProxy) -> Option<gdk::MemoryTexture> {
    let images = property::<Vec<(i32, i32, Vec<u8>)>>(object, "IconPixmap")?;
    let (width, height, mut bytes) = images
        .into_iter()
        .filter(|(w, h, data)| {
            (1..=512).contains(w)
                && (1..=512).contains(h)
                && data.len() == *w as usize * *h as usize * 4
        })
        .min_by_key(|(w, h, _)| (w.max(h) - 15).abs())?;
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.rotate_left(1);
    }
    Some(gdk::MemoryTexture::new(
        width,
        height,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(bytes),
        width as usize * 4,
    ))
}

fn invoke(item: &gio::DBusProxy, action: &'static str) {
    let item = item.clone();
    glib::MainContext::default().spawn_local(async move {
        let _ = item
            .call_future(
                action,
                Some(&(0i32, 0i32).to_variant()),
                gio::DBusCallFlags::NONE,
                1_000,
            )
            .await;
    });
}

async fn button(identifier: &str) -> Option<gtk::Button> {
    let (name, path) = identifier
        .find('/')
        .map_or((identifier, "/StatusNotifierItem"), |i| {
            identifier.split_at(i)
        });
    let item = proxy(name, path, "org.kde.StatusNotifierItem").await?;
    if item.name_owner().is_none() {
        return None;
    }
    let image = if let Some(name) = property::<String>(&item, "IconName").filter(|s| !s.is_empty())
    {
        gtk::Image::from_icon_name(&name)
    } else if let Some(texture) = pixels(&item) {
        gtk::Image::from_paintable(Some(&texture))
    } else {
        gtk::Image::from_icon_name("application-x-executable-symbolic")
    };
    image.set_pixel_size(15);
    let button = gtk::Button::new();
    button.add_css_class("tray-item");
    button.set_child(Some(&image));
    button.set_tooltip_text(property::<String>(&item, "Title").as_deref());
    let action = if property::<bool>(&item, "ItemIsMenu").unwrap_or_default() {
        "ContextMenu"
    } else {
        "Activate"
    };
    button.connect_clicked({
        let item = item.clone();
        move |_| invoke(&item, action)
    });
    let right = gtk::GestureClick::new();
    right.set_button(3);
    right.connect_released(move |_, _, _, _| invoke(&item, "ContextMenu"));
    button.add_controller(right);
    Some(button)
}

pub fn build() -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    glib::MainContext::default().spawn_local({
        let container = container.clone();
        async move {
            let mut watcher = None;
            let mut previous = Vec::new();
            loop {
                if watcher.is_none() {
                    watcher = proxy(WATCHER, "/StatusNotifierWatcher", WATCHER).await;
                }
                let registered = watcher
                    .as_ref()
                    .and_then(|watcher| {
                        property::<Vec<String>>(watcher, "RegisteredStatusNotifierItems")
                    })
                    .unwrap_or_default();
                if previous != registered {
                    crate::clear(&container);
                    for id in &registered {
                        if let Some(button) = button(id).await {
                            container.append(&button);
                        }
                    }
                    previous = registered;
                }
                glib::timeout_future(Duration::from_secs(2)).await;
            }
        }
    });
    container
}
