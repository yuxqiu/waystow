//! Watch the clipboard: every copy reports the MIME types it offers. The data
//! itself is only read on demand.

use std::sync::Mutex;

use anyhow::Context;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat::WlSeat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self as ext_device, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self as ext_offer, ExtDataControlOfferV1},
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self as wlr_device, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::{self as wlr_offer, ZwlrDataControlOfferV1},
};

type OnCopy = Box<dyn Fn(Vec<String>) + Send>;

/// MIME types collected for one offer.
#[derive(Default)]
pub struct Mimes(Mutex<Vec<String>>);

impl Mimes {
    fn push(&self, m: String) {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(m);
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

struct State {
    on_copy: OnCopy,
    armed: bool,
}

impl State {
    fn selection(&self, mimes: Vec<String>) {
        if self.armed && !mimes.is_empty() {
            (self.on_copy)(mimes);
        }
    }
}

/// Call `on_copy` with the offered MIME types after every copy, from a
/// background thread.
///
/// # Errors
///
/// No Wayland connection, or the compositor lacks data-control.
pub fn watch(on_copy: impl Fn(Vec<String>) + Send + 'static) -> anyhow::Result<()> {
    let conn = Connection::connect_to_env().context("connecting to Wayland")?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let seat: WlSeat = globals.bind(&qh, 1..=1, ()).context("no wl_seat")?;
    let mut state = State { on_copy: Box::new(on_copy), armed: false };
    if let Ok(m) = globals.bind::<ExtDataControlManagerV1, _, _>(&qh, 1..=1, ()) {
        m.get_data_device(&seat, &qh, ());
    } else {
        let m: ZwlrDataControlManagerV1 =
            globals.bind(&qh, 1..=2, ()).context("the compositor supports neither ext- nor wlr-data-control")?;
        m.get_data_device(&seat, &qh, ());
    }
    // A new device reports the current selection at once; that isn't a copy.
    queue.roundtrip(&mut state)?;
    state.armed = true;
    std::thread::Builder::new().name("clipboard-watch".into()).spawn(move || {
        loop {
            if let Err(e) = queue.blocking_dispatch(&mut state) {
                eprintln!("waystow: clipboard watch stopped: {e}");
                return;
            }
        }
    })?;
    Ok(())
}

impl Dispatch<ExtDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        device: &ExtDataControlDeviceV1,
        event: ext_device::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_device::Event::Selection { id: Some(offer) } => {
                let mimes = offer.data::<Mimes>().map(Mimes::take).unwrap_or_default();
                state.selection(mimes);
                offer.destroy();
            }
            ext_device::Event::PrimarySelection { id: Some(offer) } => offer.destroy(),
            ext_device::Event::Finished => device.destroy(),
            _ => {}
        }
    }

    event_created_child!(State, ExtDataControlDeviceV1, [
        ext_device::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, Mimes::default()),
    ]);
}

impl Dispatch<ExtDataControlOfferV1, Mimes> for State {
    fn event(
        _: &mut Self,
        _: &ExtDataControlOfferV1,
        event: ext_offer::Event,
        data: &Mimes,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_offer::Event::Offer { mime_type } = event {
            data.push(mime_type);
        }
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        device: &ZwlrDataControlDeviceV1,
        event: wlr_device::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wlr_device::Event::Selection { id: Some(offer) } => {
                let mimes = offer.data::<Mimes>().map(Mimes::take).unwrap_or_default();
                state.selection(mimes);
                offer.destroy();
            }
            wlr_device::Event::PrimarySelection { id: Some(offer) } => offer.destroy(),
            wlr_device::Event::Finished => device.destroy(),
            _ => {}
        }
    }

    event_created_child!(State, ZwlrDataControlDeviceV1, [
        wlr_device::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, Mimes::default()),
    ]);
}

impl Dispatch<ZwlrDataControlOfferV1, Mimes> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlOfferV1,
        event: wlr_offer::Event,
        data: &Mimes,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wlr_offer::Event::Offer { mime_type } = event {
            data.push(mime_type);
        }
    }
}

/// Objects whose events we don't need.
macro_rules! ignore_events {
    ($state:ty: $($ty:ty),*) => {$(
        impl Dispatch<$ty, ()> for $state {
            fn event(_: &mut Self, _: &$ty, _: <$ty as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}

ignore_events!(State: WlSeat, ExtDataControlManagerV1, ZwlrDataControlManagerV1);

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// A state for one-shot registry listings.
pub struct Ignore;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Ignore {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
