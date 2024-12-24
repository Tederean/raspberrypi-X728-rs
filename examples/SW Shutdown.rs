// X728 full shutdown through Software

use rpipal::gpio::{Gpio, Mode, Value};
use std::env;
use std::time::Duration;
use tokio::time::sleep;

const BUTTON: u8 = 26;

#[tokio::main]
async fn main() {
    let mut gpio = Gpio::new().unwrap();
    gpio.export_pin(BUTTON).unwrap();
    gpio.set_mode(BUTTON, Mode::Output);
    gpio.set_value(BUTTON, Value::High);

    let sleep_duration_secs: f64 = match env::args().nth(1) {
        Some(arg) => arg.parse().expect("Sleep time must be a valid number"),
        None => 4.0, // Default sleep time
    };

    println!("X728 Shutting down...");
    sleep(Duration::from_secs_f64(sleep_duration_secs)).await;

    // Restore GPIO 26
    gpio.set_value(BUTTON, Value::Low);
}
