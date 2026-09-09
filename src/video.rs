use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{
    CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution,
};
use nokhwa::{Buffer, Camera, query};
use std::sync::Arc;
use std::thread;
use tokio::sync::watch;

use image;

use base64::prelude::*;

use minifb::{Window, WindowOptions};

pub fn start_camera_thread() -> watch::Receiver<Option<Arc<Buffer>>> {
    let (tx, rx) = watch::channel(None);

    thread::spawn(move || {
        let cameras = query(ApiBackend::Auto).expect("Failed to query cameras");

        // Match against the actual system string "c920"
        let target_camera_info = cameras
            .into_iter()
            .find(|info| info.human_name().to_lowercase().contains("c920"))
            .expect("C920 camera not found on USB bus");

        // let target_format = CameraFormat::new(Resolution::new(1280, 720), FrameFormat::YUYV, 30);
        let target_format = CameraFormat::new(Resolution::new(1280, 720), FrameFormat::YUYV, 5);
        let index = target_camera_info.index();
        let requested =
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::Exact(target_format));

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

pub fn run_gui(camera_rx: watch::Receiver<Option<Arc<nokhwa::Buffer>>>) {
    const WIDTH: usize = 512;
    const HEIGHT: usize = 512;

    let mut window = Window::new("Agent Vision Feed", WIDTH, HEIGHT, WindowOptions::default())
        .expect("Failed to open window");

    let mut buffer_u32: Vec<u32> = vec![0; WIDTH * HEIGHT];

    while window.is_open() && !window.is_key_down(minifb::Key::Escape) {
        let frame_arc = {
            let rx_lock = camera_rx.borrow();
            rx_lock.clone()
        };

        if let Some(frame) = frame_arc {
            if let Ok(decoded_rgb) = frame.decode_image::<nokhwa::pixel_format::RgbFormat>() {
                // Replicate the exact pipeline used for the LLM
                let img = image::DynamicImage::ImageRgb8(decoded_rgb);
                let resized = img.resize_to_fill(
                    WIDTH as u32,
                    HEIGHT as u32,
                    image::imageops::FilterType::Nearest,
                );

                let rgb_bytes = resized.into_rgb8();

                // Convert 8-bit RGB to 32-bit ARGB for minifb
                for (i, pixel) in rgb_bytes.chunks_exact(3).enumerate() {
                    if i < buffer_u32.len() {
                        let r = pixel[0] as u32;
                        let g = pixel[1] as u32;
                        let b = pixel[2] as u32;
                        buffer_u32[i] = (255 << 24) | (r << 16) | (g << 8) | b;
                    }
                }
            }
        }

        window
            .update_with_buffer(&buffer_u32, WIDTH, HEIGHT)
            .expect("Buffer update failed");
    }
}
pub async fn process_and_encode_frame(frame: Arc<nokhwa::Buffer>) -> String {
    tokio::task::spawn_blocking(move || {
        let decoded = frame
            .decode_image::<RgbFormat>()
            .expect("Failed to decode RGB");
        let img = image::DynamicImage::ImageRgb8(decoded);

                let resized = img.resize_to_fill(
                    512,
                    512,
                    image::imageops::FilterType::Nearest,
                );

        let mut jpeg_bytes: Vec<u8> = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut jpeg_bytes);
        resized
            .write_to(&mut cursor, image::ImageFormat::Jpeg)
            .expect("Failed to encode JPEG");

        BASE64_STANDARD.encode(&jpeg_bytes)
    })
    .await
    .expect("Image processing thread panicked")
}
