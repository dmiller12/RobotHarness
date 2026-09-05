use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{
    CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution,
};
use nokhwa::{Buffer, Camera};
use std::sync::Arc;
use std::thread;
use tokio::sync::watch;

use minifb::{Window, WindowOptions};

pub fn start_camera_thread() -> watch::Receiver<Option<Arc<Buffer>>> {
    let (tx, rx) = watch::channel(None);

    thread::spawn(move || {
        let index = CameraIndex::Index(0);
        let target_format = CameraFormat::new(Resolution::new(1280, 720), FrameFormat::YUYV, 30);
        let requested =
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(target_format));

        let mut camera = match Camera::new(index.clone(), requested) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to initialize camera {}: {}", index, e);
                return;
            }
        };

        if let Err(e) = camera.open_stream() {
            eprintln!("Failed to open stream for camera {}: {}", index, e);
            return;
        }

        loop {
            match camera.frame() {
                Ok(frame) => {
                    if tx.send(Some(Arc::new(frame))).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("Error capturing frame: {}", e);
                    // Decide if you want to break or continue polling here
                }
            }
        }
    });

    rx
}

use nokhwa::utils::ApiBackend;

pub fn probe_camera_hardware() {
    let cameras = nokhwa::query(ApiBackend::Auto).unwrap_or_else(|e| {
        eprintln!("Failed to query cameras: {}", e);
        vec![]
    });

    println!("Found {} cameras.", cameras.len());
    for info in cameras {
        println!(
            "Index: {}, Name: {}, Desc: {}",
            info.index(),
            info.human_name(),
            info.description()
        );
    }
}

pub fn run_gui(camera_rx: watch::Receiver<Option<Arc<Buffer>>>) {
    let mut window = Window::new("Agent Vision Feed", 1280, 720, WindowOptions::default())
        .expect("Failed to open window");

    let mut buffer_u32: Vec<u32> = vec![0; 1280 * 720];

    while window.is_open() && !window.is_key_down(minifb::Key::Escape) {
        let frame_arc = {
            let rx_lock = camera_rx.borrow();
            rx_lock.clone()
        };

        if let Some(frame) = frame_arc {
            if let Ok(decoded_rgb) = frame.decode_image::<RgbFormat>() {
                // Convert 8-bit RGB to 32-bit ARGB
                for (i, pixel) in decoded_rgb.chunks_exact(3).enumerate() {
                    if i < buffer_u32.len() {
                        let r = pixel[0] as u32;
                        let g = pixel[1] as u32;
                        let b = pixel[2] as u32;
                        // Pack into ARGB format (A is top 8 bits, R, G, B)
                        buffer_u32[i] = (255 << 24) | (r << 16) | (g << 8) | b;
                    }
                }
            }
        }

        window
            .update_with_buffer(&buffer_u32, 1280, 720)
            .expect("Buffer update failed");
    }
}
