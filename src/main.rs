use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::image::*;

use std::convert::TryInto;
use std::collections::HashMap;

x11rb::atom_manager! {
	pub Atoms: AtomCollectionCookie {
		UTF8_STRING,
		_NET_DESKTOP_NAMES,
		_NET_CURRENT_DESKTOP,
		ESETROOT_PMAP_ID,
		_XROOTPMAP_ID,
		_98_UPDATE,
	}
}

fn print_err<A>(x: Result<A, Box<dyn std::error::Error>>) -> Option<A> {
	match x {
		Ok(a) => Some(a),
		Err(e) => {
			eprintln!("{}", e);
			None
		}
	}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let indir = match &std::env::args().collect::<Vec<_>>()[..] {
		[] => unreachable!(),
		[_, a] => a.clone(),
		[name, ..] => {eprintln!("Usage: {} <dir>", name); std::process::exit(1); }
	};
	let (conn, screen_num) = x11rb::connect(None)?;
	let screen = &conn.setup().roots[screen_num];

    let atoms = Atoms::new(&conn)?.reply()?;

	let backgrounds = std::cell::RefCell::new(HashMap::new());
	let get_background = |window: Drawable, name: String, width: u16, height: u16| -> Option<Pixmap> {
		*backgrounds.borrow_mut().entry((window, name.clone(), width, height)).or_insert_with(|| {
			print_err((|| -> Result<Pixmap, Box<dyn std::error::Error>> {
				let filename = format!("{}/{}-{}x{}.png", indir, name, width, height);
				let img = image::io::Reader::open(filename)?.decode()?;
				let img = Image::new(
					width, height,
					ScanlinePad::Pad8,
					24,
					BitsPerPixel::B24,
					x11rb::image::ImageOrder::MSBFirst,
					std::borrow::Cow::Borrowed(img.as_rgb8().unwrap())
				)?;
				let img = img.native(&conn.setup())?;

				let pixmap = conn.generate_id()?;
				let gc = conn.generate_id()?;
				conn.create_pixmap(24, pixmap, window, width, height)?.check()?;
				conn.create_gc(gc, pixmap, &CreateGCAux::new())?.check()?;
				img.put(&conn, pixmap, gc, 0, 0)?;
				conn.free_gc(gc)?;
				Ok(pixmap)
			})())
		})
	};

	let update = |window: Window| -> Result<(), Box<dyn std::error::Error>> {
		let names = conn.get_property(false, window, atoms._NET_DESKTOP_NAMES, atoms.UTF8_STRING, 0, u32::MAX)?;
		let index = conn.get_property(false, window, atoms._NET_CURRENT_DESKTOP, AtomEnum::CARDINAL, 0, u32::MAX)?;
		let geom  = conn.get_geometry(window)?;

		let names = names.reply()?.value;
		let index = index.reply()?.value;
		let geom  = geom .reply()?;

		let names = std::str::from_utf8(&names)?.split_terminator('\0').collect::<Vec<_>>();
		let name = index.try_into().ok().map(u32::from_le_bytes).and_then(|a| names.get(a as usize)).unwrap();

		if let Some(pixmap) = get_background(window, name.to_string(), geom.width, geom.height) {
			conn.change_property(PropMode::REPLACE, window, atoms._XROOTPMAP_ID, AtomEnum::PIXMAP, 32, 1, &pixmap.to_le_bytes())?;
			conn.change_property(PropMode::REPLACE, window, atoms.ESETROOT_PMAP_ID, AtomEnum::PIXMAP, 32, 1, &pixmap.to_le_bytes())?;
			conn.change_window_attributes(window, &ChangeWindowAttributesAux::new().background_pixmap(pixmap))?;
			conn.clear_area(true, window, 0, 0, geom.width, geom.height)?;
			conn.flush()?;
		}

		Ok(())
	};

	let change = ChangeWindowAttributesAux::default().event_mask(EventMask::PROPERTY_CHANGE);
	conn.change_window_attributes(screen.root, &change)?;
	print_err(update(screen.root));

	let mut dirty = false;
	loop {
		match conn.wait_for_event()? {
			Event::PropertyNotify(e) => {
				if e.atom == atoms._NET_CURRENT_DESKTOP || e.atom == atoms._NET_DESKTOP_NAMES {
					// This is the best synchronization mechanism I've been able to find
					if !dirty {
						dirty = true;
						conn.change_property(PropMode::REPLACE, e.window, atoms._98_UPDATE, AtomEnum::INTEGER, 32, 0, &[])?;
						conn.flush()?;
					}
				} else if e.atom == atoms._98_UPDATE {
					print_err(update(e.window));
					dirty = false;
				}
			}
			e => println!("{:?}", e),
		}
	}
}
