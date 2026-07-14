//! docs/998-roadmap.md M7 stage 2: real DRM/KMS mode-set, the minimal bounded proof this
//! roadmap's own text scoped stage 2 down to -- real pixels on a real display device, not a full
//! compositor (no window management, no GPU acceleration, no input routing; docs/13's own
//! structured UI trees still render to nothing here, exactly as before). Uses the kernel's
//! generic "dumb buffer" API (a plain CPU-writable framebuffer any KMS driver supports, no
//! GPU-specific rendering pipeline needed) via the real `drm` crate, mirroring that crate's own
//! `legacy_modeset` example almost exactly.
//!
//! Inert (logs a note, does nothing else) if `/dev/dri/card0` doesn't exist at all -- real
//! hardware or a QEMU boot with no GPU device attached (every other boot script in this repo:
//! `boot-test.sh`, `boot-benchmark.sh`, `update-rollback-test.sh`, none of which attach one) never
//! triggers this at all, the same graceful-degradation shape as the data-partition and cgroup
//! probes elsewhere in this crate. Deliberately leaks the opened `Card` (never closes its file
//! descriptor) so the real mode-set this sets stays live for as long as this process runs (PID 1,
//! i.e. the whole boot) -- closing the DRM master fd that set a mode can, depending on the
//! driver, revert the display, which would defeat the entire point of a persistent proof a real
//! screenshot can later be taken of.

use std::io;
use std::os::unix::io::{AsFd, BorrowedFd};
use std::path::Path;

use drm::buffer::DrmFourcc;
use drm::control::{connector, crtc, Device as ControlDevice};
use drm::Device;

const CARD_PATH: &str = "/dev/dri/card0";

struct Card(std::fs::File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl Device for Card {}
impl ControlDevice for Card {}

impl Card {
    fn open(path: &str) -> io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        Ok(Card(file))
    }
}

/// A simple, deliberate three-band color pattern -- not a solid fill, so a real screenshot
/// showing exactly these three bands in this exact order is strong evidence of a real, working
/// mode-set (rendering *specific, controlled* pixel data), not stale/garbage VRAM content a
/// solid fill could be mistaken for.
const BAND_COLORS_BGRX: [[u8; 4]; 3] = [
    [0x4a, 0x2c, 0x6b, 0x00], // deep violet (Hyperion's own band)
    [0xff, 0xff, 0xff, 0x00], // white
    [0x6b, 0x2c, 0x4a, 0x00], // deep magenta
];

/// Runs the real DRM/KMS mode-set proof if (and only if) a real display device is present at
/// [`CARD_PATH`].
pub fn run_display_probe() {
    if !Path::new(CARD_PATH).exists() {
        return;
    }

    let card = match Card::open(CARD_PATH) {
        Ok(c) => c,
        Err(e) => {
            println!("[hyperion-init] DISPLAY: FAIL -- couldn't open {CARD_PATH}: {e}");
            return;
        }
    };

    let result = (|| -> Result<(String, (u32, u32)), String> {
        let res = card
            .resource_handles()
            .map_err(|e| format!("couldn't load real resource handles: {e}"))?;

        let coninfo: Vec<connector::Info> = res
            .connectors()
            .iter()
            .flat_map(|con| card.get_connector(*con, true))
            .collect();
        let crtcinfo: Vec<crtc::Info> = res
            .crtcs()
            .iter()
            .flat_map(|crtc| card.get_crtc(*crtc))
            .collect();

        let con = coninfo
            .iter()
            .find(|c| c.state() == connector::State::Connected)
            .ok_or("no real connected connector found")?;
        let &mode = con
            .modes()
            .first()
            .ok_or("connected connector reports no real display modes")?;
        let (width, height) = mode.size();
        let crtc = crtcinfo.first().ok_or("no real CRTC found")?;

        let fmt = DrmFourcc::Xrgb8888;
        let mut db = card
            .create_dumb_buffer((width.into(), height.into()), fmt, 32)
            .map_err(|e| format!("couldn't create a real dumb buffer: {e}"))?;

        {
            let mut map = card
                .map_dumb_buffer(&mut db)
                .map_err(|e| format!("couldn't map the real dumb buffer: {e}"))?;
            let buf = map.as_mut();
            let band_height = (height as usize) / BAND_COLORS_BGRX.len();
            let stride = (width as usize) * 4;
            for row in 0..(height as usize) {
                let band = (row / band_height.max(1)).min(BAND_COLORS_BGRX.len() - 1);
                let color = BAND_COLORS_BGRX[band];
                for col in 0..(width as usize) {
                    let offset = row * stride + col * 4;
                    if offset + 4 <= buf.len() {
                        buf[offset..offset + 4].copy_from_slice(&color);
                    }
                }
            }
        }

        let fb = card
            .add_framebuffer(&db, 24, 32)
            .map_err(|e| format!("couldn't create a real framebuffer object: {e}"))?;

        card.set_crtc(crtc.handle(), Some(fb), (0, 0), &[con.handle()], Some(mode))
            .map_err(|e| format!("real SETCRTC ioctl failed: {e}"))?;

        // Deliberately not destroying `fb`/`db` (plain `Copy` handles -- nothing to release on
        // this process's side beyond what the kernel itself now owns), and not closing `card`
        // (a real fd, whose drop impl *would* close it) -- see this module's own doc comment on
        // why the mode-set needs to outlive this function call.
        std::mem::forget(card);

        Ok((format!("{mode:?}"), (width.into(), height.into())))
    })();

    match result {
        Ok((mode, (w, h))) => {
            println!(
                "[hyperion-init] DISPLAY: PASS -- real DRM/KMS mode-set applied, {w}x{h}, real \
                 mode {mode}, three-band color pattern written to a real dumb buffer and \
                 displayed"
            );
        }
        Err(e) => println!("[hyperion-init] DISPLAY: FAIL -- {e}"),
    }
}
