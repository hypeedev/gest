// https://github.com/rvaiya/keyd/blob/master/scripts/keyd-application-mapper <3

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

const WLROOTS_INTERFACE_NAME: &str = "zwlr_foreign_toplevel_manager_v1";

#[derive(Debug)]
struct Wayland {
    socket: UnixStream,
}

impl Wayland {
    fn new(interface_name: &str) -> Self {
        let mut path = std::env::var("WAYLAND_DISPLAY").expect("WAYLAND_DISPLAY not set (is wayland running?)");
        if !path.starts_with('/') {
            let xdg_runtime_dir = std::env::var("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR not set");
            path = format!("{}/{}", xdg_runtime_dir, path);
        }

        let socket = UnixStream::connect(path).expect("Failed to connect to WAYLAND_DISPLAY");
        let mut wayland = Wayland { socket };
        wayland.bind_interface(interface_name);

        wayland
    }

    fn bind_interface(&mut self, name: &str) {
        self.send_message(1, 1, &[0x02, 0x00, 0x00, 0x00]);
        self.send_message(1, 0, &[0x03, 0x00, 0x00, 0x00]);
        loop {
            let (obj, event, payload) = self.receive_message();
            if obj == 2 && event == 0 {
                let wl_interface = self.read_string(&payload[4..]);

                if wl_interface == name {
                    let mut new_payload = payload.to_vec();
                    new_payload.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]);
                    self.send_message(2, 0, &new_payload);
                    return;
                }
            }

            if obj == 3 {
                panic!("Could not find interface {}", name);
            }
        }
    }

    fn send_message(&mut self, object_id: u32, opcode: u32, payload: &[u8]) {
        let size = payload.len() as u32 + 8;
        let full_opcode = opcode | (size << 16);
        let mut message = object_id.to_le_bytes().to_vec();
        message.extend_from_slice(&full_opcode.to_le_bytes());
        message.extend_from_slice(payload);
        self.socket.write_all(&message).expect("Failed to send message");
    }

    fn receive_message(&mut self) -> (u32, u32, Vec<u8>) {
        let mut header = [0u8; 8];
        self.socket.read_exact(&mut header).expect("Failed to read message header");
        let object_id = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        let evcode = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let size = (evcode >> 16) as usize;
        let evcode = evcode & 0xFFFF;

        let mut message = vec![0u8; size - 8];
        self.socket.read_exact(&mut message).expect("Failed to read full message");

        (object_id, evcode, message)
    }

    fn read_string(&self, payload: &[u8]) -> String {
        let len = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
        String::from_utf8(payload[4..4 + len - 1].to_vec()).expect("Failed to read string")
    }
}

type OnWindowChange = Box<dyn Fn(String, String) + Send + Sync>;

struct Window {
    title: Option<String>,
    class_name: Option<String>,
}

pub struct WlrootsMonitor {
    wayland: Wayland,
    on_window_change: OnWindowChange,
}

impl WlrootsMonitor {
    pub fn new(on_window_change: OnWindowChange) -> Self {
        let wayland = Wayland::new(WLROOTS_INTERFACE_NAME);
        WlrootsMonitor { wayland, on_window_change }
    }

    pub fn run(&mut self) {
        let mut windows = HashMap::new();

        loop {
            let (obj, event, payload) = self.wayland.receive_message();
            if obj == 4 && event == 0 {
                let window = Window { title: None, class_name: None };
                windows.insert(u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]), window);
            }

            if let Some(win) = windows.get_mut(&obj) {
                match event {
                    0 => win.title = Some(self.wayland.read_string(&payload)),
                    1 => win.class_name = Some(self.wayland.read_string(&payload)),
                    4 if payload[0] > 0 && payload[4] == 2 => {
                        (self.on_window_change)(
                            win.class_name.clone().unwrap_or_default().to_string(),
                            win.title.clone().unwrap_or_default().to_string(),
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}