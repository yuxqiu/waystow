//! dnd-probe: drives drag-and-drop with a wlr virtual pointer, for WayStow's
//! automated tests in a headless sway. Never run it on a real desktop: it
//! moves and clicks a pointer.
//!
//! ```text
//! dnd-probe drag-in X Y [X2 Y2] -- FILE...   drag FILEs from the probe to (X,Y), then (X2,Y2), drop
//! dnd-probe drag-out X Y                     press at (X,Y), drag to the probe, print what it receives
//! dnd-probe move X Y                         move the pointer
//! dnd-probe click X Y                        click
//! dnd-probe drag X1 Y1 X2 Y2                 press at 1, move to 2, release (drags inside other clients)
//! ```
//!
//! The probe is a 160×160 layer surface at the top-left corner; coordinates
//! are in the first output's logical pixels.

use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::data_device_manager::data_device::{DataDevice, DataDeviceHandler};
use smithay_client_toolkit::data_device_manager::data_offer::{DataOfferHandler, DragOffer};
use smithay_client_toolkit::data_device_manager::data_source::{DataSourceHandler, DragSource};
use smithay_client_toolkit::data_device_manager::{DataDeviceManagerState, WritePipe};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_data_device, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, registry_handlers,
};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_data_device::WlDataDevice;
use wayland_client::protocol::wl_data_device_manager::DndAction;
use wayland_client::protocol::wl_data_source::WlDataSource;
use wayland_client::protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

const SIZE: u32 = 160;

#[derive(Debug, Clone)]
enum Mode {
    DragIn { path: Vec<(f64, f64)>, files: Vec<PathBuf> },
    DragOut { from: (f64, f64) },
    Move(f64, f64),
    Click(f64, f64),
    Drag((f64, f64), (f64, f64)),
}

fn parse_args() -> anyhow::Result<Mode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let num = |i: usize| -> anyhow::Result<f64> {
        args.get(i).context("missing coordinate")?.parse().context("coordinates are numbers")
    };
    Ok(match args.first().map(String::as_str) {
        Some("drag-in") => {
            let sep = args.iter().position(|a| a == "--").context("drag-in needs -- FILE...")?;
            let coords: Vec<f64> = (1..sep).map(num).collect::<Result<_, _>>()?;
            if coords.len() < 2 || !coords.len().is_multiple_of(2) {
                bail!("drag-in takes X Y [X2 Y2 ...]");
            }
            let path = coords.chunks(2).map(|c| (c[0], c[1])).collect();
            let files =
                args[sep + 1..].iter().map(|f| std::fs::canonicalize(f).context("file")).collect::<Result<_, _>>()?;
            Mode::DragIn { path, files }
        }
        Some("drag-out") => Mode::DragOut { from: (num(1)?, num(2)?) },
        Some("move") => Mode::Move(num(1)?, num(2)?),
        Some("click") => Mode::Click(num(1)?, num(2)?),
        Some("drag") => Mode::Drag((num(1)?, num(2)?), (num(3)?, num(4)?)),
        _ => bail!(
            "usage: dnd-probe drag-in X Y [X2 Y2] -- FILE... | drag-out X Y | drag X1 Y1 X2 Y2 | move X Y | click X Y"
        ),
    })
}

struct Probe {
    registry: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    _compositor: CompositorState,
    shm: Shm,
    ddm: DataDeviceManagerState,
    layer: LayerSurface,
    pool: SlotPool,
    vp: ZwlrVirtualPointerV1,
    device: Option<DataDevice>,
    pointer: Option<wl_pointer::WlPointer>,
    configured: bool,
    pressed_serial: Option<u32>,
    source: Option<DragSource>,
    uri_list: String,
    done: Option<Result<String, String>>,
    extent: (u32, u32),
    start: Instant,
}

fn main() -> anyhow::Result<()> {
    let mode = parse_args()?;
    let conn = Connection::connect_to_env().context("no Wayland display")?;
    let (globals, queue) = registry_queue_init::<Probe>(&conn)?;
    let qh = queue.handle();
    let mut ev: EventLoop<Probe> = EventLoop::try_new()?;
    WaylandSource::new(conn.clone(), queue).insert(ev.handle()).map_err(|e| anyhow::anyhow!("{e}"))?;

    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;
    let vpm: ZwlrVirtualPointerManagerV1 = globals.bind(&qh, 1..=2, ()).context("no virtual pointer protocol")?;
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).context("no seat")?;
    let vp = vpm.create_virtual_pointer(Some(&seat), &qh, ());
    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("dnd-probe"), None);
    layer.set_anchor(Anchor::TOP | Anchor::LEFT);
    layer.set_size(SIZE, SIZE);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();

    let mut probe = Probe {
        registry: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        ddm: DataDeviceManagerState::bind(&globals, &qh)?,
        pool: SlotPool::new((SIZE * SIZE * 4) as usize, &shm)?,
        _compositor: compositor,
        shm,
        layer,
        vp,
        device: None,
        pointer: None,
        configured: false,
        pressed_serial: None,
        source: None,
        uri_list: String::new(),
        done: None,
        extent: (0, 0),
        start: Instant::now(),
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    let pump = |probe: &mut Probe, ev: &mut EventLoop<Probe>, d: Duration| -> anyhow::Result<()> {
        let until = Instant::now() + d;
        while Instant::now() < until {
            ev.dispatch(Duration::from_millis(5), probe)?;
        }
        Ok(())
    };
    while !(probe.configured && probe.extent.0 > 0 && probe.pointer.is_some()) {
        if Instant::now() > deadline {
            bail!("timed out waiting for the surface, the output and a pointer");
        }
        ev.dispatch(Duration::from_millis(20), &mut probe)?;
    }
    pump(&mut probe, &mut ev, Duration::from_millis(150))?;

    match mode {
        Mode::Move(x, y) => {
            probe.move_to(x, y);
            pump(&mut probe, &mut ev, Duration::from_millis(100))?;
        }
        Mode::Click(x, y) => {
            probe.move_to(x, y);
            pump(&mut probe, &mut ev, Duration::from_millis(100))?;
            probe.button(true);
            pump(&mut probe, &mut ev, Duration::from_millis(60))?;
            probe.button(false);
            pump(&mut probe, &mut ev, Duration::from_millis(100))?;
        }
        Mode::Drag(from, to) => {
            probe.move_to(from.0, from.1);
            pump(&mut probe, &mut ev, Duration::from_millis(150))?;
            probe.button(true);
            pump(&mut probe, &mut ev, Duration::from_millis(100))?;
            let mid = (from.0 + 25.0, from.1 + 5.0);
            probe.glide(&mut ev, from, mid)?;
            probe.glide(&mut ev, mid, to)?;
            pump(&mut probe, &mut ev, Duration::from_millis(400))?;
            probe.button(false);
            pump(&mut probe, &mut ev, Duration::from_millis(300))?;
        }
        Mode::DragIn { path, files } => {
            probe.uri_list = files.iter().map(|f| file_uri(f) + "\r\n").collect();
            probe.move_to(f64::from(SIZE) / 2.0, f64::from(SIZE) / 2.0);
            pump(&mut probe, &mut ev, Duration::from_millis(100))?;
            probe.button(true);
            while probe.source.is_none() {
                if Instant::now() > deadline {
                    bail!("the press never reached the probe surface");
                }
                ev.dispatch(Duration::from_millis(5), &mut probe)?;
            }
            let mut from = (f64::from(SIZE) / 2.0, f64::from(SIZE) / 2.0);
            for to in path {
                probe.glide(&mut ev, from, to)?;
                pump(&mut probe, &mut ev, Duration::from_millis(400))?;
                from = to;
            }
            probe.button(false);
            while probe.done.is_none() {
                if Instant::now() > deadline {
                    bail!("the drop never finished");
                }
                ev.dispatch(Duration::from_millis(5), &mut probe)?;
            }
        }
        Mode::DragOut { from } => {
            probe.move_to(from.0, from.1);
            pump(&mut probe, &mut ev, Duration::from_millis(150))?;
            probe.button(true);
            pump(&mut probe, &mut ev, Duration::from_millis(100))?;
            // GTK starts a drag after the pointer moves past its threshold.
            probe.glide(&mut ev, from, (from.0 - 20.0, from.1 + 10.0))?;
            let to = (f64::from(SIZE) / 2.0, f64::from(SIZE) / 2.0);
            probe.glide(&mut ev, (from.0 - 20.0, from.1 + 10.0), to)?;
            pump(&mut probe, &mut ev, Duration::from_millis(400))?;
            probe.button(false);
            while probe.done.is_none() {
                if Instant::now() > deadline {
                    bail!("nothing was dropped on the probe");
                }
                ev.dispatch(Duration::from_millis(5), &mut probe)?;
            }
        }
    }
    match probe.done.take() {
        Some(Ok(s)) => {
            print!("{s}");
            Ok(())
        }
        Some(Err(e)) => bail!("{e}"),
        None => Ok(()),
    }
}

fn file_uri(p: &std::path::Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut s = String::from("file://");
    for &b in p.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

impl Probe {
    fn time(&self) -> u32 {
        self.start.elapsed().as_millis() as u32
    }

    fn move_to(&self, x: f64, y: f64) {
        let (w, h) = self.extent;
        let t = self.time();
        self.vp.motion_absolute(t, x.clamp(0.0, f64::from(w - 1)) as u32, y.clamp(0.0, f64::from(h - 1)) as u32, w, h);
        self.vp.frame();
    }

    fn button(&self, pressed: bool) {
        let state = if pressed { wl_pointer::ButtonState::Pressed } else { wl_pointer::ButtonState::Released };
        self.vp.button(self.time(), BTN_LEFT, state);
        self.vp.frame();
    }

    fn glide(&mut self, ev: &mut EventLoop<Self>, from: (f64, f64), to: (f64, f64)) -> anyhow::Result<()> {
        for i in 1..=20 {
            let k = f64::from(i) / 20.0;
            self.move_to(from.0 + (to.0 - from.0) * k, from.1 + (to.1 - from.1) * k);
            let until = Instant::now() + Duration::from_millis(15);
            while Instant::now() < until {
                ev.dispatch(Duration::from_millis(3), self)?;
            }
        }
        Ok(())
    }

    fn set_extent(&mut self, output: &wl_output::WlOutput) {
        let Some(info) = self.output_state.info(output) else { return };
        let size = info.logical_size.or_else(|| {
            info.modes
                .iter()
                .find(|m| m.current)
                .map(|m| (m.dimensions.0 / info.scale_factor, m.dimensions.1 / info.scale_factor))
        });
        if let Some((w, h)) = size {
            self.extent = (w as u32, h as u32);
        }
    }

    fn draw(&mut self) {
        let Ok((buffer, canvas)) =
            self.pool.create_buffer(SIZE as i32, SIZE as i32, SIZE as i32 * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        for px in canvas.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&[0x80, 0x40, 0xe0, 0xff]);
        }
        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, SIZE as i32, SIZE as i32);
        let _ = buffer.attach_to(surface);
        self.layer.commit();
    }
}

impl CompositorHandler for Probe {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Probe {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.set_extent(&output);
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.set_extent(&output);
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Probe {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.done = Some(Err("the probe surface was closed".into()));
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        self.configured = true;
        self.draw();
    }
}

impl SeatHandler for Probe {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if self.device.is_none() {
            self.device = Some(self.ddm.get_data_device(qh, &seat));
        }
        if cap == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, _: Capability) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Probe {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for e in events {
            if let PointerEventKind::Press { button: BTN_LEFT, serial, .. } = e.kind
                && &e.surface == self.layer.wl_surface()
            {
                self.pressed_serial = Some(serial);
                if !self.uri_list.is_empty() && self.source.is_none() {
                    let source = self.ddm.create_drag_and_drop_source(qh, vec!["text/uri-list"], DndAction::Copy);
                    if let Some(dev) = &self.device {
                        source.start_drag(dev, self.layer.wl_surface(), None, serial);
                    }
                    self.source = Some(source);
                }
            }
        }
    }
}

impl DataDeviceHandler for Probe {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        dev: &WlDataDevice,
        _: f64,
        _: f64,
        _: &wl_surface::WlSurface,
    ) {
        if let Some(offer) = self.device.as_ref().filter(|d| d.inner() == dev).and_then(|d| d.data().drag_offer()) {
            offer.accept_mime_type(offer.serial, Some("text/uri-list".into()));
            offer.set_actions(DndAction::Copy, DndAction::Copy);
        }
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64) {}
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn drop_performed(&mut self, conn: &Connection, _: &QueueHandle<Self>, dev: &WlDataDevice) {
        let Some(offer) = self.device.as_ref().filter(|d| d.inner() == dev).and_then(|d| d.data().drag_offer()) else {
            return;
        };
        let has = offer.with_mime_types(|m| m.iter().any(|m| m == "text/uri-list"));
        if !has {
            self.done = Some(Err("the drop offered no text/uri-list".into()));
            return;
        }
        let result = offer.receive("text/uri-list".into()).map_err(|e| e.to_string()).and_then(|pipe| {
            conn.flush().map_err(|e| e.to_string())?;
            let fd: OwnedFd = pipe.into();
            let mut f = std::fs::File::from(fd);
            let mut s = String::new();
            f.read_to_string(&mut s).map_err(|e| e.to_string())?;
            Ok(s)
        });
        offer.finish();
        offer.destroy();
        self.done = Some(result);
    }
}

impl DataOfferHandler for Probe {
    fn source_actions(&mut self, _: &Connection, _: &QueueHandle<Self>, offer: &mut DragOffer, _: DndAction) {
        offer.set_actions(DndAction::Copy, DndAction::Copy);
    }
    fn selected_action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}
}

impl DataSourceHandler for Probe {
    fn accept_mime(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: Option<String>) {}
    fn send_request(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, mime: String, pipe: WritePipe) {
        if mime == "text/uri-list" {
            let fd: OwnedFd = pipe.into();
            let mut f = std::fs::File::from(fd);
            let _ = f.write_all(self.uri_list.as_bytes());
        }
    }
    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        source.destroy();
        // sway also sends `cancelled` right after `dnd_finished`; a finished drop stands.
        if self.done.is_none() {
            self.done = Some(Err("the drag was cancelled (nobody accepted the drop)".into()));
        }
    }
    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        source.destroy();
        self.done = Some(Ok("dropped\n".into()));
    }
    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}
}

impl ShmHandler for Probe {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Probe {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerManagerV1,
        _: <ZwlrVirtualPointerManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<ZwlrVirtualPointerV1, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerV1,
        _: <ZwlrVirtualPointerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<wl_seat::WlSeat, ()> for Probe {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

delegate_compositor!(Probe);
delegate_output!(Probe);
delegate_shm!(Probe);
delegate_seat!(Probe);
delegate_pointer!(Probe);
delegate_layer!(Probe);
delegate_data_device!(Probe);
delegate_registry!(Probe);
