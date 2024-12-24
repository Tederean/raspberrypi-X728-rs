//x728 Powering on /reboot /full shutdown through hardware

use rpipal::gpio::{Gpio, Mode, Value};
use std::time::{Duration, Instant};
use tokio::time::sleep;

const SHUTDOWN: u8 = 5;
const REBOOTPULSEMINIMUM: i64 = 200;
const REBOOTPULSEMAXIMUM: i64 = 600;
const BOOT: u8 = 12;

#[tokio::main]
async fn main() {
    let mut gpio = Gpio::new().unwrap();

    gpio.export_pin(SHUTDOWN).unwrap();
    gpio.set_mode(SHUTDOWN, Mode::Input);

    gpio.export_pin(BOOT).unwrap();
    gpio.set_mode(BOOT, Mode::Output);
    gpio.set_value(BOOT, Value::High);

    println!("X728 Shutting down...");

    loop {
        let shutdown_signal = gpio.get_value(SHUTDOWN).unwrap();

        if shutdown_signal == Value::Low {
            sleep(Duration::from_millis(200)).await;
        } else {
            let pulse_start = Instant::now();

            while gpio.get_value(SHUTDOWN).unwrap() == Value::High {
                sleep(Duration::from_millis(20)).await;

                let elapsed = pulse_start.elapsed().as_millis() as i64;

                if elapsed > REBOOTPULSEMAXIMUM {
                    println!("X728 Shutting down, halting Rpi ...");
                    // Insert your shutdown logic here
                    // sudo poweroff
                    break;
                }
            }

            if pulse_start.elapsed().as_millis() as i64 > REBOOTPULSEMINIMUM {
                println!("X728 Rebooting, recycling Rpi ...");
                // Insert your reboot logic here
                // sudo reboot
                break;
            }
        }
    }
}
